//! Um worker por controle: lê todas as interfaces do aparelho, mantém o estado cru e
//! alimenta o Xbox 360 virtual, a calibração ou o "teste de entradas".
//!
//! O laço é o `cmd_run` do script Python: a cada `SYN_REPORT` de qualquer interface,
//! recalcula o estado de saída e emite o que mudou.

use crate::pad::Pad;
use crate::sys::{self, EventFd};
use evdev::raw_stream::RawDevice;
use gamesir_core::api::LiveState;
use gamesir_core::calibration::{AxisInfo, CalEvent, Calibrator};
use gamesir_core::codes::EV_SYN;
use gamesir_core::mapper::{InputState, OUT_STICKS, OUT_TRIGGERS};
use gamesir_core::target::TargetKind;
use gamesir_core::{Mapper, Mapping, Target};
use std::collections::BTreeMap;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Intervalo mínimo entre dois envios de estado ao vivo para o app (~30 Hz).
const LIVE_INTERVAL: Duration = Duration::from_millis(33);
/// O app precisa renovar o pedido de estado ao vivo antes disso.
pub const WATCH_TTL: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum Cmd {
    /// `None`: sem mapeamento (controle desconhecido).
    SetMapping(Option<Mapping>),
    SetEnabled(bool),
    Calibrate {
        steps: Vec<Target>,
        timeout: Duration,
        capture: bool,
    },
    CalSkip,
    CalBack,
    CalCancel,
    Watch(bool),
    Stop,
}

#[derive(Debug)]
pub enum Event {
    /// Interfaces abertas; `output` é o controle virtual, se a ponte estiver ligada.
    Running {
        output: Option<String>,
    },
    /// O controle virtual mudou (criado ou destruído).
    Output(Option<String>),
    Cal(CalEvent),
    CalCancelled,
    /// Fim de uma captura ("aperte para remapear").
    Captured {
        target: Target,
        rule: Option<gamesir_core::Rule>,
    },
    Live(LiveState),
    /// Problema não fatal (ex.: sem /dev/uinput).
    Error(String),
    /// O worker terminou sozinho (desconectado ou erro ao abrir).
    Exited {
        reason: String,
        pad: Option<Pad>,
    },
}

pub type Sink = Sender<(String, u64, Event)>;

pub struct Handle {
    pub nodes: Vec<PathBuf>,
    pub generation: u64,
    tx: Sender<Cmd>,
    wake: Arc<EventFd>,
    join: Option<JoinHandle<Option<Pad>>>,
}

impl Handle {
    pub fn send(&self, cmd: Cmd) {
        if self.tx.send(cmd).is_ok() {
            self.wake.wake();
        }
    }

    /// Para o worker e devolve o controle virtual, para reaproveitar.
    pub fn stop(mut self) -> Option<Pad> {
        self.send(Cmd::Stop);
        self.join.take().and_then(|j| j.join().ok()).flatten()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if self.join.is_some() {
            let _ = self.tx.send(Cmd::Stop);
            self.wake.wake();
        }
    }
}

pub struct Config {
    pub id: String,
    pub generation: u64,
    pub nodes: Vec<PathBuf>,
    pub mapping: Option<Mapping>,
    pub enabled: bool,
    pub pad: Option<Pad>,
}

pub fn spawn(cfg: Config, sink: Sink) -> std::io::Result<Handle> {
    let (tx, rx) = std::sync::mpsc::channel();
    let wake = Arc::new(EventFd::new()?);
    let nodes = cfg.nodes.clone();
    let generation = cfg.generation;
    let w = wake.clone();
    let join = std::thread::Builder::new()
        .name(format!("ctl-{}", cfg.id))
        .spawn(move || run(cfg, rx, w, sink))?;
    Ok(Handle {
        nodes,
        generation,
        tx,
        wake,
        join: Some(join),
    })
}

struct Cal {
    cal: Calibrator,
    capture: bool,
    first: Target,
}

struct Worker {
    id: String,
    generation: u64,
    devices: Vec<RawDevice>,
    state: InputState,
    axes: BTreeMap<u16, AxisInfo>,
    mapping: Option<Mapping>,
    mapper: Option<Mapper>,
    enabled: bool,
    grabbed: bool,
    pad: Option<Pad>,
    cal: Option<Cal>,
    watch_until: Option<Instant>,
    live_dirty: bool,
    last_live: Instant,
    sink: Sink,
}

