//! Editor de mapeamento: o controle ao vivo à esquerda (o que você aperta acende) e uma
//! linha por botão do Xbox dizendo de onde ele vem. O lápis espera você apertar o botão físico.

use crate::labels;
use crate::pad::PadView;
use crate::window::App;
use adw::prelude::*;
use gamesir_core::api::{Captured, LiveState};
use gamesir_core::target::TargetKind;
use gamesir_core::{Mapping, Rule, Target};
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

const CAPTURE_TIMEOUT: f64 = 10.0;

struct Inner {
    app: Weak<App>,
    mapping: RefCell<Mapping>,
    rows: BTreeMap<Target, adw::ActionRow>,
    edit_buttons: BTreeMap<Target, gtk::Button>,
    pad: PadView,
    meters: BTreeMap<Target, (gtk::LevelBar, gtk::Label)>,
    capturing: Cell<Option<Target>>,
    save: gtk::Button,
    deadzone: adw::SpinRow,
    invert_ly: adw::SwitchRow,
    invert_ry: adw::SwitchRow,
}

#[derive(Clone)]
pub struct Editor {
    pub id: String,
    inner: Rc<Inner>,
}

pub fn open(app: &Rc<App>, id: &str) {
    let Some(b) = app.backend() else { return };
    let (app, id) = (app.clone(), id.to_owned());
    glib::spawn_future_local(async move {
        match b.mapping(&id).await {
            Ok(m) => {
                let ed = Editor::new(&app, &id, m);
                let page = ed.page();
                *app.editor.borrow_mut() = Some(ed);
                app.nav.push(&page);
            }
            Err(e) => app.toast(&format!("Não consegui ler o mapeamento: {e}")),
        }
    });
}

fn detail(t: Target, r: Option<&Rule>) -> String {
    let src = labels::rule(r);
    match (t, r) {
        (Target::Home, Some(_)) => {
            format!("← {src} · vira o botão Guide (abre o overlay da Steam)")
        }
        (Target::Lt | Target::Rt, Some(Rule::Key { .. })) => format!("← {src} · digital: 0 ou 255"),
        _ => format!("← {src}"),
    }
}

