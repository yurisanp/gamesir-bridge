//! Perfis de controle: um JSON por controle suportado.
//!
//! Um perfil é um `mapping.json` com três campos a mais (`id`, `name`, `notes`), então
//! contribuir com um controle novo é mandar um arquivo para `profiles/`.

use crate::mapping::{parse_vid_pid, Issue, Mapping, MappingError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// Pares VID:PID atendidos por este perfil.
    pub devices: Vec<(u16, u16)>,
    #[serde(default)]
    pub notes: Vec<String>,
    pub mapping: Mapping,
    /// De onde o perfil foi lido (vazio para perfis montados em memória).
    #[serde(default)]
    pub source: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("{path}: {source}")]
    Mapping {
        path: String,
        #[source]
        source: MappingError,
    },
    #[error("{path}: {message}")]
    Field { path: String, message: String },
}

#[derive(Deserialize)]
struct Header {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    devices: Vec<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    notes: Vec<String>,
}

impl Profile {
    /// Lê um perfil. `fallback_id` é usado quando o arquivo não tem `id` (o nome do arquivo).
    pub fn from_json(
        text: &str,
        fallback_id: &str,
        origin: &str,
    ) -> Result<(Profile, Vec<Issue>), ProfileError> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| ProfileError::Mapping {
                path: origin.into(),
                source: MappingError::Json(e.to_string()),
            })?;
        let header: Header =
            serde_json::from_value(value.clone()).map_err(|e| ProfileError::Field {
                path: origin.into(),
                message: e.to_string(),
            })?;
        let parsed = Mapping::from_value(value).map_err(|source| ProfileError::Mapping {
            path: origin.into(),
            source,
        })?;

        let mut ids: Vec<String> = header.devices;
        if let Some(d) = header.device {
            ids.insert(0, d);
        }
        let mut devices = Vec::new();
        for d in &ids {
            match parse_vid_pid(d) {
                Some(p) => devices.push(p),
                None => {
                    return Err(ProfileError::Field {
                        path: origin.into(),
                        message: format!("dispositivo {d:?} não está no formato vvvv:pppp"),
                    })
                }
            }
        }
        if devices.is_empty() {
            return Err(ProfileError::Field {
                path: origin.into(),
                message: "perfil sem \"device\" (vvvv:pppp)".into(),
            });
        }
        let id = header.id.unwrap_or_else(|| fallback_id.to_owned());
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ProfileError::Field {
                path: origin.into(),
                message: format!("id {id:?} inválido (use letras, números, - e _)"),
            });
        }
        let name = header
            .name
            .or(header.description.clone())
            .unwrap_or_else(|| id.clone());
        Ok((
            Profile {
                id,
                name,
                devices,
                notes: header.notes,
                mapping: parsed.mapping,
                source: None,
            },
            parsed.warnings,
        ))
    }

    pub fn load(path: &Path) -> Result<(Profile, Vec<Issue>), ProfileError> {
        let origin = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|e| ProfileError::Field {
            path: origin.clone(),
            message: e.to_string(),
        })?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("perfil");
        let (mut p, w) = Profile::from_json(&text, stem, &origin)?;
        p.source = Some(path.to_owned());
        Ok((p, w))
    }

    pub fn matches(&self, vid: u16, pid: u16) -> bool {
        self.devices.contains(&(vid, pid))
    }

    /// Serializa no formato de arquivo de perfil (o mesmo de `profiles/*.json`).
    pub fn to_file_json(&self) -> String {
        let mut obj = serde_json::Map::new();
        obj.insert("id".into(), self.id.clone().into());
        obj.insert("name".into(), self.name.clone().into());
        let devs: Vec<String> = self
            .devices
            .iter()
            .map(|(v, p)| crate::mapping::format_vid_pid(*v, *p))
            .collect();
        obj.insert("device".into(), devs[0].clone().into());
        if devs.len() > 1 {
            obj.insert("devices".into(), devs[1..].to_vec().into());
        }
        if let Some(d) = &self.mapping.description {
            obj.insert("description".into(), d.clone().into());
        }
        if !self.notes.is_empty() {
            obj.insert("notes".into(), self.notes.clone().into());
        }
        let m = serde_json::to_value(&self.mapping).expect("serializa");
        obj.insert("rules".into(), m["rules"].clone());
        if let Some(o) = m.get("options") {
            obj.insert("options".into(), o.clone());
        }
        serde_json::to_string_pretty(&serde_json::Value::Object(obj)).expect("serializa") + "\n"
    }
}

