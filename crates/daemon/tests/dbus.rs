//! Teste de ponta a ponta da interface D-Bus, num barramento de sessão privado e com um
//! sysfs falso (dois dongles T4 idênticos). Não precisa de /dev/input nem de /dev/uinput:
//! os workers não conseguem abrir os dispositivos falsos, e o serviço tem que lidar com isso.
//!
//! Precisa do `dbus-daemon` instalado; sem ele o teste é pulado.

use gamesir_bridged::config::Paths;
use gamesir_bridged::Daemon;
use gamesir_core::api::Status;
use gamesir_core::client;
use std::process::{Command, Stdio};
use std::time::Duration;

const T4: &str = include_str!("../../../profiles/gamesir-t4-cyclone-pro-dongle.json");

struct Bus(std::process::Child);
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn private_bus() -> Option<(Bus, String)> {
    let mut child = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address"])
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let mut line = String::new();
    use std::io::Read;
    let mut buf = [0u8; 1];
    while out.read(&mut buf).ok()? == 1 && buf[0] != b'\n' {
        line.push(buf[0] as char);
    }
    Some((Bus(child), line.trim().to_owned()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn servico_por_dbus() {
    let Some((_bus, addr)) = private_bus() else {
        eprintln!("dbus-daemon não encontrado; pulando");
        return;
    };
    // SAFETY: único teste deste binário; ninguém lê o ambiente em paralelo.
    unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &addr) };

    let dir = tempfile::tempdir().unwrap();
    gamesir_core::device::testing::fake_sysfs(&dir.path().join("sys"));
    let paths = Paths {
        system_profiles: dir.path().join("share"),
        config_dir: dir.path().join("etc"),
        sys_root: dir.path().join("sys"),
        dev_input: dir.path().join("dev"),
        udev_rule: None,
    };
    std::fs::create_dir_all(&paths.system_profiles).unwrap();
    std::fs::write(paths.system_profiles.join("t4.json"), T4).unwrap();

    let daemon = Daemon::new(paths.clone());
    daemon.rescan();
    let d = daemon.clone();
    tokio::spawn(async move { gamesir_bridged::dbus::serve(d, false).await.unwrap() });

    // Espera o serviço aparecer no barramento.
    let mut proxy = None;
    for _ in 0..50 {
        if let Ok(p) = client::connect(true).await {
            if p.version().await.is_ok() {
                proxy = Some(p);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let p = proxy.expect("serviço não respondeu");
    assert_eq!(p.version().await.unwrap(), gamesir_core::VERSION);

    // Dois dongles iguais viram dois controles, cada um com o perfil do T4.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let list = client::controllers(&p).await.unwrap();
    assert_eq!(list.len(), 2, "{list:#?}");
    for c in &list {
        assert_eq!(c.device, "3537:1007");
        assert_eq!(
            c.profile_id.as_deref(),
            Some("gamesir-t4-cyclone-pro-dongle")
        );
        assert_eq!(c.name, "GameSir T4 Cyclone Pro");
        assert_eq!(c.mapped, 21);
        // Os /dev/input/event* falsos não abrem: o controle fica aguardando, sem derrubar nada.
        assert_eq!(c.status, Status::Waiting);
    }
    let id = list[0].id.clone();

    // Mapeamento: leitura, gravação válida e inválida.
    let m = p.get_mapping(&id).await.unwrap();
    assert!(m.contains("\"HOME\""));
    let err = p.save_mapping(&id, "{ quebrado").await.unwrap_err();
    assert!(err.to_string().contains("JSON inválido"), "{err}");
    let mut m: serde_json::Value = serde_json::from_str(&m).unwrap();
    m["options"] = serde_json::json!({"deadzone": 0.08});
    let path = p.save_mapping(&id, &m.to_string()).await.unwrap();
    assert!(path.ends_with("mappings/gamesir-t4-cyclone-pro-dongle.json"));
    let list = client::controllers(&p).await.unwrap();
    assert_eq!(list[0].mapping_source.as_deref(), Some("calibrated"));
    p.reset_mapping(&id).await.unwrap();
    let list = client::controllers(&p).await.unwrap();
    assert_eq!(list[0].mapping_source.as_deref(), Some("official"));

    // Ponte desligada fica gravada.
    p.set_enabled(&id, false).await.unwrap();
    let s = std::fs::read_to_string(paths.settings()).unwrap();
    assert!(s.contains("\"enabled\": false"), "{s}");
    assert!(!client::controllers(&p).await.unwrap()[0].enabled);

    // Calibrar exige o controle conectado.
    let err = p.start_calibration(&id, 10.0).await.unwrap_err();
    assert!(err.to_string().contains("não está conectado"), "{err}");
    let err = p.set_enabled("nao-existe", true).await.unwrap_err();
    assert!(err.to_string().contains("não existe"), "{err}");

    // Recarregar com um perfil do usuário quebrado não derruba o serviço.
    std::fs::create_dir_all(paths.user_profiles()).unwrap();
    std::fs::write(paths.user_profiles().join("ruim.json"), "{").unwrap();
    p.reload().await.unwrap();
    assert_eq!(client::controllers(&p).await.unwrap().len(), 2);
}
