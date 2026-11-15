//! どの URL をアプリ内で開き、どれを既定のブラウザに渡すかを決める。

/// Notion 本体のホスト (サブドメインを含む)
const NOTION_HOSTS: &[&str] = &[
    "notion.so",
    "notion.com",
    "notion.site",
    "notion-static.com",
    "notionusercontent.com",
];

/// ログイン (OAuth / SSO) で使われるホスト。ポップアップや遷移をアプリ内で許可する。
const AUTH_HOSTS: &[&str] = &[
    "accounts.google.com",
    "accounts.youtube.com",
    "appleid.apple.com",
    "idmsa.apple.com",
    "login.microsoftonline.com",
    "login.live.com",
    "login.microsoft.com",
    "okta.com",
    "onelogin.com",
    "auth0.com",
    "slack.com",
    "github.com",
];

/// `scheme://host[:port]/...` からホスト名 (小文字) を取り出す。
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = if let Some(stripped) = host_port.strip_prefix('[') {
        stripped.split(']').next()?
    } else {
        host_port.split(':').next()?
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn matches(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn scheme_of(url: &str) -> &str {
    url.split_once(':').map(|(s, _)| s).unwrap_or("")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// Notion 本体
    Notion,
    /// ログイン関連 (アプリ内で開く)
    Auth,
    /// about:blank や data: など、WebView 内部のもの
    Internal,
    /// それ以外の http(s) / mailto など: 外部ブラウザ・アプリで開く
    External,
}

pub struct LinkPolicy {
    extra_hosts: Vec<String>,
}

impl LinkPolicy {
    pub fn new(extra_hosts: &[String]) -> Self {
        Self {
            extra_hosts: extra_hosts.iter().map(|h| h.to_ascii_lowercase()).collect(),
        }
    }

    pub fn classify(&self, url: &str) -> LinkKind {
        match scheme_of(url) {
            "about" | "data" | "blob" | "javascript" | "" => return LinkKind::Internal,
            "http" | "https" => {}
            _ => return LinkKind::External,
        }
        let Some(host) = host_of(url) else {
            return LinkKind::Internal;
        };
        if NOTION_HOSTS.iter().any(|d| matches(&host, d)) {
            LinkKind::Notion
        } else if AUTH_HOSTS.iter().any(|d| matches(&host, d))
            || self.extra_hosts.iter().any(|d| matches(&host, d))
        {
            LinkKind::Auth
        } else {
            LinkKind::External
        }
    }

    /// アプリ内で開いてよい URL か
    pub fn is_in_app(&self, url: &str) -> bool {
        self.classify(url) != LinkKind::External
    }
}

/// 外部ブラウザ (xdg-open) で開く
pub fn open_external(url: &str) {
    if let Err(e) = open::that_detached(url) {
        eprintln!("[notion-linux] 外部で開けませんでした {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_parsing() {
        assert_eq!(
            host_of("https://www.notion.so/abc?x=1").as_deref(),
            Some("www.notion.so")
        );
        assert_eq!(
            host_of("https://user@Example.COM:8080/").as_deref(),
            Some("example.com")
        );
        assert_eq!(host_of("https://[::1]:80/").as_deref(), Some("::1"));
        assert_eq!(host_of("about:blank"), None);
    }

    #[test]
    fn classify_urls() {
        let p = LinkPolicy::new(&["sso.example.co.jp".into()]);
        assert_eq!(
            p.classify("https://www.notion.so/page-123"),
            LinkKind::Notion
        );
        assert_eq!(p.classify("https://myteam.notion.site/x"), LinkKind::Notion);
        assert_eq!(p.classify("https://file.notion.so/f/abc"), LinkKind::Notion);
        assert_eq!(
            p.classify("https://accounts.google.com/o/oauth2"),
            LinkKind::Auth
        );
        assert_eq!(
            p.classify("https://login.sso.example.co.jp/"),
            LinkKind::Auth
        );
        assert_eq!(p.classify("about:blank"), LinkKind::Internal);
        assert_eq!(p.classify("https://example.com/"), LinkKind::External);
        assert_eq!(p.classify("https://evilnotion.so/"), LinkKind::External);
        assert_eq!(
            p.classify("https://notion.so.evil.com/"),
            LinkKind::External
        );
        assert_eq!(p.classify("mailto:a@b.c"), LinkKind::External);
    }
}
