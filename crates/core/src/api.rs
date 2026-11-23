//! Tipos e nomes da interface D-Bus entre o serviço e o app.
//!
//! Estruturas compostas trafegam como JSON (`s`) para que serviço e app usem
//! exatamente os mesmos tipos Rust de um lado e do outro.

use crate::mapping::Rule;
use crate::target::Target;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BUS_NAME: &str = "io.github.yurisanp.GameSirBridge";
pub const OBJECT_PATH: &str = "/io/github/yurisanp/GameSirBridge";
pub const INTERFACE: &str = "io.github.yurisanp.GameSirBridge1";
/// Ação polkit exigida para mudar qualquer coisa (padrão: liberado para a sessão ativa).
pub const POLKIT_ACTION: &str = "io.github.yurisanp.GameSirBridge.configure";

/// Estado de um controle, em uma palavra (como no design).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Ponte ligada, aparece como Xbox 360.
    Active,
    /// Ponte desligada pelo usuário.
    Off,
    /// Conhecido, mas desconectado.
    Waiting,
    /// Parece um controle, mas não há perfil.
    Unmapped,
    /// Calibrando ou capturando uma entrada.
    Calibrating,
    /// Algo deu errado (mapeamento inválido, sem /dev/uinput...). Ver `error`.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputNodeInfo {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControllerInfo {
    /// Identificador estável (VID_PID-serial ou VID_PID-porta).
    pub id: String,
    pub name: String,
    /// `3537:1007`
    pub device: String,
    pub status: Status,
    pub enabled: bool,
    pub connected: bool,
    /// Posição de jogador (1, 2...) entre os controles ativos.
    pub player: Option<u32>,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    /// `official` (perfil do pacote), `custom` (perfil do usuário) ou `calibrated`
    /// (perfil oficial com mapeamento recalibrado).
    pub mapping_source: Option<String>,
    pub mapped: usize,
    pub total: usize,
    pub inputs: Vec<InputNodeInfo>,
    /// Caminho do controle virtual, quando ativo.
    pub output: Option<String>,
    pub notes: Vec<String>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

/// Estado ao vivo, para o editor e o "Testar entradas".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LiveState {
    /// Teclas cruas apertadas.
    pub keys: Vec<u16>,
    /// Eixos crus.
    pub abs: BTreeMap<u16, i32>,
    /// Alvos do Xbox ativos agora (botões apertados, gatilhos/analógicos fora do repouso).
    pub active: Vec<Target>,
    /// Valores de saída dos analógicos e gatilhos (LX, LY, RX, RY, LT, RT).
    pub axes: BTreeMap<Target, i32>,
}

/// Uma regra capturada em "aperte para remapear".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Captured {
    pub target: Target,
    pub rule: Option<Rule>,
    pub description: String,
}
