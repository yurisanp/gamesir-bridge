//! `gamesir-bridge`: linha de comando. Fala com o serviço por D-Bus; `list` e
//! `calibrate --local` leem os dispositivos direto (precisam de root).

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use evdev::raw_stream::RawDevice;
use futures_util::StreamExt;
use gamesir_bridged::config::Paths;
use gamesir_bridged::sys;
use gamesir_core::api::{ControllerInfo, Status};
use gamesir_core::calibration::{AxisInfo, CalEvent, Calibrator};
use gamesir_core::client::{self, BridgeProxy};
use gamesir_core::codes::{abs_name, key_name, EV_SYN};
use gamesir_core::device::{self, DeviceGroup};
use gamesir_core::mapping::{format_vid_pid, parse_vid_pid};
use gamesir_core::{InputState, Mapping, Target};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    name = "gamesir-bridge",
    version,
    about = "Controles no Linux como um Xbox 360 virtual"
)]
struct Cli {
    /// Usa o barramento de sessão (desenvolvimento).
    #[arg(long, global = true)]
    session_bus: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mostra os controles e as interfaces de cada um (lê o sysfs; não precisa do serviço).
    List {
        /// Mostra também dispositivos que não parecem controles.
        #[arg(long)]
        all: bool,
    },
    /// Estado de cada controle no serviço.
    Status,
    /// Liga a ponte de um controle.
    Enable { id: Option<String> },
    /// Desliga a ponte (o controle volta a aparecer como ele mesmo).
    Disable { id: Option<String> },
    /// Mostra o mapeamento em uso (JSON).
    Mapping { id: Option<String> },
    /// Pede cada botão/eixo em ordem e grava o mapeamento.
    Calibrate {
        id: Option<String>,
        /// Segundos por item antes de pular.
        #[arg(long, default_value_t = 10.0)]
        timeout: f64,
        /// Sem o serviço: lê o dispositivo direto e grava em `--map` (como o script antigo).
        #[arg(long)]
        local: bool,
        /// Arquivo de saída no modo `--local`.
        #[arg(long, default_value = "/etc/gamesir-bridge/mapping.json")]
        map: PathBuf,
        /// VID:PID do controle no modo `--local` (padrão: o primeiro que parecer controle).
        #[arg(long)]
        device: Option<String>,
    },
    /// Relê perfis, mapeamentos e preferências.
    Reload,
    /// Instala (ou atualiza) o serviço, a regra udev e as políticas no sistema, pedindo a
    /// senha uma vez (pkexec). É o que o AppImage e a tela de primeira execução usam.
    Setup {
        /// Remove do sistema o que foi instalado.
        #[arg(long)]
        uninstall: bool,
        /// Só instala se ainda não estiver instalado ou se esta versão for diferente.
        #[arg(long)]
        if_needed: bool,
        /// Só diz se está instalado (código de saída 0) ou não (1).
        #[arg(long)]
        check: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let r = match cli.cmd {
        Cmd::List { all } => cmd_list(all),
        Cmd::Calibrate {
            local: true,
            timeout,
            map,
            device,
            ..
        } => cmd_calibrate_local(timeout, map, device),
        Cmd::Setup {
            uninstall,
            if_needed,
            check,
        } => setup::run(uninstall, if_needed, check),
        cmd => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(remote(cli.session_bus, cmd)),
    };
    if let Err(e) = r {
        eprintln!("erro: {e:#}");
        std::process::exit(1);
    }
}

mod setup;

// ---------------------------------------------------------------------- local

fn describe_codes(
    keys: impl Iterator<Item = u16>,
    abs: impl Iterator<Item = u16>,
) -> (String, String) {
    let k: Vec<String> = keys
        .map(|c| {
            key_name(c)
                .map(str::to_owned)
                .unwrap_or_else(|| c.to_string())
        })
        .collect();
    let a: Vec<String> = abs
        .map(|c| {
            abs_name(c)
                .map(str::to_owned)
                .unwrap_or_else(|| c.to_string())
        })
        .collect();
    (k.join(", "), a.join(", "))
}

fn cmd_list(all: bool) -> Result<()> {
    let paths = Paths::default();
    let profiles = paths.load_profiles();
    let groups = device::group(&device::scan(&paths.sys_root, &paths.dev_input));
    let mut shown = 0;
    for g in &groups {
        let profile = profiles.find(g.vendor, g.product);
        if !all && profile.is_none() && !g.looks_like_gamepad() {
            continue;
        }
        shown += 1;
        println!(
            "{}  {}  [{}]",
            format_vid_pid(g.vendor, g.product),
            g.name(),
            profile.map_or("sem perfil".to_owned(), |p| format!("perfil: {}", p.name))
        );
        println!("  id: {}", g.stable_id());
        for n in &g.nodes {
            println!("  {}  \"{}\"", n.dev_path.display(), n.name);
            let (k, a) = describe_codes(n.keys.iter().copied(), n.abs.iter().copied());
            if !k.is_empty() {
                println!("    EV_KEY: {k}");
            }
            if !a.is_empty() {
                println!("    EV_ABS: {a}");
            }
        }
    }
    if shown == 0 {
        println!(
            "Nenhum controle encontrado. O dongle está plugado? (Se o serviço estiver rodando, \
             ele não some daqui; só fica capturado.)"
        );
        std::process::exit(1);
    }
    Ok(())
}

fn pick_group(groups: &[DeviceGroup], device: Option<&str>) -> Result<DeviceGroup> {
    let want = device
        .map(|d| parse_vid_pid(d).ok_or_else(|| anyhow!("--device precisa ser vvvv:pppp")))
        .transpose()?;
    groups
        .iter()
        .find(|g| match want {
            Some((v, p)) => g.vendor == v && g.product == p,
            None => g.looks_like_gamepad(),
        })
        .cloned()
        .ok_or_else(|| anyhow!("controle não encontrado; confira com `gamesir-bridge list`"))
}

fn cmd_calibrate_local(timeout: f64, map: PathBuf, device: Option<String>) -> Result<()> {
    let paths = Paths::default();
    let groups = device::group(&device::scan(&paths.sys_root, &paths.dev_input));
    let g = pick_group(&groups, device.as_deref())?;
    let mut devs = Vec::new();
    let mut state = InputState::default();
    let mut axes = BTreeMap::new();
    for n in &g.nodes {
        let mut d = RawDevice::open(&n.dev_path)
            .with_context(|| format!("abrindo {} (rode com sudo)", n.dev_path.display()))?;
        d.grab().with_context(|| {
            "o dispositivo está capturado; pare o serviço (sudo systemctl stop gamesir-bridge) \
             ou rode sem --local"
                .to_owned()
        })?;
        sys::set_nonblocking(d.as_raw_fd())?;
        if let Ok(k) = d.get_key_state() {
            state.keys.extend(k.iter().map(|k| k.0));
        }
        if let (Some(sup), Ok(abs)) = (d.supported_absolute_axes(), d.get_abs_state()) {
            for c in sup.iter() {
                let i = abs[c.0 as usize];
                state.abs.insert(c.0, i.value);
                axes.insert(
                    c.0,
                    AxisInfo {
                        value: i.value,
                        min: i.minimum,
                        max: i.maximum,
                    },
                );
            }
        }
        devs.push(d);
    }
    println!(
        "Calibrando {} ({}).",
        g.name(),
        g.nodes
            .iter()
            .map(|n| n.dev_path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "Para cada item: aperte e segure até aparecer OK. Sem input em {timeout:.0}s o item é pulado.\n"
    );
    let (mut cal, evs) = Calibrator::full(
        axes,
        Duration::from_secs_f64(timeout),
        &state,
        Instant::now(),
    );
    let mut rules = None;
    let handle = |evs: Vec<CalEvent>, rules: &mut Option<_>| {
        for ev in evs {
            print_cal(&ev);
            if let CalEvent::Finished { rules: r } = ev {
                *rules = Some(r);
            }
        }
    };
    handle(evs, &mut rules);
    while rules.is_none() {
        let now = Instant::now();
        let t = cal
            .next_deadline(now)
            .map(|d| d.saturating_duration_since(now));
        let fds: Vec<_> = devs.iter().map(|d| d.as_raw_fd()).collect();
        let ready = sys::poll(&fds, t)?;
        for (i, r) in ready.iter().enumerate() {
            if !r.readable && !r.gone {
                continue;
            }
            let events: Vec<_> = match devs[i].fetch_events() {
                Ok(it) => it.collect(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(e) => bail!("dispositivo sumiu: {e}"),
            };
            for ev in events {
                let (t, c, v) = (ev.event_type().0, ev.code(), ev.value());
                state.apply(t, c, v);
                if t != EV_SYN {
                    let evs = cal.feed(t, c, v, &state, Instant::now());
                    handle(evs, &mut rules);
                }
            }
        }
        let evs = cal.tick(&state, Instant::now());
        handle(evs, &mut rules);
    }
    let rules = rules.unwrap_or_default();
    let n = rules.len();
    let m = Mapping {
        device: Some(format_vid_pid(g.vendor, g.product)),
        rules,
        ..Default::default()
    };
    sys::write_atomic(&map, &(m.to_json_pretty() + "\n"))
        .with_context(|| format!("gravando {}", map.display()))?;
    println!(
        "\nMapeamento gravado em {} ({n}/{} itens).",
        map.display(),
        Target::ALL.len()
    );
    Ok(())
}

fn print_cal(ev: &CalEvent) {
    match ev {
        CalEvent::Step { target, .. } => {
            print!("  -> {} ... ", target.prompt());
            let _ = std::io::stdout().flush();
        }
        CalEvent::Hit { rule, clash, .. } => {
            let note = if clash.is_empty() {
                String::new()
            } else {
                let c: Vec<_> = clash.iter().map(|t| t.name()).collect();
                format!("  (ATENÇÃO: igual a {})", c.join(", "))
            };
            println!("OK  {}{note}", rule.describe());
        }
        CalEvent::Skipped { .. } => println!("pulado"),
        CalEvent::Finished { .. } => {}
    }
}

// ---------------------------------------------------------------------- D-Bus

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Active => "Ativo",
        Status::Off => "Ponte desligada",
        Status::Waiting => "Aguardando",
        Status::Unmapped => "Sem mapeamento",
        Status::Calibrating => "Calibrando",
        Status::Error => "Erro",
    }
}

fn resolve(list: &[ControllerInfo], id: Option<String>) -> Result<String> {
    if let Some(id) = id {
        if list.iter().any(|c| c.id == id) {
            return Ok(id);
        }
        // Aceita um prefixo do id, ou o VID:PID quando só houver um.
        let m: Vec<_> = list
            .iter()
            .filter(|c| c.id.starts_with(&id) || c.device == id)
            .collect();
        return match m.as_slice() {
            [c] => Ok(c.id.clone()),
            [] => bail!("controle {id:?} não existe; veja `gamesir-bridge status`"),
            _ => bail!("{id:?} combina com mais de um controle; use o id completo"),
        };
    }
    let connected: Vec<_> = list.iter().filter(|c| c.connected).collect();
    match connected.as_slice() {
        [c] => Ok(c.id.clone()),
        [] => bail!("nenhum controle conectado"),
        _ => bail!("há mais de um controle; diga qual (veja `gamesir-bridge status`)"),
    }
}

async fn remote(session: bool, cmd: Cmd) -> Result<()> {
    let p = client::connect(session).await.context(
        "não consegui falar com o serviço; ele está rodando? (systemctl status gamesir-bridge)",
    )?;
    let list = client::controllers(&p).await?;
    match cmd {
        Cmd::Status => {
            if list.is_empty() {
                println!("Nenhum controle conhecido.");
            }
            for c in &list {
                let player = c
                    .player
                    .map(|p| format!(" · Jogador {p}"))
                    .unwrap_or_default();
                println!("{}  ({}){player}", c.name, c.device);
                println!("  estado: {}", status_word(c.status));
                println!("  id: {}", c.id);
                if let Some(pn) = &c.profile_name {
                    println!(
                        "  mapeamento: {pn} ({}) · {} de {} itens",
                        c.mapping_source.as_deref().unwrap_or("?"),
                        c.mapped,
                        c.total
                    );
                }
                if !c.inputs.is_empty() {
                    let i: Vec<_> = c.inputs.iter().map(|i| i.path.as_str()).collect();
                    println!("  entradas: {}", i.join(" + "));
                }
                if let Some(o) = &c.output {
                    println!("  saída virtual: 045e:028e · {o}");
                }
                if let Some(e) = &c.error {
                    println!("  erro: {e}");
                }
                for w in &c.warnings {
                    println!("  aviso: {w}");
                }
            }
        }
        Cmd::Enable { id } => p.set_enabled(&resolve(&list, id)?, true).await?,
        Cmd::Disable { id } => p.set_enabled(&resolve(&list, id)?, false).await?,
        Cmd::Mapping { id } => println!("{}", p.get_mapping(&resolve(&list, id)?).await?),
        Cmd::Reload => p.reload().await?,
        Cmd::Calibrate { id, timeout, .. } => {
            calibrate_remote(&p, resolve(&list, id)?, timeout).await?
        }
        Cmd::List { .. } | Cmd::Setup { .. } => unreachable!(),
    }
    Ok(())
}

async fn calibrate_remote(p: &BridgeProxy<'_>, id: String, timeout: f64) -> Result<()> {
    let mut step = p.receive_calibration_step().await?;
    let mut hit = p.receive_calibration_hit().await?;
    let mut skipped = p.receive_calibration_skipped().await?;
    let mut done = p.receive_calibration_finished().await?;
    p.start_calibration(&id, timeout).await?;
    println!("Para cada item: aperte e segure até aparecer OK. Sem input em {timeout:.0}s o item é pulado.");
    println!("Ctrl+C cancela sem gravar.\n");
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    loop {
        tokio::select! {
            Some(s) = step.next() => {
                let a = s.args()?;
                if a.id == id {
                    print!("  -> {} ... ", a.prompt);
                    let _ = std::io::stdout().flush();
                }
            }
            Some(s) = hit.next() => {
                let a = s.args()?;
                if a.id == id {
                    let note = if a.clash.is_empty() { String::new() }
                        else { format!("  (ATENÇÃO: igual a {})", a.clash.join(", ")) };
                    println!("OK  {}{note}", a.description);
                }
            }
            Some(s) = skipped.next() => {
                if s.args()?.id == id { println!("pulado"); }
            }
            Some(s) = done.next() => {
                let a = s.args()?;
                if a.id == id {
                    println!("\n{}", a.message);
                    return if a.saved { Ok(()) } else { Err(anyhow!("nada foi gravado")) };
                }
            }
            _ = &mut ctrl_c => {
                p.calibration_control(&id, "cancel").await?;
                println!("\nInterrompido; nada foi gravado.");
                return Ok(());
            }
        }
    }
}
