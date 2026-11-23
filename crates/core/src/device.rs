//! Descoberta de dispositivos pelo sysfs e agrupamento por aparelho físico.
//!
//! O dongle expõe duas interfaces (gamepad e "Consumer Control" com o Home). O script
//! Python juntava as duas só pelo VID/PID, então dois dongles iguais trocavam os Homes.
//! Aqui as interfaces são agrupadas pelo dispositivo USB de onde vêm (o diretório do
//! sysfs que tem `idVendor`), e, fora do USB (Bluetooth), pelo `phys` sem o `/inputN`
//! mais o `uniq`.
//!
//! Tudo é lido de `/sys/class/input`, sem abrir nenhum `/dev/input/event*`; por isso as
//! funções recebem a raiz do sysfs e podem ser testadas com uma árvore falsa.

use crate::codes::GAMEPAD_KEY_RANGE;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Uma interface `/dev/input/eventN`, como o sysfs a descreve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputNode {
    /// `event17`
    pub node: String,
    /// `/dev/input/event17`
    pub dev_path: PathBuf,
    pub name: String,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
    pub phys: String,
    pub uniq: String,
    /// Caminho canônico do dispositivo no sysfs (`/sys/devices/...`).
    pub sys_path: PathBuf,
    /// Caminho do dispositivo USB pai, quando houver.
    pub usb_path: Option<PathBuf>,
    pub keys: BTreeSet<u16>,
    pub abs: BTreeSet<u16>,
}

impl InputNode {
    pub fn is_virtual(&self) -> bool {
        self.sys_path
            .components()
            .any(|c| c.as_os_str() == "virtual")
    }

    pub fn looks_like_gamepad(&self) -> bool {
        !self.abs.is_empty() && self.keys.iter().any(|k| GAMEPAD_KEY_RANGE.contains(k))
    }

    /// Chave do aparelho físico ao qual a interface pertence.
    pub fn group_key(&self) -> String {
        match &self.usb_path {
            Some(p) => format!("usb:{}", p.display()),
            None => {
                let base = strip_input_suffix(&self.phys);
                if base.is_empty() {
                    format!("sys:{}", self.sys_path.display())
                } else if self.uniq.is_empty() {
                    format!("phys:{base}")
                } else {
                    format!("phys:{base}#{}", self.uniq)
                }
            }
        }
    }
}

/// `usb-0000:00:14.0-1.2/input0` → `usb-0000:00:14.0-1.2`.
pub fn strip_input_suffix(phys: &str) -> &str {
    match phys.rsplit_once('/') {
        Some((base, last)) if last.starts_with("input") => base,
        _ => phys,
    }
}

/// Lê um bitmap de capacidades do sysfs (`capabilities/key`): palavras hexadecimais de
/// `long`, a mais significativa primeiro.
pub fn parse_bitmap(text: &str) -> BTreeSet<u16> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = BTreeSet::new();
    for (i, w) in words.iter().rev().enumerate() {
        let Ok(bits) = u64::from_str_radix(w, 16) else {
            continue;
        };
        for b in 0..64 {
            if bits & (1u64 << b) != 0 {
                if let Ok(code) = u16::try_from(i * 64 + b) {
                    out.insert(code);
                }
            }
        }
    }
    out
}

fn read_trim(p: &Path) -> String {
    std::fs::read_to_string(p)
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

fn read_hex(p: &Path) -> u16 {
    u16::from_str_radix(&read_trim(p), 16).unwrap_or(0)
}

/// Lê uma interface a partir de `<sys>/class/input/eventN`.
pub fn read_node(class_dir: &Path, dev_dir: &Path, node: &str) -> Option<InputNode> {
    let event = class_dir.join(node);
    // `eventN/device` aponta para o `inputM`, que tem nome, ids e capacidades.
    let input = std::fs::canonicalize(event.join("device")).ok()?;
    let usb_path = input
        .ancestors()
        .skip(1)
        .find(|p| p.join("idVendor").is_file() && p.join("idProduct").is_file())
        .map(Path::to_path_buf);
    Some(InputNode {
        node: node.to_owned(),
        dev_path: dev_dir.join(node),
        name: read_trim(&input.join("name")),
        vendor: read_hex(&input.join("id/vendor")),
        product: read_hex(&input.join("id/product")),
        version: read_hex(&input.join("id/version")),
        phys: read_trim(&input.join("phys")),
        uniq: read_trim(&input.join("uniq")),
        keys: parse_bitmap(&read_trim(&input.join("capabilities/key"))),
        abs: parse_bitmap(&read_trim(&input.join("capabilities/abs"))),
        sys_path: input,
        usb_path,
    })
}

/// Todas as interfaces `eventN` do sistema.
pub fn scan(sys_root: &Path, dev_dir: &Path) -> Vec<InputNode> {
    let class_dir = sys_root.join("class/input");
    let Ok(entries) = std::fs::read_dir(&class_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("event"))
        .collect();
    names.sort_by_key(|n| n[5..].parse::<u32>().unwrap_or(u32::MAX));
    names
        .iter()
        .filter_map(|n| read_node(&class_dir, dev_dir, n))
        .collect()
}

/// Um aparelho físico: todas as interfaces que vêm do mesmo dispositivo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceGroup {
    pub key: String,
    pub vendor: u16,
    pub product: u16,
    pub nodes: Vec<InputNode>,
}

