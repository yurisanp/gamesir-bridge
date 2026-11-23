//! Compara o Mapper em Rust com valores gerados pelo Mapper do script Python original
//! (ver `tools/gen_python_golden.py`). Qualquer diferença aqui muda o comportamento
//! que já foi testado no hardware.

use gamesir_core::{InputState, Mapper, Mapping};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Case {
    mapping: String,
    keys: Vec<u16>,
    abs: BTreeMap<String, i32>,
    out: Vec<(u16, u16, i32)>,
}

#[derive(Deserialize)]
struct Golden {
    mappings: BTreeMap<String, serde_json::Value>,
    cases: Vec<Case>,
}

#[test]
fn mesmo_resultado_que_o_python() {
    let g: Golden = serde_json::from_str(include_str!("fixtures/python_golden.json")).unwrap();
    let mappers: BTreeMap<_, _> = g
        .mappings
        .into_iter()
        .map(|(k, v)| (k, Mapper::new(&Mapping::from_value(v).unwrap().mapping)))
        .collect();
    assert_eq!(g.cases.len(), 600);
    for (i, c) in g.cases.iter().enumerate() {
        let mut s = InputState::default();
        s.keys.extend(c.keys.iter().copied());
        for (k, v) in &c.abs {
            s.abs.insert(k.parse().unwrap(), *v);
        }
        let got = mappers[&c.mapping].compute(&s);
        let mut want = c.out.clone();
        let mut got = got.0.clone();
        want.sort();
        got.sort();
        assert_eq!(got, want, "caso {i} ({})", c.mapping);
    }
}
