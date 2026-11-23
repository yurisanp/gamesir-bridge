//! Conversa com o serviço pelo D-Bus. Com `GAMESIR_BRIDGE_DEMO=1`, usa dados de exemplo
//! (para desenvolver a interface sem hardware e para as capturas de tela do README).

use futures_util::StreamExt;
use gamesir_core::api::{Captured, ControllerInfo, InputNodeInfo, LiveState, Status};
use gamesir_core::client::{self, BridgeProxy};
use gamesir_core::mapper::{InputState, Mapper};
use gamesir_core::{Mapping, Target};
use gtk::glib;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Eventos do serviço para a interface.
#[derive(Debug, Clone)]
pub enum Ev {
    Changed,
    Live(String, LiveState),
    Step {
        id: String,
        index: u32,
        total: u32,
        target: Target,
    },
    Hit {
        id: String,
        description: String,
        clash: Vec<String>,
    },
    Skipped {
        id: String,
    },
    Finished {
        id: String,
        saved: bool,
        message: String,
    },
    Captured(String, Captured),
    /// O serviço sumiu do barramento.
    Lost,
}

type R<T> = Result<T, String>;

fn e(err: zbus::Error) -> String {
    match err {
        zbus::Error::MethodError(_, Some(msg), _) => msg,
        other => other.to_string(),
    }
}

#[derive(Clone)]
pub enum Backend {
    Dbus(BridgeProxy<'static>),
    Demo(Rc<RefCell<Demo>>),
}

impl Backend {
    /// Conecta e começa a repassar os sinais para `tx`.
    pub async fn connect(tx: async_channel::Sender<Ev>) -> R<Backend> {
        if std::env::var_os("GAMESIR_BRIDGE_DEMO").is_some() {
            return Ok(Backend::Demo(Rc::new(RefCell::new(Demo::new(tx)))));
        }
        let session = std::env::var_os("GAMESIR_BRIDGE_SESSION_BUS").is_some();
        let proxy = client::connect(session).await.map_err(e)?;
        proxy.version().await.map_err(e)?;
        let p = proxy.clone();
        glib::spawn_future_local(async move {
            if let Err(err) = forward(p, tx.clone()).await {
                eprintln!("sinais do serviço: {err}");
                let _ = tx.send(Ev::Lost).await;
            }
        });
        Ok(Backend::Dbus(proxy))
    }

    pub async fn version(&self) -> R<String> {
        match self {
            Backend::Dbus(p) => p.version().await.map_err(e),
            Backend::Demo(_) => Ok(gamesir_core::VERSION.into()),
        }
    }

    pub async fn controllers(&self) -> R<Vec<ControllerInfo>> {
        match self {
            Backend::Dbus(p) => client::controllers(p).await.map_err(e),
            Backend::Demo(d) => Ok(d.borrow().controllers.clone()),
        }
    }

    pub async fn mapping(&self, id: &str) -> R<Mapping> {
        match self {
            Backend::Dbus(p) => {
                let text = p.get_mapping(id).await.map_err(e)?;
                Mapping::from_json(&text)
                    .map(|p| p.mapping)
                    .map_err(|e| e.to_string())
            }
            Backend::Demo(d) => Ok(d.borrow().mapping.clone()),
        }
    }

    pub async fn set_enabled(&self, id: &str, on: bool) -> R<()> {
        match self {
            Backend::Dbus(p) => p.set_enabled(id, on).await.map_err(e),
            Backend::Demo(d) => {
                d.borrow_mut().set_enabled(id, on);
                Ok(())
            }
        }
    }

    pub async fn save_mapping(&self, id: &str, m: &Mapping) -> R<String> {
        match self {
            Backend::Dbus(p) => p.save_mapping(id, &m.to_json_pretty()).await.map_err(e),
            Backend::Demo(d) => {
                d.borrow_mut().mapping = m.clone();
                Ok("/etc/gamesir-bridge/mappings/demo.json".into())
            }
        }
    }

    pub async fn reset_mapping(&self, id: &str) -> R<()> {
        match self {
            Backend::Dbus(p) => p.reset_mapping(id).await.map_err(e),
            Backend::Demo(_) => Ok(()),
        }
    }

    pub async fn start_calibration(&self, id: &str, timeout: f64) -> R<()> {
        match self {
            Backend::Dbus(p) => p.start_calibration(id, timeout).await.map_err(e),
            Backend::Demo(d) => {
                Demo::calibrate(d.clone(), id.to_owned());
                Ok(())
            }
        }
    }

    pub async fn calibration_control(&self, id: &str, action: &str) -> R<()> {
        match self {
            Backend::Dbus(p) => p.calibration_control(id, action).await.map_err(e),
            Backend::Demo(d) => {
                d.borrow_mut().cal_action = Some(action.to_owned());
                Ok(())
            }
        }
    }

