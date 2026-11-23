//! Converte o estado cru do controle no estado do Xbox 360 virtual.
//!
//! Porte fiel do `Mapper` do script Python, inclusive o arredondamento
//! (`round()` do Python arredonda empates para o par: `round_ties_even`).

use crate::codes::*;
use crate::mapping::{Mapping, Options, Rule};
use crate::target::Target;
use std::collections::{BTreeMap, BTreeSet};

pub const STICK_MAX: i32 = 32767;
pub const TRIGGER_MAX: i32 = 255;

/// Botões do Xbox 360 virtual, na ordem em que o script original os emitia.
pub const OUT_KEYS: [(Target, u16); 11] = [
    (Target::A, BTN_A),
    (Target::B, BTN_B),
    (Target::X, BTN_X),
    (Target::Y, BTN_Y),
    (Target::Lb, BTN_TL),
    (Target::Rb, BTN_TR),
    (Target::Back, BTN_SELECT),
    (Target::Start, BTN_START),
    (Target::Home, BTN_MODE),
    (Target::Ls, BTN_THUMBL),
    (Target::Rs, BTN_THUMBR),
];
pub const OUT_STICKS: [(Target, u16); 4] = [
    (Target::Lx, ABS_X),
    (Target::Ly, ABS_Y),
    (Target::Rx, ABS_RX),
    (Target::Ry, ABS_RY),
];
pub const OUT_TRIGGERS: [(Target, u16); 2] = [(Target::Lt, ABS_Z), (Target::Rt, ABS_RZ)];

/// Estado cru acumulado de todas as interfaces de um controle.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InputState {
    pub keys: BTreeSet<u16>,
    pub abs: BTreeMap<u16, i32>,
}

impl InputState {
    /// Aplica um evento evdev. Devolve `true` num `SYN_REPORT` (hora de emitir).
    pub fn apply(&mut self, ev_type: u16, code: u16, value: i32) -> bool {
        match ev_type {
            EV_KEY => {
                if value != 0 {
                    self.keys.insert(code);
                } else {
                    self.keys.remove(&code);
                }
                false
            }
            EV_ABS => {
                self.abs.insert(code, value);
                false
            }
            EV_SYN => code == SYN_REPORT,
            _ => false,
        }
    }
}

/// Um valor de saída: (tipo do evento, código, valor).
pub type OutEvent = (u16, u16, i32);

/// Estado completo do controle virtual, sempre na mesma ordem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputState(pub Vec<OutEvent>);

impl OutputState {
    pub fn get(&self, ev_type: u16, code: u16) -> Option<i32> {
        self.0
            .iter()
            .find(|(t, c, _)| *t == ev_type && *c == code)
            .map(|(_, _, v)| *v)
    }

    /// Eventos que mudaram em relação a `last` (que é atualizado).
    pub fn diff(&self, last: &mut BTreeMap<(u16, u16), i32>) -> Vec<OutEvent> {
        let mut out = Vec::new();
        for &(t, c, v) in &self.0 {
            if last.get(&(t, c)) != Some(&v) {
                last.insert((t, c), v);
                out.push((t, c, v));
            }
        }
        out
    }
}

#[derive(Debug, Clone, Default)]
pub struct Mapper {
    rules: BTreeMap<Target, Rule>,
    options: Options,
}

fn round(x: f64) -> i32 {
    x.round_ties_even() as i32
}

fn extreme(rest_min_max_sign: (i32, i32, i32)) -> i32 {
    let (min, max, sign) = rest_min_max_sign;
    if sign > 0 {
        max
    } else {
        min
    }
}

impl Mapper {
    pub fn new(mapping: &Mapping) -> Self {
        Mapper {
            rules: mapping.rules.clone(),
            options: mapping.options,
        }
    }

    pub fn pressed(&self, target: Target, s: &InputState) -> bool {
        match self.rules.get(&target) {
            None => false,
            Some(Rule::Key { code }) => s.keys.contains(code),
            Some(&Rule::Abs {
                code,
                rest,
                sign,
                min,
                max,
            }) => {
                let v = s.abs.get(&code).copied().unwrap_or(rest);
                let dist = match (extreme((min, max, sign)) - rest).abs() {
                    0 => 1,
                    d => d,
                };
                f64::from((v - rest) * sign) > 0.5 * f64::from(dist)
            }
        }
    }

