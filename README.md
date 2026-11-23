<p align="center">
  <img src="docs/assets/banner.svg" alt="GameSir Bridge: as interfaces do dongle viram um único controle Xbox 360" width="860">
</p>

<p align="center">
  <a href="https://github.com/yurisanp/gamesir-bridge/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/yurisanp/gamesir-bridge/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/yurisanp/gamesir-bridge/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/yurisanp/gamesir-bridge?include_prereleases&label=release&color=4b57e3"></a>
  <img alt="AppImage" src="https://img.shields.io/badge/AppImage-x86__64-4b57e3">
  <img alt="Rust 1.89+" src="https://img.shields.io/badge/rust-1.89%2B-4b57e3?logo=rust">
  <a href="LICENSE"><img alt="Licença MIT" src="https://img.shields.io/badge/licen%C3%A7a-MIT-4b57e3"></a>
</p>

**GameSir Bridge** faz o **GameSir T4 Cyclone Pro** (pelo dongle 2.4 GHz) e outros controles
aparecerem no Linux como um **controle Xbox 360**, com todos os botões e o **Home abrindo o
overlay da Steam**. Vários controles ao mesmo tempo, cada um com seu jogador.

<p align="center"><img src="docs/assets/screenshots/controller.png" alt="App do GameSir Bridge mostrando um GameSir T4 Cyclone Pro ativo como Jogador 1" width="760"></p>

- **Funciona em qualquer jogo:** Steam, SDL, Proton e emuladores já conhecem o Xbox 360.
- **Vários controles:** dois dongles iguais viram Jogador 1 e Jogador 2, sem trocar os Homes.
- **Outros controles:** calibre qualquer gamepad em 21 passos, ou contribua com um perfil JSON.
- **Veja o que chega:** o app desenha o controle ao vivo e troca qualquer botão com um clique.
- **Some quando funciona:** um serviço do sistema faz a ponte, mesmo sem nenhum app aberto.
- **Instalação em um passo:** um AppImage para qualquer distro, inclusive SteamOS e Bazzite.

## Instalação

