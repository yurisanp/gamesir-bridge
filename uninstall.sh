#!/usr/bin/env bash
# Remove o gamesir-bridge. Use --purge para apagar também o mapeamento em /etc.
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "Rode com sudo: sudo $0 $*" >&2
    exit 1
fi

systemctl disable --now gamesir-bridge 2>/dev/null || true
rm -f /usr/local/bin/gamesir-bridge \
      /etc/udev/rules.d/65-gamesir-bridge.rules \
      /etc/systemd/system/gamesir-bridge.service
rm -rf /usr/local/share/gamesir-bridge
[[ "${1:-}" == "--purge" ]] && rm -rf /etc/gamesir-bridge

systemctl daemon-reload
udevadm control --reload-rules
udevadm trigger --subsystem-match=input
echo "Removido. Replugue o dongle."
