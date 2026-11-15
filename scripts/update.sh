#!/usr/bin/env bash
# リポジトリを更新し、ビルドして、インストール済みの notion-linux を上書きする。
#
# 使い方: ./scripts/update.sh [--deb | --local] [--no-restart]
#   1. git pull (更新がなくても続行)
#   2. ビルド
#   3. インストール済みのアプリを上書き
#      - .deb で入れたもの      → .deb を作って apt で入れ直す
#      - ~/.local に入れたもの  → ~/.local/bin などを上書き
#      両方入っていれば両方とも上書きする。どちらも無ければ ~/.local に入れる。
#   4. 起動中のアプリ (トレイに格納中も含む) を再起動して新しいバージョンにする
#
#   --deb / --local  上書きする対象を指定する (自動判定しない)
#   --no-restart     起動中のアプリを再起動しない
set -euo pipefail
cd "$(dirname "$0")/.."

targets=() restart=1
for arg in "$@"; do
  case "$arg" in
    --deb) targets+=(deb) ;;
    --local) targets+=(local) ;;
    --no-restart) restart=0 ;;
    --restart|--force) ;; # 旧オプション (今は既定の動作)
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "不明なオプション: $arg" >&2; exit 2 ;;
  esac
done

PREFIX="${PREFIX:-$HOME/.local}"
if [ ${#targets[@]} -eq 0 ]; then
  if dpkg-query -W -f='${Status}' notion-linux 2>/dev/null | grep -q "install ok installed"; then
    targets+=(deb)
  fi
  if [ -e "$PREFIX/bin/notion-linux" ]; then
    targets+=(local)
  fi
  if [ ${#targets[@]} -eq 0 ]; then
    echo "==> インストール済みのアプリが見つからないため、$PREFIX にインストールします"
    targets+=(local)
  fi
fi
echo "==> 上書きする対象: ${targets[*]}"

# Rust を rustup で入れた直後でも cargo が見つかるように
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

# 1. git の更新を取り込む
echo "==> [1/3] git pull"
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "ローカルに未コミットの変更があります。commit するか 'git stash' してから実行してください。" >&2
  git status --short --untracked-files=no >&2
  exit 1
fi
before=$(git rev-parse HEAD)
git pull --ff-only
after=$(git rev-parse HEAD)
if [ "$before" = "$after" ]; then
  echo "    git の更新はありません (このままビルドして上書きします)"
else
  echo "    取り込んだ変更:"
  git log --oneline "$before..$after" | sed 's/^/      /'
fi

# 2. ビルド
echo "==> [2/3] ビルド"
cargo build --release

# 3. インストール済みのアプリを上書き
echo "==> [3/3] インストール済みのアプリを上書き"
for t in "${targets[@]}"; do
  if [ "$t" = deb ]; then
    command -v cargo-deb >/dev/null 2>&1 || cargo install cargo-deb --locked
    rm -f target/debian/notion-linux_*.deb
    cargo deb --no-build
    deb=$(ls -t target/debian/notion-linux_*.deb | head -n1)
    echo "    .deb を入れ直します: $deb (sudo のパスワードを求められます)"
    sudo apt install -y --reinstall "./$deb"
    installed=/usr/bin/notion-linux
  else
    ./scripts/install-local.sh --no-build
    installed="$PREFIX/bin/notion-linux"
  fi
  if cmp -s target/release/notion-linux "$installed"; then
    echo "    上書きしました: $installed"
  else
    echo "    警告: $installed がビルドしたものと一致しません" >&2
  fi
done

# どのコマンドが起動されるか (両方ある場合は PATH の先頭が使われる)
if command -v notion-linux >/dev/null 2>&1; then
  echo "    'notion-linux' で起動されるのは: $(command -v notion-linux)"
fi

# 4. 起動中のアプリを再起動 (× でトレイに格納していると動き続けているため)
if pgrep -x notion-linux >/dev/null 2>&1; then
  if [ "$restart" -eq 1 ]; then
    echo "==> 起動中の notion-linux を再起動します"
    pkill -x notion-linux || true
    for _ in $(seq 1 40); do pgrep -x notion-linux >/dev/null || break; sleep 0.25; done
    pkill -9 -x notion-linux 2>/dev/null || true
    nohup notion-linux >/dev/null 2>&1 &
  else
    echo "==> notion-linux が起動中です。トレイの「終了」か Ctrl+Q で終了してから起動し直してください。"
  fi
fi
echo "==> 完了: $(git log -1 --format='%h %s')"
