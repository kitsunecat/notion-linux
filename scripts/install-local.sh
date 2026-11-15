#!/usr/bin/env bash
# .deb を使わずに ~/.local へインストールする (sudo 不要)
set -euo pipefail
cd "$(dirname "$0")/.."
# --no-build: ビルド済みの target/release/notion-linux をそのまま使う (update.sh から呼ぶ)
if [ "${1:-}" != "--no-build" ]; then
  cargo build --release
fi
PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 target/release/notion-linux "$PREFIX/bin/notion-linux"
NOTION_ICON="$PREFIX/share/icons/hicolor/256x256/apps/notion-linux.png"
if [ ! -e "$NOTION_ICON" ]; then
  install -Dm644 assets/notion-linux.svg "$PREFIX/share/icons/hicolor/scalable/apps/notion-linux.svg"
  # Notion のアイコンを notion.so から取得して保存 (取得済みなら何もしない。失敗したらオリジナルのまま)
  "$PREFIX/bin/notion-linux" --fetch-icon \
    || echo "Notion のアイコンを取得できなかったため、オリジナルのアイコンを使います"
fi
sed "s|^Exec=notion-linux|Exec=$PREFIX/bin/notion-linux|" assets/notion-linux.desktop \
  > /tmp/notion-linux.desktop
install -Dm644 /tmp/notion-linux.desktop "$PREFIX/share/applications/notion-linux.desktop"
rm -f /tmp/notion-linux.desktop
update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
echo "インストールしました: $PREFIX/bin/notion-linux"
echo "メニュー「アプリケーション → オフィス」から Notion を起動できます。"