fn run(cfg: Config, rx: Receiver<Cmd>, wake: Arc<EventFd>, sink: Sink) -> Option<Pad> {
    let (id, generation) = (cfg.id.clone(), cfg.generation);
    let mut w = match Worker::open(cfg, sink.clone()) {
        Ok(w) => w,
        Err((reason, pad)) => {
            log::warn!("[{id}] {reason}");
            let _ = sink.send((id, generation, Event::Exited { reason, pad }));
            return None;
        }
    };
    let output = w.pad.as_ref().and_then(|p| p.path.clone());
    w.send(Event::Running { output });

    loop {
        // Comandos primeiro.
        loop {
            match rx.try_recv() {
                Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => {
                    return w.shutdown();
                }
                Ok(cmd) => w.command(cmd),
                Err(TryRecvError::Empty) => break,
            }
        }
        wake.clear();

        let now = Instant::now();
        let mut timeout = w.cal.as_ref().and_then(|c| c.cal.next_deadline(now));
        if w.live_dirty {
            let t = w.last_live + LIVE_INTERVAL;
            timeout = Some(timeout.map_or(t, |x| x.min(t)));
        }
        let timeout = timeout.map(|t| t.saturating_duration_since(now));

        let mut fds: Vec<_> = w.devices.iter().map(|d| d.as_raw_fd()).collect();
        fds.push(wake.as_raw_fd());
        let ready = match sys::poll(&fds, timeout) {
            Ok(r) => r,
            Err(e) => return w.exit(format!("poll falhou: {e}")),
        };

        for (i, r) in ready.iter().take(w.devices.len()).enumerate() {
            if r.readable || r.gone {
                if let Err(e) = w.read_device(i) {
                    let msg = match e.raw_os_error() {
                        Some(libc::ENODEV) => "desconectado".to_owned(),
                        _ => format!("desconectado ({e})"),
                    };
                    return w.exit(msg);
                }
            }
        }
        w.tick();
    }
}

impl Worker {
    fn open(cfg: Config, sink: Sink) -> Result<Worker, (String, Option<Pad>)> {
        let mut devices = Vec::new();
        let mut state = InputState::default();
        let mut axes = BTreeMap::new();
        for path in &cfg.nodes {
            let dev = match RawDevice::open(path) {
                Ok(d) => d,
                Err(e) => return Err((format!("não abriu {}: {e}", path.display()), cfg.pad)),
            };
            if let Err(e) = sys::set_nonblocking(dev.as_raw_fd()) {
                return Err((e.to_string(), cfg.pad));
            }
            if let Ok(keys) = dev.get_key_state() {
                state.keys.extend(keys.iter().map(|k| k.0));
            }
            if let (Some(supported), Ok(abs)) = (dev.supported_absolute_axes(), dev.get_abs_state())
            {
                for code in supported.iter() {
                    let info = abs[code.0 as usize];
                    state.abs.insert(code.0, info.value);
                    axes.insert(
                        code.0,
                        AxisInfo {
                            value: info.value,
                            min: info.minimum,
                            max: info.maximum,
                        },
                    );
                }
            }
            devices.push(dev);
        }
        let mapper = cfg.mapping.as_ref().map(Mapper::new);
        let mut w = Worker {
            id: cfg.id,
            generation: cfg.generation,
            devices,
            state,
            axes,
            mapping: cfg.mapping,
            mapper,
            enabled: cfg.enabled,
            grabbed: false,
            pad: cfg.pad,
            cal: None,
            watch_until: None,
            live_dirty: false,
            last_live: Instant::now(),
            sink,
        };
        w.apply_mode();
        w.emit();
        Ok(w)
    }

    fn send(&self, ev: Event) {
        let _ = self.sink.send((self.id.clone(), self.generation, ev));
    }

    fn bridging(&self) -> bool {
        self.enabled && self.mapper.is_some()
    }

    /// Ajusta grab e controle virtual ao modo atual.
    fn apply_mode(&mut self) {
        let want_grab = self.bridging() || self.cal.is_some();
        if want_grab != self.grabbed {
            for d in &mut self.devices {
                let r = if want_grab { d.grab() } else { d.ungrab() };
                if let Err(e) = r {
                    log::warn!("[{}] grab={want_grab}: {e}", self.id);
                }
            }
            self.grabbed = want_grab;
        }
        if self.bridging() && self.pad.is_none() {
            match Pad::create() {
                Ok(p) => {
                    log::info!("[{}] controle virtual criado: {:?}", self.id, p.path);
                    self.send(Event::Output(p.path.clone()));
                    self.pad = Some(p);
                }
                Err(e) => self.send(Event::Error(format!(
                    "não foi possível criar o controle virtual (/dev/uinput): {e}"
                ))),
            }
        } else if !self.bridging() {
            if let Some(mut p) = self.pad.take() {
                p.release_all();
                self.send(Event::Output(None));
            }
        }
    }

    fn emit(&mut self) {
        if self.cal.is_some() {
            return;
        }
        if let (Some(m), Some(pad)) = (&self.mapper, &mut self.pad) {
            if let Err(e) = pad.emit(&m.compute(&self.state)) {
                log::warn!("[{}] falha ao escrever no controle virtual: {e}", self.id);
            }
        }
    }

