#!/usr/bin/env bash
# Prepara uma versão: atualiza Cargo.toml, metainfo e CHANGELOG, faz o commit e cria a tag.
# O push da tag dispara .github/workflows/release.yml.
#
#   tools/release.sh 0.3.0
set -euo pipefail

V="${1:?uso: tools/release.sh X.Y.Z}"
[[ "$V" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "versão inválida: $V" >&2; exit 2; }
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
[[ -z "$(git status --porcelain)" ]] || { echo "há mudanças não commitadas" >&2; exit 1; }
grep -q '^## \[Unreleased\]' CHANGELOG.md || { echo "CHANGELOG.md sem [Unreleased]" >&2; exit 1; }

TODAY="$(date +%F)"
OLD="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"
sed -i "0,/^version = \".*\"/s//version = \"$V\"/" Cargo.toml
sed -i "s/gamesir-core = { path = \"crates\/core\", version = \"[^\"]*\" }/gamesir-core = { path = \"crates\/core\", version = \"$V\" }/" Cargo.toml
sed -i "s|<releases>|<releases>\n    <release version=\"$V\" date=\"$TODAY\"/>|" data/metainfo/*.metainfo.xml
sed -i "s/^## \[Unreleased\]/## [Unreleased]\n\n## [$V] - $TODAY/" CHANGELOG.md
sed -i "s|^\[Unreleased\]: .*|[Unreleased]: https://github.com/yurisanp/gamesir-bridge/compare/v$V...HEAD\n[$V]: https://github.com/yurisanp/gamesir-bridge/compare/v$OLD...v$V|" CHANGELOG.md
cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace >/dev/null

git commit -am "chore: versão $V"
git tag -a "v$V" -m "GameSir Bridge $V"
echo "Pronto. Revise com 'git show' e publique com: git push && git push origin v$V"
