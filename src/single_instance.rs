//! 二重起動の防止。2 つ目のプロセスは既存のウィンドウを前面に出すよう依頼して終了する。
//! (同じ Web データディレクトリを 2 つの WebKit プロセスで使うと壊れる恐れがあるため)

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let user = std::env::var("USER").unwrap_or_default();
    dir.join(format!("{}-{user}.sock", crate::config::APP_ID))
}

pub enum Instance {
    /// 自分が最初のインスタンス。listener で他プロセスからの依頼を受ける
    Primary(UnixListener),
    /// 既に起動中のインスタンスに依頼済み
    Secondary,
}

/// `url` は既存ウィンドウで開いてほしい URL (無ければ前面に出すだけ)
pub fn acquire(url: Option<&str>) -> Instance {
    let path = socket_path();
    if let Ok(mut stream) = UnixStream::connect(&path) {
        let msg = match url {
            Some(u) => format!("open {u}\n"),
            None => "show\n".to_string(),
        };
        if stream.write_all(msg.as_bytes()).is_ok() {
            return Instance::Secondary;
        }
    }
    // 接続できない = 古いソケットファイルが残っているだけ
    let _ = std::fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(l) => Instance::Primary(l),
        Err(e) => {
            eprintln!(
                "[notion-linux] ソケットを作成できません ({e})。二重起動チェックなしで続行します"
            );
            // ダミーとして抽象名前空間ではなく一時ファイルにバインドを試みる
            let fallback =
                std::env::temp_dir().join(format!("notion-linux-{}.sock", std::process::id()));
            Instance::Primary(UnixListener::bind(fallback).expect("unix socket"))
        }
    }
}

pub enum Request {
    Show,
    Open(String),
}

/// 別スレッドで依頼を待ち受け、届くたびに `on_request` を呼ぶ。
pub fn listen(listener: UnixListener, on_request: impl Fn(Request) + Send + 'static) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(stream).read_line(&mut line).is_ok() {
                let line = line.trim();
                if let Some(url) = line.strip_prefix("open ") {
                    on_request(Request::Open(url.to_string()));
                } else {
                    on_request(Request::Show);
                }
            }
        }
    });
}

pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}