    pub fn trigger(&self, target: Target, s: &InputState) -> i32 {
        match self.rules.get(&target) {
            None => 0,
            Some(Rule::Key { code }) => {
                if s.keys.contains(code) {
                    TRIGGER_MAX
                } else {
                    0
                }
            }
            Some(&Rule::Abs {
                code,
                rest,
                sign,
                min,
                max,
            }) => {
                let v = s.abs.get(&code).copied().unwrap_or(rest);
                let dist = match (extreme((min, max, sign)) - rest).abs() {
                    0 => 1,
                    d => d,
                };
                let frac = f64::from((v - rest) * sign) / f64::from(dist);
                round(frac.clamp(0.0, 1.0) * f64::from(TRIGGER_MAX))
            }
        }
    }

    pub fn stick(&self, target: Target, s: &InputState) -> i32 {
        let Some(&Rule::Abs {
            code,
            sign,
            min,
            max,
            ..
        }) = self.rules.get(&target)
        else {
            return 0;
        };
        let mid = f64::from(min + max) / 2.0;
        let half = f64::from(max - min) / 2.0;
        let half = if half == 0.0 { 1.0 } else { half };
        let v = s.abs.get(&code).map(|v| f64::from(*v)).unwrap_or(mid);
        let mut x = ((v - mid) / half * f64::from(sign)).clamp(-1.0, 1.0);

        let o = &self.options;
        if (target == Target::Ly && o.invert_ly) || (target == Target::Ry && o.invert_ry) {
            x = -x;
        }
        if o.deadzone > 0.0 {
            let dz = o.deadzone;
            x = if x.abs() <= dz {
                0.0
            } else {
                x.signum() * (x.abs() - dz) / (1.0 - dz)
            };
        }
        round(x * f64::from(STICK_MAX))
    }

    pub fn compute(&self, s: &InputState) -> OutputState {
        let mut out = Vec::with_capacity(19);
        for (t, code) in OUT_KEYS {
            out.push((EV_KEY, code, i32::from(self.pressed(t, s))));
        }
        for (t, code) in OUT_STICKS {
            out.push((EV_ABS, code, self.stick(t, s)));
        }
        for (t, code) in OUT_TRIGGERS {
            out.push((EV_ABS, code, self.trigger(t, s)));
        }
        let p = |t| i32::from(self.pressed(t, s));
        out.push((
            EV_ABS,
            ABS_HAT0X,
            p(Target::DpadRight) - p(Target::DpadLeft),
        ));
        out.push((EV_ABS, ABS_HAT0Y, p(Target::DpadDown) - p(Target::DpadUp)));
        OutputState(out)
    }
}

/// Estado com tudo solto (o que se emite ao desconectar).
pub fn neutral_state() -> OutputState {
    Mapper::default().compute(&InputState::default())
}

#[cfg(test)]
mod tests {
    //! Os mesmos casos de `tests/test_mapper.py` do projeto original.
    use super::*;
    use crate::mapping::Mapping;

    const T4: &str = include_str!("../../../profiles/gamesir-t4-cyclone-pro-dongle.json");

    fn t4() -> Mapper {
        Mapper::new(&Mapping::from_json(T4).unwrap().mapping)
    }

    fn state(keys: &[u16], abs: &[(u16, i32)]) -> InputState {
        let mut s = InputState::default();
        for (c, v) in [
            (ABS_X, 128),
            (ABS_Y, 128),
            (ABS_Z, 128),
            (ABS_RZ, 128),
            (ABS_HAT0X, 0),
            (ABS_HAT0Y, 0),
        ] {
            s.abs.insert(c, v);
        }
        for &(c, v) in abs {
            s.abs.insert(c, v);
        }
        s.keys.extend(keys.iter().copied());
        s
    }

    #[test]
    fn repouso_e_neutro() {
        let o = t4().compute(&state(&[], &[]));
        assert!(o.0.iter().filter(|e| e.0 == EV_KEY).all(|e| e.2 == 0));
        for c in [ABS_X, ABS_Y, ABS_RX, ABS_RY] {
            assert!(o.get(EV_ABS, c).unwrap().abs() < 300);
        }
    }

    #[test]
    fn home_vira_btn_mode() {
        let o = t4().compute(&state(&[KEY_HOMEPAGE], &[]));
        assert_eq!(o.get(EV_KEY, BTN_MODE), Some(1));
    }