impl Editor {
    fn new(app: &Rc<App>, id: &str, mapping: Mapping) -> Editor {
        let pad = PadView::new(320, 210);
        let mut rows = BTreeMap::new();
        let mut edit_buttons = BTreeMap::new();
        for t in Target::ALL {
            let row = adw::ActionRow::builder()
                .title(labels::target(t))
                .subtitle(glib::markup_escape_text(&detail(t, mapping.rules.get(&t))))
                .build();
            row.add_css_class("monospace-subtitle");
            let edit = gtk::Button::builder()
                .icon_name("document-edit-symbolic")
                .valign(gtk::Align::Center)
                .tooltip_text(format!("Trocar: aperte o botão para {}", labels::target(t)))
                .css_classes(["flat"])
                .build();
            row.add_suffix(&edit);
            rows.insert(t, row);
            edit_buttons.insert(t, edit);
        }
        let mut meters = BTreeMap::new();
        for t in [
            Target::Lx,
            Target::Ly,
            Target::Rx,
            Target::Ry,
            Target::Lt,
            Target::Rt,
        ] {
            let bar = gtk::LevelBar::builder()
                .min_value(0.0)
                .max_value(1.0)
                .hexpand(true)
                .valign(gtk::Align::Center)
                .build();
            bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_LOW));
            bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_HIGH));
            bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_FULL));
            let val = gtk::Label::builder()
                .width_chars(6)
                .xalign(1.0)
                .css_classes(["monospace", "caption"])
                .build();
            meters.insert(t, (bar, val));
        }
        let save = gtk::Button::builder()
            .label("Salvar")
            .css_classes(["suggested-action"])
            .sensitive(false)
            .build();
        let deadzone = adw::SpinRow::builder()
            .title("Zona morta")
            .subtitle("Ignora pequenos movimentos perto do centro (%)")
            .adjustment(&gtk::Adjustment::new(
                (mapping.options.deadzone * 100.0).round(),
                0.0,
                50.0,
                1.0,
                5.0,
                0.0,
            ))
            .build();
        let invert_ly = adw::SwitchRow::builder()
            .title("Inverter eixo Y do analógico esquerdo")
            .active(mapping.options.invert_ly)
            .build();
        let invert_ry = adw::SwitchRow::builder()
            .title("Inverter eixo Y do analógico direito")
            .active(mapping.options.invert_ry)
            .build();
        let ed = Editor {
            id: id.to_owned(),
            inner: Rc::new(Inner {
                app: Rc::downgrade(app),
                mapping: RefCell::new(mapping),
                rows,
                edit_buttons,
                pad,
                meters,
                capturing: Cell::new(None),
                save,
                deadzone,
                invert_ly,
                invert_ry,
            }),
        };
        for (t, b) in &ed.inner.edit_buttons {
            let (e, t) = (ed.clone(), *t);
            b.connect_clicked(move |_| e.start_capture(t));
        }
        let e = ed.clone();
        ed.inner.deadzone.connect_value_notify(move |r| {
            e.inner.mapping.borrow_mut().options.deadzone = (r.value() / 100.0).clamp(0.0, 0.5);
            e.dirty();
        });
        let e = ed.clone();
        ed.inner.invert_ly.connect_active_notify(move |r| {
            e.inner.mapping.borrow_mut().options.invert_ly = r.is_active();
            e.dirty();
        });
        let e = ed.clone();
        ed.inner.invert_ry.connect_active_notify(move |r| {
            e.inner.mapping.borrow_mut().options.invert_ry = r.is_active();
            e.dirty();
        });
        let e = ed.clone();
        ed.inner.save.connect_clicked(move |_| e.save());
        ed
    }

    fn app(&self) -> Option<Rc<App>> {
        self.inner.app.upgrade()
    }

    fn dirty(&self) {
        self.inner.save.set_sensitive(true);
    }

    fn page(&self) -> adw::NavigationPage {
        let header = adw::HeaderBar::new();
        header.pack_end(&self.inner.save);
        let menu = gio::Menu::new();
        menu.append(Some("Exportar…"), Some("editor.export"));
        menu.append(Some("Restaurar o perfil oficial"), Some("editor.reset"));
        header.pack_end(
            &gtk::MenuButton::builder()
                .icon_name("view-more-symbolic")
                .menu_model(&menu)
                .tooltip_text("Mais opções")
                .build(),
        );

        // Controle ao vivo e medidores.
        let live = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        live.append(&self.inner.pad.area);
        let grid = gtk::Grid::builder()
            .column_spacing(10)
            .row_spacing(6)
            .margin_start(12)
            .margin_end(12)
            .build();
        for (i, (t, (bar, val))) in self.inner.meters.iter().enumerate() {
            let col = (i % 2) as i32 * 3;
            let row = (i / 2) as i32;
            let name = gtk::Label::builder()
                .label(t.name())
                .css_classes(["monospace", "caption", "dim-label"])
                .build();
            grid.attach(&name, col, row, 1, 1);
            grid.attach(bar, col + 1, row, 1, 1);
            grid.attach(val, col + 2, row, 1, 1);
        }
        live.append(&grid);
        let live_group = adw::PreferencesGroup::new();
        live_group.add(&live);

        let page = adw::PreferencesPage::new();
        page.add(&live_group);
        let mapping = self.inner.mapping.borrow();
        let profile = self
            .app()
            .and_then(|a| a.current())
            .and_then(|c| c.profile_name)
            .unwrap_or_default();
        let _ = mapping;
        for (title, targets) in [
            (
                "Botões",
                vec![
                    Target::A,
                    Target::B,
                    Target::X,
                    Target::Y,
                    Target::Lb,
                    Target::Rb,
                    Target::Back,
                    Target::Start,
                    Target::Ls,
                    Target::Rs,
                    Target::Home,
                ],
            ),
            (
                "D-pad e gatilhos",
                vec![
                    Target::DpadUp,
                    Target::DpadDown,
                    Target::DpadLeft,
                    Target::DpadRight,
                    Target::Lt,
                    Target::Rt,
                ],
            ),
            (
                "Analógicos",
                vec![Target::Lx, Target::Ly, Target::Rx, Target::Ry],
            ),
        ] {
            let g = adw::PreferencesGroup::builder().title(title).build();
            if title == "Botões" && !profile.is_empty() {
                g.set_description(Some(&format!(
                    "Perfil: {profile}. Para trocar, clique no lápis e aperte o botão físico."
                )));
            }
            for t in targets {
                g.add(&self.inner.rows[&t]);
            }
            if title == "Analógicos" {
                g.add(&self.inner.deadzone);
                g.add(&self.inner.invert_ly);
                g.add(&self.inner.invert_ry);
            }
            page.add(&g);
        }

        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&page));
        let nav_page = adw::NavigationPage::builder()
            .title("Mapeamento")
            .tag("editor")
            .child(&view)
            .build();

        let actions = gio::SimpleActionGroup::new();
        let export = gio::SimpleAction::new("export", None);
        let e = self.clone();
        export.connect_activate(move |_, _| e.export());
        actions.add_action(&export);
        let reset = gio::SimpleAction::new("reset", None);
        let e = self.clone();
        reset.connect_activate(move |_, _| e.reset());
        actions.add_action(&reset);
        nav_page.insert_action_group("editor", Some(&actions));

        let e = self.clone();
        nav_page.connect_hidden(move |_| {
            if let Some(app) = e.app() {
                let mine = app.editor.borrow().as_ref().is_some_and(|x| x.id == e.id);
                if mine && app.nav.find_page("editor").is_none() {
                    *app.editor.borrow_mut() = None;
                }
            }
        });
        nav_page
    }

    pub fn set_live(&self, live: &LiveState) {
        let i = &self.inner;
        i.pad.set_live(live);
        for (t, (bar, val)) in &i.meters {
            let v = *live.axes.get(t).unwrap_or(&0);
            let (frac, text) = if t.kind() == TargetKind::Trigger {
                (f64::from(v) / 255.0, format!("{v}"))
            } else {
                let x = f64::from(v) / 32767.0;
                ((x + 1.0) / 2.0, format!("{x:+.2}").replace('.', ","))
            };
            bar.set_value(frac.clamp(0.0, 1.0));
            val.set_text(&text);
        }
        for (t, row) in &i.rows {
            if live.active.contains(t) {
                row.add_css_class("live-row");
            } else {
                row.remove_css_class("live-row");
            }
        }
    }

    fn start_capture(&self, t: Target) {
        let (Some(app), None) = (self.app(), self.inner.capturing.get()) else {
            return;
        };
        let Some(b) = app.backend() else { return };
        self.inner.capturing.set(Some(t));
        self.inner.rows[&t].set_subtitle("Aperte o botão agora… (10 s)");
        self.inner.rows[&t].add_css_class("capturing-row");
        for btn in self.inner.edit_buttons.values() {
            btn.set_sensitive(false);
        }
        let (e, id) = (self.clone(), self.id.clone());
        glib::spawn_future_local(async move {
            if let Err(err) = b.capture(&id, t, CAPTURE_TIMEOUT).await {
                app.toast(&err);
                e.end_capture();
            }
        });
    }

    fn end_capture(&self) {
        if let Some(t) = self.inner.capturing.take() {
            let r = self.inner.mapping.borrow().rules.get(&t).copied();
            self.inner.rows[&t].set_subtitle(&glib::markup_escape_text(&detail(t, r.as_ref())));
            self.inner.rows[&t].remove_css_class("capturing-row");
        }
        for btn in self.inner.edit_buttons.values() {
            btn.set_sensitive(true);
        }
    }

    pub fn captured(&self, c: &Captured) {
        if self.inner.capturing.get() != Some(c.target) {
            return;
        }
        match c.rule {
            Some(rule) => {
                self.inner.mapping.borrow_mut().rules.insert(c.target, rule);
                self.dirty();
                if let Some(app) = self.app() {
                    app.toast(&format!(
                        "{} ← {}",
                        labels::target(c.target),
                        labels::rule(Some(&rule))
                    ));
                }
            }
            None => {
                if let Some(app) = self.app() {
                    app.toast("Nada chegou em 10 s; mantive como estava");
                }
            }
        }
        self.end_capture();
    }

    fn save(&self) {
        let Some(app) = self.app() else { return };
        let Some(b) = app.backend() else { return };
        let m = self.inner.mapping.borrow().clone();
        let (e, id) = (self.clone(), self.id.clone());
        self.inner.save.set_sensitive(false);
        glib::spawn_future_local(async move {
            match b.save_mapping(&id, &m).await {
                Ok(_) => app.toast("Mapeamento salvo"),
                Err(err) => {
                    app.toast(&format!("Não salvou: {err}"));
                    e.dirty();
                }
            }
        });
    }

    fn reset(&self) {
        let Some(app) = self.app() else { return };
        let Some(b) = app.backend() else { return };
        let id = self.id.clone();
        glib::spawn_future_local(async move {
            if let Err(e) = b.reset_mapping(&id).await {
                app.toast(&e);
                return;
            }
            app.nav.pop_to_tag("controller");
            *app.editor.borrow_mut() = None;
            app.toast("Perfil oficial restaurado");
            open(&app, &id);
        });
    }

    fn export(&self) {
        let Some(app) = self.app() else { return };
        let json = self.inner.mapping.borrow().to_json_pretty() + "\n";
        let dialog = gtk::FileDialog::builder()
            .title("Exportar mapeamento")
            .initial_name("mapping.json")
            .build();
        let window = app.window.clone();
        glib::spawn_future_local(async move {
            if let Ok(file) = dialog.save_future(Some(&window)).await {
                let r = file
                    .replace_contents_future(
                        json.into_bytes(),
                        None,
                        false,
                        gio::FileCreateFlags::NONE,
                    )
                    .await;
                match r {
                    Ok(_) => app.toast("Mapeamento exportado"),
                    Err((_, e)) => app.toast(&format!("Não exportou: {e}")),
                }
            }
        });
    }
}
