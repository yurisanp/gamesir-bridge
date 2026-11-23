//! Estado do serviço: quais controles existem, qual worker atende cada um, e as
//! operações que o D-Bus e a linha de comando expõem.

use crate::config::{self, ControllerSettings, Paths, Settings};
use crate::pad::Pad;
use crate::worker::{self, Cmd, Event, Handle};
use gamesir_core::api::{Captured, ControllerInfo, InputNodeInfo, LiveState, Status};
use gamesir_core::calibration::CalEvent;
use gamesir_core::device::{self, DeviceGroup};
use gamesir_core::mapping::format_vid_pid;
use gamesir_core::{Mapping, ProfileSet, Target};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Quanto tempo o controle virtual sobrevive a uma desconexão (o dongle às vezes
/// re-enumera; assim o jogo não perde o controle).
const PAD_GRACE: Duration = Duration::from_secs(10);

/// Depois de uma falha ao abrir as interfaces, espera isso antes de tentar de novo
/// (a menos que o conjunto de interfaces mude).
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(30);

/// Avisos para quem estiver ouvindo (o D-Bus).
#[derive(Debug, Clone)]
pub enum Signal {
    ControllersChanged,
    Live(String, LiveState),
    CalStep {
        id: String,
        index: u32,
        total: u32,
        target: Target,
    },
    CalHit {
        id: String,
        target: Target,
        description: String,
        clash: Vec<String>,
    },
    CalSkipped {
        id: String,
        target: Target,
    },
    CalFinished {
        id: String,
        saved: bool,
        message: String,
    },
    Captured(String, Captured),
}

pub type Notifier = tokio::sync::mpsc::UnboundedSender<Signal>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("controle {0:?} não existe")]
    NoSuchController(String),
    #[error("o controle não está conectado")]
    NotConnected,
    #[error("o controle não tem perfil; calibre primeiro")]
    NoProfile,
    #[error("já existe uma calibração em andamento neste controle")]
    Busy,
    #[error("nenhuma calibração em andamento")]
    NotCalibrating,
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

struct Controller {
    id: String,
    name: String,
    vendor: u16,
    product: u16,
    group: Option<DeviceGroup>,
    profile_id: Option<String>,
    worker: Option<Handle>,
    generation: u64,
    running: bool,
    output: Option<String>,
    spare_pad: Option<(Pad, Instant)>,
    calibrating: bool,
    capturing: bool,
    error: Option<String>,
    runtime_error: Option<String>,
    warnings: Vec<String>,
    mapping_source: Option<&'static str>,
    mapped: usize,
    active_since: Option<Instant>,
    /// Última vez que as interfaces não abriram (para não tentar de novo a cada varredura).
    failed: Option<(Vec<PathBuf>, Instant)>,
}

impl Controller {
    fn new(id: String, name: String, vendor: u16, product: u16) -> Controller {
        Controller {
            id,
            name,
            vendor,
            product,
            group: None,
            profile_id: None,
            worker: None,
            generation: 0,
            running: false,
            output: None,
            spare_pad: None,
            calibrating: false,
            capturing: false,
            error: None,
            runtime_error: None,
            warnings: Vec::new(),
            mapping_source: None,
            mapped: 0,
            active_since: None,
            failed: None,
        }
    }
}

struct Inner {
    profiles: ProfileSet,
    settings: Settings,
    controllers: BTreeMap<String, Controller>,
    next_generation: u64,
}

pub struct Daemon {
    pub paths: Paths,
    inner: Mutex<Inner>,
    sink: worker::Sink,
    notify: Mutex<Option<Notifier>>,
}

