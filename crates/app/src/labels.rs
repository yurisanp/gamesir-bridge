//! Textos da interface que dependem do alvo, da regra ou do estado.

use gamesir_core::api::{ControllerInfo, Status};
use gamesir_core::codes::key_name;
use gamesir_core::{Rule, Target};

/// Nome curto do botão do Xbox.
pub fn target(t: Target) -> &'static str {
    match t {
        Target::Back => "View",
        Target::Start => "Menu",
        Target::Ls => "L3",
        Target::Rs => "R3",
        Target::Home => "Home",
        Target::DpadUp => "Cima",
        Target::DpadDown => "Baixo",
        Target::DpadLeft => "Esquerda",
        Target::DpadRight => "Direita",
        other => other.name(),
    }
}

/// Instrução grande da calibração.
pub fn instruction(t: Target) -> String {
    match t {
        Target::A | Target::B | Target::X | Target::Y => format!("Aperte o botão {}", t.name()),
        Target::Lb => "Aperte o LB (ombro esquerdo)".into(),
        Target::Rb => "Aperte o RB (ombro direito)".into(),
        Target::Back => "Aperte View (botão pequeno da esquerda)".into(),
        Target::Start => "Aperte Menu (botão pequeno da direita)".into(),
        Target::Ls => "Clique o analógico esquerdo (L3)".into(),
        Target::Rs => "Clique o analógico direito (R3)".into(),
        Target::Home => "Aperte o botão central (Home)".into(),
        Target::DpadUp => "Aperte o D-pad para cima".into(),
        Target::DpadDown => "Aperte o D-pad para baixo".into(),
        Target::DpadLeft => "Aperte o D-pad para a esquerda".into(),
        Target::DpadRight => "Aperte o D-pad para a direita".into(),
        Target::Lt => "Aperte o gatilho LT até o fim".into(),
        Target::Rt => "Aperte o gatilho RT até o fim".into(),
        Target::Lx => "Empurre o analógico esquerdo todo para a direita".into(),
        Target::Ly => "Empurre o analógico esquerdo todo para baixo".into(),
        Target::Rx => "Empurre o analógico direito todo para a direita".into(),
        Target::Ry => "Empurre o analógico direito todo para baixo".into(),
    }
}

/// De onde vem o valor, no estilo do design: `BTN 304`, `ABS 17 · −1`.
pub fn rule(r: Option<&Rule>) -> String {
    match r {
        None => "sem regra".into(),
        Some(Rule::Key { code }) => match key_name(*code) {
            Some(n) if !n.starts_with("BTN_") => format!("{n} {code}"),
            _ => format!("BTN {code}"),
        },
        Some(&Rule::Abs {
            code,
            rest,
            sign,
            min,
            max,
        }) => {
            let s = if sign > 0 { "+1" } else { "−1" };
            if max - min <= 2 {
                format!("ABS {code} · {s}")
            } else {
                format!("ABS {code} · {min}–{max} · repouso {rest} · {s}")
            }
        }
    }
}

pub fn status(s: Status) -> &'static str {
    match s {
        Status::Active => "Ativo",
        Status::Off => "Ponte desligada",
        Status::Waiting => "Desconectado",
        Status::Unmapped => "Sem mapeamento",
        Status::Calibrating => "Calibrando",
        Status::Error => "Erro",
    }
}

/// Classe de cor do selo (cor e texto juntos, nunca só cor).
pub fn status_class(s: Status) -> &'static str {
    match s {
        Status::Active => "success",
        Status::Off | Status::Waiting => "dim-label",
        Status::Unmapped | Status::Calibrating => "warning",
        Status::Error => "error",
    }
}

pub fn subtitle(c: &ControllerInfo) -> String {
    match c.player {
        Some(p) if c.status == Status::Active => format!("Jogador {p} · Ativo"),
        _ if c.status == Status::Unmapped => format!("Dispositivo {} · Sem mapeamento", c.device),
        _ => status(c.status).into(),
    }
}

pub fn source(s: Option<&str>) -> &'static str {
    match s {
        Some("official") => "oficial",
        Some("custom") => "seu perfil",
        Some("calibrated") => "recalibrado",
        _ => "",
    }
}

/// `/dev/input/event17` → `event17`.
pub fn node(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_has_text() {
        for t in Target::ALL {
            assert!(!target(t).is_empty());
            assert!(instruction(t).starts_with(['A', 'C', 'E']), "{t:?}");
        }
    }

    #[test]
    fn rule_text_matches_design() {
        assert_eq!(rule(None), "sem regra");
        assert_eq!(rule(Some(&Rule::Key { code: 304 })), "BTN 304");
        assert_eq!(rule(Some(&Rule::Key { code: 172 })), "KEY_HOMEPAGE 172");
        let hat = Rule::Abs {
            code: 17,
            rest: 0,
            sign: -1,
            min: -1,
            max: 1,
        };
        assert_eq!(rule(Some(&hat)), "ABS 17 · −1");
        let stick = Rule::Abs {
            code: 0,
            rest: 128,
            sign: 1,
            min: 0,
            max: 255,
        };
        assert_eq!(rule(Some(&stick)), "ABS 0 · 0–255 · repouso 128 · +1");
    }

    #[test]
    fn node_name() {
        assert_eq!(node("/dev/input/event17"), "event17");
        assert_eq!(node("event3"), "event3");
    }
}