    fn command(&mut self, cmd: Cmd) {
        let now = Instant::now();
        match cmd {
            Cmd::SetMapping(m) => {
                self.mapper = m.as_ref().map(Mapper::new);
                self.mapping = m;
                self.apply_mode();
                self.emit();
            }
            Cmd::SetEnabled(on) => {
                self.enabled = on;
                self.apply_mode();
                self.emit();
            }
            Cmd::Calibrate {
                steps,
                timeout,
                capture,
            } => {
                if let Some(p) = &mut self.pad {
                    // O jogo não deve ver os botões apertados durante a calibração.
                    p.release_all();
                }
                let Some(&first) = steps.first() else { return };
                let (cal, evs) =
                    Calibrator::new(steps, self.axes.clone(), timeout, &self.state, now);
                self.cal = Some(Cal {
                    cal,
                    capture,
                    first,
                });
                self.apply_mode();
                self.cal_events(evs);
            }
            Cmd::CalSkip => {
                if let Some(c) = &mut self.cal {
                    let evs = c.cal.skip(&self.state, now);
                    self.cal_events(evs);
                }
            }
            Cmd::CalBack => {
                if let Some(c) = &mut self.cal {
                    let evs = c.cal.back(&self.state, now);
                    self.cal_events(evs);
                }
            }
            Cmd::CalCancel => {
                if self.cal.take().is_some() {
                    self.send(Event::CalCancelled);
                    self.apply_mode();
                    self.emit();
                }
            }
            Cmd::Watch(on) => {
                self.watch_until = on.then(|| now + WATCH_TTL);
                self.live_dirty = on;
            }
            Cmd::Stop => unreachable!("tratado no laço"),
        }
    }

    fn cal_events(&mut self, evs: Vec<CalEvent>) {
        for ev in evs {
            let Some(c) = &self.cal else { return };
            let capture = c.capture;
            let first = c.first;
            match ev {
                CalEvent::Finished { rules } => {
                    if capture {
                        self.send(Event::Captured {
                            target: first,
                            rule: rules.get(&first).copied(),
                        });
                    } else {
                        self.send(Event::Cal(CalEvent::Finished { rules }));
                    }
                    self.cal = None;
                    self.apply_mode();
                    self.emit();
                }
                // Na captura, só o resultado final interessa.
                ev if !capture => self.send(Event::Cal(ev)),
                _ => {}
            }
        }
    }

    fn read_device(&mut self, i: usize) -> std::io::Result<()> {
        loop {
            let events: Vec<_> = match self.devices[i].fetch_events() {
                Ok(it) => it.collect(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(e),
            };
            if events.is_empty() {
                return Ok(());
            }
            let now = Instant::now();
            for ev in events {
                let (t, c, v) = (ev.event_type().0, ev.code(), ev.value());
                let syn = self.state.apply(t, c, v);
                if let Some(cal) = &mut self.cal {
                    if t != EV_SYN {
                        let evs = cal.cal.feed(t, c, v, &self.state, now);
                        self.cal_events(evs);
                    }
                } else if syn {
                    self.emit();
                }
                if syn {
                    self.live_dirty = self.watch_until.is_some();
                }
            }
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if let Some(c) = &mut self.cal {
            let evs = c.cal.tick(&self.state, now);
            self.cal_events(evs);
        }
        if let Some(until) = self.watch_until {
            if now >= until {
                self.watch_until = None;
                self.live_dirty = false;
            } else if self.live_dirty && now >= self.last_live + LIVE_INTERVAL {
                self.live_dirty = false;
                self.last_live = now;
                self.send(Event::Live(self.live_state()));
            }
        }
    }

    fn live_state(&self) -> LiveState {
        let mut live = LiveState {
            keys: self.state.keys.iter().copied().collect(),
            abs: self.state.abs.clone(),
            ..Default::default()
        };
        if let Some(m) = &self.mapper {
            for t in Target::ALL {
                let on = match t.kind() {
                    TargetKind::Button | TargetKind::Dpad => m.pressed(t, &self.state),
                    TargetKind::Trigger => m.trigger(t, &self.state) > 0,
                    TargetKind::Stick => m.stick(t, &self.state).abs() > 8000,
                };
                if on {
                    live.active.push(t);
                }
            }
            for (t, _) in OUT_STICKS {
                live.axes.insert(t, m.stick(t, &self.state));
            }
            for (t, _) in OUT_TRIGGERS {
                live.axes.insert(t, m.trigger(t, &self.state));
            }
        }
        live
    }

    fn release(&mut self) -> Option<Pad> {
        for d in &mut self.devices {
            if self.grabbed {
                let _ = d.ungrab();
            }
        }
        self.grabbed = false;
        let mut pad = self.pad.take();
        if let Some(p) = &mut pad {
            p.release_all();
        }
        pad
    }

    fn shutdown(mut self) -> Option<Pad> {
        self.release()
    }

    fn exit(mut self, reason: String) -> Option<Pad> {
        log::info!("[{}] {reason}", self.id);
        let pad = self.release();
        self.send(Event::Exited { reason, pad });
        None
    }
}
