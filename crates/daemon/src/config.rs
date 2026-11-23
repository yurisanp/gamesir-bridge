//! Onde ficam perfis, mapeamentos e preferências, e como são lidos e gravados.
//!
//! ```text
//! /usr/share/gamesir-bridge/profiles/*.json   perfis oficiais (do pacote)
//! /etc/gamesir-bridge/profiles/*.json         perfis do usuário (controles calibrados do zero)
//! /etc/gamesir-bridge/mappings/<perfil>.json  recalibração de um perfil oficial
//! /etc/gamesir-bridge/mapping.json            arquivo do script antigo (ainda é lido)
//! /etc/gamesir-bridge/settings.json           ponte ligada/desligada por controle
//! ```

use crate::sys::write_atomic;
use gamesir_core::mapping::{format_vid_pid, parse_vid_pid, Mapping, MappingError};
use gamesir_core::{Profile, ProfileSet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub system_profiles: PathBuf,
    pub config_dir: PathBuf,
    pub sys_root: PathBuf,
    pub dev_input: PathBuf,
    /// Regra udev gerada para esconder controles com perfil do usuário.
    pub udev_rule: Option<PathBuf>,
}

impl Default for Paths {
    fn default() -> Self {
        Paths {
            system_profiles: PathBuf::from("/usr/share/gamesir-bridge/profiles"),
            config_dir: PathBuf::from("/etc/gamesir-bridge"),
            sys_root: PathBuf::from("/sys"),
            dev_input: PathBuf::from("/dev/input"),
            udev_rule: Some(PathBuf::from(
                "/etc/udev/rules.d/66-gamesir-bridge-custom.rules",
            )),
        }
    }
}