    #[test]
    fn botoes_faciais() {
        let o = t4().compute(&state(&[BTN_A, BTN_Y], &[]));
        assert_eq!(o.get(EV_KEY, BTN_A), Some(1));
        assert_eq!(o.get(EV_KEY, BTN_Y), Some(1));
        assert_eq!(o.get(EV_KEY, BTN_B), Some(0));
    }

    #[test]
    fn dpad_vira_hat() {
        let o = t4().compute(&state(&[], &[(ABS_HAT0Y, -1), (ABS_HAT0X, 1)]));
        assert_eq!(o.get(EV_ABS, ABS_HAT0Y), Some(-1));
        assert_eq!(o.get(EV_ABS, ABS_HAT0X), Some(1));
    }

    #[test]
    fn gatilhos_digitais() {
        let o = t4().compute(&state(&[BTN_TL2], &[]));
        assert_eq!(o.get(EV_ABS, ABS_Z), Some(255));
        assert_eq!(o.get(EV_ABS, ABS_RZ), Some(0));
    }

    #[test]
    fn analogicos_extremos() {
        let o = t4().compute(&state(
            &[],
            &[(ABS_X, 255), (ABS_Y, 0), (ABS_Z, 0), (ABS_RZ, 255)],
        ));
        assert_eq!(o.get(EV_ABS, ABS_X), Some(32767));
        assert_eq!(o.get(EV_ABS, ABS_Y), Some(-32767));
        assert_eq!(o.get(EV_ABS, ABS_RX), Some(-32767));
        assert_eq!(o.get(EV_ABS, ABS_RY), Some(32767));
    }

    #[test]
    fn gatilho_analogico_proporcional() {
        let m = Mapping::from_json(
            r#"{"rules": {"LT": {"src": "abs", "code": 10, "rest": 0, "sign": 1, "min": 0, "max": 255}}}"#,
        )
        .unwrap()
        .mapping;
        let mut s = InputState::default();
        s.abs.insert(ABS_BRAKE, 128);
        assert_eq!(Mapper::new(&m).compute(&s).get(EV_ABS, ABS_Z), Some(128));
    }

    #[test]
    fn valores_iguais_ao_python() {
        // Valores conferidos com o Mapper do script Python para o perfil do T4.
        let m = t4();
        let cases = [
            (128, 128),
            (0, -32767),
            (255, 32767),
            (64, -16319),
            (200, 18632),
        ];
        for (raw, want) in cases {
            let o = m.compute(&state(&[], &[(ABS_X, raw)]));
            assert_eq!(o.get(EV_ABS, ABS_X), Some(want), "raw {raw}");
        }
    }

    #[test]
    fn sem_opcoes_nada_muda_e_com_opcoes_aplica() {
        let mut map = Mapping::from_json(T4).unwrap().mapping;
        let s = state(&[], &[(ABS_Y, 140), (ABS_X, 255)]);
        let base = Mapper::new(&map).compute(&s);
        map.options.invert_ly = true;
        map.options.deadzone = 0.1;
        let o = Mapper::new(&map).compute(&s);
        // 140 está dentro da zona morta de 10% → 0; X no extremo continua no extremo.
        assert_eq!(o.get(EV_ABS, ABS_Y), Some(0));
        assert_eq!(o.get(EV_ABS, ABS_X), Some(32767));
        assert_ne!(base.get(EV_ABS, ABS_Y), Some(0));
        let s = state(&[], &[(ABS_Y, 0)]);
        assert_eq!(
            Mapper::new(&map).compute(&s).get(EV_ABS, ABS_Y),
            Some(32767)
        );
    }

    #[test]
    fn neutro_tem_tudo_zerado() {
        assert!(neutral_state().0.iter().all(|e| e.2 == 0));
        assert_eq!(neutral_state().0.len(), 19);
    }

    #[test]
    fn diff_so_manda_o_que_mudou() {
        let mut last = BTreeMap::new();
        let first = neutral_state().diff(&mut last);
        assert_eq!(first.len(), 19);
        assert!(neutral_state().diff(&mut last).is_empty());
        let o = t4().compute(&state(&[BTN_A], &[]));
        let d = o.diff(&mut last);
        assert!(d.contains(&(EV_KEY, BTN_A, 1)));
    }
}
