# gamesir-bridge

Driver em espaço de usuário que faz o **GameSir T4 Cyclone Pro** (via dongle 2.4 GHz)
funcionar no Linux como um **controle Xbox 360**, com todos os botões e o **Home
abrindo o overlay da Steam**.

## O problema

Plugado no Linux, o dongle tenta primeiro se apresentar como um Nintendo Switch Pro
Controller (`057e:2009`). O kernel falha ao ler o descritor de configuração:

```
usb 3-1.2: New USB device found, idVendor=057e, idProduct=2009
usb 3-1.2: unable to read config index 0 descriptor/start: -32
usb 3-1.2: can't read configurations, error -32
```

Um segundo depois, ele desiste dessa identidade e volta como `3537:1007 GameSir-Dongle`,
em **modo DInput**, tratado pelo `hid-generic`:

- os botões saem num gamepad HID genérico, sem mapeamento conhecido pela Steam ou pelos jogos;
- o **Home** sai como a tecla `KEY_HOMEPAGE`, numa interface "Consumer Control"
  separada (como uma tecla multimídia), e não como botão de gamepad.

O quirk `usbcore.quirks=057e:2009:r` não resolve.

## A solução

O `gamesir-bridge` captura (`EVIOCGRAB`) as duas interfaces do dongle e cria, via
`uinput`, um único **Microsoft X-Box 360 pad** (`045e:028e`), que Steam, SDL e jogos
já reconhecem. O Home vira `BTN_MODE` (botão Guide). Uma regra udev esconde o
dongle cru, para não aparecerem dois controles.

```
3537:1007 input0 (gamepad DInput) ──┐
                                    ├─► gamesir-bridge ─► uinput: Xbox 360 pad
3537:1007 input1 (KEY_HOMEPAGE)  ───┘
```

## Instalação

```bash
git clone https://github.com/<seu-usuario>/gamesir-bridge.git
cd gamesir-bridge
sudo ./install.sh
```

O `install.sh` instala o `python-evdev` (pacman, apt ou dnf), o script em
`/usr/local/bin`, a regra udev, o serviço systemd e o mapeamento do T4 Cyclone Pro.
Depois ele ativa o serviço. Por fim, **replugue o dongle**.

Para conferir, rode `sudo evtest`: deve aparecer um `Microsoft X-Box 360 pad`.

## Outros controles / recalibrar

O mapeamento incluído (`mappings/t4-cyclone-pro-dongle.json`) foi calibrado num
T4 Cyclone Pro. Se o seu controle numerar diferente, ou se for outro modelo que use o
mesmo dongle `3537:1007`, recalibre:

```bash
sudo systemctl stop gamesir-bridge
sudo gamesir-bridge list        # mostra as interfaces do dongle
sudo gamesir-bridge calibrate   # pede cada botão/eixo em ordem
sudo systemctl start gamesir-bridge
```

O mapeamento fica em `/etc/gamesir-bridge/mapping.json`. Contribuições com
mapeamentos de outros controles em `mappings/` são bem-vindas.

## Limitações

- **Gatilhos digitais:** no modo DInput o dongle manda LT/RT como botões (0 ou 100%).
- **Sem rumble:** o modo DInput não expõe force feedback.
- **M, L4/R4** não chegam ao PC. Programe as traseiras no próprio controle:
  segure **M + L4/R4** até o Home piscar, aperte o botão de destino e depois L4/R4.

## Desinstalar

```bash
sudo ./uninstall.sh          # mantém /etc/gamesir-bridge/mapping.json
sudo ./uninstall.sh --purge  # remove tudo
```

## Testes

```bash
python3 -m unittest discover tests
```

## Testado em

CachyOS (KDE Plasma 6, Wayland), Steam Big Picture, dongle `3537:1007`
(bcdDevice 1.01).

## Licença

MIT. Projeto independente, sem relação com a GameSir.
