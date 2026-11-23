//! Serviço do GameSir Bridge (`gamesir-bridged`) e cliente de linha de comando.

pub mod config;
pub mod daemon;
pub mod dbus;
pub mod pad;
pub mod sys;
pub mod worker;

pub use daemon::Daemon;
