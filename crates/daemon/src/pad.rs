//! O controle Xbox 360 virtual (uinput).

use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, InputEvent, InputId, KeyCode, UinputAbsSetup,
};
use gamesir_core::codes::{ABS_HAT0X, ABS_HAT0Y};
use gamesir_core::mapper::{
    neutral_state, OutputState, OUT_KEYS, OUT_STICKS, OUT_TRIGGERS, TRIGGER_MAX,
};
use std::collections::BTreeMap;
use std::io;

/// Mesma identidade que o xpad dá a um controle 360 com fio: Steam/SDL já conhecem o
/// layout, então não precisa de mapeamento manual em lugar nenhum.
pub const PAD_NAME: &str = "Microsoft X-Box 360 pad";
pub const PAD_VENDOR: u16 = 0x045e;
pub const PAD_PRODUCT: u16 = 0x028e;
pub const PAD_VERSION: u16 = 0x0110;

pub struct Pad {
    dev: VirtualDevice,
    last: BTreeMap<(u16, u16), i32>,
    pub path: Option<String>,
}

impl std::fmt::Debug for Pad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pad").field("path", &self.path).finish()
    }
}

impl Pad {
    pub fn create() -> io::Result<Pad> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for (_, code) in OUT_KEYS {
            keys.insert(KeyCode(code));
        }
        let stick = AbsInfo::new(0, -32768, 32767, 16, 128, 0);
        let trig = AbsInfo::new(0, 0, TRIGGER_MAX, 0, 0, 0);
        let hat = AbsInfo::new(0, -1, 1, 0, 0, 0);
        let mut b = VirtualDevice::builder()?
            .name(PAD_NAME)
            .input_id(InputId::new(
                BusType::BUS_USB,
                PAD_VENDOR,
                PAD_PRODUCT,
                PAD_VERSION,
            ))
            .with_keys(&keys)?;
        for (_, code) in OUT_STICKS {
            b = b.with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(code), stick))?;
        }
        for (_, code) in OUT_TRIGGERS {
            b = b.with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(code), trig))?;
        }
        for code in [ABS_HAT0X, ABS_HAT0Y] {
            b = b.with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(code), hat))?;
        }
        let mut dev = b.build()?;
        let path = dev
            .enumerate_dev_nodes_blocking()
            .ok()
            .and_then(|mut it| it.find_map(|p| p.ok()))
            .map(|p| p.display().to_string());
        let mut pad = Pad {
            dev,
            last: BTreeMap::new(),
            path,
        };
        pad.emit(&neutral_state())?;
        Ok(pad)
    }

    /// Envia só o que mudou, com um SYN_REPORT no fim (como o `emit` do script).
    pub fn emit(&mut self, state: &OutputState) -> io::Result<()> {
        let changed = state.diff(&mut self.last);
        if changed.is_empty() {
            return Ok(());
        }
        let events: Vec<InputEvent> = changed
            .iter()
            .map(|&(t, c, v)| InputEvent::new(t, c, v))
            .collect();
        self.dev.emit(&events)
    }

    /// Solta tudo que estiver apertado.
    pub fn release_all(&mut self) {
        if let Err(e) = self.emit(&neutral_state()) {
            log::warn!("não consegui soltar os botões do controle virtual: {e}");
        }
    }
}
