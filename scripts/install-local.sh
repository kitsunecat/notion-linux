#!/usr/bin/env bash
# .deb を使わずに ~/.local へインストールする (sudo 不要)
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release
PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 target/release/notion-linux "$PREFIX/bin/notion-linux"
install -Dm644 assets/notion-linux.svg "$PREFIX/share/icons/hicolor/scalable/apps/notion-linux.svg"
# Notion のアイコンを notion.so から取得して ~/.local/share/icons に保存 (失敗したらオリジナルのまま)
"$PREFIX/bin/notion-linux" --fetch-icon \
  || echo "Notion のアイコンを取得できなかったため、オリジナルのアイコンを使います"
sed "s|^Exec=notion-linux|Exec=$PREFIX/bin/notion-linux|" assets/notion-linux.desktop \
  > /tmp/notion-linux.desktop
install -Dm644 /tmp/notion-linux.desktop "$PREFIX/share/applications/notion-linux.desktop"
rm -f /tmp/notion-linux.desktop
update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
echo "インストールしました: $PREFIX/bin/notion-linux"
echo "メニュー「アプリケーション → オフィス」から Notion を起動できます。"
