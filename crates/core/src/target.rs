//! Alvos do controle virtual (Xbox 360) e a ordem de calibração.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Uma entrada do controle Xbox 360 virtual que pode receber uma regra.
///
/// Os nomes em JSON são os mesmos do `mapping.json` original (`"A"`, `"DPAD_UP"`, `"LX"`...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Target {
    A,
    B,
    X,
    Y,
    #[serde(rename = "LB")]
    Lb,
    #[serde(rename = "RB")]
    Rb,
    Back,
    Start,
    #[serde(rename = "LS")]
    Ls,
    #[serde(rename = "RS")]
    Rs,
    Home,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    #[serde(rename = "LT")]
    Lt,
    #[serde(rename = "RT")]
    Rt,
    #[serde(rename = "LX")]
    Lx,
    #[serde(rename = "LY")]
    Ly,
    #[serde(rename = "RX")]
    Rx,
    #[serde(rename = "RY")]
    Ry,
}

/// Que tipo de saída o alvo produz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Button,
    Dpad,
    Trigger,
    Stick,
}

impl Target {
    /// Todos os alvos, na ordem da calibração (a mesma do script Python).
    pub const ALL: [Target; 21] = [
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
        Target::DpadUp,
        Target::DpadDown,
        Target::DpadLeft,
        Target::DpadRight,
        Target::Lt,
        Target::Rt,
        Target::Lx,
        Target::Ly,
        Target::Rx,
        Target::Ry,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Target::A => "A",
            Target::B => "B",
            Target::X => "X",
            Target::Y => "Y",
            Target::Lb => "LB",
            Target::Rb => "RB",
            Target::Back => "BACK",
            Target::Start => "START",
            Target::Ls => "LS",
            Target::Rs => "RS",
            Target::Home => "HOME",
            Target::DpadUp => "DPAD_UP",
            Target::DpadDown => "DPAD_DOWN",
            Target::DpadLeft => "DPAD_LEFT",
            Target::DpadRight => "DPAD_RIGHT",
            Target::Lt => "LT",
            Target::Rt => "RT",
            Target::Lx => "LX",
            Target::Ly => "LY",
            Target::Rx => "RX",
            Target::Ry => "RY",
        }
    }

    pub fn kind(self) -> TargetKind {
        match self {
            Target::DpadUp | Target::DpadDown | Target::DpadLeft | Target::DpadRight => {
                TargetKind::Dpad
            }
            Target::Lt | Target::Rt => TargetKind::Trigger,
            Target::Lx | Target::Ly | Target::Rx | Target::Ry => TargetKind::Stick,
            _ => TargetKind::Button,
        }
    }

    /// O que pedir ao usuário durante a calibração (texto do script original).
    pub fn prompt(self) -> &'static str {
        match self {
            Target::A => "botão A",
            Target::B => "botão B",
            Target::X => "botão X",
            Target::Y => "botão Y",
            Target::Lb => "LB (ombro esquerdo)",
            Target::Rb => "RB (ombro direito)",
            Target::Back => "View / Select (botão pequeno da esquerda)",
            Target::Start => "Menu / Start (botão pequeno da direita)",
            Target::Ls => "clique do analógico ESQUERDO (L3)",
            Target::Rs => "clique do analógico DIREITO (R3)",
            Target::Home => "botão central / Home",
            Target::DpadUp => "D-pad para CIMA",
            Target::DpadDown => "D-pad para BAIXO",
            Target::DpadLeft => "D-pad para a ESQUERDA",
            Target::DpadRight => "D-pad para a DIREITA",
            Target::Lt => "gatilho LT (aperte até o fim)",
            Target::Rt => "gatilho RT (aperte até o fim)",
            Target::Lx => "analógico ESQUERDO todo para a DIREITA",
            Target::Ly => "analógico ESQUERDO todo para BAIXO",
            Target::Rx => "analógico DIREITO todo para a DIREITA",
            Target::Ry => "analógico DIREITO todo para BAIXO",
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Target {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Target::ALL
            .iter()
            .copied()
            .find(|t| t.name() == s)
            .ok_or_else(|| format!("alvo desconhecido: {s:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nomes_batem_com_serde() {
        for t in Target::ALL {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(json, format!("\"{}\"", t.name()));
            assert_eq!(t.name().parse::<Target>().unwrap(), t);
        }
    }
}
