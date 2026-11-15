//! Notion 公式アイコンの取得と保存。
//!
//! Notion のロゴは Notion Labs, Inc. の商標なので、このリポジトリには含めない。
//! 代わりに **インストール時** に notion.so からアイコンをダウンロードしてローカルに保存する。
//! - .deb: postinst が `notion-linux --fetch-icon --system` を実行 → /usr/share/icons/hicolor
//! - install-local.sh: `notion-linux --fetch-icon` を実行 → ~/.local/share/icons/hicolor
//!
//! アプリ起動時はネットワークに接続せず、保存済みのアイコンを読むだけ。
//! 保存済みのものが無い場合や `use_notion_icon: false` の場合は、同梱のオリジナルアイコンを使う。

use std::path::{Path, PathBuf};
use std::time::Duration;

use gtk::gdk_pixbuf::{InterpType, Pixbuf};

const SITE: &str = "https://www.notion.so";
const SIZE: i32 = 256;
const MAX_BYTES: u64 = 5 * 1024 * 1024;
const SYSTEM_ICONS_DIR: &str = "/usr/share/icons/hicolor";

/// アイコンの保存先
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// ~/.local/share/icons/hicolor
    User,
    /// /usr/share/icons/hicolor (root 権限が必要)
    System,
}

fn icons_dir(target: Target) -> PathBuf {
    match target {
        Target::User => dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("icons/hicolor"),
        Target::System => PathBuf::from(SYSTEM_ICONS_DIR),
    }
}

/// ウィンドウ・トレイ用に保存する PNG
pub fn png_path(target: Target) -> PathBuf {
    icons_dir(target).join(format!(
        "{s}x{s}/apps/{}.png",
        crate::config::APP_ID,
        s = SIZE
    ))
}

/// アプリメニュー用。PNG を同じ名前の SVG で包み、scalable のオリジナルアイコンより優先させる
fn svg_path(target: Target) -> PathBuf {
    icons_dir(target).join(format!("scalable/apps/{}.svg", crate::config::APP_ID))
}

/// インストール時に保存した Notion アイコンを読み込む (ユーザー → システムの順)
pub fn load_installed() -> Option<Pixbuf> {
    [Target::User, Target::System]
        .into_iter()
        .find_map(|t| Pixbuf::from_file(png_path(t)).ok())
}

/// `--fetch-icon` の本体: ダウンロードして保存する
pub fn fetch_and_install(target: Target) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let downloaded = fetch(crate::config::DEFAULT_USER_AGENT)?;
    install(&downloaded, target)?;
    Ok(png_path(target))
}

