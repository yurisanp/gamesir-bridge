//! Calibração passo a passo, como máquina de estados pura (sem relógio nem dispositivo).
//!
//! Segue o `calibrate` do script Python: para cada alvo, espera um botão apertado
//! (`EV_KEY` com valor 1) ou um eixo que ande mais de 40% do curso a partir do repouso;
//! depois espera tudo ser solto (eixos a menos de 25% do repouso), mais 250 ms, e só
//! então tira um novo retrato do repouso para o próximo passo. Sem entrada em
//! `timeout`, o passo é pulado.

use crate::codes::{EV_ABS, EV_KEY};
use crate::mapper::InputState;
use crate::mapping::Rule;
use crate::target::Target;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Quanto um eixo precisa andar (fração do curso) para contar.
pub const DETECT_FRACTION: f64 = 0.4;
/// Abaixo disso (fração do curso) o eixo conta como solto.
pub const RELEASE_FRACTION: f64 = 0.25;
/// Pausa depois de soltar, antes do próximo passo.
pub const SETTLE: Duration = Duration::from_millis(250);
/// De quanto em quanto tempo conferir se tudo foi solto.
pub const RELEASE_POLL: Duration = Duration::from_millis(50);
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Faixa de um eixo, como o kernel informa (`EVIOCGABS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxisInfo {
    pub value: i32,
    pub min: i32,
    pub max: i32,
}

impl AxisInfo {
    fn span(&self) -> f64 {
        match self.max - self.min {
            0 => 1.0,
            s => f64::from(s),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CalEvent {
    /// Começou o passo `index` (a partir de 0) de `total`.
    Step {
        index: usize,
        total: usize,
        target: Target,
    },
    /// Entrada reconhecida. `clash` lista alvos anteriores com a mesma fonte.
    Hit {
        target: Target,
        rule: Rule,
        clash: Vec<Target>,
    },
    Skipped {
        target: Target,
    },
    Finished {
        rules: BTreeMap<Target, Rule>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Waiting { deadline: Instant },
    Releasing,
    Settling { until: Instant },
    Done,
}

#[derive(Debug, Clone)]
pub struct Calibrator {
    steps: Vec<Target>,
    index: usize,
    phase: Phase,
    timeout: Duration,
    axes: BTreeMap<u16, AxisInfo>,
    rest: BTreeMap<u16, i32>,
    rules: BTreeMap<Target, Rule>,
}

impl Calibrator {
    /// Começa a calibração. Devolve o calibrador e o evento do primeiro passo.
    pub fn new(
        steps: Vec<Target>,
        axes: BTreeMap<u16, AxisInfo>,
        timeout: Duration,
        state: &InputState,
        now: Instant,
    ) -> (Calibrator, Vec<CalEvent>) {
        let mut c = Calibrator {
            steps,
            index: 0,
            phase: Phase::Done,
            timeout,
            axes,
            rest: BTreeMap::new(),
            rules: BTreeMap::new(),
        };
        let ev = c.begin_step(state, now);
        (c, ev)
    }

    /// Calibração completa, na ordem do script original.
    pub fn full(
        axes: BTreeMap<u16, AxisInfo>,
        timeout: Duration,
        state: &InputState,
        now: Instant,
    ) -> (Calibrator, Vec<CalEvent>) {
        Self::new(Target::ALL.to_vec(), axes, timeout, state, now)
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    pub fn current(&self) -> Option<Target> {
        self.steps.get(self.index).copied()
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn total(&self) -> usize {
        self.steps.len()
    }

    pub fn rules(&self) -> &BTreeMap<Target, Rule> {
        &self.rules
    }

    /// Quando chamar `tick` de novo, se nada chegar antes.
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        match self.phase {
            Phase::Waiting { deadline } => Some(deadline),
            Phase::Releasing => Some(now + RELEASE_POLL),
            Phase::Settling { until } => Some(until),
            Phase::Done => None,
        }
    }

    fn rest_value(&self, code: u16, info: &AxisInfo) -> i32 {
        self.rest.get(&code).copied().unwrap_or(info.value)
    }

    fn begin_step(&mut self, state: &InputState, now: Instant) -> Vec<CalEvent> {
        if self.index >= self.steps.len() {
            self.phase = Phase::Done;
            return vec![CalEvent::Finished {
                rules: self.rules.clone(),
            }];
        }
        self.rest = self
            .axes
            .iter()
            .map(|(&c, info)| (c, state.abs.get(&c).copied().unwrap_or(info.value)))
            .collect();
        self.phase = Phase::Waiting {
            deadline: now + self.timeout,
        };
        vec![CalEvent::Step {
            index: self.index,
            total: self.steps.len(),
            target: self.steps[self.index],
        }]
    }

    /// Alimenta um evento evdev cru. `state` já deve incluir o evento.
    pub fn feed(
        &mut self,
        ev_type: u16,
        code: u16,
        value: i32,
        state: &InputState,
        now: Instant,
    ) -> Vec<CalEvent> {
        let Phase::Waiting { .. } = self.phase else {
            return self.tick(state, now);
        };
        let hit = if ev_type == EV_KEY && value == 1 {
            Some(Rule::Key { code })
        } else if ev_type == EV_ABS {
            self.axes.get(&code).and_then(|info| {
                let r = self.rest_value(code, info);
                (f64::from((value - r).abs()) > DETECT_FRACTION * info.span()).then_some(
                    Rule::Abs {
                        code,
                        rest: r,
                        sign: if value > r { 1 } else { -1 },
                        min: info.min,
                        max: info.max,
                    },
                )
            })
        } else {
            None
        };
        let Some(rule) = hit else {
            return self.tick(state, now);
        };
        let target = self.steps[self.index];
        let clash: Vec<Target> = self
            .rules
            .iter()
            .filter(|(t, r)| **t != target && r.same_source(&rule))
            .map(|(t, _)| *t)
            .collect();
        self.rules.insert(target, rule);
        self.phase = Phase::Releasing;
        let mut out = vec![CalEvent::Hit {
            target,
            rule,
            clash,
        }];
        out.extend(self.tick(state, now));
        out
    }

    fn released(&self, state: &InputState) -> bool {
        if !state.keys.is_empty() {
            return false;
        }
        self.axes.iter().all(|(&c, info)| {
            let v = state.abs.get(&c).copied().unwrap_or(info.value);
            f64::from((v - self.rest_value(c, info)).abs()) <= RELEASE_FRACTION * info.span()
        })
    }

    /// Avança o relógio: pula por tempo, detecta que tudo foi solto, termina a pausa.
    pub fn tick(&mut self, state: &InputState, now: Instant) -> Vec<CalEvent> {
        match self.phase {
            Phase::Waiting { deadline } if now >= deadline => {
                let target = self.steps[self.index];
                self.index += 1;
                let mut out = vec![CalEvent::Skipped { target }];
                out.extend(self.begin_step(state, now));
                out
            }
            Phase::Releasing if self.released(state) => {
                self.phase = Phase::Settling {
                    until: now + SETTLE,
                };
                Vec::new()
            }
            Phase::Settling { until } if now >= until => {
                self.index += 1;
                self.begin_step(state, now)
            }
            _ => Vec::new(),
        }
    }

    /// Pula o passo atual (botão "Pular").
    pub fn skip(&mut self, state: &InputState, now: Instant) -> Vec<CalEvent> {
        if self.is_done() {
            return Vec::new();
        }
        let target = self.steps[self.index];
        self.rules.remove(&target);
        self.index += 1;
        let mut out = vec![CalEvent::Skipped { target }];
        out.extend(self.begin_step(state, now));
        out
    }

    /// Volta um passo e refaz (botão "Voltar").
    pub fn back(&mut self, state: &InputState, now: Instant) -> Vec<CalEvent> {
        if self.is_done() {
            return Vec::new();
        }
        if let Some(t) = self.current() {
            self.rules.remove(&t);
        }
        self.index = self.index.saturating_sub(1);
        if let Some(t) = self.current() {
            self.rules.remove(&t);
        }
        self.begin_step(state, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::*;

    fn axes() -> BTreeMap<u16, AxisInfo> {
        let mut a = BTreeMap::new();
        a.insert(
            ABS_X,
            AxisInfo {
                value: 128,
                min: 0,
                max: 255,
            },
        );
        a.insert(
            ABS_HAT0Y,
            AxisInfo {
                value: 0,
                min: -1,
                max: 1,
            },
        );
        a
    }

    struct Sim {
        cal: Calibrator,
        state: InputState,
        now: Instant,
        log: Vec<CalEvent>,
    }

    impl Sim {
        fn new(steps: Vec<Target>) -> Sim {
            let mut state = InputState::default();
            state.abs.insert(ABS_X, 128);
            state.abs.insert(ABS_HAT0Y, 0);
            let now = Instant::now();
            let (cal, log) = Calibrator::new(steps, axes(), Duration::from_secs(10), &state, now);
            Sim {
                cal,
                state,
                now,
                log,
            }
        }
        fn ev(&mut self, t: u16, c: u16, v: i32) {
            self.state.apply(t, c, v);
            let out = self.cal.feed(t, c, v, &self.state, self.now);
            self.log.extend(out);
        }
        fn wait(&mut self, ms: u64) {
            let end = self.now + Duration::from_millis(ms);
            while self.now < end {
                self.now += Duration::from_millis(10);
                let out = self.cal.tick(&self.state, self.now);
                self.log.extend(out);
            }
        }
        fn press_release(&mut self, code: u16) {
            self.ev(EV_KEY, code, 1);
            self.ev(EV_KEY, code, 0);
            self.wait(300);
        }
    }

    #[test]
    fn botao_eixo_e_pulo() {
        let mut s = Sim::new(vec![Target::A, Target::DpadUp, Target::Lx, Target::Home]);
        s.press_release(BTN_A);
        // D-pad para cima: HAT0Y vai a -1 e volta.
        s.ev(EV_ABS, ABS_HAT0Y, -1);
        s.ev(EV_ABS, ABS_HAT0Y, 0);
        s.wait(300);
        // Analógico para a direita, devagar: só conta depois de 40% do curso.
        s.ev(EV_ABS, ABS_X, 200);
        assert_eq!(s.cal.current(), Some(Target::Lx));
        s.ev(EV_ABS, ABS_X, 255);
        s.ev(EV_ABS, ABS_X, 130);
        s.wait(300);
        // Home: nada em 10 s → pulado.
        s.wait(10_100);
        assert!(s.cal.is_done());

        let rules = match s.log.last().unwrap() {
            CalEvent::Finished { rules } => rules.clone(),
            e => panic!("{e:?}"),
        };
        assert_eq!(rules[&Target::A], Rule::Key { code: BTN_A });
        assert_eq!(
            rules[&Target::DpadUp],
            Rule::Abs {
                code: ABS_HAT0Y,
                rest: 0,
                sign: -1,
                min: -1,
                max: 1
            }
        );
        assert_eq!(
            rules[&Target::Lx],
            Rule::Abs {
                code: ABS_X,
                rest: 128,
                sign: 1,
                min: 0,
                max: 255
            }
        );
        assert!(!rules.contains_key(&Target::Home));
        assert!(s.log.contains(&CalEvent::Skipped {
            target: Target::Home
        }));
    }

    #[test]
    fn segurar_nao_avanca_ate_soltar() {
        let mut s = Sim::new(vec![Target::A, Target::B]);
        s.ev(EV_KEY, BTN_A, 1);
        s.wait(2_000);
        assert_eq!(s.cal.current(), Some(Target::A));
        // Outro botão enquanto segura não conta para o B.
        s.ev(EV_KEY, BTN_B, 1);
        s.ev(EV_KEY, BTN_B, 0);
        s.ev(EV_KEY, BTN_A, 0);
        s.wait(100);
        assert_eq!(s.cal.current(), Some(Target::A), "ainda na pausa de 250 ms");
        s.wait(200);
        assert_eq!(s.cal.current(), Some(Target::B));
    }

    #[test]
    fn avisa_repetido() {
        let mut s = Sim::new(vec![Target::A, Target::B]);
        s.press_release(BTN_A);
        s.press_release(BTN_A);
        assert!(s.log.iter().any(|e| matches!(e,
            CalEvent::Hit { target: Target::B, clash, .. } if clash == &vec![Target::A])));
    }

    #[test]
    fn pular_e_voltar() {
        let mut s = Sim::new(vec![Target::A, Target::B, Target::X]);
        s.press_release(BTN_A);
        let (st, now) = (s.state.clone(), s.now);
        s.log.extend(s.cal.skip(&st, now));
        assert_eq!(s.cal.current(), Some(Target::X));
        s.log.extend(s.cal.back(&st, now));
        assert_eq!(s.cal.current(), Some(Target::B));
        s.press_release(BTN_B);
        s.press_release(BTN_X);
        assert!(s.cal.is_done());
        assert_eq!(s.cal.rules().len(), 3);
    }

    #[test]
    fn eixo_com_repouso_fora_do_centro() {
        // Gatilho analógico que repousa em 0: o retrato do repouso vem do estado atual.
        let mut a = BTreeMap::new();
        a.insert(
            ABS_BRAKE,
            AxisInfo {
                value: 0,
                min: 0,
                max: 255,
            },
        );
        let mut state = InputState::default();
        state.abs.insert(ABS_BRAKE, 0);
        let now = Instant::now();
        let (mut cal, _) = Calibrator::new(vec![Target::Lt], a, DEFAULT_TIMEOUT, &state, now);
        state.apply(EV_ABS, ABS_BRAKE, 255);
        let ev = cal.feed(EV_ABS, ABS_BRAKE, 255, &state, now);
        assert!(matches!(
            ev[0],
            CalEvent::Hit {
                rule: Rule::Abs {
                    rest: 0,
                    sign: 1,
                    ..
                },
                ..
            }
        ));
    }
}