impl DeviceGroup {
    /// Nome para mostrar: o da interface que parece gamepad, senão o da primeira.
    pub fn name(&self) -> &str {
        self.nodes
            .iter()
            .find(|n| n.looks_like_gamepad())
            .or(self.nodes.first())
            .map(|n| n.name.as_str())
            .unwrap_or("")
    }

    pub fn looks_like_gamepad(&self) -> bool {
        self.nodes.iter().any(InputNode::looks_like_gamepad)
    }

    pub fn uniq(&self) -> &str {
        self.nodes
            .iter()
            .map(|n| n.uniq.as_str())
            .find(|u| !u.is_empty())
            .unwrap_or("")
    }

    /// Identificador estável (para guardar preferências): VID:PID + serial, ou + porta.
    pub fn stable_id(&self) -> String {
        let vp = crate::mapping::format_vid_pid(self.vendor, self.product);
        let uniq = self.uniq();
        let tail = if !uniq.is_empty() {
            uniq.to_owned()
        } else if let Some(usb) = self.key.strip_prefix("usb:") {
            // `/sys/devices/.../usb3/3-1.2` → `usb-3-1.2` (barramento e porta).
            let port = Path::new(usb)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("usb-{port}")
        } else {
            self.key.clone()
        };
        let tail: String = tail
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{}-{tail}", vp.replace(':', "_"))
    }
}

/// Agrupa interfaces por aparelho físico. Ignora dispositivos virtuais (inclusive os
/// controles Xbox que o próprio serviço cria).
pub fn group(nodes: &[InputNode]) -> Vec<DeviceGroup> {
    let mut by_key: BTreeMap<String, DeviceGroup> = BTreeMap::new();
    for n in nodes.iter().filter(|n| !n.is_virtual()) {
        let key = n.group_key();
        by_key
            .entry(key.clone())
            .or_insert_with(|| DeviceGroup {
                key,
                vendor: n.vendor,
                product: n.product,
                nodes: Vec::new(),
            })
            .nodes
            .push(n.clone());
    }
    by_key.into_values().collect()
}

/// Ajuda para testes: árvore sysfs falsa com dois dongles T4 idênticos e um pad virtual.
#[doc(hidden)]
pub mod testing {
    use std::fs;
    use std::path::Path;