impl Daemon {
    /// Cria o serviço e a thread que recebe os eventos dos workers.
    pub fn new(paths: Paths) -> Arc<Daemon> {
        let (sink, rx) = mpsc::channel();
        let profiles = paths.load_profiles();
        log::info!(
            "{} perfil(is): {}",
            profiles.profiles.len(),
            profiles
                .profiles
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let settings = Settings::load(&paths.settings());
        let d = Arc::new(Daemon {
            paths,
            inner: Mutex::new(Inner {
                profiles,
                settings,
                controllers: BTreeMap::new(),
                next_generation: 1,
            }),
            sink,
            notify: Mutex::new(None),
        });
        {
            let mut inner = d.inner.lock().unwrap();
            let known: Vec<_> = inner
                .settings
                .controllers
                .iter()
                .map(|(id, c)| (id.clone(), c.clone()))
                .collect();
            for (id, c) in known {
                let (v, p) = gamesir_core::mapping::parse_vid_pid(&c.device).unwrap_or((0, 0));
                inner
                    .controllers
                    .insert(id.clone(), Controller::new(id, c.name, v, p));
            }
        }
        let weak = Arc::downgrade(&d);
        std::thread::Builder::new()
            .name("eventos".into())
            .spawn(move || {
                while let Ok((id, generation, ev)) = rx.recv() {
                    let Some(d) = weak.upgrade() else { break };
                    d.on_worker_event(&id, generation, ev);
                }
            })
            .expect("thread de eventos");
        d
    }

    pub fn set_notifier(&self, n: Notifier) {
        *self.notify.lock().unwrap() = Some(n);
    }

    fn signal(&self, s: Signal) {
        if let Some(n) = &*self.notify.lock().unwrap() {
            let _ = n.send(s);
        }
    }

    // ------------------------------------------------------------------ descoberta

    /// Relê o sysfs e (re)inicia workers onde o conjunto de interfaces mudou.
    ///
    /// É isso que resolve a corrida do Home: se a interface do Home aparece depois da
    /// do gamepad, o worker é reiniciado com as duas (o controle virtual é mantido).
    pub fn rescan(&self) {
        let nodes = device::scan(&self.paths.sys_root, &self.paths.dev_input);
        let groups = device::group(&nodes);
        let mut changed = false;
        let mut inner = self.inner.lock().unwrap();
        let mut seen = Vec::new();

        for g in groups {
            let profile = inner.profiles.find(g.vendor, g.product).cloned();
            if profile.is_none() && !g.looks_like_gamepad() {
                continue;
            }
            let id = g.stable_id();
            seen.push(id.clone());
            // Para um controle sem perfil, só as interfaces de gamepad (não capturar teclado).
            let wanted: Vec<PathBuf> = g
                .nodes
                .iter()
                .filter(|n| profile.is_some() || n.looks_like_gamepad())
                .map(|n| n.dev_path.clone())
                .collect();

            if !inner.controllers.contains_key(&id) {
                log::info!(
                    "novo controle {id}: {} ({} interface(s))",
                    g.name(),
                    wanted.len()
                );
                let c = Controller::new(id.clone(), g.name().to_owned(), g.vendor, g.product);
                inner.controllers.insert(id.clone(), c);
                if profile.is_some() {
                    inner
                        .settings
                        .controllers
                        .entry(id.clone())
                        .or_insert(ControllerSettings {
                            enabled: true,
                            name: profile.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
                            device: format_vid_pid(g.vendor, g.product),
                        });
                    if let Err(e) = inner.settings.save(&self.paths.settings()) {
                        log::warn!("não salvei {}: {e}", self.paths.settings().display());
                    }
                }
                changed = true;
            }

            let same = inner.controllers[&id]
                .worker
                .as_ref()
                .is_some_and(|w| w.nodes == wanted);
            let backoff = inner.controllers[&id]
                .failed
                .as_ref()
                .is_some_and(|(n, t)| *n == wanted && t.elapsed() < RETRY_AFTER_FAILURE);
            if same || backoff {
                continue;
            }
            if let Some(name) = profile.as_ref().map(|p| p.name.clone()) {
                inner.controllers.get_mut(&id).unwrap().name = name;
            }
            inner.controllers.get_mut(&id).unwrap().group = Some(g);
            self.start_worker(&mut inner, &id, wanted);
            changed = true;
        }

        // Controles que sumiram e não têm worker: só mostra como "aguardando".
        let now = Instant::now();
        for c in inner.controllers.values_mut() {
            if !seen.contains(&c.id) && c.worker.is_none() && c.group.take().is_some() {
                changed = true;
            }
            if c.spare_pad
                .as_ref()
                .is_some_and(|(_, t)| now.duration_since(*t) > PAD_GRACE)
            {
                c.spare_pad = None;
                log::info!("[{}] controle virtual removido", c.id);
            }
        }
        drop(inner);
        if changed {
            self.signal(Signal::ControllersChanged);
        }
    }

    fn start_worker(&self, inner: &mut Inner, id: &str, nodes: Vec<PathBuf>) {
        let generation = inner.next_generation;
        inner.next_generation += 1;
        let enabled = inner.settings.enabled(id);
        let (mapping, source, error, warnings) = {
            let c = &inner.controllers[id];
            match inner.profiles.find(c.vendor, c.product) {
                Some(p) => {
                    let e = config::effective_mapping(&self.paths, p);
                    if let Some(err) = &e.error {
                        log::error!("[{id}] {err}");
                    }
                    (Some(e.mapping), Some(e.source), e.error, e.warnings)
                }
                None => (None, None, None, Vec::new()),
            }
        };
        let profile_id = {
            let c = &inner.controllers[id];
            inner
                .profiles
                .find(c.vendor, c.product)
                .map(|p| p.id.clone())
        };
        let c = inner.controllers.get_mut(id).unwrap();
        let mut pad = c.worker.take().and_then(Handle::stop);
        if pad.is_none() {
            pad = c.spare_pad.take().map(|(p, _)| p);
        }
        c.profile_id = profile_id;
        c.mapped = mapping.as_ref().map_or(0, |m| m.rules.len());
        c.mapping_source = source;
        c.error = error;
        c.warnings = warnings;
        c.runtime_error = None;
        c.calibrating = false;
        c.capturing = false;
        c.generation = generation;
        c.running = false;
        let cfg = worker::Config {
            id: id.to_owned(),
            generation,
            nodes,
            mapping,
            enabled,
            pad,
        };
        match worker::spawn(cfg, self.sink.clone()) {
            Ok(h) => c.worker = Some(h),
            Err(e) => c.runtime_error = Some(format!("não consegui iniciar: {e}")),
        }
    }

    fn on_worker_event(&self, id: &str, generation: u64, ev: Event) {
        let mut inner = self.inner.lock().unwrap();
        let Some(c) = inner.controllers.get_mut(id) else {
            return;
        };
        if c.generation != generation {
            // Evento de um worker antigo; só aproveita o controle virtual.
            if let Event::Exited { pad: Some(p), .. } = ev {
                if c.spare_pad.is_none() && c.worker.is_none() {
                    c.spare_pad = Some((p, Instant::now()));
                }
            }
            return;
        }
        let mut changed = true;
        let mut signal = None;
        match ev {
            Event::Running { output } => {
                c.running = true;
                c.failed = None;
                c.output = output;
            }
            Event::Output(o) => c.output = o,
            Event::Error(e) => {
                log::error!("[{id}] {e}");
                c.runtime_error = Some(e);
            }
            Event::Exited { reason, pad } => {
                log::info!("[{id}] {reason}");
                if !c.running {
                    // Nem chegou a abrir: guarda para não insistir a cada varredura.
                    let nodes = c
                        .worker
                        .as_ref()
                        .map(|w| w.nodes.clone())
                        .unwrap_or_default();
                    c.failed = Some((nodes, Instant::now()));
                    c.runtime_error = Some(reason.clone());
                }
                c.worker = None;
                c.running = false;
                c.output = None;
                c.calibrating = false;
                c.capturing = false;
                c.group = None;
                if let Some(p) = pad {
                    c.spare_pad = Some((p, Instant::now()));
                }
            }
            Event::Live(live) => {
                changed = false;
                signal = Some(Signal::Live(id.to_owned(), live));
            }
            Event::CalCancelled => {
                c.calibrating = false;
                signal = Some(Signal::CalFinished {
                    id: id.to_owned(),
                    saved: false,
                    message: "Calibração cancelada; nada foi gravado.".into(),
                });
            }
            Event::Captured { target, rule } => {
                c.capturing = false;
                signal = Some(Signal::Captured(
                    id.to_owned(),
                    Captured {
                        target,
                        rule,
                        description: rule.map(|r| r.describe()).unwrap_or_default(),
                    },
                ));
            }
            Event::Cal(ev) => {
                changed = false;
                signal = Some(match ev {
                    CalEvent::Step {
                        index,
                        total,
                        target,
                    } => Signal::CalStep {
                        id: id.to_owned(),
                        index: index as u32,
                        total: total as u32,
                        target,
                    },
                    CalEvent::Hit {
                        target,
                        rule,
                        clash,
                    } => Signal::CalHit {
                        id: id.to_owned(),
                        target,
                        description: rule.describe(),
                        clash: clash.iter().map(|t| t.to_string()).collect(),
                    },
                    CalEvent::Skipped { target } => Signal::CalSkipped {
                        id: id.to_owned(),
                        target,
                    },
                    CalEvent::Finished { rules } => {
                        changed = true;
                        c.calibrating = false;
                        let (saved, message) = match self.finish_calibration(&mut inner, id, rules)
                        {
                            Ok(m) => (true, m),
                            Err(e) => (false, e.to_string()),
                        };
                        Signal::CalFinished {
                            id: id.to_owned(),
                            saved,
                            message,
                        }
                    }
                });
            }
        }
        self.update_players(&mut inner);
        drop(inner);
        if let Some(s) = signal {
            self.signal(s);
        }
        if changed {
            self.signal(Signal::ControllersChanged);
        }
    }

    fn finish_calibration(
        &self,
        inner: &mut Inner,
        id: &str,
        rules: BTreeMap<Target, gamesir_core::Rule>,
    ) -> Result<String, Error> {
        let total = rules.len();
        let c = &inner.controllers[id];
        let (vendor, product, name) = (c.vendor, c.product, c.name.clone());
        let profile = inner.profiles.find(vendor, product).cloned();
        let (path, mapping) = match profile {
            Some(p) => {
                let mut m = config::effective_mapping(&self.paths, &p).mapping;
                // Calibrar substitui as regras, mas mantém zona morta e inversões.
                m.rules = rules;
                let path = config::save_mapping(&self.paths, &p, &m)?;
                (path, m)
            }
            None => {
                let m = Mapping {
                    rules,
                    ..Default::default()
                };
                let p = config::create_user_profile(&self.paths, vendor, product, &name, &m)?;
                let path = p.source.clone().unwrap_or_default();
                inner.profiles.insert(p);
                self.write_udev_rule(inner);
                inner.settings.controllers.insert(
                    id.to_owned(),
                    ControllerSettings {
                        enabled: true,
                        name,
                        device: format_vid_pid(vendor, product),
                    },
                );
                let _ = inner.settings.save(&self.paths.settings());
                (path, m)
            }
        };
        log::info!(
            "[{id}] mapeamento gravado em {} ({total}/21)",
            path.display()
        );
        self.apply_mapping(inner, id, Some(mapping));
        Ok(format!(
            "Mapeamento gravado em {} ({total}/{} itens).",
            path.display(),
            Target::ALL.len()
        ))
    }

    fn write_udev_rule(&self, inner: &Inner) {
        let Some(rule_path) = &self.paths.udev_rule else {
            return;
        };
        let user_dir = self.paths.user_profiles();
        let custom: Vec<_> = inner
            .profiles
            .profiles
            .iter()
            .filter(|p| {
                p.source
                    .as_deref()
                    .is_some_and(|s| s.starts_with(&user_dir))
            })
            .collect();
        let text = config::udev_rules_for(&custom);
        if let Err(e) = crate::sys::write_atomic(rule_path, &text) {
            log::warn!("não consegui escrever {}: {e}", rule_path.display());
            return;
        }
        let _ = std::process::Command::new("udevadm")
            .args(["control", "--reload-rules"])
            .status();
    }

    /// Atualiza o mapeamento de um controle e de todos os outros do mesmo perfil.
    fn apply_mapping(&self, inner: &mut Inner, id: &str, mapping: Option<Mapping>) {
        let profile_id = {
            let c = &inner.controllers[id];
            inner
                .profiles
                .find(c.vendor, c.product)
                .map(|p| p.id.clone())
        };
        let ids: Vec<String> = inner
            .controllers
            .values()
            .filter(|c| {
                c.id == id
                    || (profile_id.is_some()
                        && inner.profiles.find(c.vendor, c.product).map(|p| &p.id)
                            == profile_id.as_ref())
            })
            .map(|c| c.id.clone())
            .collect();
        let source = profile_id
            .as_ref()
            .and_then(|pid| inner.profiles.get(pid))
            .map(|p| config::effective_mapping(&self.paths, p).source);
        for cid in ids {
            let c = inner.controllers.get_mut(&cid).unwrap();
            c.profile_id.clone_from(&profile_id);
            c.mapped = mapping.as_ref().map_or(0, |m| m.rules.len());
            c.mapping_source = source;
            c.error = None;
            if let Some(w) = &c.worker {
                w.send(Cmd::SetMapping(mapping.clone()));
            }
        }
    }

    /// A posição de jogador segue a ordem em que cada controle ficou ativo.
    fn update_players(&self, inner: &mut Inner) {
        for c in inner.controllers.values_mut() {
            if c.running && c.output.is_some() {
                c.active_since.get_or_insert_with(Instant::now);
            } else {
                c.active_since = None;
            }
        }
    }

    // ------------------------------------------------------------------ consultas

    pub fn list(&self) -> Vec<ControllerInfo> {
        let inner = self.inner.lock().unwrap();
        let mut active: Vec<(Instant, &str)> = inner
            .controllers
            .values()
            .filter_map(|c| c.active_since.map(|t| (t, c.id.as_str())))
            .collect();
        active.sort();
        let player = |id: &str| {
            active
                .iter()
                .position(|(_, i)| *i == id)
                .map(|p| p as u32 + 1)
        };
        inner
            .controllers
            .values()
            .map(|c| {
                let profile = inner.profiles.find(c.vendor, c.product);
                let enabled = inner.settings.enabled(&c.id);
                let connected = c.running;
                let status = if c.calibrating || c.capturing {
                    Status::Calibrating
                } else if !connected {
                    Status::Waiting
                } else if profile.is_none() {
                    Status::Unmapped
                } else if c.runtime_error.is_some() {
                    Status::Error
                } else if !enabled {
                    Status::Off
                } else {
                    Status::Active
                };
                let mut warnings = c.warnings.clone();
                if let Some(e) = &c.error {
                    warnings.insert(0, e.clone());
                }
                ControllerInfo {
                    id: c.id.clone(),
                    name: profile
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| c.name.clone()),
                    device: format_vid_pid(c.vendor, c.product),
                    status,
                    enabled,
                    connected,
                    player: player(&c.id),
                    profile_id: profile.map(|p| p.id.clone()),
                    profile_name: profile.map(|p| p.name.clone()),
                    mapping_source: c.mapping_source.map(str::to_owned),
                    mapped: c.mapped,
                    total: Target::ALL.len(),
                    inputs: c
                        .group
                        .iter()
                        .flat_map(|g| g.nodes.iter())
                        .filter(|n| {
                            c.worker
                                .as_ref()
                                .is_some_and(|w| w.nodes.contains(&n.dev_path))
                        })
                        .map(|n| InputNodeInfo {
                            path: n.dev_path.display().to_string(),
                            name: n.name.clone(),
                        })
                        .collect(),
                    output: c.output.clone(),
                    notes: profile.map(|p| p.notes.clone()).unwrap_or_default(),
                    error: c.runtime_error.clone(),
                    warnings,
                }
            })
            .collect()
    }