impl Paths {
    pub fn user_profiles(&self) -> PathBuf {
        self.config_dir.join("profiles")
    }
    pub fn overrides(&self) -> PathBuf {
        self.config_dir.join("mappings")
    }
    pub fn override_for(&self, profile_id: &str) -> PathBuf {
        self.overrides().join(format!("{profile_id}.json"))
    }
    pub fn legacy_mapping(&self) -> PathBuf {
        self.config_dir.join("mapping.json")
    }
    pub fn settings(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    pub fn load_profiles(&self) -> ProfileSet {
        let set = ProfileSet::load_dirs(&[&self.system_profiles, &self.user_profiles()]);
        for e in &set.errors {
            log::error!("perfil ignorado: {e}");
        }
        for w in &set.warnings {
            log::warn!("{w}");
        }
        set
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ControllerSettings {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub device: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub controllers: BTreeMap<String, ControllerSettings>,
}

impl Settings {
    /// Lê as preferências. Arquivo ausente ou inválido nunca impede o serviço de subir.
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::error!("{}: {e}; usando preferências padrão", path.display());
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        write_atomic(
            path,
            &(serde_json::to_string_pretty(self).expect("serializa") + "\n"),
        )
    }

    pub fn enabled(&self, id: &str) -> bool {
        self.controllers.get(id).is_none_or(|c| c.enabled)
    }
}

/// O mapeamento que vale para um perfil, e de onde veio.
#[derive(Debug, Clone)]
pub struct Effective {
    pub mapping: Mapping,
    /// `official`, `custom` ou `calibrated`.
    pub source: &'static str,
    /// Mapeamento do usuário que não pôde ser usado (o perfil oficial vale no lugar).
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

/// Resolve o mapeamento de um perfil: recalibração do usuário > `mapping.json` antigo > perfil.
pub fn effective_mapping(paths: &Paths, profile: &Profile) -> Effective {
    let custom = profile
        .source
        .as_deref()
        .is_some_and(|p| p.starts_with(paths.user_profiles()));
    let base = Effective {
        mapping: profile.mapping.clone(),
        source: if custom { "custom" } else { "official" },
        error: None,
        warnings: Vec::new(),
    };

    let mut candidates = vec![paths.override_for(&profile.id)];
    candidates.push(paths.legacy_mapping());
    for (i, path) in candidates.iter().enumerate() {
        if !path.exists() {
            continue;
        }
        let legacy = i == 1;
        match Mapping::load(path) {
            Ok(parsed) => {
                if legacy {
                    // O arquivo antigo só vale para o aparelho dele (o T4, por padrão).
                    let dev = parsed.mapping.device.as_deref().and_then(parse_vid_pid);
                    if !dev.is_some_and(|(v, p)| profile.matches(v, p)) {
                        continue;
                    }
                }
                return Effective {
                    mapping: parsed.mapping,
                    source: "calibrated",
                    error: None,
                    warnings: parsed
                        .warnings
                        .iter()
                        .map(|w| format!("{}: {w}", path.display()))
                        .collect(),
                };
            }
            Err(MappingError::Io { .. }) => continue,
            Err(e) => {
                if legacy {
                    // Não dá para saber de que aparelho era; só avisa.
                    log::error!("{}: {e}", path.display());
                    continue;
                }
                return Effective {
                    error: Some(format!(
                        "{}: {e}. Usando o mapeamento do perfil até corrigir.",
                        path.display()
                    )),
                    ..base
                };
            }
        }
    }
    base
}

/// Grava a recalibração de um perfil (ou o próprio perfil, se for do usuário).
pub fn save_mapping(
    paths: &Paths,
    profile: &Profile,
    mapping: &Mapping,
) -> std::io::Result<PathBuf> {
    let custom = profile
        .source
        .as_deref()
        .is_some_and(|p| p.starts_with(paths.user_profiles()));
    if custom {
        let mut p = profile.clone();
        p.mapping = mapping.clone();
        let path = profile
            .source
            .clone()
            .expect("perfil do usuário tem caminho");
        write_atomic(&path, &p.to_file_json())?;
        return Ok(path);
    }
    let mut m = mapping.clone();
    if m.device.is_none() {
        m.device = profile.devices.first().map(|(v, p)| format_vid_pid(*v, *p));
    }
    let path = paths.override_for(&profile.id);
    write_atomic(&path, &(m.to_json_pretty() + "\n"))?;
    Ok(path)
}

/// Cria um perfil do usuário para um controle que não tinha perfil.
pub fn create_user_profile(
    paths: &Paths,
    vendor: u16,
    product: u16,
    name: &str,
    mapping: &Mapping,
) -> std::io::Result<Profile> {
    let id = format!("custom-{vendor:04x}-{product:04x}");
    let path = paths.user_profiles().join(format!("{id}.json"));
    let mut m = mapping.clone();
    m.device = Some(format_vid_pid(vendor, product));
    let profile = Profile {
        id,
        name: if name.is_empty() {
            format!("Controle {}", format_vid_pid(vendor, product))
        } else {
            name.to_owned()
        },
        devices: vec![(vendor, product)],
        notes: Vec::new(),
        mapping: m,
        source: Some(path.clone()),
    };
    write_atomic(&path, &profile.to_file_json())?;
    Ok(profile)
}

/// Regra udev que esconde o controle cru dos jogos (o mesmo que `65-gamesir-bridge.rules`
/// faz para o T4), para os perfis criados pelo usuário. Vale a partir do próximo replug.
pub fn udev_rules_for(profiles: &[&Profile]) -> String {
    let mut out = String::from(
        "# Gerado pelo gamesir-bridged para controles com perfil do usuário. Não edite.\n",
    );
    for p in profiles {
        for (v, pid) in &p.devices {
            out.push_str(&format!(
                "SUBSYSTEM==\"input\", ATTRS{{idVendor}}==\"{v:04x}\", ATTRS{{idProduct}}==\"{pid:04x}\", \
                 ENV{{ID_INPUT_JOYSTICK}}=\"\", ENV{{LIBINPUT_IGNORE_DEVICE}}=\"1\"\n"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const T4: &str = include_str!("../../../profiles/gamesir-t4-cyclone-pro-dongle.json");

    fn setup() -> (tempfile::TempDir, Paths, Profile) {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths {
            system_profiles: d.path().join("share"),
            config_dir: d.path().join("etc"),
            sys_root: d.path().join("sys"),
            dev_input: d.path().join("dev"),
            udev_rule: None,
        };
        std::fs::create_dir_all(&paths.system_profiles).unwrap();
        std::fs::write(paths.system_profiles.join("t4.json"), T4).unwrap();
        let set = paths.load_profiles();
        let p = set.profiles[0].clone();
        (d, paths, p)
    }

    #[test]
    fn sem_nada_vale_o_oficial() {
        let (_d, paths, p) = setup();
        let e = effective_mapping(&paths, &p);
        assert_eq!(e.source, "official");
        assert!(e.error.is_none());
        assert_eq!(e.mapping.rules.len(), 21);
    }

    #[test]
    fn mapping_json_antigo_ainda_vale() {
        let (_d, paths, p) = setup();
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(
            paths.legacy_mapping(),
            r#"{"device": "3537:1007", "rules": {"A": {"src": "key", "code": 305}}}"#,
        )
        .unwrap();
        let e = effective_mapping(&paths, &p);
        assert_eq!(e.source, "calibrated");
        assert_eq!(e.mapping.rules.len(), 1);
    }

    #[test]
    fn mapping_json_antigo_de_outro_aparelho_e_ignorado() {
        let (_d, paths, p) = setup();
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(
            paths.legacy_mapping(),
            r#"{"device": "1111:2222", "rules": {}}"#,
        )
        .unwrap();
        assert_eq!(effective_mapping(&paths, &p).source, "official");
    }

    #[test]
    fn mapeamento_quebrado_nao_derruba_e_explica() {
        let (_d, paths, p) = setup();
        std::fs::create_dir_all(paths.overrides()).unwrap();
        std::fs::write(paths.override_for(&p.id), "{ isso não é json").unwrap();
        let e = effective_mapping(&paths, &p);
        assert_eq!(e.source, "official");
        assert_eq!(
            e.mapping.rules.len(),
            21,
            "continua funcionando com o oficial"
        );
        assert!(e.error.unwrap().contains("JSON inválido"));
    }

    #[test]
    fn salvar_e_reler() {
        let (_d, paths, p) = setup();
        let mut m = p.mapping.clone();
        m.options.deadzone = 0.08;
        let path = save_mapping(&paths, &p, &m).unwrap();
        assert_eq!(path, paths.override_for(&p.id));
        let e = effective_mapping(&paths, &p);
        assert_eq!(e.source, "calibrated");
        assert_eq!(e.mapping.options.deadzone, 0.08);
    }

    #[test]
    fn perfil_do_usuario() {
        let (_d, paths, p) = setup();
        let prof = create_user_profile(&paths, 0x2dc8, 0x3106, "8BitDo", &p.mapping).unwrap();
        let set = paths.load_profiles();
        let loaded = set.find(0x2dc8, 0x3106).unwrap();
        assert_eq!(loaded.id, prof.id);
        assert_eq!(effective_mapping(&paths, loaded).source, "custom");
        let rules = udev_rules_for(&[loaded]);
        assert!(rules.contains("ATTRS{idVendor}==\"2dc8\""));
    }

    #[test]
    fn preferencias() {
        let (_d, paths, _) = setup();
        let mut s = Settings::load(&paths.settings());
        assert!(s.enabled("x"));
        s.controllers.insert(
            "x".into(),
            ControllerSettings {
                enabled: false,
                ..Default::default()
            },
        );
        s.save(&paths.settings()).unwrap();
        assert!(!Settings::load(&paths.settings()).enabled("x"));
        std::fs::write(paths.settings(), "lixo").unwrap();
        assert!(Settings::load(&paths.settings()).enabled("x"));
    }
}
