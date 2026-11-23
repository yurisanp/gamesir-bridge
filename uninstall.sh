#!/usr/bin/env bash
# Remove o que o install.sh instalou (lista em /etc/gamesir-bridge/installed-files).
# --purge apaga também mapeamentos e preferências em /etc/gamesir-bridge.
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "Rode como root: sudo $0 $*" >&2
    exit 1
fi
MANIFEST=/etc/gamesir-bridge/installed-files

systemctl disable --now gamesir-bridge 2>/dev/null || true
if [[ -f "$MANIFEST" ]]; then
    while IFS= read -r f; do
        [[ -n "$f" && "$f" == /* ]] && rm -f -- "$f"
    done < "$MANIFEST"
fi
rm -f /etc/udev/rules.d/66-gamesir-bridge-custom.rules
rm -rf /usr/local/share/gamesir-bridge /var/lib/gamesir-bridge
[[ -L /usr/local/bin/gamesir-bridge ]] && rm -f /usr/local/bin/gamesir-bridge
# Versão antiga (Python).
if grep -qs python3 /etc/systemd/system/gamesir-bridge.service; then
    rm -f /etc/systemd/system/gamesir-bridge.service /usr/local/bin/gamesir-bridge \
          /etc/udev/rules.d/65-gamesir-bridge.rules
fi
[[ "${1:-}" == "--purge" ]] && rm -rf /etc/gamesir-bridge

systemctl daemon-reload
udevadm control --reload-rules
udevadm trigger --subsystem-match=input --action=change
echo "Removido. Replugue o dongle."
