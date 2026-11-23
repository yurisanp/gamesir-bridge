//! Janela principal: lista de controles à esquerda, o controle selecionado à direita.

use crate::backend::{Backend, Ev};
use crate::pad::PadView;
use crate::{calibration, editor, labels, prefs, setup};
use adw::prelude::*;
use gamesir_core::api::{ControllerInfo, Status};
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

pub struct App {
    pub window: adw::ApplicationWindow,
    pub toasts: adw::ToastOverlay,
    stack: gtk::Stack,
    pub setup: setup::SetupPage,
    split: adw::NavigationSplitView,
    connected_list: gtk::ListBox,
    known_list: gtk::ListBox,
    known_title: gtk::Label,
    pub nav: adw::NavigationView,
    controller_page: adw::NavigationPage,
    backend: RefCell<Option<Backend>>,
    pub controllers: RefCell<Vec<ControllerInfo>>,
    pub selected: RefCell<Option<String>>,
    live_pad: RefCell<Option<(PadView, gtk::Label)>>,
    pub editor: RefCell<Option<editor::Editor>>,
    pub calibration: RefCell<Option<calibration::CalDialog>>,
    pub tx: async_channel::Sender<Ev>,
    rebuilding: Cell<bool>,
    loaded: Cell<bool>,
    known_ids: RefCell<Vec<String>>,
}

impl App {
    pub fn new(app: &adw::Application) -> Rc<App> {
        let (tx, rx) = async_channel::unbounded();

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("GameSir Bridge")
            .default_width(980)
            .default_height(680)
            .width_request(360)
            .height_request(480)
            .build();

        // Barra lateral.
        let connected_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .build();
        let known_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .build();
        let heading = |t: &str| {
            gtk::Label::builder()
                .label(t)
                .xalign(0.0)
                .margin_start(18)
                .margin_top(12)
                .margin_bottom(4)
                .css_classes(["heading", "dim-label", "caption-heading"])
                .build()
        };
        let known_title = heading("Conhecidos");
        let side_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        side_box.append(&heading("Conectados"));
        side_box.append(&connected_list);
        side_box.append(&known_title);
        side_box.append(&known_list);
        let side_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&side_box)
            .build();
        let side_header = adw::HeaderBar::new();
        let menu = gtk::gio::Menu::new();
        menu.append(Some("Preferências"), Some("app.preferences"));
        menu.append(Some("Sobre o GameSir Bridge"), Some("app.about"));
        side_header.pack_end(
            &gtk::MenuButton::builder()
                .icon_name("open-menu-symbolic")
                .menu_model(&menu)
                .tooltip_text("Menu principal")
                .primary(true)
                .build(),
        );
        let side_view = adw::ToolbarView::new();
        side_view.add_top_bar(&side_header);
        side_view.set_content(Some(&side_scroll));
        let sidebar = adw::NavigationPage::builder()
            .title("Controles")
            .child(&side_view)
            .build();

        // Conteúdo.
        let controller_page = adw::NavigationPage::builder()
            .title("Controle")
            .tag("controller")
            .build();
        let nav = adw::NavigationView::new();
        nav.add(&controller_page);
        let content = adw::NavigationPage::builder()
            .title("Controle")
            .child(&nav)
            .build();

        let split = adw::NavigationSplitView::builder()
            .sidebar(&sidebar)
            .content(&content)
            .min_sidebar_width(260.0)
            .build();