Baixe o `GameSir_Bridge-*.AppImage` da [última release](https://github.com/yurisanp/gamesir-bridge/releases/latest), dê permissão de execução e abra
(dois cliques, ou pelo terminal):

```bash
chmod +x GameSir_Bridge-*.AppImage
./GameSir_Bridge-*.AppImage
```

Na primeira vez o app mostra o que falta e, com um clique em **Configurar**, instala o
serviço, a regra udev e as políticas do sistema, pedindo sua senha **uma vez** (pkexec).
Depois, **replugue o dongle**. A ponte roda no serviço: não precisa deixar o app aberto.

Para atualizar, abra o AppImage novo; ele percebe a versão diferente e oferece reinstalar.
Para remover: **Preferências → Avançado → Remover do sistema**, ou
`./GameSir_Bridge-*.AppImage setup --uninstall`.

A interface precisa de glibc 2.39 ou mais nova (Ubuntu 24.04, Fedora 40, Debian 13, Arch e
derivados). Em sistemas mais antigos o AppImage segue pela linha de comando: instala o
serviço e mostra o `status`.

<details>
<summary>Instalar a partir do código-fonte</summary>

Precisa de Rust 1.89 ou mais novo.

```bash
git clone https://github.com/yurisanp/gamesir-bridge.git
cd gamesir-bridge
sudo ./install.sh          # compila, instala em /usr/local e ativa o serviço
sudo ./uninstall.sh        # remove (mantém /etc/gamesir-bridge; --purge apaga tudo)
```

Se você usava a versão em Python, o `install.sh` remove o serviço antigo e mantém seu
`/etc/gamesir-bridge/mapping.json`, que continua valendo.
</details>

## O app

| | |
| --- | --- |
| <img src="docs/assets/screenshots/editor.png" alt="Editor de mapeamento com o controle ao vivo e medidores dos eixos" width="420"> | <img src="docs/assets/screenshots/calibration.png" alt="Calibração passo a passo pedindo o botão B" width="420"> |
| **Mapeamento:** o que você aperta acende. Clique no lápis e aperte o botão físico para trocar. Zona morta e inversão do eixo Y ficam aqui. | **Calibração:** 21 passos, um por tela, com o tempo até pular. Serve para controles que ainda não têm perfil. |
| <img src="docs/assets/screenshots/setup.png" alt="Primeira execução listando serviço, regra udev e serviço rodando" width="420"> | <img src="docs/assets/screenshots/controller-dark.png" alt="Página do controle no tema escuro" width="420"> |
| **Primeira execução:** confere o sistema e instala tudo com uma senha. | **Tema claro e escuro**, seguindo o sistema ou escolhido nas preferências. |

## Linha de comando

Tudo que o app faz também existe no terminal (`./GameSir_Bridge-*.AppImage <comando>`, ou
`gamesir-bridge <comando>` depois de instalado):

<p align="center"><img src="docs/assets/terminal-status.svg" alt="Saída do gamesir-bridge status com dois controles ativos" width="680"></p>

| Comando | O que faz |
| --- | --- |
| `gamesir-bridge status` | Estado de cada controle: ativo, aguardando, sem mapeamento ou erro |
| `gamesir-bridge list` | Interfaces de cada controle, com os códigos evdev (não precisa do serviço) |
| `gamesir-bridge calibrate [id]` | Pede cada botão e eixo em ordem e grava o mapeamento |
| `gamesir-bridge disable [id]` | Desliga a ponte (o controle volta a aparecer como ele mesmo) |
| `gamesir-bridge enable [id]` | Liga a ponte de novo |
| `gamesir-bridge mapping [id]` | Mostra o mapeamento em uso, em JSON |

Com um controle só conectado, o `id` é opcional. Os logs ficam em `journalctl -u gamesir-bridge -f`.

## Controles suportados

| Controle | Perfil | Observações |
| --- | --- | --- |
| GameSir T4 Cyclone Pro (dongle 2.4 GHz, `3537:1007`) | [`gamesir-t4-cyclone-pro-dongle`](profiles/gamesir-t4-cyclone-pro-dongle.json) | LT/RT digitais e sem vibração nesse modo do dongle |
| Qualquer outro gamepad | criado na calibração | `gamesir-bridge calibrate` gera um perfil em `/etc/gamesir-bridge/profiles` |

Calibrou um controle novo? Mande o perfil para `profiles/` num PR: é um JSON só. Veja
[docs/profiles.md](docs/profiles.md) e o [guia de contribuição](CONTRIBUTING.md).

## Como funciona

Plugado no Linux, o dongle tenta se apresentar como um Switch Pro Controller (`057e:2009`),
falha e volta como `3537:1007` em **modo DInput**: os botões saem num gamepad HID genérico e
o **Home** sai como a tecla `KEY_HOMEPAGE`, numa interface "Consumer Control" separada.

O `gamesir-bridged` agrupa as interfaces de cada aparelho pela porta USB, captura todas
(`EVIOCGRAB`) e cria um `Microsoft X-Box 360 pad` (`045e:028e`) via `uinput`, com o Home como
`BTN_MODE`. Uma regra udev esconde o dongle cru para a Steam não ver dois controles.

```text
 app / gamesir-bridge (usuário) ── D-Bus + polkit ──► gamesir-bridged (root, systemd)
                                                          │
             /dev/input/event* (gamepad + Home) ──lê──────┤  perfis JSON + Mapper
                                                          └──cria──► Xbox 360 virtual (uinput)
```

O app e a linha de comando nunca tocam em `/dev/input` ou `/dev/uinput`: pedem tudo ao
serviço pelo D-Bus ([docs/dbus.md](docs/dbus.md)), e o polkit libera a sessão local ativa.

## Limitações

- **Gatilhos digitais:** no modo DInput o dongle manda LT/RT como botões (0 ou 100%).
- **Sem vibração:** o modo DInput não expõe force feedback.
- **M, L4/R4** não chegam ao PC. Programe as traseiras no próprio controle: segure
  **M + L4/R4** até o Home piscar, aperte o botão de destino e depois L4/R4.

## Desenvolvimento

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev dbus   # ou o equivalente da sua distro
cargo test --workspace        # testes (o de D-Bus precisa do dbus-daemon instalado)
cargo clippy --workspace --all-targets
GAMESIR_BRIDGE_DEMO=1 cargo run -p gamesir-bridge-app   # o app com controles simulados
```

O código está em três crates: [`gamesir-core`](crates/core) (mapeamento, perfis, calibração e
descoberta de dispositivos, sem I/O), [`gamesir-bridged`](crates/daemon) (serviço, D-Bus e
linha de comando) e [`gamesir-bridge-app`](crates/app) (GTK4 + libadwaita). O porte do mapeador é conferido em CI contra o script Python original, caso
a caso. Detalhes em [CONTRIBUTING.md](CONTRIBUTING.md).

Testado em CachyOS (KDE Plasma 6, Wayland), Steam Big Picture, dongle `3537:1007` (bcdDevice 1.01).

## Licença

[MIT](LICENSE). Projeto independente, sem relação com a GameSir.
