#!/usr/bin/env bash
# Ubuntu / Ubuntu MATE (22.04 以降) でビルドに必要なパッケージを入れる
set -euo pipefail
sudo apt-get update
sudo apt-get install -y \
  build-essential curl pkg-config \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  libnotify-bin xdg-utils
if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust が見つかりません。rustup をインストールします..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  echo '新しいシェルを開くか `source "$HOME/.cargo/env"` を実行してください'
fi
