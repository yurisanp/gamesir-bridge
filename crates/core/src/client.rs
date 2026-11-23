//! Cliente D-Bus do serviço (feature `client`). Usado pela linha de comando e pelo app.

use crate::api::ControllerInfo;
use zbus::proxy;

#[proxy(
    interface = "io.github.yurisanp.GameSirBridge1",
    default_service = "io.github.yurisanp.GameSirBridge",
    default_path = "/io/github/yurisanp/GameSirBridge"
)]
pub trait Bridge {
    #[zbus(property)]
    fn version(&self) -> zbus::Result<String>;

    fn list_controllers(&self) -> zbus::Result<String>;
    fn list_profiles(&self) -> zbus::Result<String>;
    fn get_mapping(&self, id: &str) -> zbus::Result<String>;
    fn set_enabled(&self, id: &str, enabled: bool) -> zbus::Result<()>;
    fn save_mapping(&self, id: &str, mapping: &str) -> zbus::Result<String>;
    fn reset_mapping(&self, id: &str) -> zbus::Result<()>;
    fn start_calibration(&self, id: &str, step_timeout: f64) -> zbus::Result<()>;
    fn calibration_control(&self, id: &str, action: &str) -> zbus::Result<()>;
    fn capture_input(&self, id: &str, target_name: &str, step_timeout: f64) -> zbus::Result<()>;
    fn watch_input(&self, id: &str, enable: bool) -> zbus::Result<()>;
    fn forget(&self, id: &str) -> zbus::Result<()>;
    fn reload(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn controllers_changed(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn input_state(&self, id: &str, state: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn calibration_step(
        &self,
        id: &str,
        index: u32,
        total: u32,
        target: &str,
        prompt: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    fn calibration_hit(
        &self,
        id: &str,
        target: &str,
        description: &str,
        clash: Vec<String>,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    fn calibration_skipped(&self, id: &str, target: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn calibration_finished(&self, id: &str, saved: bool, message: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn captured(&self, id: &str, result: &str) -> zbus::Result<()>;
}

/// Conecta ao serviço no barramento do sistema (ou de sessão, para desenvolvimento).
pub async fn connect(session: bool) -> zbus::Result<BridgeProxy<'static>> {
    let conn = if session {
        zbus::Connection::session().await?
    } else {
        zbus::Connection::system().await?
    };
    BridgeProxy::new(&conn).await
}

/// `ListControllers` já decodificado.
pub async fn controllers(p: &BridgeProxy<'_>) -> zbus::Result<Vec<ControllerInfo>> {
    let text = p.list_controllers().await?;
    serde_json::from_str(&text).map_err(|e| zbus::Error::Failure(e.to_string()))
}