    /// Monta uma árvore sysfs falsa com dois dongles idênticos em portas diferentes.
    pub fn fake_sysfs(root: &Path) {
        let mut n = 10;
        for port in ["1.2", "1.3"] {
            let usb = root.join(format!("devices/pci0000:00/0000:00:14.0/usb3/3-{port}"));
            fs::create_dir_all(&usb).unwrap();
            fs::write(usb.join("idVendor"), "3537\n").unwrap();
            fs::write(usb.join("idProduct"), "1007\n").unwrap();
            for (iface, name, key, abs) in [
                ("0", "GameSir-Dongle", "7fff000000000000 0 0 0 0", "30027"),
                (
                    "1",
                    "GameSir-Dongle Consumer Control",
                    "100000000000 0 0",
                    "0",
                ),
            ] {
                let input = usb.join(format!(
                    "3-{port}:1.{iface}/0003:3537:1007.000{n}/input/input{n}"
                ));
                fs::create_dir_all(input.join("id")).unwrap();
                fs::create_dir_all(input.join("capabilities")).unwrap();
                fs::write(input.join("name"), format!("{name}\n")).unwrap();
                fs::write(input.join("id/vendor"), "3537\n").unwrap();
                fs::write(input.join("id/product"), "1007\n").unwrap();
                fs::write(input.join("id/version"), "0101\n").unwrap();
                fs::write(
                    input.join("phys"),
                    format!("usb-0000:00:14.0-{port}/input{iface}\n"),
                )
                .unwrap();
                fs::write(input.join("uniq"), "\n").unwrap();
                fs::write(input.join("capabilities/key"), format!("{key}\n")).unwrap();
                fs::write(input.join("capabilities/abs"), format!("{abs}\n")).unwrap();
                let ev = input.join(format!("event{n}"));
                fs::create_dir_all(&ev).unwrap();
                fs::create_dir_all(root.join("class/input")).unwrap();
                std::os::unix::fs::symlink(&ev, root.join(format!("class/input/event{n}")))
                    .unwrap();
                std::os::unix::fs::symlink(&input, ev.join("device")).unwrap();
                n += 1;
            }
        }
        // Um controle virtual (o nosso): precisa ser ignorado.
        let v = root.join("devices/virtual/input/input99");
        fs::create_dir_all(v.join("id")).unwrap();
        fs::create_dir_all(v.join("capabilities")).unwrap();
        fs::write(v.join("name"), "Microsoft X-Box 360 pad\n").unwrap();
        fs::write(v.join("id/vendor"), "045e\n").unwrap();
        fs::write(v.join("id/product"), "028e\n").unwrap();
        fs::write(v.join("capabilities/key"), "7fdb000000000000 0 0 0 0\n").unwrap();
        fs::write(v.join("capabilities/abs"), "3003f\n").unwrap();
        let ev = v.join("event99");
        fs::create_dir_all(&ev).unwrap();
        std::os::unix::fs::symlink(&ev, root.join("class/input/event99")).unwrap();
        std::os::unix::fs::symlink(&v, ev.join("device")).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::testing::fake_sysfs;
    use super::*;

    #[test]
    fn bitmap() {
        let keys = parse_bitmap("7fff000000000000 0 0 0 0");
        assert!(keys.contains(&0x130) && keys.contains(&0x13e) && !keys.contains(&0x13f));
        assert_eq!(parse_bitmap("100000000000 0 0"), BTreeSet::from([172]));
        assert_eq!(parse_bitmap("30027"), BTreeSet::from([0, 1, 2, 5, 16, 17]));
        assert!(parse_bitmap("0").is_empty());
        assert!(parse_bitmap("").is_empty());
    }

    #[test]
    fn phys() {
        assert_eq!(
            strip_input_suffix("usb-0000:00:14.0-1.2/input0"),
            "usb-0000:00:14.0-1.2"
        );
        assert_eq!(strip_input_suffix("aa:bb:cc:dd:ee:ff"), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn dois_dongles_iguais_nao_se_misturam() {
        let dir = tempfile::tempdir().unwrap();
        fake_sysfs(dir.path());
        let nodes = scan(dir.path(), Path::new("/dev/input"));
        assert_eq!(nodes.len(), 5);
        let groups = group(&nodes);
        assert_eq!(groups.len(), 2, "{groups:#?}");
        for g in &groups {
            assert_eq!((g.vendor, g.product), (0x3537, 0x1007));
            assert_eq!(g.nodes.len(), 2);
            assert!(g.looks_like_gamepad());
            assert_eq!(g.name(), "GameSir-Dongle");
            // Cada grupo tem um gamepad e um Home, da mesma porta.
            let ports: BTreeSet<_> = g
                .nodes
                .iter()
                .map(|n| strip_input_suffix(&n.phys).to_owned())
                .collect();
            assert_eq!(ports.len(), 1);
            assert!(g.nodes.iter().any(|n| n.keys.contains(&172)));
        }
        assert_eq!(groups[0].stable_id(), "3537_1007-usb-3-1.2");
        assert_eq!(groups[1].stable_id(), "3537_1007-usb-3-1.3");
        assert_eq!(nodes[0].dev_path, Path::new("/dev/input/event10"));
    }

    #[test]
    fn bluetooth_agrupa_por_phys_e_uniq() {
        let mk = |uniq: &str, phys: &str| InputNode {
            node: "event1".into(),
            dev_path: "/dev/input/event1".into(),
            name: "x".into(),
            vendor: 1,
            product: 2,
            version: 0,
            phys: phys.into(),
            uniq: uniq.into(),
            sys_path: "/sys/devices/bt/input1".into(),
            usb_path: None,
            keys: BTreeSet::new(),
            abs: BTreeSet::new(),
        };
        let a = mk("aa", "00:11:22:33:44:55");
        let b = mk("bb", "00:11:22:33:44:55");
        assert_ne!(a.group_key(), b.group_key());
        assert_eq!(group(&[a.clone(), b, a]).len(), 2);
    }
}
