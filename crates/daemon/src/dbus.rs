//! Interface D-Bus do serviço (barramento do sistema).
//!
//! Consultas são livres; tudo que muda algo (ou lê os botões ao vivo) passa pelo polkit,
//! com a ação `io.github.yurisanp.GameSirBridge.configure` (liberada sem senha para a
//! sessão local ativa, como o NetworkManager faz com o Wi-Fi).

use crate::daemon::{Daemon, Error, Signal};
use gamesir_core::api::{self, POLKIT_ACTION};
use gamesir_core::Target;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface, Connection, Variant};

#[zbus::proxy(
    interface = "org.freedesktop.PolicyKit1.Authority",
    default_service = "org.freedesktop.PolicyKit1",
    default_path = "/org/freedesktop/PolicyKit1/Authority"
)]
trait Authority {
    #[allow(clippy::type_complexity)]
    fn check_authorization(
        &self,
        subject: &(&str, HashMap<&str, Variant<'_>>),
        action_id: &str,
        details: HashMap<&str, &str>,
        flags: u32,
        cancellation_id: &str,
    ) -> zbus::Result<(bool, bool, HashMap<String, String>)>;
}

fn err(e: Error) -> fdo::Error {
    match e {
        Error::NoSuchController(_) | Error::Invalid(_) => fdo::Error::InvalidArgs(e.to_string()),
        _ => fdo::Error::Failed(e.to_string()),
    }
}

fn json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serializa")
}

pub struct Service {
    daemon: Arc<Daemon>,
    /// Sem polkit (testes, sessão de usuário): só root pode mudar algo.
    use_polkit: bool,
}

impl Service {
    async fn authorize(&self, hdr: &Header<'_>, conn: &Connection) -> fdo::Result<()> {
        let sender = hdr
            .sender()
            .ok_or_else(|| fdo::Error::AccessDenied("chamada sem remetente".into()))?
            .to_owned();
        let dbus = fdo::DBusProxy::new(conn).await?;
        let uid = dbus.get_connection_unix_user(sender.clone().into()).await?;
        if uid == 0 {
            return Ok(());
        }
        if !self.use_polkit {
            // Barramento de sessão (desenvolvimento): só o mesmo usuário do serviço.
            // SAFETY: geteuid não falha nem toca memória.
            if uid == unsafe { libc::geteuid() } {
                return Ok(());
            }
            return Err(fdo::Error::AccessDenied("outro usuário".into()));
        }
        match polkit_check(conn, sender.as_str()).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(fdo::Error::AccessDenied(
                "não autorizado (polkit: io.github.yurisanp.GameSirBridge.configure)".into(),
            )),
            Err(e) => {
                // Sem polkit ou sem a política instalada (ex.: /usr somente leitura): vale a
                // mesma regra padrão da política, "sessão local ativa", conferida no logind.
                log::debug!("polkit indisponível ({e}); conferindo a sessão no logind");
                if logind_active_local(conn, uid).await.unwrap_or(false) {
                    Ok(())
                } else {
                    Err(fdo::Error::AccessDenied(
                        "não autorizado: só a sessão local ativa pode mudar a configuração".into(),
                    ))
                }
            }
        }
    }
}

async fn polkit_check(conn: &Connection, sender: &str) -> zbus::Result<bool> {
    let authority = AuthorityProxy::new(conn).await?;
    let mut subject = HashMap::new();
    subject.insert("name", Variant::from(sender));
    let (ok, _, _) = authority
        .check_authorization(
            &("system-bus-name", subject),
            POLKIT_ACTION,
            HashMap::new(),
            1,
            "",
        )
        .await?;
    Ok(ok)
}

