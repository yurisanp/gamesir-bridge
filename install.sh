#!/usr/bin/env bash
# Instala o GameSir Bridge no sistema.
#
#   sudo ./install.sh                     compila (cargo) e instala a partir do código-fonte
#   sudo ./install.sh --bindir DIR        instala binários já compilados (é o que o AppImage faz)
#   ./install.sh --destdir DIR            só monta a árvore /usr em DIR (para empacotar)
#
# Numa instalação direta, os arquivos de integração vão para /etc (systemd, udev, D-Bus),
# que é gravável em qualquer distro, inclusive nas de /usr somente leitura (SteamOS,
# Bazzite). Os binários vão para /usr/local, ou /var/lib/gamesir-bridge se /usr/local
# não for gravável. A lista do que foi instalado fica em /etc/gamesir-bridge/installed-files,
# e o uninstall.sh remove exatamente isso.
#
# Opções:
#   --bindir DIR    de onde copiar gamesir-bridged e gamesir-bridge (pula o cargo)
#   --destdir DIR   monta a árvore de pacote em DIR (layout /usr) e sai
#   --prefix P      força o prefixo dos binários
#   --no-enable     não ativa o serviço
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA="$HERE/data"
PROFILES="$HERE/profiles"
BINDIR=
DESTDIR=
PREFIX=
ENABLE=1

while [[ $# -gt 0 ]]; do
    case "$1" in
        --bindir) BINDIR="$2"; shift 2 ;;
        --destdir) DESTDIR="$2"; shift 2 ;;
        --prefix) PREFIX="$2"; shift 2 ;;
        --no-enable) ENABLE=0; shift ;;
        -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
        *) echo "opção desconhecida: $1" >&2; exit 2 ;;
    esac
done

if [[ -z "$BINDIR" && -x "$HERE/bin/gamesir-bridged" ]]; then
    BINDIR="$HERE/bin"   # tarball da release: binários já compilados
fi
if [[ -z "$BINDIR" ]]; then
    BINDIR="$HERE/target/release"
    if [[ ! -x "$BINDIR/gamesir-bridged" || -n "${FORCE_BUILD:-}" ]]; then
        if [[ -n "${SUDO_USER:-}" && $EUID -eq 0 ]]; then
            # Compila como o usuário, não como root (o cargo e o ~/.cargo são dele).
            sudo -u "$SUDO_USER" -H bash -lc "cd '$HERE' && cargo build --release --locked -p gamesir-bridged"
        else
            (cd "$HERE" && cargo build --release --locked -p gamesir-bridged)
        fi
    fi
fi