        let setup = setup::SetupPage::new();
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&setup.widget, Some("setup"));
        stack.add_named(&split, Some("main"));
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&stack));
        window.set_content(Some(&toasts));

        // Em tela estreita, a lista e o controle viram uma pilha.
        let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            640.0,
            adw::LengthUnit::Sp,
        ));
        bp.add_setter(&split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(bp);

        let this = Rc::new(App {
            window,
            toasts,
            stack,
            setup,
            split,
            connected_list,
            known_list,
            known_title,
            nav,
            controller_page,
            backend: RefCell::new(None),
            controllers: RefCell::new(Vec::new()),
            selected: RefCell::new(None),
            live_pad: RefCell::new(None),
            editor: RefCell::new(None),
            calibration: RefCell::new(None),
            tx,
            rebuilding: Cell::new(false),
            loaded: Cell::new(false),
            known_ids: RefCell::new(Vec::new()),
        });

        for list in [&this.connected_list, &this.known_list] {
            let weak = Rc::downgrade(&this);
            list.connect_row_selected(move |list, row| {
                let (Some(app), Some(row)) = (weak.upgrade(), row) else {
                    return;
                };
                if app.rebuilding.get() {
                    return;
                }
                let id = row.widget_name().to_string();
                // Só uma lista com seleção de cada vez.
                if list == &app.connected_list {
                    app.known_list.unselect_all();
                } else {
                    app.connected_list.unselect_all();
                }
                app.select(Some(id));
                app.split.set_show_content(true);
            });
        }

        let weak = Rc::downgrade(&this);
        this.setup.connect_done(move || {
            if let Some(app) = weak.upgrade() {
                app.clone().connect();
            }
        });

        // Eventos do serviço.
        let weak = Rc::downgrade(&this);
        glib::spawn_future_local(async move {
            while let Ok(ev) = rx.recv().await {
                let Some(app) = weak.upgrade() else { break };
                app.on_event(ev);
            }
        });

        // Renova o "estado ao vivo" enquanto um controle conectado estiver na tela.
        let weak = Rc::downgrade(&this);
        glib::timeout_add_local(Duration::from_secs(5), move || {
            let Some(app) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            app.renew_watch();
            glib::ControlFlow::Continue
        });

        this
    }

    pub fn backend(&self) -> Option<Backend> {
        self.backend.borrow().clone()
    }

    pub fn toast(&self, text: &str) {
        self.toasts.add_toast(adw::Toast::new(text));
    }

    /// Conecta ao serviço; sem ele, mostra a primeira execução.
    pub fn connect(self: Rc<Self>) {
        glib::spawn_future_local(async move {
            match Backend::connect(self.tx.clone()).await {
                Ok(b) => {
                    let version = b.version().await.unwrap_or_default();
                    *self.backend.borrow_mut() = Some(b);
                    self.stack.set_visible_child_name("main");
                    if version != gamesir_core::VERSION && setup::in_appimage() {
                        self.toast(&format!(
                            "O serviço instalado é a versão {version}; atualize em Preferências › Avançado"
                        ));
                    }
                    self.refresh();
                }
                Err(err) => {
                    *self.backend.borrow_mut() = None;
                    self.setup.update(Some(&err));
                    self.stack.set_visible_child_name("setup");
                }
            }
        });
    }

    fn on_event(self: &Rc<Self>, ev: Ev) {
        match ev {
            Ev::Changed => self.clone().refresh(),
            Ev::Lost => {
                *self.backend.borrow_mut() = None;
                self.setup.update(Some("O serviço parou."));
                self.stack.set_visible_child_name("setup");
            }
            Ev::Live(id, live) => {
                if self.selected.borrow().as_deref() == Some(id.as_str()) {
                    if let Some((pad, label)) = &*self.live_pad.borrow() {
                        pad.set_live(&live);
                        if let Some(t) = live.active.first() {
                            label.set_text(&format!("Último botão: {}", labels::target(*t)));
                        }
                    }
                    if let Some(ed) = &*self.editor.borrow() {
                        ed.set_live(&live);
                    }
                }
            }
            Ev::Captured(id, c) => {
                if let Some(ed) = &*self.editor.borrow() {
                    if ed.id == id {
                        ed.captured(&c);
                    }
                }
            }
            ev @ (Ev::Step { .. } | Ev::Hit { .. } | Ev::Skipped { .. } | Ev::Finished { .. }) => {
                let dlg = self.calibration.borrow().clone();
                if let Some(d) = dlg {
                    d.on_event(&ev);
                }
            }
        }
    }

    /// Relê a lista do serviço e redesenha.
    pub fn refresh(self: Rc<Self>) {
        let Some(b) = self.backend() else { return };
        glib::spawn_future_local(async move {
            match b.controllers().await {
                Ok(list) => {
                    let old = self.controllers.replace(list);
                    self.notify_new(&old);
                    self.rebuild();
                }
                Err(e) => self.toast(&format!("Não consegui ler os controles: {e}")),
            }
        });
    }

    /// Notificação do sistema quando um controle passa a ativo com a janela em segundo plano.
    fn notify_new(&self, old: &[ControllerInfo]) {
        // A primeira leitura só mostra o que já estava lá.
        if !self.loaded.replace(true) || !crate::prefs::get().notify || self.window.is_active() {
            return;
        }
        let Some(app) = self.window.application() else {
            return;
        };
        for c in self.controllers.borrow().iter() {
            let was_active = old
                .iter()
                .any(|o| o.id == c.id && o.status == Status::Active);
            if c.status != Status::Active || was_active {
                continue;
            }
            let n = gtk::gio::Notification::new(&c.name);
            n.set_body(Some(&match c.player {
                Some(p) => format!("Conectado como Jogador {p}"),
                None => "Conectado".to_owned(),
            }));
            app.send_notification(Some(&format!("connected-{}", c.id)), &n);
        }
    }

    fn row(c: &ControllerInfo) -> gtk::ListBoxRow {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&c.name))
            .subtitle(labels::subtitle(c))
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("input-gaming-symbolic"));
        let dot = gtk::Label::builder()
            .label("●")
            .css_classes([labels::status_class(c.status)])
            .tooltip_text(labels::status(c.status))
            .build();
        row.add_suffix(&dot);
        row.set_widget_name(&c.id);
        row.upcast()
    }

    fn rebuild(self: &Rc<Self>) {
        self.rebuilding.set(true);
        for list in [&self.connected_list, &self.known_list] {
            while let Some(r) = list.row_at_index(0) {
                list.remove(&r);
            }
        }
        let list = self.controllers.borrow().clone();
        let mut known = Vec::new();
        for c in &list {
            let row = Self::row(c);
            if c.connected {
                self.connected_list.append(&row);
            } else {
                known.push(c.id.clone());
                self.known_list.append(&row);
            }
        }
        if list.iter().all(|c| !c.connected) {
            let empty = gtk::Label::builder()
                .label("Plugue o dongle ou ligue o controle")
                .wrap(true)
                .margin_top(12)
                .margin_bottom(12)
                .margin_start(12)
                .margin_end(12)
                .css_classes(["dim-label"])
                .build();
            let row = gtk::ListBoxRow::builder()
                .child(&empty)
                .selectable(false)
                .activatable(false)
                .build();
            self.connected_list.append(&row);
        }
        self.known_title.set_visible(!known.is_empty());
        *self.known_ids.borrow_mut() = known;

        // Mantém a seleção; senão, o primeiro conectado.
        let sel = self.selected.borrow().clone();
        let sel = sel
            .filter(|id| list.iter().any(|c| &c.id == id))
            .or_else(|| list.iter().find(|c| c.connected).map(|c| c.id.clone()))
            .or_else(|| list.first().map(|c| c.id.clone()));
        for l in [&self.connected_list, &self.known_list] {
            let mut i = 0;
            while let Some(r) = l.row_at_index(i) {
                if Some(r.widget_name().as_str()) == sel.as_deref() {
                    l.select_row(Some(&r));
                }
                i += 1;
            }
        }
        self.rebuilding.set(false);
        self.select(sel);
    }

    pub fn current(&self) -> Option<ControllerInfo> {
        let sel = self.selected.borrow().clone()?;
        self.controllers
            .borrow()
            .iter()
            .find(|c| c.id == sel)
            .cloned()
    }

    fn renew_watch(&self) {
        let (Some(b), Some(c)) = (self.backend(), self.current()) else {
            return;
        };
        if c.connected && c.profile_id.is_some() {
            glib::spawn_future_local(async move {
                let _ = b.watch(&c.id, true).await;
            });
        }
    }

    pub fn select(self: &Rc<Self>, id: Option<String>) {
        let old = self.selected.replace(id.clone());
        let changed = old != id;
        if changed {
            if let (Some(b), Some(old)) = (self.backend(), old.clone()) {
                glib::spawn_future_local(async move {
                    let _ = b.watch(&old, false).await;
                });
            }
        }
        // O editor é de um controle só; ao trocar, volta para a página do controle.
        if changed && self.editor.borrow().is_some() {
            self.nav.pop_to_tag("controller");
            *self.editor.borrow_mut() = None;
        }
        let Some(c) = self.current() else {
            let status = adw::StatusPage::builder()
                .icon_name("input-gaming-symbolic")
                .title("Nenhum controle")
                .description("Plugue o dongle ou ligue o controle. Ele aparece aqui sozinho.")
                .build();
            self.controller_page.set_child(Some(&status));
            self.controller_page.set_title("Controles");
            *self.live_pad.borrow_mut() = None;
            return;
        };
        self.controller_page.set_title(&c.name);
        self.controller_page
            .set_child(Some(&self.controller_view(&c)));
        if old != id {
            self.renew_watch();
        }
    }

    fn controller_view(self: &Rc<Self>, c: &ControllerInfo) -> gtk::Widget {
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());

        let page = adw::PreferencesPage::new();

        if let Some(err) = c.error.as_ref().or(c.warnings.first()) {
            let banner = adw::Banner::builder().title(err).revealed(true).build();
            view.add_top_bar(&banner);
        }

        // Topo: desenho ao vivo, nome e estado.
        let hero = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_bottom(6)
            .build();
        let pad = PadView::new(300, 190);
        pad.area.set_halign(gtk::Align::Center);
        pad.area.set_size_request(280, 176);
        hero.append(&pad.area);
        let title = gtk::Label::builder()
            .label(&c.name)
            .css_classes(["title-2"])
            .wrap(true)
            .build();
        hero.append(&title);
        let state_text = match c.status {
            Status::Active => "Ativo · aparece como controle Xbox 360".to_owned(),
            s => labels::status(s).to_owned(),
        };
        let state = gtk::Label::builder()
            .label(&state_text)
            .css_classes([labels::status_class(c.status), "heading"])
            .build();
        hero.append(&state);
        let last = gtk::Label::builder()
            .label(" ")
            .css_classes(["dim-label", "caption"])
            .build();
        if c.connected && c.profile_id.is_some() {
            hero.append(&last);
            *self.live_pad.borrow_mut() = Some((pad, last));
        } else {
            *self.live_pad.borrow_mut() = None;
        }
        let hero_group = adw::PreferencesGroup::new();
        hero_group.add(&hero);
        page.add(&hero_group);

        if c.status == Status::Unmapped {
            let g = adw::PreferencesGroup::builder()
                .title("Sem mapeamento")
                .description(
                    "Este controle ainda não tem perfil. Calibre em 21 passos: ele passa a aparecer \
                     como um Xbox 360 e o perfil fica salvo para os próximos.",
                )
                .build();
            let b = gtk::Button::builder()
                .label("Calibrar controle")
                .halign(gtk::Align::Center)
                .css_classes(["pill", "suggested-action"])
                .margin_top(12)
                .build();
            let app = self.clone();
            let id = c.id.clone();
            b.connect_clicked(move |_| calibration::open(&app, &id));
            g.add(&b);
            page.add(&g);
        } else if c.profile_id.is_some() {
            let g = adw::PreferencesGroup::new();
            let bridge = adw::SwitchRow::builder()
                .title("Ponte ligada")
                .subtitle("Desligue para usar o controle sem conversão")
                .active(c.enabled)
                .build();
            let app = self.clone();
            let id = c.id.clone();
            bridge.connect_active_notify(move |row| {
                let Some(b) = app.backend() else { return };
                let (id, on, app) = (id.clone(), row.is_active(), app.clone());
                glib::spawn_future_local(async move {
                    if let Err(e) = b.set_enabled(&id, on).await {
                        app.toast(&e);
                        app.clone().refresh();
                    }
                });
            });
            g.add(&bridge);
            if let Some(p) = c.player {
                let row = adw::ActionRow::builder()
                    .title(format!("Jogador {p}"))
                    .subtitle("Pela ordem em que os controles conectaram")
                    .build();
                g.add(&row);
            }
            page.add(&g);

            let g = adw::PreferencesGroup::builder().title("Mapeamento").build();
            let src = labels::source(c.mapping_source.as_deref());
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&format!(
                    "{}{}",
                    c.profile_name.as_deref().unwrap_or("Perfil"),
                    if src.is_empty() {
                        String::new()
                    } else {
                        format!(" ({src})")
                    }
                )))
                .subtitle(format!("{} de {} itens", c.mapped, c.total))
                .build();
            let buttons = gtk::Box::builder()
                .spacing(6)
                .valign(gtk::Align::Center)
                .build();
            let edit = gtk::Button::builder().label("Testar e editar").build();
            let recal = gtk::Button::builder().label("Recalibrar").build();
            edit.set_sensitive(c.connected);
            recal.set_sensitive(c.connected);
            let app = self.clone();
            let id = c.id.clone();
            edit.connect_clicked(move |_| editor::open(&app, &id));
            let app = self.clone();
            let id = c.id.clone();
            recal.connect_clicked(move |_| calibration::open(&app, &id));
            buttons.append(&edit);
            buttons.append(&recal);
            row.add_suffix(&buttons);
            g.add(&row);
            page.add(&g);
        }

        // Detalhes técnicos, recolhidos em fonte mono.
        let g = adw::PreferencesGroup::builder().title("Detalhes").build();
        let mono_row = |title: &str, value: &str| {
            let row = adw::ActionRow::builder()
                .title(title)
                .subtitle(glib::markup_escape_text(value))
                .subtitle_selectable(true)
                .build();
            row.add_css_class("property");
            row.add_css_class("monospace-subtitle");
            row
        };
        g.add(&mono_row("Dispositivo", &c.device));
        if !c.inputs.is_empty() {
            let inputs: Vec<String> = c
                .inputs
                .iter()
                .map(|i| {
                    let n = labels::node(&i.path);
                    if i.name.contains("Consumer") {
                        format!("{n} (Home)")
                    } else {
                        n.to_owned()
                    }
                })
                .collect();
            g.add(&mono_row("Entradas", &inputs.join(" + ")));
        }
        if let Some(o) = &c.output {
            g.add(&mono_row(
                "Saída virtual",
                &format!("045e:028e · {}", labels::node(o)),
            ));
        }
        g.add(&mono_row("Identificador", &c.id));
        page.add(&g);

        if !c.notes.is_empty() {
            let g = adw::PreferencesGroup::new();
            let notes = gtk::Label::builder()
                .label(c.notes.join(" "))
                .wrap(true)
                .xalign(0.0)
                .css_classes(["dim-label", "caption"])
                .build();
            g.add(&notes);
            page.add(&g);
        }

        if !c.connected {
            let g = adw::PreferencesGroup::new();
            let forget = gtk::Button::builder()
                .label("Esquecer este controle")
                .halign(gtk::Align::Center)
                .css_classes(["pill", "destructive-action"])
                .build();
            let app = self.clone();
            let id = c.id.clone();
            forget.connect_clicked(move |_| {
                let Some(b) = app.backend() else { return };
                let (id, app) = (id.clone(), app.clone());
                glib::spawn_future_local(async move {
                    if let Err(e) = b.forget(&id).await {
                        app.toast(&e);
                    }
                });
            });
            g.add(&forget);
            page.add(&g);
        }

        view.set_content(Some(&page));
        view.upcast()
    }

    /// Mostra a tela de primeira execução (usada pelo modo demo).
    pub fn show_setup(&self) {
        self.setup.update(None);
        self.stack.set_visible_child_name("setup");
    }

    pub fn show_preferences(self: &Rc<Self>) {
        prefs::open(self);
    }
}