fn fetch(user_agent: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into();
    let get = |url: &str| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut resp = agent.get(url).header("User-Agent", user_agent).call()?;
        Ok(resp
            .body_mut()
            .with_config()
            .limit(MAX_BYTES)
            .read_to_vec()?)
    };

    // トップページの <link rel="...icon..."> から候補を探し、だめなら定番のパスを試す
    let mut candidates = match get(&format!("{SITE}/")) {
        Ok(html) => icon_links(&String::from_utf8_lossy(&html)),
        Err(_) => vec![],
    };
    for fallback in [
        "/apple-touch-icon.png",
        "/images/favicon.ico",
        "/favicon.ico",
    ] {
        candidates.push(format!("{SITE}{fallback}"));
    }

    let mut last_err: Box<dyn std::error::Error> = "候補が見つかりません".into();
    for url in candidates {
        match get(&url) {
            Ok(bytes) if !bytes.is_empty() => {
                let tmp = std::env::temp_dir().join(format!(
                    "{}-icon-{}.download",
                    crate::config::APP_ID,
                    std::process::id()
                ));
                std::fs::write(&tmp, bytes)?;
                // 画像として読めるか確認 (読めなければ次の候補へ)
                if Pixbuf::from_file(&tmp).is_ok() {
                    return Ok(tmp);
                }
                let _ = std::fs::remove_file(&tmp);
                last_err = format!("{url} は画像として読み込めません").into();
            }
            Ok(_) => last_err = format!("{url} が空です").into(),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// HTML から `<link rel="apple-touch-icon|icon" href="...">` を探し、良さそうな順に返す。
fn icon_links(html: &str) -> Vec<String> {
    let mut found: Vec<(i32, String)> = vec![];
    let lower = html.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(start) = lower[pos..].find("<link") {
        let start = pos + start;
        let Some(end) = lower[start..].find('>') else {
            break;
        };
        let end = start + end;
        let tag = &html[start..end];
        pos = end;

        let Some(rel) = attr(tag, "rel").map(|r| r.to_ascii_lowercase()) else {
            continue;
        };
        let Some(href) = attr(tag, "href") else {
            continue;
        };
        let size: i32 = attr(tag, "sizes")
            .and_then(|s| s.split(['x', 'X']).next().and_then(|n| n.parse().ok()))
            .unwrap_or(0);
        let score = if rel.contains("apple-touch-icon") {
            10_000 + size
        } else if rel.split_whitespace().any(|r| r == "icon") {
            if href.ends_with(".svg") { 5_000 } else { size }
        } else {
            continue;
        };
        found.push((score, absolutize(&href)));
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, u)| u).collect()
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut search = 0;
    while let Some(i) = lower[search..].find(name) {
        let i = search + i;
        search = i + name.len();
        // 属性名の直前は空白であること (例: data-href を href と誤認しない)
        if i == 0 || !lower.as_bytes()[i - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = tag[i + name.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next()?,
            _ => rest.split(|c: char| c.is_whitespace() || c == '/').next()?,
        };
        return Some(value.replace("&amp;", "&"));
    }
    None
}

fn absolutize(href: &str) -> String {
    if href.starts_with("https://") || href.starts_with("http://") {
        href.to_string()
    } else if let Some(rest) = href.strip_prefix("//") {
        format!("https://{rest}")
    } else if href.starts_with('/') {
        format!("{SITE}{href}")
    } else {
        format!("{SITE}/{href}")
    }
}

/// ダウンロードした画像を 256px の PNG として保存し、アプリメニュー用の SVG も作る。
pub fn install(downloaded: &Path, target: Target) -> Result<Pixbuf, Box<dyn std::error::Error>> {
    let src = Pixbuf::from_file(downloaded)?;
    let _ = std::fs::remove_file(downloaded);
    let pixbuf = if src.width() == SIZE && src.height() == SIZE {
        src
    } else {
        src.scale_simple(SIZE, SIZE, InterpType::Hyper)
            .ok_or("アイコンの縮小に失敗しました")?
    };

    let png = png_path(target);
    std::fs::create_dir_all(png.parent().unwrap())?;
    pixbuf.savev(&png, "png", &[])?;

    let svg = svg_path(target);
    std::fs::create_dir_all(svg.parent().unwrap())?;
    let b64 = gtk::glib::base64_encode(&std::fs::read(&png)?);
    std::fs::write(
        &svg,
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
             width=\"{SIZE}\" height=\"{SIZE}\" viewBox=\"0 0 {SIZE} {SIZE}\">\
             <image width=\"{SIZE}\" height=\"{SIZE}\" xlink:href=\"data:image/png;base64,{b64}\"/></svg>\n"
        ),
    )?;

    // アイコンキャッシュがある場合は更新 (無ければ何もしない)
    let dir = icons_dir(target);
    if dir.join("icon-theme.cache").exists() {
        let _ = std::process::Command::new("gtk-update-icon-cache")
            .args(["-f", "-t"])
            .arg(&dir)
            .status();
    }
    Ok(pixbuf)
}

/// Pixbuf を (RGBA, 幅, 高さ) に変換
pub fn to_rgba(pixbuf: &Pixbuf) -> (Vec<u8>, u32, u32) {
    let pb = pixbuf
        .add_alpha(false, 0, 0, 0)
        .unwrap_or_else(|_| pixbuf.clone());
    let (w, h) = (pb.width() as usize, pb.height() as usize);
    let stride = pb.rowstride() as usize;
    let bytes = pb.read_pixel_bytes();
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        out.extend_from_slice(&bytes[y * stride..y * stride + w * 4]);
    }
    (out, w as u32, h as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_icon_links_in_order() {
        let html = r#"<html><head>
            <link rel="shortcut icon" href="/images/favicon.ico">
            <link rel="icon" type="image/png" sizes="32x32" href="/images/favicon-32.png">
            <link data-href="/nope.png" rel="apple-touch-icon" href="/images/logo-ios.png?v=2&amp;x=1">
            <link rel="stylesheet" href="/app.css">
            <LINK REL='icon' SIZES='192x192' HREF='https://cdn.example.com/i192.png'/>
        </head></html>"#;
        assert_eq!(
            icon_links(html),
            vec![
                "https://www.notion.so/images/logo-ios.png?v=2&x=1".to_string(),
                "https://cdn.example.com/i192.png".to_string(),
                "https://www.notion.so/images/favicon-32.png".to_string(),
                "https://www.notion.so/images/favicon.ico".to_string(),
            ]
        );
    }

    #[test]
    fn absolutize_urls() {
        assert_eq!(absolutize("//a.b/c.png"), "https://a.b/c.png");
        assert_eq!(absolutize("x.png"), "https://www.notion.so/x.png");
    }

    #[test]
    fn install_writes_png_and_svg() {
        let tmp = std::env::temp_dir().join(format!("nl-icon-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // SAFETY: このテストだけが XDG_DATA_HOME を使う
        unsafe { std::env::set_var("XDG_DATA_HOME", &tmp) };
        let src = Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, true, 8, 32, 32).unwrap();
        src.fill(0x336699ff);
        let dl = tmp.join("dl.png");
        src.savev(&dl, "png", &[]).unwrap();

        let pb = install(&dl, Target::User).unwrap();
        assert_eq!((pb.width(), pb.height()), (SIZE, SIZE));
        assert!(png_path(Target::User).starts_with(&tmp) && png_path(Target::User).exists());
        let svg = std::fs::read_to_string(svg_path(Target::User)).unwrap();
        assert!(svg.contains("data:image/png;base64,"));
        let (rgba, w, h) = to_rgba(&load_installed().unwrap());
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
