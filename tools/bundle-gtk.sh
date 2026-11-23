#!/usr/bin/env bash
# Põe o app (GTK4 + libadwaita) e as bibliotecas dele dentro de um AppDir.
#
#   tools/bundle-gtk.sh APPDIR BINÁRIO_DO_APP
#
# Roda na máquina que compilou o app (no CI, ubuntu-24.04). Copia as bibliotecas que o
# binário usa, menos as que todo Linux tem e que não devem ir junto (glibc, drivers de
# vídeo, fontconfig/freetype, X11), seguindo a lista de exclusão do AppImage. Junta também
# o carregador de SVG do gdk-pixbuf, os esquemas GSettings do GTK e os ícones simbólicos
# do Adwaita. O AppRun aponta o GTK para tudo isso.
set -euo pipefail

APPDIR="$1"
APPBIN="$2"
LIB="$APPDIR/usr/lib"
MULTIARCH="$(gcc -print-multiarch 2>/dev/null || echo x86_64-linux-gnu)"
SYSLIB="/usr/lib/$MULTIARCH"

# Nunca copiar: vêm do sistema do usuário (glibc, GPU, fontes, X11, som).
EXCLUDE='^(ld-linux.*|libc|libm|libdl|libpthread|librt|libresolv|libutil|libanl|libnss_.*|libBrokenLocale|libmvec|libGL|libGLX.*|libEGL.*|libGLdispatch|libGLESv2|libOpenGL|libdrm.*|libgbm|libvulkan|libX11|libX11-xcb|libxcb.*|libfontconfig|libfreetype|libharfbuzz|libexpat|libz|libgcc_s|libstdc\+\+|libasound|libjack|libpipewire.*|libudev|libdbus-1|libgpg-error|libcom_err|libgmp|libuuid)\.so'

mkdir -p "$APPDIR/usr/bin" "$LIB"
install -m755 "$APPBIN" "$APPDIR/usr/bin/gamesir-bridge-app"

# Carregadores do gdk-pixbuf (o de SVG é o que importa para os ícones simbólicos).
PIXBUF_SRC="$SYSLIB/gdk-pixbuf-2.0/2.10.0/loaders"
PIXBUF_DST="$LIB/gdk-pixbuf-2.0/2.10.0/loaders"
mkdir -p "$PIXBUF_DST"
for l in svg png jpeg gif; do
    f="$PIXBUF_SRC/libpixbufloader-$l.so"
    [[ -e "$f" ]] && install -m644 "$f" "$PIXBUF_DST/"
done
if [[ -z "$(ls -A "$PIXBUF_DST")" ]]; then
    echo "aviso: nenhum carregador do gdk-pixbuf encontrado em $PIXBUF_SRC" >&2
fi

# Bibliotecas: o ldd já dá a lista completa (com as dependências das dependências).
copy_deps() {
    ldd "$1" | awk '/=> \//{print $3}' | while read -r so; do
        name="$(basename "$so")"
        if [[ "$name" =~ $EXCLUDE ]] || [[ -e "$LIB/$name" ]]; then
            continue
        fi
        install -m644 "$(readlink -f "$so")" "$LIB/$name"
    done
}
copy_deps "$APPBIN"
for f in "$PIXBUF_DST"/*.so; do
    [[ -e "$f" ]] && copy_deps "$f"
done

# Cache dos carregadores com caminho trocável; o AppRun troca @APPDIR@ pelo caminho real.
QUERY="$SYSLIB/gdk-pixbuf-2.0/gdk-pixbuf-query-loaders"
[[ -x "$QUERY" ]] || QUERY="$(command -v gdk-pixbuf-query-loaders || true)"
if [[ -n "$QUERY" ]] && compgen -G "$PIXBUF_DST/*.so" >/dev/null; then
    LD_LIBRARY_PATH="$LIB" "$QUERY" "$PIXBUF_DST"/*.so \
        | sed "s|$APPDIR|@APPDIR@|g" >"$LIB/gdk-pixbuf-2.0/2.10.0/loaders.cache.in"
fi

# Esquemas GSettings do GTK 4 (o seletor de arquivos aborta sem eles).
SCHEMAS="$APPDIR/usr/share/glib-2.0/schemas"
mkdir -p "$SCHEMAS"
cp /usr/share/glib-2.0/schemas/org.gtk.gtk4.Settings.*.gschema.xml "$SCHEMAS/" 2>/dev/null || true
glib-compile-schemas "$SCHEMAS"

# Ícones simbólicos do Adwaita (só o que o app usa de fato: os simbólicos).
ICONS="$APPDIR/usr/share/icons"
mkdir -p "$ICONS/Adwaita"
if [[ -d /usr/share/icons/Adwaita ]]; then
    cp /usr/share/icons/Adwaita/index.theme "$ICONS/Adwaita/"
    for d in symbolic symbolic-up-to-32; do
        [[ -d "/usr/share/icons/Adwaita/$d" ]] && cp -r "/usr/share/icons/Adwaita/$d" "$ICONS/Adwaita/"
    done
fi
[[ -e /usr/share/icons/hicolor/index.theme ]] && install -Dm644 /usr/share/icons/hicolor/index.theme "$ICONS/hicolor/index.theme"
if command -v gtk4-update-icon-cache >/dev/null; then
    gtk4-update-icon-cache -q -t -f "$ICONS/Adwaita" || true
    gtk4-update-icon-cache -q -t -f "$ICONS/hicolor" || true
fi

# Módulos GIO do sistema podem ser de outra versão da GLib; o app usa uma pasta própria.
mkdir -p "$LIB/gio/modules"

echo "app: $(du -sh "$LIB" | cut -f1) de bibliotecas, $(find "$LIB" -maxdepth 1 -name '*.so*' | wc -l) arquivos"
