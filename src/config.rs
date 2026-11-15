//! 設定ファイルとウィンドウ状態の保存/読み込み。
//!
//! - 設定:       ~/.config/notion-linux/config.json   (ユーザーが手で編集する)
//! - 状態:       ~/.local/state/notion-linux/state.json (アプリが自動で書き込む)
//! - Web データ: ~/.local/share/notion-linux/webdata  (Cookie・ログイン情報など)

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const APP_ID: &str = "notion-linux";

pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 起動時に開く URL
    pub start_url: String,
    /// 前回最後に開いていたページを次回起動時に開く
    pub restore_last_page: bool,
    /// ウィンドウの × ボタンでトレイに格納する (false なら終了する)
    pub close_to_tray: bool,
    /// トレイアイコンを表示する
    pub tray_icon: bool,
    /// WebView に送る User-Agent。空文字なら WebKitGTK の既定値を使う
    pub user_agent: String,
    /// WebKitGTK の DMA-BUF レンダラーを無効化する (NVIDIA 環境での白画面対策)
    pub disable_dmabuf_renderer: bool,
    /// スペルチェックを有効にする
    pub spell_checking: bool,
    /// スペルチェックの言語 (例: ["en_US"])
    pub spell_checking_languages: Vec<String>,
    /// アプリ内で開く追加ホスト (SSO の IdP など)。サブドメインも含む
    pub extra_internal_hosts: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            start_url: "https://www.notion.so/".into(),
            restore_last_page: true,
            close_to_tray: true,
            tray_icon: true,
            user_agent: DEFAULT_USER_AGENT.into(),
            disable_dmabuf_renderer: true,
            spell_checking: true,
            spell_checking_languages: vec!["en_US".into()],
            extra_internal_hosts: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
    pub zoom: f64,
    /// 最後に開いていたページ (次回起動時に復元)
    pub last_url: Option<String>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            width: 1280,
            height: 860,
            maximized: false,
            zoom: 1.0,
            last_url: None,
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_ID)
}

pub fn state_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_ID)
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_ID)
}

fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

fn state_path() -> PathBuf {
    state_dir().join("state.json")
}

impl Config {
    /// 設定を読み込む。ファイルが無ければ既定値で作成する。
    pub fn load() -> Self {
        let path = config_path();
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                eprintln!(
                    "[notion-linux] {} の読み込みに失敗: {e} (既定値を使用)",
                    path.display()
                );
                Self::default()
            }),
            Err(_) => {
                let cfg = Self::default();
                if fs::create_dir_all(config_dir()).is_ok()
                    && let Ok(s) = serde_json::to_string_pretty(&cfg)
                {
                    let _ = fs::write(&path, s);
                }
                cfg
            }
        }
    }
}

impl WindowState {
    pub fn load() -> Self {
        fs::read_to_string(state_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if fs::create_dir_all(state_dir()).is_err() {
            return;
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let tmp = state_path().with_extension("json.tmp");
            if fs::write(&tmp, s).is_ok() {
                let _ = fs::rename(tmp, state_path());
            }
        }
    }
}
