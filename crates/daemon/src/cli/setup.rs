//! `gamesir-bridge setup`: instala o serviço a partir do AppImage.
//!
//! O AppImage roda montado por FUSE só para o usuário, então o root não enxerga os
//! arquivos dele. Por isso o conteúdo de instalação (`payload`: binários, `install.sh`,
//! regra udev, unit do systemd, políticas) é copiado antes para um diretório temporário
//! comum, e só então o `install.sh` roda como root via `pkexec`.

use anyhow::{bail, Context, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const MANIFEST: &str = "/etc/gamesir-bridge/installed-files";
const VERSION_FILE: &str = "/etc/gamesir-bridge/installed-version";

/// Onde está o payload: `$APPDIR/usr/share/gamesir-bridge/payload` dentro do AppImage, ou
/// ao lado do executável (`../share/gamesir-bridge/payload`) num tarball.
fn payload_dir() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(appdir) = std::env::var_os("APPDIR") {
        candidates.push(PathBuf::from(appdir).join("usr/share/gamesir-bridge/payload"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin) = exe.parent() {
            candidates.push(bin.join("../share/gamesir-bridge/payload"));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.join("install.sh").is_file())
        .context(
            "conteúdo de instalação não encontrado; use o AppImage ou o tarball da release \\
             (a partir do código-fonte: sudo ./install.sh)",
        )
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dst = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dst)?;
        } else {
            std::fs::copy(e.path(), &dst)?;
        }
    }
    Ok(())
}

pub fn installed_version() -> Option<String> {
    std::fs::read_to_string(VERSION_FILE)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|_| Path::new(MANIFEST).exists())
}

fn as_root(script: &Path, args: &[&str]) -> Result<()> {
    let is_root = unsafe { libc::geteuid() } == 0;
    let mut cmd = if is_root {
        Command::new(script)
    } else {
        let pkexec = ["/usr/bin/pkexec", "/bin/pkexec"]
            .into_iter()
            .find(|p| Path::new(p).exists());
        let Some(pkexec) = pkexec else {
            bail!(
                "pkexec não encontrado. Rode como root: sudo {} {}",
                script.display(),
                args.join(" ")
            );
        };
        let mut c = Command::new(pkexec);
        c.arg(script);
        c
    };
    let status = cmd.args(args).status().context("executando o instalador")?;
    match status.code() {
        Some(0) => Ok(()),
        Some(126) | Some(127) if !is_root => bail!("autenticação cancelada ou negada"),
        _ => bail!("o instalador falhou ({status})"),
    }
}

pub fn run(uninstall: bool, if_needed: bool, check: bool) -> Result<()> {
    let version = gamesir_core::VERSION;
    let installed = installed_version();
    if check {
        match &installed {
            Some(v) => println!("instalado: {v} (este: {version})"),
            None => {
                println!("não instalado (este: {version})");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if if_needed && !uninstall && installed.as_deref() == Some(version) {
        println!("GameSir Bridge {version} já está instalado.");
        return Ok(());
    }

    let payload = payload_dir()?;
    let tmp = std::env::temp_dir().join(format!("gamesir-bridge-setup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    copy_dir(&payload, &tmp).with_context(|| format!("copiando para {}", tmp.display()))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700))?;
    for f in [
        "install.sh",
        "uninstall.sh",
        "bin/gamesir-bridged",
        "bin/gamesir-bridge",
    ] {
        let p = tmp.join(f);
        if p.exists() {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    let result = if uninstall {
        println!("Removendo o GameSir Bridge do sistema (vai pedir sua senha)...");
        as_root(&tmp.join("uninstall.sh"), &[])
    } else {
        println!(
            "Instalando o GameSir Bridge {version}{} (vai pedir sua senha)...",
            installed
                .map(|v| format!(" sobre o {v}"))
                .unwrap_or_default()
        );
        let bindir = tmp.join("bin");
        as_root(
            &tmp.join("install.sh"),
            &[
                "--bindir",
                bindir.to_str().context("caminho temporário inválido")?,
            ],
        )
    };
    let _ = std::fs::remove_dir_all(&tmp);
    result
}