/// O usuário tem uma sessão local (não SSH) ativa agora?
async fn logind_active_local(conn: &Connection, uid: u32) -> zbus::Result<bool> {
    const DEST: &str = "org.freedesktop.login1";
    let mgr = zbus::Proxy::new(
        conn,
        DEST,
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?;
    let sessions: Vec<(String, u32, String, String, zbus::OwnedObjectPath)> =
        mgr.call("ListSessions", &()).await?;
    for (_, suid, _, _, path) in sessions {
        if suid != uid {
            continue;
        }
        let s = zbus::Proxy::new(conn, DEST, path, "org.freedesktop.login1.Session").await?;
        let active: bool = s.get_property("Active").await?;
        let remote: bool = s.get_property("Remote").await?;
        if active && !remote {
            return Ok(true);
        }
    }
    Ok(false)
}

fn target(s: &str) -> fdo::Result<Target> {
    s.parse().map_err(fdo::Error::InvalidArgs)
}

fn timeout(secs: f64) -> fdo::Result<Duration> {
    if secs.is_finite() && (1.0..=120.0).contains(&secs) {
        Ok(Duration::from_secs_f64(secs))
    } else {
        Err(fdo::Error::InvalidArgs(
            "tempo por passo entre 1 e 120 s".into(),
        ))
    }
}

#[interface(name = "io.github.yurisanp.GameSirBridge1")]
impl Service {
    #[zbus(property)]
    fn version(&self) -> String {
        gamesir_core::VERSION.to_owned()
    }

    /// Lista de controles (JSON: `[ControllerInfo]`).
    fn list_controllers(&self) -> String {
        json(&self.daemon.list())
    }

    /// Perfis conhecidos (JSON: `[Profile]`).
    fn list_profiles(&self) -> String {
        json(&self.daemon.profiles())
    }

    /// Mapeamento em uso por um controle (JSON no formato do `mapping.json`).
    fn get_mapping(&self, id: &str) -> fdo::Result<String> {
        self.daemon
            .mapping(id)
            .map(|m| m.to_json_pretty())
            .map_err(err)
    }

    async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        self.daemon.set_enabled(id, enabled).map_err(err)
    }

    /// Valida e grava um mapeamento. Devolve o caminho do arquivo.
    async fn save_mapping(
        &self,
        id: &str,
        mapping: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<String> {
        self.authorize(&hdr, conn).await?;
        self.daemon.save_mapping(id, mapping).map_err(err)
    }

    async fn reset_mapping(
        &self,
        id: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        self.daemon.reset_mapping(id).map_err(err)
    }

    async fn start_calibration(
        &self,
        id: &str,
        step_timeout: f64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        self.daemon
            .start_calibration(id, timeout(step_timeout)?)
            .map_err(err)
    }

    /// `skip`, `back` ou `cancel`.
    async fn calibration_control(
        &self,
        id: &str,
        action: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        self.daemon.calibration_control(id, action).map_err(err)
    }

    /// "Aperte para remapear": espera uma entrada para `target` e responde com o sinal `Captured`.
    async fn capture_input(
        &self,
        id: &str,
        target_name: &str,
        step_timeout: f64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        self.daemon
            .capture(id, target(target_name)?, timeout(step_timeout)?)
            .map_err(err)
    }

    /// Liga o envio de `InputState` (~30 Hz) por 10 s; o app renova enquanto a tela estiver aberta.
    async fn watch_input(
        &self,
        id: &str,
        enable: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        self.daemon.watch(id, enable).map_err(err)
    }

    async fn forget(
        &self,
        id: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        self.daemon.forget(id).map_err(err)
    }

    async fn reload(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        let d = self.daemon.clone();
        tokio::task::spawn_blocking(move || d.reload())
            .await
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    #[zbus(signal)]
    async fn controllers_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn input_state(emitter: &SignalEmitter<'_>, id: &str, state: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn calibration_step(
        emitter: &SignalEmitter<'_>,
        id: &str,
        index: u32,
        total: u32,
        target: &str,
        prompt: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn calibration_hit(
        emitter: &SignalEmitter<'_>,
        id: &str,
        target: &str,
        description: &str,
        clash: Vec<String>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn calibration_skipped(
        emitter: &SignalEmitter<'_>,
        id: &str,
        target: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn calibration_finished(
        emitter: &SignalEmitter<'_>,
        id: &str,
        saved: bool,
        message: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn captured(emitter: &SignalEmitter<'_>, id: &str, result: &str) -> zbus::Result<()>;
}

/// Publica o serviço e repassa os avisos do daemon como sinais. Não retorna.
pub async fn serve(daemon: Arc<Daemon>, system_bus: bool) -> anyhow::Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    daemon.set_notifier(tx);
    let service = Service {
        daemon: daemon.clone(),
        use_polkit: system_bus,
    };
    let builder = if system_bus {
        zbus::connection::Builder::system()
    } else {
        zbus::connection::Builder::session()
    };
    let conn = builder
        .name(api::BUS_NAME)
        .serve_at(api::OBJECT_PATH, service)
        .build()
        .await?;
    log::info!(
        "D-Bus: {} em {} ({})",
        api::BUS_NAME,
        api::OBJECT_PATH,
        if system_bus { "sistema" } else { "sessão" }
    );
    let iface = conn
        .object_server()
        .interface::<_, Service>(api::OBJECT_PATH)
        .await?;
    let em = iface.signal_emitter().clone();
    while let Some(s) = rx.recv().await {
        let r = match s {
            Signal::ControllersChanged => Service::controllers_changed(&em).await,
            Signal::Live(id, live) => Service::input_state(&em, &id, &json(&live)).await,
            Signal::CalStep {
                id,
                index,
                total,
                target,
            } => {
                Service::calibration_step(&em, &id, index, total, target.name(), target.prompt())
                    .await
            }
            Signal::CalHit {
                id,
                target,
                description,
                clash,
            } => Service::calibration_hit(&em, &id, target.name(), &description, clash).await,
            Signal::CalSkipped { id, target } => {
                Service::calibration_skipped(&em, &id, target.name()).await
            }
            Signal::CalFinished { id, saved, message } => {
                Service::calibration_finished(&em, &id, saved, &message).await
            }
            Signal::Captured(id, c) => Service::captured(&em, &id, &json(&c)).await,
        };
        if let Err(e) = r {
            log::warn!("falha ao emitir sinal D-Bus: {e}");
        }
    }
    Ok(())
}
