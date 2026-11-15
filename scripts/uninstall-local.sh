#!/usr/bin/env bash
# install-local.sh で入れたファイルを削除する (ログイン情報などのデータは残す)
set -euo pipefail
PREFIX="${PREFIX:-$HOME/.local}"
rm -f "$PREFIX/bin/notion-linux" \
      "$PREFIX/share/applications/notion-linux.desktop" \
      "$PREFIX/share/icons/hicolor/scalable/apps/notion-linux.svg"
rm -f "$PREFIX/share/icons/hicolor/256x256/apps/notion-linux.png"
echo "削除しました。データも消す場合: rm -rf ~/.config/notion-linux ~/.local/share/notion-linux ~/.local/state/notion-linux"
