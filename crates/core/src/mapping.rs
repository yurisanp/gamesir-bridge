//! Formato do `mapping.json` e validação.
//!
//! O formato é exatamente o do script Python:
//!
//! ```json
//! {"device": "3537:1007", "rules": {"A": {"src": "key", "code": 304},
//!   "LX": {"src": "abs", "code": 0, "rest": 128, "sign": 1, "min": 0, "max": 255}}}
//! ```
//!
//! Campos novos (`options`) são opcionais e, ausentes, não mudam nada no comportamento.

use crate::codes;
use crate::target::Target;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// De onde vem o valor de um alvo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "src", rename_all = "lowercase")]
pub enum Rule {
    /// Um botão/tecla evdev (`EV_KEY`).
    Key { code: u16 },
    /// Um eixo evdev (`EV_ABS`), com o repouso e a faixa vistos na calibração.
    Abs {
        code: u16,
        rest: i32,
        sign: i32,
        min: i32,
        max: i32,
    },
}

impl Rule {
    /// Mesma fonte física (para avisar de regras repetidas na calibração).
    pub fn same_source(&self, other: &Rule) -> bool {
        match (self, other) {
            (Rule::Key { code: a }, Rule::Key { code: b }) => a == b,
            (
                Rule::Abs {
                    code: a, sign: sa, ..
                },
                Rule::Abs {
                    code: b, sign: sb, ..
                },
            ) => a == b && sa == sb,
            _ => false,
        }
    }

    /// Descrição curta, no estilo do `calibrate` original.
    pub fn describe(&self) -> String {
        match *self {
            Rule::Key { code } => codes::key_name(code)
                .map(str::to_owned)
                .unwrap_or_else(|| code.to_string()),
            Rule::Abs {
                code, sign, rest, ..
            } => {
                let name = codes::abs_name(code)
                    .map(str::to_owned)
                    .unwrap_or_else(|| code.to_string());
                let s = if sign > 0 { "+" } else { "-" };
                format!("{name} {s} (repouso {rest})")
            }
        }
    }
}

/// Ajustes opcionais aplicados depois do mapeamento.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    /// Zona morta dos analógicos, fração do curso (0.0 a 0.5). 0 desliga.
    #[serde(skip_serializing_if = "is_zero")]
    pub deadzone: f64,
    /// Inverte o eixo Y do analógico esquerdo.
    #[serde(skip_serializing_if = "is_false")]
    pub invert_ly: bool,
    /// Inverte o eixo Y do analógico direito.
    #[serde(skip_serializing_if = "is_false")]
    pub invert_ry: bool,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}
fn is_false(v: &bool) -> bool {
    !*v
}

impl Options {
    pub fn is_default(&self) -> bool {
        *self == Options::default()
    }
}

/// Um mapeamento: regras por alvo + ajustes.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(try_from = "serde_json::Value")]
pub struct Mapping {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub rules: BTreeMap<Target, Rule>,
    #[serde(skip_serializing_if = "Options::is_default")]
    pub options: Options,
}

/// Problema encontrado ao ler um mapeamento.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// Onde está o problema (`rules.LX`, `options`...).
    pub at: String,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.at, self.message)
    }
}

/// Erro ao carregar um mapeamento. Nunca derruba o serviço: o controle fica em "Erro".
#[derive(Debug, thiserror::Error)]
pub enum MappingError {
    #[error("não foi possível ler {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("JSON inválido: {0}")]
    Json(String),
    #[error("mapeamento inválido: {}", .0.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("; "))]
    Invalid(Vec<Issue>),
}

/// Resultado de uma leitura bem-sucedida: o mapeamento e avisos não fatais.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub mapping: Mapping,
    pub warnings: Vec<Issue>,
}

#[derive(Deserialize)]
struct RawMapping {
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    description: Option<String>,
    rules: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    options: Option<serde_json::Value>,
}

impl Mapping {
    /// Lê e valida um mapeamento a partir de JSON.
    ///
    /// Alvos desconhecidos são ignorados com aviso (o script Python também os ignorava);
    /// regras malformadas e opções fora da faixa são erro.
    pub fn from_json(text: &str) -> Result<Parsed, MappingError> {
        let raw: RawMapping =
            serde_json::from_str(text).map_err(|e| MappingError::Json(e.to_string()))?;
        Self::from_raw(raw)
    }

    /// Lê um mapeamento a partir de um `serde_json::Value` (usado pelos perfis).
    pub fn from_value(value: serde_json::Value) -> Result<Parsed, MappingError> {
        let raw: RawMapping =
            serde_json::from_value(value).map_err(|e| MappingError::Json(e.to_string()))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawMapping) -> Result<Parsed, MappingError> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut rules = BTreeMap::new();

        for (name, value) in raw.rules {
            let at = format!("rules.{name}");
            let target = match name.parse::<Target>() {
                Ok(t) => t,
                Err(_) => {
                    warnings.push(Issue {
                        at,
                        message: "alvo desconhecido, ignorado".into(),
                    });
                    continue;
                }
            };
            match serde_json::from_value::<Rule>(value) {
                Ok(rule) => {
                    if let Err(message) = check_rule(&rule) {
                        errors.push(Issue { at, message });
                    } else {
                        rules.insert(target, rule);
                    }
                }
                Err(e) => errors.push(Issue {
                    at,
                    message: e.to_string(),
                }),
            }
        }