# ------------------------------------------------------------------ árvore de pacote
if [[ -n "$DESTDIR" ]]; then
    P="${PREFIX:-/usr}"
    D="$DESTDIR"
    install -Dm755 "$BINDIR/gamesir-bridged" "$D$P/bin/gamesir-bridged"
    install -Dm755 "$BINDIR/gamesir-bridge" "$D$P/bin/gamesir-bridge"
    install -d "$D$P/share/gamesir-bridge/profiles"
    install -m644 "$PROFILES"/*.json "$D$P/share/gamesir-bridge/profiles/"
    install -Dm644 "$DATA/udev/65-gamesir-bridge.rules" "$D$P/lib/udev/rules.d/65-gamesir-bridge.rules"
    install -Dm644 "$DATA/systemd/gamesir-bridge.service" "$D$P/lib/systemd/system/gamesir-bridge.service"
    install -Dm644 "$DATA/modules-load.d/gamesir-bridge.conf" "$D$P/lib/modules-load.d/gamesir-bridge.conf"
    install -Dm644 "$DATA/dbus/io.github.yurisanp.GameSirBridge.conf" \
        "$D$P/share/dbus-1/system.d/io.github.yurisanp.GameSirBridge.conf"
    install -Dm644 "$DATA/polkit/io.github.yurisanp.GameSirBridge.policy" \
        "$D$P/share/polkit-1/actions/io.github.yurisanp.GameSirBridge.policy"
    install -Dm644 "$HERE/LICENSE" "$D$P/share/licenses/gamesir-bridge/LICENSE"
    exit 0
fi

# ------------------------------------------------------------------ instalação direta
if [[ $EUID -ne 0 ]]; then
    echo "Rode como root: sudo $0 $*" >&2
    exit 1
fi

writable() { local d="$1"; mkdir -p "$d" 2>/dev/null && [[ -w "$d" ]] && touch "$d/.gsb-test" 2>/dev/null && rm -f "$d/.gsb-test"; }

if [[ -z "$PREFIX" ]]; then
    if writable /usr/local/bin; then PREFIX=/usr/local; else PREFIX=/var/lib/gamesir-bridge; fi
fi
SHARE="$PREFIX/share/gamesir-bridge"
[[ "$PREFIX" == /var/lib/gamesir-bridge ]] && SHARE="$PREFIX"
MANIFEST=/etc/gamesir-bridge/installed-files
mkdir -p /etc/gamesir-bridge
: > "$MANIFEST.new"
put() { # put MODO ORIGEM DESTINO
    install -Dm"$1" "$2" "$3"
    echo "$3" >> "$MANIFEST.new"
}

# Restos da versão em Python (0.1): o serviço em /etc sobrescreveria o novo.
if grep -qs python3 /etc/systemd/system/gamesir-bridge.service; then
    echo "Removendo a versão antiga em Python (o mapeamento em /etc/gamesir-bridge é mantido)"
    systemctl disable --now gamesir-bridge 2>/dev/null || true
    rm -f /etc/systemd/system/gamesir-bridge.service /usr/local/bin/gamesir-bridge
    rm -rf /usr/local/share/gamesir-bridge
fi
systemctl stop gamesir-bridge 2>/dev/null || true

put 755 "$BINDIR/gamesir-bridged" "$PREFIX/bin/gamesir-bridged"
put 755 "$BINDIR/gamesir-bridge" "$PREFIX/bin/gamesir-bridge"
[[ "$PREFIX" == /usr/local ]] || ln -sf "$PREFIX/bin/gamesir-bridge" /usr/local/bin/gamesir-bridge 2>/dev/null || true
rm -rf "$SHARE/profiles"
for f in "$PROFILES"/*.json; do put 644 "$f" "$SHARE/profiles/$(basename "$f")"; done
put 644 "$DATA/udev/65-gamesir-bridge.rules" /etc/udev/rules.d/65-gamesir-bridge.rules
put 644 "$DATA/modules-load.d/gamesir-bridge.conf" /etc/modules-load.d/gamesir-bridge.conf
put 644 "$DATA/dbus/io.github.yurisanp.GameSirBridge.conf" /etc/dbus-1/system.d/io.github.yurisanp.GameSirBridge.conf
if writable /usr/share/polkit-1/actions; then
    put 644 "$DATA/polkit/io.github.yurisanp.GameSirBridge.policy" \
        /usr/share/polkit-1/actions/io.github.yurisanp.GameSirBridge.policy
else
    echo "Aviso: /usr é somente leitura; sem a política do polkit o serviço libera só a sessão local ativa (logind)."
fi
UNIT=/etc/systemd/system/gamesir-bridge.service
sed -e "s|^ExecStart=.*|ExecStart=$PREFIX/bin/gamesir-bridged --profiles $SHARE/profiles|" \
    -e "s|^ReadWritePaths=.*|ReadWritePaths=/etc/gamesir-bridge /etc/udev/rules.d|" \
    "$DATA/systemd/gamesir-bridge.service" > "$UNIT.new"
put 644 "$UNIT.new" "$UNIT"
rm -f "$UNIT.new"
"$PREFIX/bin/gamesir-bridged" --version | awk '{print $2}' > /etc/gamesir-bridge/installed-version
mv "$MANIFEST.new" "$MANIFEST"
printf '%s\n' "$MANIFEST" /etc/gamesir-bridge/installed-version >> "$MANIFEST"

modprobe uinput || true
udevadm control --reload-rules
udevadm trigger --subsystem-match=input --action=change
systemctl daemon-reload
systemctl reload dbus 2>/dev/null || systemctl reload dbus-broker 2>/dev/null || true

if [[ $ENABLE -eq 1 ]]; then
    systemctl enable gamesir-bridge
    systemctl restart gamesir-bridge
    echo "Serviço ativo. Logs: journalctl -u gamesir-bridge -f"
fi
echo "Pronto ($PREFIX). Replugue o dongle para a regra udev valer. Confira com: gamesir-bridge status"
