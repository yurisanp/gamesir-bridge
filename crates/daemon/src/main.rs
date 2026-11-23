//! `gamesir-bridged`: o serviço que roda como root (systemd) e faz a ponte.

use clap::Parser;
use gamesir_bridged::config::Paths;
use gamesir_bridged::sys::{self, DirWatch};
use gamesir_bridged::Daemon;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Junta as interfaces de cada controle num Xbox 360 virtual e atende o app por D-Bus.
#[derive(Parser, Debug)]
#[command(name = "gamesir-bridged", version)]
struct Args {
    /// Diretório dos perfis oficiais.
    #[arg(long, default_value = "/usr/share/gamesir-bridge/profiles")]
    profiles: PathBuf,
    /// Diretório de configuração (mapeamentos, perfis do usuário, preferências).
    #[arg(long, default_value = "/etc/gamesir-bridge")]
    config: PathBuf,
    /// Publica no barramento de sessão em vez do de sistema (para desenvolvimento).
    #[arg(long)]
    session_bus: bool,
    /// Não publica nada no D-Bus; só faz a ponte (como o `run` antigo).
    #[arg(long)]
    no_dbus: bool,
    /// Não gera a regra udev para perfis do usuário.
    #[arg(long)]
    no_udev: bool,
    /// Raiz do sysfs (testes).
    #[arg(long, default_value = "/sys", hide = true)]
    sysfs: PathBuf,
    /// Diretório dos dispositivos de entrada (testes).
    #[arg(long, default_value = "/dev/input", hide = true)]
    dev_input: PathBuf,
    /// Mais detalhes no log.
    #[arg(short, long)]
    verbose: bool,
}

/// Observa /dev/input e relê o sysfs quando algo entra ou sai (e a cada 5 s, por garantia).
fn hotplug(daemon: Arc<Daemon>) {
    let dir = daemon.paths.dev_input.clone();
    let watch = match DirWatch::new(&dir) {
        Ok(w) => Some(w),
        Err(e) => {
            log::warn!("sem inotify em {}: {e}; relendo a cada 2 s", dir.display());
            None
        }
    };
    loop {
        match &watch {
            Some(w) => {
                let _ = sys::poll(&[w.as_raw_fd()], Some(Duration::from_secs(5)));
                if w.drain() {
                    // Junta a rajada de eventos de um replug (as duas interfaces chegam juntas).
                    std::thread::sleep(Duration::from_millis(300));
                    w.drain();
                }
            }
            None => std::thread::sleep(Duration::from_secs(2)),
        }
        daemon.rescan();
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(if args.verbose { "debug" } else { "info" }),
    )
    .format_timestamp(None)
    .init();

    let paths = Paths {
        system_profiles: args.profiles,
        config_dir: args.config,
        udev_rule: if args.no_udev {
            None
        } else {
            Paths::default().udev_rule
        },
        sys_root: args.sysfs,
        dev_input: args.dev_input,
    };
    log::info!("gamesir-bridged {}", gamesir_core::VERSION);
    let daemon = Daemon::new(paths);
    daemon.rescan();

    let d = daemon.clone();
    std::thread::Builder::new()
        .name("hotplug".into())
        .spawn(move || hotplug(d))?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let result = rt.block_on(async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut hup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
        let serve = async {
            if args.no_dbus {
                std::future::pending::<anyhow::Result<()>>().await
            } else {
                if let Err(e) =
                    gamesir_bridged::dbus::serve(daemon.clone(), !args.session_bus).await
                {
                    // Sem D-Bus o app não funciona, mas a ponte continua (nada de reiniciar em laço).
                    log::error!("D-Bus indisponível ({e:#}); seguindo só com a ponte");
                }
                std::future::pending::<anyhow::Result<()>>().await
            }
        };
        tokio::pin!(serve);
        loop {
            tokio::select! {
                r = &mut serve => break r,
                _ = term.recv() => break Ok(()),
                _ = tokio::signal::ctrl_c() => break Ok(()),
                _ = hup.recv() => {
                    log::info!("SIGHUP: relendo configuração");
                    let d = daemon.clone();
                    tokio::task::spawn_blocking(move || d.reload());
                }
            }
        }
    });
    log::info!("encerrando; soltando os controles");
    daemon.shutdown();
    result
}
