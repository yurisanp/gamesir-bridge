# Segurança

O `gamesir-bridged` roda como root (precisa de `/dev/input` e `/dev/uinput`) e aceita
chamadas D-Bus de usuários comuns, então falhas nele importam.

## Como relatar

Não abra uma issue pública. Use o [relato privado de vulnerabilidades do GitHub](https://github.com/yurisanp/gamesir-bridge/security/advisories/new)
com os passos para reproduzir e a versão (`gamesir-bridge --version`).

## O que está no escopo

- Qualquer forma de um usuário sem permissão mudar a configuração, ler os botões ao vivo ou
  fazer o serviço escrever fora de `/etc/gamesir-bridge` e `/etc/udev/rules.d`.
- Arquivos de perfil ou mapeamento que façam o serviço travar ou consumir recursos sem limite.
- O instalador (`install.sh` e `gamesir-bridge setup`).
