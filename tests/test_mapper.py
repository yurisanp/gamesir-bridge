"""Testes da conversão estado cru -> Xbox 360. Rode com: python3 -m unittest discover tests"""

import importlib.machinery
import importlib.util
import json
import pathlib
import unittest

from evdev import ecodes as e

ROOT = pathlib.Path(__file__).resolve().parent.parent
_loader = importlib.machinery.SourceFileLoader('gamesir_bridge', str(ROOT / 'gamesir-bridge'))
_spec = importlib.util.spec_from_loader('gamesir_bridge', _loader)
gb = importlib.util.module_from_spec(_spec)
_loader.exec_module(gb)

RULES = json.loads((ROOT / 'mappings' / 't4-cyclone-pro-dongle.json').read_text())['rules']
REST = {e.ABS_X: 128, e.ABS_Y: 128, e.ABS_Z: 128, e.ABS_RZ: 128, e.ABS_HAT0X: 0, e.ABS_HAT0Y: 0}


class MapperTest(unittest.TestCase):
    def setUp(self):
        self.m = gb.Mapper(RULES)

    def out(self, keys=(), **absvals):
        state = dict(REST)
        state.update({getattr(e, k): v for k, v in absvals.items()})
        return self.m.compute(set(keys), state)

    def test_repouso_e_neutro(self):
        o = self.out()
        self.assertTrue(all(v == 0 for (t, _), v in o.items() if t == e.EV_KEY))
        for code in (e.ABS_X, e.ABS_Y, e.ABS_RX, e.ABS_RY):
            self.assertLess(abs(o[(e.EV_ABS, code)]), 300)

    def test_home_vira_btn_mode(self):
        o = self.out(keys=[e.KEY_HOMEPAGE])
        self.assertEqual(o[(e.EV_KEY, e.BTN_MODE)], 1)

    def test_botoes_faciais(self):
        o = self.out(keys=[e.BTN_SOUTH, e.BTN_WEST])
        self.assertEqual(o[(e.EV_KEY, e.BTN_A)], 1)
        self.assertEqual(o[(e.EV_KEY, e.BTN_Y)], 1)
        self.assertEqual(o[(e.EV_KEY, e.BTN_B)], 0)

    def test_dpad_vira_hat(self):
        o = self.out(ABS_HAT0Y=-1, ABS_HAT0X=1)
        self.assertEqual(o[(e.EV_ABS, e.ABS_HAT0Y)], -1)
        self.assertEqual(o[(e.EV_ABS, e.ABS_HAT0X)], 1)

    def test_gatilhos_digitais(self):
        o = self.out(keys=[e.BTN_TL2])
        self.assertEqual(o[(e.EV_ABS, e.ABS_Z)], 255)
        self.assertEqual(o[(e.EV_ABS, e.ABS_RZ)], 0)

    def test_analogicos_extremos(self):
        o = self.out(ABS_X=255, ABS_Y=0, ABS_Z=0, ABS_RZ=255)
        self.assertEqual(o[(e.EV_ABS, e.ABS_X)], 32767)
        self.assertEqual(o[(e.EV_ABS, e.ABS_Y)], -32767)
        self.assertEqual(o[(e.EV_ABS, e.ABS_RX)], -32767)
        self.assertEqual(o[(e.EV_ABS, e.ABS_RY)], 32767)

    def test_gatilho_analogico_proporcional(self):
        m = gb.Mapper({'LT': {'src': 'abs', 'code': e.ABS_BRAKE, 'rest': 0, 'sign': 1, 'min': 0, 'max': 255}})
        self.assertEqual(m.compute(set(), {e.ABS_BRAKE: 128})[(e.EV_ABS, e.ABS_Z)], 128)


if __name__ == '__main__':
    unittest.main()
