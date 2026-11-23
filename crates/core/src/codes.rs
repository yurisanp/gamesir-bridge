//! Códigos evdev usados pelo projeto (de `linux/input-event-codes.h`).
//!
//! O core não depende de nenhuma biblioteca de input: só precisa dos números.

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;

pub const SYN_REPORT: u16 = 0;

pub const KEY_HOMEPAGE: u16 = 172;

pub const BTN_JOYSTICK: u16 = 0x120;
pub const BTN_A: u16 = 0x130;
pub const BTN_B: u16 = 0x131;
pub const BTN_C: u16 = 0x132;
pub const BTN_X: u16 = 0x133;
pub const BTN_Y: u16 = 0x134;
pub const BTN_Z: u16 = 0x135;
pub const BTN_TL: u16 = 0x136;
pub const BTN_TR: u16 = 0x137;
pub const BTN_TL2: u16 = 0x138;
pub const BTN_TR2: u16 = 0x139;
pub const BTN_SELECT: u16 = 0x13a;
pub const BTN_START: u16 = 0x13b;
pub const BTN_MODE: u16 = 0x13c;
pub const BTN_THUMBL: u16 = 0x13d;
pub const BTN_THUMBR: u16 = 0x13e;

pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const ABS_Z: u16 = 0x02;
pub const ABS_RX: u16 = 0x03;
pub const ABS_RY: u16 = 0x04;
pub const ABS_RZ: u16 = 0x05;
pub const ABS_THROTTLE: u16 = 0x06;
pub const ABS_RUDDER: u16 = 0x07;
pub const ABS_WHEEL: u16 = 0x08;
pub const ABS_GAS: u16 = 0x09;
pub const ABS_BRAKE: u16 = 0x0a;
pub const ABS_HAT0X: u16 = 0x10;
pub const ABS_HAT0Y: u16 = 0x11;

/// Faixa de teclas que indica um gamepad/joystick (BTN_JOYSTICK..BTN_THUMBR).
pub const GAMEPAD_KEY_RANGE: std::ops::RangeInclusive<u16> = BTN_JOYSTICK..=BTN_THUMBR;

/// Nome legível de uma tecla/botão evdev, quando conhecido.
pub fn key_name(code: u16) -> Option<&'static str> {
    Some(match code {
        KEY_HOMEPAGE => "KEY_HOMEPAGE",
        0x120 => "BTN_TRIGGER",
        0x121 => "BTN_THUMB",
        0x122 => "BTN_THUMB2",
        0x123 => "BTN_TOP",
        0x124 => "BTN_TOP2",
        0x125 => "BTN_PINKIE",
        0x126 => "BTN_BASE",
        0x127 => "BTN_BASE2",
        0x128 => "BTN_BASE3",
        0x129 => "BTN_BASE4",
        0x12a => "BTN_BASE5",
        0x12b => "BTN_BASE6",
        0x12f => "BTN_DEAD",
        BTN_A => "BTN_SOUTH",
        BTN_B => "BTN_EAST",
        BTN_C => "BTN_C",
        BTN_X => "BTN_NORTH",
        BTN_Y => "BTN_WEST",
        BTN_Z => "BTN_Z",
        BTN_TL => "BTN_TL",
        BTN_TR => "BTN_TR",
        BTN_TL2 => "BTN_TL2",
        BTN_TR2 => "BTN_TR2",
        BTN_SELECT => "BTN_SELECT",
        BTN_START => "BTN_START",
        BTN_MODE => "BTN_MODE",
        BTN_THUMBL => "BTN_THUMBL",
        BTN_THUMBR => "BTN_THUMBR",
        _ => return None,
    })
}

/// Nome legível de um eixo absoluto evdev, quando conhecido.
pub fn abs_name(code: u16) -> Option<&'static str> {
    Some(match code {
        ABS_X => "ABS_X",
        ABS_Y => "ABS_Y",
        ABS_Z => "ABS_Z",
        ABS_RX => "ABS_RX",
        ABS_RY => "ABS_RY",
        ABS_RZ => "ABS_RZ",
        ABS_THROTTLE => "ABS_THROTTLE",
        ABS_RUDDER => "ABS_RUDDER",
        ABS_WHEEL => "ABS_WHEEL",
        ABS_GAS => "ABS_GAS",
        ABS_BRAKE => "ABS_BRAKE",
        ABS_HAT0X => "ABS_HAT0X",
        ABS_HAT0Y => "ABS_HAT0Y",
        0x12 => "ABS_HAT1X",
        0x13 => "ABS_HAT1Y",
        _ => return None,
    })
}
