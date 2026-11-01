#!/usr/bin/env bash
# Instala o gamesir-bridge: script, regra udev, serviço systemd e o mapeamento padrão.
# Uso: sudo ./install.sh            (instala e ativa o serviço)
#      sudo ./install.sh --no-enable (instala sem ativar)
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "Rode com sudo: sudo $0 $*" >&2
    exit 1
fi

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENABLE=1
[[ "${1:-}" == "--no-enable" ]] && ENABLE=0

if ! python3 -c 'import evdev' 2>/dev/null; then
    if command -v pacman >/dev/null; then
        pacman -S --needed --noconfirm python-evdev
    elif command -v apt-get >/dev/null; then
        apt-get install -y python3-evdev
    elif command -v dnf >/dev/null; then
        dnf install -y python3-evdev
    else
        echo "Instale o python-evdev pelo gerenciador da sua distro e rode de novo." >&2
        exit 1
    fi
fi

install -Dm755 "$HERE/gamesir-bridge"                 /usr/local/bin/gamesir-bridge
install -Dm644 "$HERE/udev/65-gamesir-bridge.rules"   /etc/udev/rules.d/65-gamesir-bridge.rules
install -Dm644 "$HERE/systemd/gamesir-bridge.service" /etc/systemd/system/gamesir-bridge.service
install -d /usr/local/share/gamesir-bridge/mappings
install -m644 "$HERE"/mappings/*.json /usr/local/share/gamesir-bridge/mappings/

if [[ ! -f /etc/gamesir-bridge/mapping.json ]]; then
    install -Dm644 "$HERE/mappings/t4-cyclone-pro-dongle.json" /etc/gamesir-bridge/mapping.json
    echo "Mapeamento padrão (T4 Cyclone Pro) copiado para /etc/gamesir-bridge/mapping.json"
else
    echo "Mantendo o mapeamento existente em /etc/gamesir-bridge/mapping.json"
fi

modprobe uinput || true
udevadm control --reload-rules
udevadm trigger --subsystem-match=input
systemctl daemon-reload

if [[ $ENABLE -eq 1 ]]; then
    systemctl enable gamesir-bridge
    systemctl restart gamesir-bridge
    echo "Serviço ativo. Logs: journalctl -u gamesir-bridge -f"
fi
echo "Pronto. Replugue o dongle para a regra udev valer."