/// Conjunto de perfis carregados de vários diretórios; os últimos sobrescrevem os primeiros (por `id`).
#[derive(Debug, Clone, Default)]
pub struct ProfileSet {
    pub profiles: Vec<Profile>,
    /// Arquivos que não puderam ser lidos, com o motivo. Não impedem os outros perfis.
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ProfileSet {
    pub fn load_dirs<P: AsRef<Path>>(dirs: &[P]) -> ProfileSet {
        let mut set = ProfileSet::default();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir.as_ref()) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect();
            paths.sort();
            for path in paths {
                match Profile::load(&path) {
                    Ok((p, warns)) => {
                        set.warnings
                            .extend(warns.iter().map(|w| format!("{}: {w}", path.display())));
                        set.insert(p);
                    }
                    Err(e) => set.errors.push(e.to_string()),
                }
            }
        }
        set
    }

    pub fn insert(&mut self, p: Profile) {
        self.profiles.retain(|q| q.id != p.id);
        self.profiles.push(p);
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// O perfil que atende um VID:PID. Em caso de empate vale o carregado por último.
    pub fn find(&self, vid: u16, pid: u16) -> Option<&Profile> {
        self.profiles.iter().rev().find(|p| p.matches(vid, pid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T4: &str = include_str!("../../../profiles/gamesir-t4-cyclone-pro-dongle.json");

    #[test]
    fn perfil_do_t4() {
        let (p, w) = Profile::from_json(T4, "x", "t4").unwrap();
        assert!(w.is_empty());
        assert_eq!(p.id, "gamesir-t4-cyclone-pro-dongle");
        assert!(p.matches(0x3537, 0x1007));
        assert_eq!(p.mapping.rules.len(), 21);
    }

    #[test]
    fn mapping_antigo_vira_perfil_com_id_do_arquivo() {
        let text = r#"{"device": "2dc8:3106", "rules": {"A": {"src": "key", "code": 304}}}"#;
        let (p, _) = Profile::from_json(text, "meu-controle", "x").unwrap();
        assert_eq!(p.id, "meu-controle");
        assert_eq!(p.name, "meu-controle");
    }

    #[test]
    fn perfil_sem_device_e_erro() {
        assert!(Profile::from_json(r#"{"rules": {}}"#, "x", "x").is_err());
        assert!(Profile::from_json(r#"{"device": "zz", "rules": {}}"#, "x", "x").is_err());
    }

    #[test]
    fn arquivo_ida_e_volta() {
        let (p, _) = Profile::from_json(T4, "x", "t4").unwrap();
        let (q, _) = Profile::from_json(&p.to_file_json(), "y", "t4").unwrap();
        assert_eq!(p, q);
    }

    #[test]
    fn diretorios_com_sobrescrita_e_arquivo_ruim() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("t4.json"), T4).unwrap();
        let mut over: serde_json::Value = serde_json::from_str(T4).unwrap();
        over["name"] = "Meu T4".into();
        std::fs::write(b.path().join("t4.json"), over.to_string()).unwrap();
        std::fs::write(b.path().join("quebrado.json"), "{").unwrap();
        std::fs::write(b.path().join("leia-me.txt"), "não é perfil").unwrap();

        let set = ProfileSet::load_dirs(&[a.path(), b.path()]);
        assert_eq!(set.profiles.len(), 1);
        assert_eq!(set.find(0x3537, 0x1007).unwrap().name, "Meu T4");
        assert_eq!(set.errors.len(), 1);
        assert!(set.find(1, 2).is_none());
    }
}
