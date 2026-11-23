//! Preferências: poucas opções, agrupadas como no GNOME. O que é por controle (zona morta,
//! inverter eixo) fica no editor.

use crate::setup;
use crate::window::App;
use adw::prelude::*;
use gtk::{gio, glib};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// `system`, `light` ou `dark`.
    pub theme: String,
    pub notify: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            theme: "system".into(),
            notify: true,
        }
    }
}

thread_local! {
    static SETTINGS: RefCell<AppSettings> = RefCell::new(load());
}

fn path() -> PathBuf {
    glib::user_config_dir().join("gamesir-bridge/app.json")
}

fn load() -> AppSettings {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn get() -> AppSettings {
    SETTINGS.with(|s| s.borrow().clone())
}

fn update(f: impl FnOnce(&mut AppSettings)) {
    SETTINGS.with(|s| {
        f(&mut s.borrow_mut());
        let p = path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(
            p,
            serde_json::to_string_pretty(&*s.borrow()).unwrap_or_default(),
        );
    });
}

pub fn apply_theme() {
    let scheme = match get().theme.as_str() {
        "light" => adw::ColorScheme::ForceLight,
        "dark" => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    };
    adw::StyleManager::default().set_color_scheme(scheme);
}

fn run_cli(app: &Rc<App>, args: &'static [&'static str], ok: &'static str) {
    let app = app.clone();
    glib::spawn_future_local(async move {
        let cli = setup::cli_path();
        let mut argv: Vec<&std::ffi::OsStr> = vec![cli.as_os_str()];
        argv.extend(args.iter().map(std::ffi::OsStr::new));
        let r = match gio::Subprocess::newv(
            &argv,
            gio::SubprocessFlags::STDERR_MERGE | gio::SubprocessFlags::STDOUT_PIPE,
        ) {
            Ok(p) => match p.wait_future().await {
                Ok(()) if p.is_successful() => Ok(()),
                Ok(()) => Err("o instalador falhou ou foi cancelado".to_owned()),
                Err(e) => Err(e.to_string()),
            },
            Err(e) => Err(e.to_string()),
        };
        match r {
            Ok(()) => {
                app.toast(ok);
                glib::timeout_future(std::time::Duration::from_millis(800)).await;
                app.clone().connect();
            }
            Err(e) => app.toast(&e),
        }
    });
}

pub fn open(app: &Rc<App>) {
    let dialog = adw::PreferencesDialog::new();

    // Geral.
    let general = adw::PreferencesPage::builder()
        .title("Geral")
        .icon_name("preferences-system-symbolic")
        .build();
    let behavior = adw::PreferencesGroup::builder()
        .title("Comportamento")
        .build();
    let start = adw::ActionRow::builder()
        .title("Iniciar com o sistema")
        .subtitle("A ponte funciona sem abrir o app (serviço gamesir-bridge)")
        .build();
    start.add_suffix(&gtk::Image::from_icon_name("emblem-ok-symbolic"));
    behavior.add(&start);
    let notify = adw::SwitchRow::builder()
        .title("Notificar ao conectar")
        .active(get().notify)
        .build();
    notify.connect_active_notify(|r| update(|s| s.notify = r.is_active()));
    behavior.add(&notify);
    general.add(&behavior);

    let output = adw::PreferencesGroup::builder().title("Saída").build();
    let kind = adw::ActionRow::builder()
        .title("Tipo de controle virtual")
        .subtitle("Xbox 360 (045e:028e), que Steam, SDL e jogos já conhecem")
        .build();
    output.add(&kind);
    let home = adw::ActionRow::builder()
        .title("Botão Home")
        .subtitle("Guide (abre o overlay da Steam)")
        .build();
    output.add(&home);
    general.add(&output);

    let look = adw::PreferencesGroup::builder().title("Aparência").build();
    let theme = adw::ComboRow::builder()
        .title("Tema")
        .model(&gtk::StringList::new(&["Sistema", "Claro", "Escuro"]))
        .selected(match get().theme.as_str() {
            "light" => 1,
            "dark" => 2,
            _ => 0,
        })
        .build();
    theme.connect_selected_notify(|r| {
        let t = ["system", "light", "dark"][r.selected().min(2) as usize];
        update(|s| s.theme = t.into());
        apply_theme();
    });
    look.add(&theme);
    general.add(&look);
    dialog.add(&general);

    // Avançado.
    let adv = adw::PreferencesPage::builder()
        .title("Avançado")
        .icon_name("applications-engineering-symbolic")
        .build();
    let svc = adw::PreferencesGroup::builder()
        .title("Serviço")
        .description("O serviço roda como root e é o único que lê os controles.")
        .build();
    let version = adw::ActionRow::builder()
        .title("Versão do serviço")
        .subtitle("…")
        .build();
    version.add_css_class("property");
    svc.add(&version);
    if let Some(b) = app.backend() {
        let v = version.clone();
        glib::spawn_future_local(async move {
            let text = match b.version().await {
                Ok(s) if s == gamesir_core::VERSION => s,
                Ok(s) => format!("{s} (o app é {})", gamesir_core::VERSION),
                Err(e) => e,
            };
            v.set_subtitle(&text);
        });
    }
    let reinstall = adw::ActionRow::builder()
        .title("Reinstalar ou atualizar o serviço")
        .subtitle("Pede sua senha e grava a versão deste app no sistema")
        .activatable(true)
        .build();
    reinstall.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let a = app.clone();
    reinstall.connect_activated(move |_| run_cli(&a, &["setup"], "Serviço instalado"));
    svc.add(&reinstall);
    let remove = adw::ActionRow::builder()
        .title("Remover do sistema")
        .subtitle("Tira o serviço, a regra udev e as políticas")
        .activatable(true)
        .css_classes(["error"])
        .build();
    remove.add_suffix(&gtk::Image::from_icon_name("user-trash-symbolic"));
    let a = app.clone();
    let dlg = dialog.clone();
    remove.connect_activated(move |_| {
        let confirm = adw::AlertDialog::builder()
            .heading("Remover o GameSir Bridge do sistema?")
            .body("O serviço, a regra udev e as políticas são removidos. Seus mapeamentos em /etc/gamesir-bridge ficam.")
            .build();
        confirm.add_responses(&[("cancel", "Cancelar"), ("remove", "Remover")]);
        confirm.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        let a = a.clone();
        confirm.connect_response(Some("remove"), move |_, _| {
            run_cli(&a, &["setup", "--uninstall"], "Removido do sistema");
        });
        confirm.present(Some(&dlg));
    });
    svc.add(&remove);
    adv.add(&svc);
    dialog.add(&adv);

    dialog.present(Some(&app.window));
}

#[cfg(test)]
mod tests {
    use super::AppSettings;

    #[test]
    fn missing_fields_use_defaults() {
        let s: AppSettings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(s.theme, "dark");
        assert!(s.notify);
        let s: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.theme, "system");
    }
}
