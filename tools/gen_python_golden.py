#!/usr/bin/env python3
"""Gera crates/core/tests/fixtures/python_golden.json a partir do Mapper do script Python
original (commit 1fc2764), para provar que o porte em Rust dá os mesmos valores.

Uso: git show 1fc2764:gamesir-bridge > /tmp/legacy.py
     python3 tools/gen_python_golden.py /tmp/legacy.py > crates/core/tests/fixtures/python_golden.json
Precisa de python-evdev.
"""
import importlib.machinery, importlib.util, json, pathlib, random, sys

loader = importlib.machinery.SourceFileLoader('legacy', sys.argv[1])
spec = importlib.util.spec_from_loader('legacy', loader)
gb = importlib.util.module_from_spec(spec)
loader.exec_module(gb)

root = pathlib.Path(__file__).resolve().parent.parent
t4 = json.loads((root / 'profiles' / 'gamesir-t4-cyclone-pro-dongle.json').read_text())['rules']
analog = {
    'A': {'src': 'key', 'code': 304},
    'HOME': {'src': 'key', 'code': 172},
    'LT': {'src': 'abs', 'code': 10, 'rest': 0, 'sign': 1, 'min': 0, 'max': 1023},
    'RT': {'src': 'abs', 'code': 9, 'rest': 1023, 'sign': -1, 'min': 0, 'max': 1023},
    'LX': {'src': 'abs', 'code': 0, 'rest': 0, 'sign': 1, 'min': -32768, 'max': 32767},
    'LY': {'src': 'abs', 'code': 1, 'rest': 0, 'sign': -1, 'min': -32768, 'max': 32767},
    'RX': {'src': 'abs', 'code': 3, 'rest': 7, 'sign': 1, 'min': 7, 'max': 7},
    'DPAD_UP': {'src': 'abs', 'code': 17, 'rest': 0, 'sign': -1, 'min': -1, 'max': 1},
    'DPAD_RIGHT': {'src': 'key', 'code': 547},
    'B': {'src': 'abs', 'code': 2, 'rest': 128, 'sign': 1, 'min': 0, 'max': 255},
}
rng = random.Random(42)
cases = []
for name, rules, codes, keys in [
    ('t4', t4, {0: (0, 255), 1: (0, 255), 2: (0, 255), 5: (0, 255), 16: (-1, 1), 17: (-1, 1)},
     [304, 305, 307, 308, 310, 311, 312, 313, 314, 315, 317, 318, 172]),
    ('analog', analog, {10: (0, 1023), 9: (0, 1023), 0: (-32768, 32767), 1: (-32768, 32767),
                        3: (0, 20), 17: (-1, 1), 2: (0, 255)}, [304, 172, 547]),
]:
    m = gb.Mapper(rules)
    for _ in range(300):
        absvals = {c: rng.choice([lo, hi, (lo + hi) // 2, rng.randint(lo, hi)])
                   for c, (lo, hi) in codes.items() if rng.random() > 0.1}
        ks = sorted(k for k in keys if rng.random() < 0.3)
        out = m.compute(set(ks), absvals)
        cases.append({'mapping': name, 'keys': ks, 'abs': {str(k): v for k, v in absvals.items()},
                      'out': [[t, c, v] for (t, c), v in out.items()]})
json.dump({'mappings': {'t4': {'rules': t4}, 'analog': {'rules': analog}}, 'cases': cases},
          sys.stdout, separators=(',', ':'))
