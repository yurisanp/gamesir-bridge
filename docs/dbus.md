# Interface D-Bus

O serviço publica `io.github.yurisanp.GameSirBridge` no **barramento do sistema**, objeto
`/io/github/yurisanp/GameSirBridge`, interface `io.github.yurisanp.GameSirBridge1`.
Estruturas compostas vão como JSON (`s`), com os tipos de
[`crates/core/src/api.rs`](../crates/core/src/api.rs); o app e a linha de comando usam o proxy
de [`crates/core/src/client.rs`](../crates/core/src/client.rs).

## Permissões

Consultas são livres. Os métodos marcados com 🔒 exigem a ação polkit
`io.github.yurisanp.GameSirBridge.configure`, liberada sem senha para a sessão local ativa
(`allow_active=yes`). Se o polkit ou a política não estiverem instalados (por exemplo em
distros com `/usr` somente leitura), o serviço confere no logind se quem chama tem uma sessão
local ativa. Root sempre pode.

## Métodos

| Método | Retorno | |
| --- | --- | --- |
| `ListControllers()` | `s` JSON `[ControllerInfo]` | |
| `ListProfiles()` | `s` JSON `[Profile]` | |
| `GetMapping(s id)` | `s` mapeamento em uso | |
| `SetEnabled(s id, b enabled)` | | 🔒 |
| `SaveMapping(s id, s mapping)` | `s` arquivo gravado | 🔒 valida antes; erro `InvalidArgs` explica |
| `ResetMapping(s id)` | | 🔒 volta ao perfil oficial |
| `StartCalibration(s id, d step_timeout)` | | 🔒 21 passos, sinais abaixo |
| `CalibrationControl(s id, s action)` | | 🔒 `skip`, `back` ou `cancel` |
| `CaptureInput(s id, s target, d step_timeout)` | | 🔒 "aperte para remapear" |
| `WatchInput(s id, b enable)` | | 🔒 `InputState` a ~30 Hz por 10 s; renove |
| `Forget(s id)` | | 🔒 esquece um controle desconectado |
| `Reload()` | | 🔒 relê perfis e preferências |

Propriedade: `Version` (`s`).

## Sinais

| Sinal | Argumentos |
| --- | --- |
| `ControllersChanged` | |
| `InputState` | `s id, s LiveState` |
| `CalibrationStep` | `s id, u index, u total, s target, s prompt` |
| `CalibrationHit` | `s id, s target, s description, as clash` |
| `CalibrationSkipped` | `s id, s target` |
| `CalibrationFinished` | `s id, b saved, s message` |
| `Captured` | `s id, s Captured` |

## Testando à mão

```bash
busctl --system call io.github.yurisanp.GameSirBridge /io/github/yurisanp/GameSirBridge \
  io.github.yurisanp.GameSirBridge1 ListControllers | cut -c3- | jq
```

Para desenvolver sem instalar, rode o serviço no barramento de sessão:
`sudo -E target/debug/gamesir-bridged --session-bus --profiles profiles` e use
`gamesir-bridge --session-bus status`.
