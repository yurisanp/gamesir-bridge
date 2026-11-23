# Changelog

Todas as mudanças relevantes ficam aqui. O formato segue o
[Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e o projeto usa
[versionamento semântico](https://semver.org/lang/pt-BR/).

## [Unreleased]

### Adicionado
- App gráfico em GTK4 + libadwaita (`gamesir-bridge-app`), que abre ao rodar o AppImage:
  - lista de controles conectados e conhecidos, com estado e jogador;
  - página do controle com o desenho ao vivo e o liga/desliga da ponte;
  - editor de mapeamento com "clique no lápis e aperte o botão", medidores dos eixos, zona
    morta, inversão do eixo Y, exportar e restaurar o perfil oficial;
  - calibração passo a passo, com voltar, pular e o tempo de cada passo;
  - primeira execução que instala o serviço com uma senha, e preferências (tema, reinstalar
    e remover do sistema).
- `GAMESIR_BRIDGE_DEMO=1` abre o app com controles simulados, sem serviço.

### Mudado
- O AppImage leva o GTK junto e abre o app; sem glibc 2.39+, segue pela linha de comando.
- `install.sh` compila só o serviço e a linha de comando (não precisa do GTK).

## [0.2.0] - 2026-11-23

Reescrita em Rust: o script virou um serviço do sistema com interface D-Bus.

### Adicionado
- Vários controles ao mesmo tempo, cada um com seu Xbox 360 virtual e posição de jogador.
- Perfis de controle como dados (`profiles/*.json`), com controles novos calibrados do zero
  virando perfis do usuário em `/etc/gamesir-bridge/profiles`.
- Serviço `gamesir-bridged` com interface D-Bus (`io.github.yurisanp.GameSirBridge1`) para o
  app: lista de controles, liga/desliga a ponte, calibração passo a passo, "aperte para
  remapear", estado ao vivo e gravação de mapeamentos. Mudanças passam pelo polkit.
- Zona morta e inversão do eixo Y dos analógicos (`options` no mapeamento).
- Linha de comando `gamesir-bridge`: `status`, `list`, `calibrate`, `enable`, `disable`,
  `mapping`, `reload` e `setup`.
- AppImage com binários estáticos que instala o serviço com uma senha (`setup`), inclusive em
  distros com `/usr` somente leitura.
- CI (formatação, clippy, testes, Rust mínimo, conferência contra o script Python, shellcheck,
  AppImage por PR) e releases automáticas nas tags.

### Corrigido
- Dois dongles idênticos trocavam os botões Home entre si: as interfaces agora são agrupadas
  pela porta USB, não só pelo VID/PID.
- Se a interface do Home aparecesse depois da do gamepad, o Home ficava morto até replugar.
- Um `mapping.json` inválido derrubava o serviço em laço a cada 2 s. Agora o erro aparece no
  `status` e o controle segue com o mapeamento do perfil.

### Mudado
- O mapeamento do botão, a calibração (21 passos, 10 s por item) e o formato do
  `mapping.json` são os mesmos da versão em Python; o `/etc/gamesir-bridge/mapping.json`
  existente continua valendo.
- `install.sh` instala em `/usr/local` (ou `/var/lib/gamesir-bridge`) e põe a integração em
  `/etc`; `uninstall.sh` remove exatamente o que foi instalado.

## [0.1.0] - 2026-11-01

- Primeira versão: script Python que junta as duas interfaces do dongle GameSir `3537:1007`
  num Xbox 360 virtual, com `list`, `calibrate` e `run`.

[Unreleased]: https://github.com/yurisanp/gamesir-bridge/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/yurisanp/gamesir-bridge/compare/1fc2764...v0.2.0
[0.1.0]: https://github.com/yurisanp/gamesir-bridge/commit/1fc2764