    pub fn profiles(&self) -> Vec<gamesir_core::Profile> {
        self.inner.lock().unwrap().profiles.profiles.clone()
    }

    /// O mapeamento em uso por um controle.
    pub fn mapping(&self, id: &str) -> Result<Mapping, Error> {
        let inner = self.inner.lock().unwrap();
        let c = inner
            .controllers
            .get(id)
            .ok_or_else(|| Error::NoSuchController(id.into()))?;
        let p = inner
            .profiles
            .find(c.vendor, c.product)
            .ok_or(Error::NoProfile)?;
        Ok(config::effective_mapping(&self.paths, p).mapping)
    }

    // ------------------------------------------------------------------ comandos

    fn with_worker<T>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Controller) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut inner = self.inner.lock().unwrap();
        let c = inner
            .controllers
            .get_mut(id)
            .ok_or_else(|| Error::NoSuchController(id.into()))?;
        if c.worker.is_none() || !c.running {
            return Err(Error::NotConnected);
        }
        f(c)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), Error> {
        {
            let mut inner = self.inner.lock().unwrap();
            let c = inner
                .controllers
                .get(id)
                .ok_or_else(|| Error::NoSuchController(id.into()))?;
            let entry = ControllerSettings {
                enabled,
                name: c.name.clone(),
                device: format_vid_pid(c.vendor, c.product),
            };
            if let Some(w) = &c.worker {
                w.send(Cmd::SetEnabled(enabled));
            }
            inner.settings.controllers.insert(id.to_owned(), entry);
            inner.settings.save(&self.paths.settings())?;
            log::info!(
                "[{id}] ponte {}",
                if enabled { "ligada" } else { "desligada" }
            );
        }
        self.signal(Signal::ControllersChanged);
        Ok(())
    }

    pub fn save_mapping(&self, id: &str, json: &str) -> Result<String, Error> {
        let parsed = Mapping::from_json(json).map_err(|e| Error::Invalid(e.to_string()))?;
        let path = {
            let mut inner = self.inner.lock().unwrap();
            let c = inner
                .controllers
                .get(id)
                .ok_or_else(|| Error::NoSuchController(id.into()))?;
            let p = inner
                .profiles
                .find(c.vendor, c.product)
                .cloned()
                .ok_or(Error::NoProfile)?;
            let path = config::save_mapping(&self.paths, &p, &parsed.mapping)?;
            if path.starts_with(self.paths.user_profiles()) {
                let mut p = p;
                p.mapping = parsed.mapping.clone();
                inner.profiles.insert(p);
            }
            self.apply_mapping(&mut inner, id, Some(parsed.mapping));
            path
        };
        self.signal(Signal::ControllersChanged);
        Ok(path.display().to_string())
    }

    /// Volta ao mapeamento oficial do perfil (apaga a recalibração).
    pub fn reset_mapping(&self, id: &str) -> Result<(), Error> {
        {
            let mut inner = self.inner.lock().unwrap();
            let c = inner
                .controllers
                .get(id)
                .ok_or_else(|| Error::NoSuchController(id.into()))?;
            let p = inner
                .profiles
                .find(c.vendor, c.product)
                .cloned()
                .ok_or(Error::NoProfile)?;
            let path = self.paths.override_for(&p.id);
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            let legacy = self.paths.legacy_mapping();
            if legacy.exists() {
                // Mantém o arquivo antigo como backup, fora do caminho de leitura.
                std::fs::rename(&legacy, legacy.with_extension("json.bak"))?;
            }
            let m = config::effective_mapping(&self.paths, &p).mapping;
            self.apply_mapping(&mut inner, id, Some(m));
        }
        self.signal(Signal::ControllersChanged);
        Ok(())
    }

    pub fn start_calibration(&self, id: &str, timeout: Duration) -> Result<(), Error> {
        self.with_worker(id, |c| {
            if c.calibrating || c.capturing {
                return Err(Error::Busy);
            }
            c.calibrating = true;
            c.worker.as_ref().unwrap().send(Cmd::Calibrate {
                steps: Target::ALL.to_vec(),
                timeout,
                capture: false,
            });
            Ok(())
        })?;
        self.signal(Signal::ControllersChanged);
        Ok(())
    }

    pub fn calibration_control(&self, id: &str, action: &str) -> Result<(), Error> {
        let cmd = match action {
            "skip" => Cmd::CalSkip,
            "back" => Cmd::CalBack,
            "cancel" => Cmd::CalCancel,
            other => return Err(Error::Invalid(format!("ação desconhecida: {other:?}"))),
        };
        self.with_worker(id, |c| {
            if !c.calibrating && !c.capturing {
                return Err(Error::NotCalibrating);
            }
            c.worker.as_ref().unwrap().send(cmd);
            Ok(())
        })
    }

    pub fn capture(&self, id: &str, target: Target, timeout: Duration) -> Result<(), Error> {
        self.with_worker(id, |c| {
            if c.calibrating || c.capturing {
                return Err(Error::Busy);
            }
            c.capturing = true;
            c.worker.as_ref().unwrap().send(Cmd::Calibrate {
                steps: vec![target],
                timeout,
                capture: true,
            });
            Ok(())
        })
    }

    pub fn watch(&self, id: &str, on: bool) -> Result<(), Error> {
        self.with_worker(id, |c| {
            c.worker.as_ref().unwrap().send(Cmd::Watch(on));
            Ok(())
        })
    }

    /// Esquece um controle desconectado (some da lista "Conhecidos").
    pub fn forget(&self, id: &str) -> Result<(), Error> {
        {
            let mut inner = self.inner.lock().unwrap();
            let c = inner
                .controllers
                .get(id)
                .ok_or_else(|| Error::NoSuchController(id.into()))?;
            if c.running {
                return Err(Error::Invalid(
                    "desconecte o controle antes de esquecê-lo".into(),
                ));
            }
            inner.controllers.remove(id);
            inner.settings.controllers.remove(id);
            inner.settings.save(&self.paths.settings())?;
        }
        self.signal(Signal::ControllersChanged);
        Ok(())
    }

    /// Relê perfis, mapeamentos e preferências e reinicia os workers.
    pub fn reload(&self) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.profiles = self.paths.load_profiles();
            inner.settings = Settings::load(&self.paths.settings());
            for c in inner.controllers.values_mut() {
                if let Some(w) = c.worker.take() {
                    if let Some(p) = w.stop() {
                        c.spare_pad = Some((p, Instant::now()));
                    }
                }
                c.running = false;
            }
        }
        self.rescan();
    }

    /// Para todos os workers (soltando os botões).
    pub fn shutdown(&self) {
        let mut inner = self.inner.lock().unwrap();
        for c in inner.controllers.values_mut() {
            if let Some(w) = c.worker.take() {
                drop(w.stop());
            }
            c.spare_pad = None;
        }
    }
}
