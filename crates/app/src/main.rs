//! App do GameSir Bridge. Não lê controle nenhum: conversa com o serviço `gamesir-bridged`
//! pelo D-Bus. `GAMESIR_BRIDGE_DEMO=1` abre com controles simulados (útil para screenshots).

mod backend;
mod calibration;
mod editor;
mod labels;
mod pad;
mod prefs;
mod setup;
mod window;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

const APP_ID: &str = "io.github.yurisanp.GameSirBridge";

const CSS: &str = r#"
@define-color accent_bg_color #4B57E3;
@define-color accent_color #4B57E3;
@define-color accent_fg_color #ffffff;

.monospace-subtitle subtitle,
.monospace-subtitle .subtitle { font-family: monospace; font-size: 0.9em; }

row.live-row { background-color: alpha(@accent_bg_color, 0.14); }
row.capturing-row { background-color: alpha(@warning_bg_color, 0.18); }

.big-instruction { font-size: 1.6em; font-weight: 800; }

"#;

const CSS_DARK: &str = r#"
@define-color accent_bg_color #5C67F0;
@define-color accent_color #8A95FF;
"#;

fn load_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let base = gtk::CssProvider::new();
    base.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &display,
        &base,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let dark = gtk::CssProvider::new();
    let style = adw::StyleManager::default();
    let d = dark.clone();
    let apply = move |s: &adw::StyleManager| {
        d.load_from_string(if s.is_dark() { CSS_DARK } else { "" });
    };
    apply(&style);
    gtk::style_context_add_provider_for_display(
        &display,
        &dark,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
    );
    style.connect_dark_notify(apply);
}

fn about(app: &std::rc::Rc<window::App>) {
    let about = adw::AboutDialog::builder()
        .application_name("GameSir Bridge")
        .application_icon(APP_ID)
        .developer_name("Yuri Pereira")
        .version(gamesir_core::VERSION)
        .website("https://github.com/yurisanp/gamesir-bridge")
        .issue_url("https://github.com/yurisanp/gamesir-bridge/issues")
        .license_type(gtk::License::MitX11)
        .comments(
            "Faz controles como o GameSir T4 Cyclone Pro aparecerem como um Xbox 360 no Linux.",
        )
        .build();
    about.present(Some(&app.window));
}

/// Só no modo demo: `GAMESIR_BRIDGE_DEMO_PAGE=editor|calibration|preferences|setup`
/// abre direto numa tela, para gerar as imagens do README.
fn demo_page(app: &std::rc::Rc<window::App>) {
    if std::env::var_os("GAMESIR_BRIDGE_DEMO").is_none() {
        return;
    }
    let Ok(page) = std::env::var("GAMESIR_BRIDGE_DEMO_PAGE") else {
        return;
    };
    let app = app.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
        let id = app.selected.borrow().clone().unwrap_or_default();
        match page.as_str() {
            "editor" => editor::open(&app, &id),
            "calibration" => calibration::open(&app, &id),
            "preferences" => app.show_preferences(),
            "setup" => app.show_setup(),
            _ => {}
        }
    });
}

fn main() -> glib::ExitCode {
    // Responde sem abrir o GTK: o AppRun usa isso para ver se o app roda neste sistema.
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("gamesir-bridge-app {}", gamesir_core::VERSION);
        return glib::ExitCode::SUCCESS;
    }
    let application = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::default())
        .build();

    application.connect_startup(|_| {
        load_css();
        prefs::apply_theme();
        gtk::Window::set_default_icon_name(APP_ID);
    });

    application.connect_activate(|application| {
        if let Some(w) = application.active_window() {
            w.present();
            return;
        }
        let app = window::App::new(application);

        let preferences = gio::SimpleAction::new("preferences", None);
        let a = app.clone();
        preferences.connect_activate(move |_, _| a.show_preferences());
        application.add_action(&preferences);
        let about_action = gio::SimpleAction::new("about", None);
        let a = app.clone();
        about_action.connect_activate(move |_, _| about(&a));
        application.add_action(&about_action);
        let quit = gio::SimpleAction::new("quit", None);
        let w = app.window.clone();
        quit.connect_activate(move |_, _| w.close());
        application.add_action(&quit);
        application.set_accels_for_action("app.preferences", &["<Ctrl>comma"]);
        application.set_accels_for_action("app.quit", &["<Ctrl>q"]);

        app.window.present();
        app.clone().connect();
        demo_page(&app);
    });

    application.run()
}