    pub async fn capture(&self, id: &str, target: Target, timeout: f64) -> R<()> {
        match self {
            Backend::Dbus(p) => p.capture_input(id, target.name(), timeout).await.map_err(e),
            Backend::Demo(d) => {
                let tx = d.borrow().tx.clone();
                let id = id.to_owned();
                glib::timeout_add_local_once(Duration::from_millis(900), move || {
                    let c = Captured {
                        target,
                        rule: Some(gamesir_core::Rule::Key { code: 0x130 }),
                        description: "BTN_SOUTH".into(),
                    };
                    let _ = tx.send_blocking(Ev::Captured(id, c));
                });
                Ok(())
            }
        }
    }

    pub async fn watch(&self, id: &str, on: bool) -> R<()> {
        match self {
            Backend::Dbus(p) => p.watch_input(id, on).await.map_err(e),
            Backend::Demo(d) => {
                Demo::watch(d.clone(), id.to_owned(), on);
                Ok(())
            }
        }
    }

    pub async fn forget(&self, id: &str) -> R<()> {
        match self {
            Backend::Dbus(p) => p.forget(id).await.map_err(e),
            Backend::Demo(d) => {
                d.borrow_mut().controllers.retain(|c| c.id != id);
                let _ = d.borrow().tx.send_blocking(Ev::Changed);
                Ok(())
            }
        }
    }
}

async fn forward(p: BridgeProxy<'static>, tx: async_channel::Sender<Ev>) -> zbus::Result<()> {
    let mut owner = p.inner().receive_owner_changed().await?;
    let t = tx.clone();
    glib::spawn_future_local(async move {
        // O nome perdeu o dono: o serviço parou.
        while let Some(o) = owner.next().await {
            if o.is_none() {
                let _ = t.send(Ev::Lost).await;
            }
        }
    });
    let mut all = p.inner().receive_all_signals().await?;
    while let Some(msg) = all.next().await {
        let h = msg.header();
        let Some(member) = h.member() else { continue };
        let body = msg.body();
        let ev = match member.as_str() {
            "ControllersChanged" => Some(Ev::Changed),
            "InputState" => body
                .deserialize::<(String, String)>()
                .ok()
                .and_then(|(id, s)| Some(Ev::Live(id, serde_json::from_str(&s).ok()?))),
            "CalibrationStep" => body
                .deserialize::<(String, u32, u32, String, String)>()
                .ok()
                .and_then(|(id, index, total, t, _)| {
                    Some(Ev::Step {
                        id,
                        index,
                        total,
                        target: t.parse().ok()?,
                    })
                }),
            "CalibrationHit" => body
                .deserialize::<(String, String, String, Vec<String>)>()
                .ok()
                .map(|(id, _, description, clash)| Ev::Hit {
                    id,
                    description,
                    clash,
                }),
            "CalibrationSkipped" => body
                .deserialize::<(String, String)>()
                .ok()
                .map(|(id, _)| Ev::Skipped { id }),
            "CalibrationFinished" => body
                .deserialize::<(String, bool, String)>()
                .ok()
                .map(|(id, saved, message)| Ev::Finished { id, saved, message }),
            "Captured" => body
                .deserialize::<(String, String)>()
                .ok()
                .and_then(|(id, s)| Some(Ev::Captured(id, serde_json::from_str(&s).ok()?))),
            _ => None,
        };
        if let Some(ev) = ev {
            if tx.send(ev).await.is_err() {
                return Ok(());
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------- demonstração

pub struct Demo {
    tx: async_channel::Sender<Ev>,
    controllers: Vec<ControllerInfo>,
    mapping: Mapping,
    watching: Option<String>,
    cal_action: Option<String>,
}

const T4: &str = include_str!("../../../profiles/gamesir-t4-cyclone-pro-dongle.json");

impl Demo {
    fn new(tx: async_channel::Sender<Ev>) -> Demo {
        let mapping = Mapping::from_json(T4).expect("perfil do T4").mapping;
        let t4 = |id: &str, player: u32, ev: u32, out: u32| ControllerInfo {
            id: id.into(),
            name: "GameSir T4 Cyclone Pro".into(),
            device: "3537:1007".into(),
            status: Status::Active,
            enabled: true,
            connected: true,
            player: Some(player),
            profile_id: Some("gamesir-t4-cyclone-pro-dongle".into()),
            profile_name: Some("GameSir T4 Cyclone Pro".into()),
            mapping_source: Some("official".into()),
            mapped: 21,
            total: 21,
            inputs: vec![
                InputNodeInfo {
                    path: format!("/dev/input/event{ev}"),
                    name: "GameSir-Dongle".into(),
                },
                InputNodeInfo {
                    path: format!("/dev/input/event{}", ev + 1),
                    name: "GameSir-Dongle Consumer Control".into(),
                },
            ],
            output: Some(format!("/dev/input/event{out}")),
            notes: vec![
                "Neste modo do dongle, LT/RT chegam como botões e não há vibração.".into(),
                "M, L4 e R4 são programados no próprio controle.".into(),
            ],
            error: None,
            warnings: vec![],
        };
        let mut unknown = t4("2dc8_3106-usb-3-2", 0, 30, 0);
        unknown.name = "8BitDo Ultimate 2.4G".into();
        unknown.device = "2dc8:3106".into();
        unknown.status = Status::Unmapped;
        unknown.player = None;
        unknown.profile_id = None;
        unknown.profile_name = None;
        unknown.mapping_source = None;
        unknown.mapped = 0;
        unknown.inputs.truncate(1);
        unknown.inputs[0].name = "8BitDo Ultimate 2.4G".into();
        unknown.output = None;
        unknown.notes.clear();
        let mut known = t4("3537_1007-usb-3-4", 0, 0, 0);
        known.status = Status::Waiting;
        known.connected = false;
        known.player = None;
        known.inputs.clear();
        known.output = None;
        Demo {
            tx,
            controllers: vec![t4("3537_1007-usb-3-1.2", 1, 17, 24), unknown, known],
            mapping,
            watching: None,
            cal_action: None,
        }
    }

    fn set_enabled(&mut self, id: &str, on: bool) {
        if let Some(c) = self.controllers.iter_mut().find(|c| c.id == id) {
            c.enabled = on;
            c.status = if on { Status::Active } else { Status::Off };
            c.output = on.then(|| "/dev/input/event24".into());
            c.player = on.then_some(1);
        }
        let _ = self.tx.send_blocking(Ev::Changed);
    }

    /// Simula alguém apertando botões e mexendo o analógico.
    fn watch(d: Rc<RefCell<Demo>>, id: String, on: bool) {
        let start = d.borrow().watching.is_none();
        d.borrow_mut().watching = on.then_some(id);
        if !on || !start {
            return;
        }
        let mapper = Mapper::new(&d.borrow().mapping);
        let mut tick = 0u32;
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(id) = d.borrow().watching.clone() else {
                return glib::ControlFlow::Break;
            };
            tick += 1;
            let mut s = InputState::default();
            let t = f64::from(tick) / 20.0;
            s.abs.insert(0, (128.0 + 127.0 * t.cos()) as i32);
            s.abs.insert(1, (128.0 + 127.0 * t.sin()) as i32);
            s.abs.insert(2, 128);
            s.abs.insert(5, 128);
            if (tick / 10) % 4 == 1 {
                s.keys.insert(0x130);
            }
            if (tick / 10) % 4 == 3 {
                s.keys.insert(0x136);
            }
            let live = live_state(&mapper, &s);
            let _ = d.borrow().tx.send_blocking(Ev::Live(id, live));
            glib::ControlFlow::Continue
        });
    }

    fn calibrate(d: Rc<RefCell<Demo>>, id: String) {
        let tx = d.borrow().tx.clone();
        let mut i = 0usize;
        let mut wait = 0u32;
        let _ = tx.send_blocking(Ev::Step {
            id: id.clone(),
            index: 0,
            total: 21,
            target: Target::ALL[0],
        });
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let action = d.borrow_mut().cal_action.take();
            wait += 1;
            let advance = match action.as_deref() {
                Some("cancel") => {
                    let _ = tx.send_blocking(Ev::Finished {
                        id: id.clone(),
                        saved: false,
                        message: "Calibração cancelada; nada foi gravado.".into(),
                    });
                    return glib::ControlFlow::Break;
                }
                Some("back") => {
                    i = i.saturating_sub(1);
                    true
                }
                Some("skip") => {
                    let _ = tx.send_blocking(Ev::Skipped { id: id.clone() });
                    i += 1;
                    true
                }
                _ if wait >= 25 => {
                    let _ = tx.send_blocking(Ev::Hit {
                        id: id.clone(),
                        description: "BTN_SOUTH".into(),
                        clash: vec![],
                    });
                    i += 1;
                    true
                }
                _ => false,
            };
            if advance {
                wait = 0;
                if i >= 21 {
                    let _ = tx.send_blocking(Ev::Finished {
                        id: id.clone(),
                        saved: true,
                        message: "Mapeamento gravado (21/21 itens).".into(),
                    });
                    return glib::ControlFlow::Break;
                }
                let _ = tx.send_blocking(Ev::Step {
                    id: id.clone(),
                    index: i as u32,
                    total: 21,
                    target: Target::ALL[i],
                });
            }
            glib::ControlFlow::Continue
        });
    }
}

fn live_state(m: &Mapper, s: &InputState) -> LiveState {
    use gamesir_core::target::TargetKind;
    let mut live = LiveState {
        keys: s.keys.iter().copied().collect(),
        abs: s.abs.clone(),
        ..Default::default()
    };
    for t in Target::ALL {
        let on = match t.kind() {
            TargetKind::Button | TargetKind::Dpad => m.pressed(t, s),
            TargetKind::Trigger => m.trigger(t, s) > 0,
            TargetKind::Stick => m.stick(t, s).abs() > 8000,
        };
        if on {
            live.active.push(t);
        }
    }
    for t in [Target::Lx, Target::Ly, Target::Rx, Target::Ry] {
        live.axes.insert(t, m.stick(t, s));
    }
    for t in [Target::Lt, Target::Rt] {
        live.axes.insert(t, m.trigger(t, s));
    }
    live
}
