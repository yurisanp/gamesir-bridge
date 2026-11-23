# Perfis de controle

Cada controle suportado é um arquivo JSON em [`profiles/`](../profiles). O formato é o mesmo
`mapping.json` que o script original gravava, com três campos a mais: `id`, `name` e `notes`.

```json
{
  "id": "gamesir-t4-cyclone-pro-dongle",
  "name": "GameSir T4 Cyclone Pro",
  "device": "3537:1007",
  "description": "GameSir T4 Cyclone Pro via dongle 2.4 GHz (modo DInput).",
  "notes": ["Neste modo do dongle, LT/RT chegam como botões e não há vibração."],
  "rules": {
    "A":       {"src": "key", "code": 304},
    "HOME":    {"src": "key", "code": 172},
    "DPAD_UP": {"src": "abs", "code": 17, "rest": 0, "sign": -1, "min": -1, "max": 1},
    "LX":      {"src": "abs", "code": 0, "rest": 128, "sign": 1, "min": 0, "max": 255}
  },
  "options": {"deadzone": 0.08, "invert_ly": false, "invert_ry": false}
}
```

| Campo | Obrigatório | Significado |
| --- | --- | --- |
| `id` | não (usa o nome do arquivo) | Identificador; letras, números, `-` e `_` |
| `name` | não | Nome mostrado no app e no `status` |
| `device` | sim | `vvvv:pppp` em hexadecimal; `devices` aceita uma lista extra |
| `notes` | não | Avisos curtos sobre limitações do controle |
| `rules` | sim | Uma regra por alvo do Xbox 360 |
| `options` | não | Ajustes aplicados depois do mapeamento (ausente = nenhum) |

## Alvos

`A B X Y LB RB BACK START LS RS HOME DPAD_UP DPAD_DOWN DPAD_LEFT DPAD_RIGHT LT RT LX LY RX RY`

## Regras

- `{"src": "key", "code": N}`: botão evdev (`EV_KEY`). Num gatilho, vale 0 ou 255.
- `{"src": "abs", "code": N, "rest": R, "sign": ±1, "min": A, "max": B}`: eixo evdev
  (`EV_ABS`). Num botão ou no D-pad, conta como apertado depois de metade do caminho entre o
  repouso e o extremo do lado de `sign`. Num gatilho, é proporcional. Num analógico, o centro é
  `(min+max)/2` e `sign: -1` inverte.

A forma mais fácil de escrever um perfil é calibrar (`gamesir-bridge calibrate`) e copiar o
arquivo que ele grava em `/etc/gamesir-bridge/profiles/` (controle novo) ou
`/etc/gamesir-bridge/mappings/` (recalibração de um perfil existente).

## Onde o serviço procura

| Caminho | O quê |
| --- | --- |
| `<instalação>/share/gamesir-bridge/profiles/*.json` | Perfis oficiais |
| `/etc/gamesir-bridge/profiles/*.json` | Perfis do usuário (sobrescrevem os oficiais pelo `id`) |
| `/etc/gamesir-bridge/mappings/<id>.json` | Recalibração de um perfil |
| `/etc/gamesir-bridge/mapping.json` | Arquivo da versão em Python; ainda vale para o aparelho dele |

Um arquivo inválido nunca derruba o serviço: o controle continua com o mapeamento do perfil e
o erro aparece no `gamesir-bridge status` (e no app).