        let options = match raw.options {
            None => Options::default(),
            Some(v) => match serde_json::from_value::<Options>(v) {
                Ok(o) => o,
                Err(e) => {
                    errors.push(Issue {
                        at: "options".into(),
                        message: e.to_string(),
                    });
                    Options::default()
                }
            },
        };
        if !(0.0..=0.5).contains(&options.deadzone) || options.deadzone.is_nan() {
            errors.push(Issue {
                at: "options.deadzone".into(),
                message: "precisa estar entre 0 e 0.5".into(),
            });
        }

        if let Some(dev) = &raw.device {
            if parse_vid_pid(dev).is_none() {
                warnings.push(Issue {
                    at: "device".into(),
                    message: format!("{dev:?} não está no formato vvvv:pppp"),
                });
            }
        }

        if !errors.is_empty() {
            return Err(MappingError::Invalid(errors));
        }
        Ok(Parsed {
            mapping: Mapping {
                device: raw.device,
                description: raw.description,
                rules,
                options,
            },
            warnings,
        })
    }

    pub fn load(path: &Path) -> Result<Parsed, MappingError> {
        let text = std::fs::read_to_string(path).map_err(|source| MappingError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_json(&text)
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("Mapping sempre serializa")
    }

    /// Alvos sem regra.
    pub fn missing(&self) -> Vec<Target> {
        Target::ALL
            .iter()
            .copied()
            .filter(|t| !self.rules.contains_key(t))
            .collect()
    }
}

impl TryFrom<serde_json::Value> for Mapping {
    type Error = MappingError;

    fn try_from(v: serde_json::Value) -> Result<Self, Self::Error> {
        Mapping::from_value(v).map(|p| p.mapping)
    }
}

fn check_rule(rule: &Rule) -> Result<(), String> {
    if let Rule::Abs { sign, min, max, .. } = *rule {
        if sign != 1 && sign != -1 {
            return Err(format!("sign precisa ser 1 ou -1 (veio {sign})"));
        }
        if min > max {
            return Err(format!("min ({min}) maior que max ({max})"));
        }
    }
    Ok(())
}

/// `"3537:1007"` → `(0x3537, 0x1007)`.
pub fn parse_vid_pid(s: &str) -> Option<(u16, u16)> {
    let (v, p) = s.split_once(':')?;
    if v.len() != 4 || p.len() != 4 {
        return None;
    }
    Some((
        u16::from_str_radix(v, 16).ok()?,
        u16::from_str_radix(p, 16).ok()?,
    ))
}

pub fn format_vid_pid(vid: u16, pid: u16) -> String {
    format!("{vid:04x}:{pid:04x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const T4: &str =
        include_str!("../../../profiles/gamesir-t4-cyclone-pro-dongle.json");

    #[test]
    fn le_o_perfil_do_t4_completo() {
        let p = Mapping::from_json(T4).unwrap();
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert_eq!(p.mapping.rules.len(), 21);
        assert!(p.mapping.missing().is_empty());
        assert_eq!(p.mapping.rules[&Target::Home], Rule::Key { code: 172 });
    }

    #[test]
    fn mapping_json_do_script_python_continua_valido() {
        // O arquivo que o calibrate antigo gravava: só "device" e "rules".
        let text = r#"{"device": "3537:1007", "rules": {
            "A": {"src": "key", "code": 304},
            "LX": {"src": "abs", "code": 0, "rest": 128, "sign": 1, "min": 0, "max": 255}}}"#;
        let p = Mapping::from_json(text).unwrap();
        assert_eq!(p.mapping.rules.len(), 2);
        assert!(p.mapping.options.is_default());
    }

    #[test]
    fn json_quebrado_vira_erro_e_nao_panico() {
        assert!(matches!(
            Mapping::from_json("{\"rules\": {"),
            Err(MappingError::Json(_))
        ));
        assert!(matches!(
            Mapping::from_json("[]"),
            Err(MappingError::Json(_))
        ));
    }

    #[test]
    fn regra_malformada_aponta_o_lugar() {
        let text = r#"{"rules": {"LX": {"src": "abs", "code": 0},
                                 "A": {"src": "key", "code": 304},
                                 "B": {"src": "abs", "code": 1, "rest": 0, "sign": 0, "min": 0, "max": 1}}}"#;
        match Mapping::from_json(text) {
            Err(MappingError::Invalid(issues)) => {
                let at: Vec<_> = issues.iter().map(|i| i.at.as_str()).collect();
                assert_eq!(at, ["rules.B", "rules.LX"]);
            }
            other => panic!("esperava Invalid, veio {other:?}"),
        }
    }

    #[test]
    fn alvo_desconhecido_e_so_aviso() {
        let text = r#"{"rules": {"PADDLE1": {"src": "key", "code": 1}, "A": {"src": "key", "code": 304}}}"#;
        let p = Mapping::from_json(text).unwrap();
        assert_eq!(p.mapping.rules.len(), 1);
        assert_eq!(p.warnings[0].at, "rules.PADDLE1");
    }

    #[test]
    fn deadzone_fora_da_faixa() {
        let text = r#"{"rules": {}, "options": {"deadzone": 0.9}}"#;
        assert!(matches!(
            Mapping::from_json(text),
            Err(MappingError::Invalid(_))
        ));
    }

    #[test]
    fn ida_e_volta() {
        let p = Mapping::from_json(T4).unwrap().mapping;
        let again = Mapping::from_json(&p.to_json_pretty()).unwrap().mapping;
        assert_eq!(p, again);
    }

    #[test]
    fn vid_pid() {
        assert_eq!(parse_vid_pid("3537:1007"), Some((0x3537, 0x1007)));
        assert_eq!(parse_vid_pid("3537-1007"), None);
        assert_eq!(format_vid_pid(0x45e, 0x28e), "045e:028e");
    }
}
