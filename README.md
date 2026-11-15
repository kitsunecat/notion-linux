# notion-linux

Rust 製の **Linux 用 Notion デスクトップアプリ (非公式)** です。
[wry](https://github.com/tauri-apps/wry) / [tao](https://github.com/tauri-apps/tao) と WebKitGTK で `https://www.notion.so` を専用ウィンドウに表示します。

> Notion Labs, Inc. とは関係のない非公式クライアントです。「Notion」および Notion のロゴは Notion Labs, Inc. の商標です。
> このリポジトリにはロゴ画像を含めていません。Notion のアイコンは、各自の環境でインストール時に notion.so から取得して保存します (下記「アイコン」参照)。

## 機能

- **ログイン状態の保持**: Cookie / ローカルストレージを `~/.local/share/notion-linux/webdata` に保存
- **ウィンドウ状態の復元**: サイズ・位置・最大化・ズーム倍率・最後に開いていたページ
- **外部リンクは既定のブラウザで開く**: Notion とログイン (Google / Apple / Microsoft など) 以外はブラウザへ
- **ログイン用ポップアップ対応**: 「Google で続行」などのポップアップをアプリ内で開き、終了後に自動で閉じる
- **システムトレイ**: × でトレイに格納、メニューから表示 / 再読み込み / 終了
- **デスクトップ通知**: Notion の通知を許可済みの状態で起動
- **二重起動防止**: 2 回目の起動は既存ウィンドウを前面に出す (`notion-linux <URL>` ならそのページを開く)
- **ダウンロード**: `~/ダウンロード` (XDG のダウンロードフォルダ) に保存して通知
- **Notion のアイコン**: インストール時に notion.so から公式アイコンを取得して保存し、ウィンドウ・トレイ・アプリメニューに使用 (起動時の通信なし)
- **キーボードショートカット**

| キー | 動作 |
| --- | --- |
| `Ctrl+R` / `F5` | 再読み込み |
| `Ctrl+=` / `Ctrl++` / `Ctrl+-` / `Ctrl+0` | 拡大 / 縮小 / 100% |
| `Alt+←` / `Alt+→` (マウスの戻る/進むボタン) | 戻る / 進む |
| `Ctrl+Q` | 終了 |

## Ubuntu MATE でのインストール

Ubuntu MATE 22.04 / 24.04 を想定しています。

### 方法 A: .deb を作ってインストール (おすすめ)

```bash
# 1. 依存パッケージと Rust を入れる
git clone https://github.com/kitsunecat/notion-linux.git
cd notion-linux
./scripts/install-deps.sh
source "$HOME/.cargo/env"   # Rust を今入れた場合のみ

# 2. .deb を作ってインストール
cargo install cargo-deb
cargo deb
sudo apt install ./target/debian/notion-linux_*.deb
```

GitHub Actions でもビルドしているので、Actions の「notion-linux-deb」アーティファクトやリリースから .deb を直接ダウンロードしても構いません。

### 方法 B: ~/.local に直接インストール (sudo 不要)

```bash
./scripts/install-deps.sh   # ビルド用パッケージは sudo が必要
./scripts/install-local.sh
```

アンインストールは `./scripts/uninstall-local.sh` です。

### 起動

メニューの **アプリケーション → オフィス → Notion** から、または端末で `notion-linux` を実行します。

### 必要な apt パッケージ (手動で入れる場合)

```bash
sudo apt install build-essential pkg-config \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  libnotify-bin xdg-utils
```

実行だけなら `libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1` で動きます (.deb は自動で入れます)。

## 設定

初回起動時に `~/.config/notion-linux/config.json` が作られます。編集後にアプリを再起動してください。

| キー | 既定値 | 説明 |
| --- | --- | --- |
| `start_url` | `https://www.notion.so/` | 起動時に開く URL |
| `restore_last_page` | `true` | 前回最後に開いていたページを開く |
| `close_to_tray` | `true` | × でトレイに格納 (`false` なら終了) |
| `tray_icon` | `true` | トレイアイコンを表示 |
| `use_notion_icon` | `true` | インストール時に保存した Notion アイコンを使う (`false` で同梱のオリジナルアイコン) |
| `user_agent` | Chrome (Linux) 相当 | 空文字にすると WebKitGTK 既定の UA |
| `disable_dmabuf_renderer` | `true` | 白い画面になる環境 (NVIDIA など) 向けの回避策 |
| `spell_checking` / `spell_checking_languages` | `true` / `["en_US"]` | スペルチェック |
| `extra_internal_hosts` | `[]` | アプリ内で開く追加ホスト (会社の SSO など)。例: `["sso.example.co.jp"]` |

保存場所:

- 設定: `~/.config/notion-linux/config.json`
- ウィンドウ状態: `~/.local/state/notion-linux/state.json`
- ログイン情報などの Web データ: `~/.local/share/notion-linux/webdata` (消すとログアウトします)

## アイコン

Notion のアイコンは **インストール時に 1 回だけ** notion.so から取得して保存します。アプリの起動時に取得し直すことはありません。

| インストール方法 | 保存先 |
| --- | --- |
| .deb (`apt install`) | `/usr/share/icons/hicolor/` (postinst が `notion-linux --fetch-icon --system` を実行) |
| `scripts/install-local.sh` | `~/.local/share/icons/hicolor/` (`notion-linux --fetch-icon` を実行) |

ネットワークに繋がっていないなどで取得できなかった場合は、同梱のオリジナルアイコンを使います。後から取得し直すには次のどちらかを実行します。

```bash
sudo notion-linux --fetch-icon --system   # .deb でインストールした場合
notion-linux --fetch-icon                 # 自分のユーザーだけに保存する場合
```

メニューのアイコンがすぐに変わらない場合は、パネルを再起動 (`mate-panel --replace &`) するか一度ログアウトしてください。
.deb をアンインストールすると、取得したアイコンも削除されます。

## トラブルシューティング

- **トレイアイコンが出ない**: MATE パネルに「インジケーターアプレット」または「通知スペース」があるか確認してください (パネルを右クリック → パネルへ追加)。
- **画面が真っ白 / 表示が乱れる**: `WEBKIT_DISABLE_COMPOSITING_MODE=1 notion-linux` を試してください。
- **Google ログインで「安全でないブラウザ」と言われる**: Notion の「メールでログイン」(ログインコード) を使うか、`user_agent` を変えてみてください。
- **開発者ツール**: `NOTION_LINUX_DEVTOOLS=1 notion-linux` で起動し、右クリック →「要素を調査」。

## 開発

```bash
cargo run              # デバッグ実行 (開発者ツール有効)
cargo test             # URL 振り分けとアイコンのテスト
cargo clippy --all-targets -- -D warnings
```

構成:

- `src/main.rs` … ウィンドウ・WebView・トレイ・イベントループ
- `src/popup.rs` … 外部リンクの振り分けとログイン用ポップアップ
- `src/links.rs` … URL をアプリ内 / 外部ブラウザに分類
- `src/config.rs` … 設定とウィンドウ状態
- `src/single_instance.rs` … 二重起動防止 (Unix ソケット)
- `src/icon.rs` … オリジナルアイコンの描画 (フォールバック用)
- `src/notion_icon.rs` … Notion 公式アイコンの取得とキャッシュ

## ライセンス

MIT
