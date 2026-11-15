#!/usr/bin/env bash
# リポジトリの更新を取り込み、ビルドして再インストールする。
#
# 使い方: ./scripts/update.sh [--deb | --local] [--force] [--restart]
#   --deb      .deb を作って apt で入れ直す
#   --local    ~/.local に入れ直す (install-local.sh と同じ)
#              (どちらも省略時は、今のインストール方法を自動で判定)
#   --force    git に更新がなくてもビルドして入れ直す
#   --restart  インストール後、起動中のアプリを再起動する
set -euo pipefail
cd "$(dirname "$0")/.."

mode="" force=0 restart=0
for arg in "$@"; do
  case "$arg" in
    --deb) mode=deb ;;
    --local) mode=local ;;
    --force) force=1 ;;
    --restart) restart=1 ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "不明なオプション: $arg" >&2; exit 2 ;;
  esac
done

if [ -z "$mode" ]; then
  if dpkg-query -W -f='${Status}' notion-linux 2>/dev/null | grep -q "install ok installed"; then
    mode=deb
  elif [ -x "${PREFIX:-$HOME/.local}/bin/notion-linux" ]; then
    mode=local
  else
    echo "インストール済みの notion-linux が見つかりません。--deb か --local を指定してください。" >&2
    exit 1
  fi
fi
echo "==> インストール方法: $mode"

# Rust を rustup で入れた直後でも cargo が見つかるように
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# 1. git の更新を取り込む
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "ローカルに未コミットの変更があります。commit するか 'git stash' してから実行してください。" >&2
  git status --short --untracked-files=no >&2
  exit 1
fi
before=$(git rev-parse HEAD)
echo "==> git pull"
git pull --ff-only
after=$(git rev-parse HEAD)

if [ "$before" = "$after" ] && [ "$force" -eq 0 ]; then
  echo "==> 更新はありません ($(git log -1 --format='%h %s'))。入れ直す場合は --force を付けてください。"
  exit 0
fi
if [ "$before" != "$after" ]; then
  echo "==> 取り込んだ変更:"
  git log --oneline "$before..$after"
fi

# 2. ビルドしてインストール
if [ "$mode" = deb ]; then
  command -v cargo-deb >/dev/null 2>&1 || cargo install cargo-deb --locked
  echo "==> .deb をビルド"
  rm -f target/debian/notion-linux_*.deb
  cargo deb
  deb=$(ls -t target/debian/notion-linux_*.deb | head -n1)
  echo "==> インストール: $deb (sudo のパスワードを求められます)"
  sudo apt install -y --reinstall "./$deb"
else
  ./scripts/install-local.sh
fi

# 3. 起動中のアプリの扱い
if pgrep -x notion-linux >/dev/null 2>&1; then
  if [ "$restart" -eq 1 ]; then
    echo "==> notion-linux を再起動"
    pkill -x notion-linux || true
    for _ in $(seq 1 20); do pgrep -x notion-linux >/dev/null || break; sleep 0.25; done
    nohup notion-linux >/dev/null 2>&1 &
  else
    echo "==> notion-linux が起動中です。新しいバージョンを使うには Ctrl+Q で終了してから起動し直してください (--restart で自動再起動)。"
  fi
fi
echo "==> 完了: $(git log -1 --format='%h %s')"
