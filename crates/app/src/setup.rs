//! Primeira execução: confere o que falta e instala o serviço com uma senha (pkexec).
//! Substitui o `sudo ./install.sh`; volta a aparecer sozinha se algo sumir.

use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub fn in_appimage() -> bool {
    std::env::var_os("APPIMAGE").is_some() || std::env::var_os("APPDIR").is_some()
}

/// A linha de comando que faz a instalação: a do AppImage, a ao lado deste executável ou a do PATH.
pub fn cli_path() -> PathBuf {
    if let Some(appdir) = std::env::var_os("APPDIR") {
        let p = PathBuf::from(appdir).join("usr/bin/gamesir-bridge");
        if p.exists() {
            return p;
        }
    }
    if let Some(p) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("gamesir-bridge")))
        .filter(|p| p.exists())
    {
        return p;
    }
    PathBuf::from("gamesir-bridge")
}

fn any_exists(paths: &[&str]) -> bool {
    paths.iter().any(|p| Path::new(p).exists())
}

type Callback = Rc<RefCell<Option<Box<dyn Fn()>>>>;

struct Check {
    row: adw::ActionRow,
    icon: gtk::Image,
}

impl Check {
    fn new(title: &str, subtitle: &str) -> Check {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .build();
        let icon = gtk::Image::new();
        row.add_suffix(&icon);
        Check { row, icon }
    }

    fn set(&self, ok: bool, ok_text: &str) {
        if ok {
            self.icon.set_icon_name(Some("emblem-ok-symbolic"));
            self.icon.set_css_classes(&["success"]);
            self.icon.set_tooltip_text(Some(ok_text));
        } else {
            self.icon.set_icon_name(Some("dialog-warning-symbolic"));
            self.icon.set_css_classes(&["warning"]);
            self.icon.set_tooltip_text(Some("Falta"));
        }
    }
}

#[derive(Clone)]
pub struct SetupPage {
    pub widget: gtk::Widget,
    service: Rc<Check>,
    udev: Rc<Check>,
    running: Rc<Check>,
    error: gtk::Label,
    button: gtk::Button,
    spinner: gtk::Spinner,
    done: Callback,
}

impl SetupPage {
    pub fn new() -> SetupPage {
        let service = Rc::new(Check::new(
            "Serviço em segundo plano",
            "Mantém a ponte ativa mesmo com o app fechado",
        ));
        let udev = Rc::new(Check::new(
            "Regra udev e módulo uinput",
            "Esconde o controle original e libera o virtual",
        ));
        let running = Rc::new(Check::new(
            "Serviço rodando",
            "O app conversa com ele pelo D-Bus",
        ));
        let group = adw::PreferencesGroup::new();
        group.add(&service.row);
        group.add(&udev.row);
        group.add(&running.row);

        let error = gtk::Label::builder()
            .wrap(true)
            .visible(false)
            .css_classes(["error", "caption"])
            .build();
        let button = gtk::Button::builder()
            .label("Configurar")
            .halign(gtk::Align::Center)
            .css_classes(["pill", "suggested-action"])
            .build();
        let spinner = gtk::Spinner::builder()
            .spinning(true)
            .visible(false)
            .build();
        let hint = gtk::Label::builder()
            .label("O sistema vai pedir sua senha uma vez.")
            .css_classes(["dim-label", "caption"])
            .build();
        let retry = gtk::Button::builder()
            .label("Tentar de novo")
            .halign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        content.append(&group);
        content.append(&error);
        content.append(&button);
        content.append(&spinner);
        content.append(&hint);
        content.append(&retry);
        let clamp = adw::Clamp::builder()
            .maximum_size(480)
            .child(&content)
            .build();

        let status = adw::StatusPage::builder()
            .icon_name("input-gaming-symbolic")
            .title("Vamos preparar seu Linux")
            .description(
                "Três ajustes deixam o controle aparecer como um Xbox 360 em qualquer jogo. \
                 Leva menos de um minuto.",
            )
            .child(&clamp)
            .vexpand(true)
            .build();
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
        view.set_content(Some(&status));

        let page = SetupPage {
            widget: view.upcast(),
            service,
            udev,
            running,
            error,
            button,
            spinner,
            done: Rc::new(RefCell::new(None)),
        };
        let p = page.clone();
        page.button.connect_clicked(move |_| p.run_setup());
        let p = page.clone();
        retry.connect_clicked(move |_| {
            if let Some(f) = &*p.done.borrow() {
                f();
            }
        });
        page
    }

    pub fn connect_done(&self, f: impl Fn() + 'static) {
        *self.done.borrow_mut() = Some(Box::new(f));
    }

    /// Atualiza os itens conferindo o sistema. `err` é o motivo de não ter conectado.
    pub fn update(&self, err: Option<&str>) {
        let installed = any_exists(&[
            "/etc/gamesir-bridge/installed-files",
            "/usr/lib/systemd/system/gamesir-bridge.service",
            "/etc/systemd/system/gamesir-bridge.service",
        ]);
        let udev = any_exists(&[
            "/etc/udev/rules.d/65-gamesir-bridge.rules",
            "/usr/lib/udev/rules.d/65-gamesir-bridge.rules",
        ]) && Path::new("/dev/uinput").exists();
        self.service.set(installed, "Instalado");
        self.udev.set(udev, "Instalada");
        self.running.set(false, "");
        self.button.set_label(if installed {
            "Reinstalar"
        } else {
            "Configurar"
        });
        match err {
            Some(e) if installed => {
                self.error.set_text(&format!(
                    "O serviço está instalado, mas não respondeu ({e}). \
                     Veja: journalctl -u gamesir-bridge"
                ));
                self.error.set_visible(true);
            }
            _ => self.error.set_visible(false),
        }
    }

    fn run_setup(&self) {
        self.button.set_sensitive(false);
        self.spinner.set_visible(true);
        self.error.set_visible(false);
        let p = self.clone();
        glib::spawn_future_local(async move {
            let cli = cli_path();
            let result = async {
                let proc = gio::Subprocess::newv(
                    &[cli.as_os_str(), "setup".as_ref()],
                    gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE,
                )
                .map_err(|e| e.to_string())?;
                let (out, _) = proc
                    .communicate_utf8_future(None)
                    .await
                    .map_err(|e| e.to_string())?;
                if proc.is_successful() {
                    Ok(())
                } else {
                    let out = out.map(|s| s.to_string()).unwrap_or_default();
                    let last = out
                        .lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("");
                    Err(last.trim_start_matches("erro: ").to_owned())
                }
            }
            .await;
            p.spinner.set_visible(false);
            p.button.set_sensitive(true);
            match result {
                Ok(()) => {
                    // O serviço leva um instante para publicar o nome no D-Bus.
                    glib::timeout_future(std::time::Duration::from_millis(800)).await;
                    if let Some(f) = &*p.done.borrow() {
                        f();
                    }
                }
                Err(e) => {
                    p.error.set_text(&format!("Não deu certo: {e}"));
                    p.error.set_visible(true);
                }
            }
        });
    }
}
