//! Núcleo do GameSir Bridge: mapeamento, perfis, calibração e descoberta de dispositivos.
//!
//! Nada aqui abre `/dev/input` nem `/dev/uinput`; o serviço (`gamesir-bridged`) faz o I/O
//! e o app usa estes mesmos tipos para mostrar e editar mapeamentos.

pub mod api;
pub mod calibration;
#[cfg(feature = "client")]
pub mod client;
pub mod codes;
pub mod device;
pub mod mapper;
pub mod mapping;
pub mod profile;
pub mod target;

pub use mapper::{InputState, Mapper, OutputState};
pub use mapping::{Mapping, MappingError, Options, Rule};
pub use profile::{Profile, ProfileSet};
pub use target::Target;

/// Versão do protocolo/serviço.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
