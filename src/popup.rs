//! 外部リンクの振り分けと、ログイン用ポップアップウィンドウの管理。

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, NavigationType, PolicyDecisionExt,
    PolicyDecisionType, URIRequestExt, WebViewExt,
};
use wry::NewWindowResponse;

use crate::icon;
use crate::links::{self, LinkKind, LinkPolicy};

/// `window.open()` / target="_blank" で新しいウィンドウを求められたときの処理。
///
/// - 外部サイト: 既定のブラウザで開き、ウィンドウは作らない
/// - Notion / ログイン関連: 親ページと繋がった (window.opener が使える) ポップアップを作る
pub fn handle_new_window(
    url: &str,
    opener: &webkit2gtk::WebView,
    policy: &Rc<LinkPolicy>,
) -> NewWindowResponse {
    match policy.classify(url) {
        LinkKind::External => {
            links::open_external(url);
            NewWindowResponse::Deny
        }
        _ => NewWindowResponse::Create {
            webview: create_popup(opener, policy.clone()),
        },
    }
}

fn create_popup(opener: &webkit2gtk::WebView, policy: Rc<LinkPolicy>) -> webkit2gtk::WebView {
    let wv = webkit2gtk::WebView::with_related_view(opener);
    if let Some(settings) = WebViewExt::settings(opener) {
        wv.set_settings(&settings);
    }

    let win = gtk::Window::new(gtk::WindowType::Toplevel);
    win.set_title("Notion");
    win.set_default_size(560, 720);
    win.set_position(gtk::WindowPosition::Center);
    if let Some(top) = opener
        .toplevel()
        .and_then(|t| t.downcast::<gtk::Window>().ok())
    {
        win.set_transient_for(Some(&top));
    }
    let size = 64;
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_bytes(
        &gtk::glib::Bytes::from_owned(icon::rgba(size as u32)),
        gtk::gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        size,
        size,
        size * 4,
    );
    win.set_icon(Some(&pixbuf));
    win.add(&wv);

    // WebKit が準備できたら表示 / window.close() で閉じる
    {
        let win = win.clone();
        wv.connect_ready_to_show(move |_| win.show_all());
    }
    {
        let win = win.clone();
        wv.connect_close(move |_| win.close());
    }
    {
        let win = win.clone();
        wv.connect_title_notify(move |wv| {
            if let Some(t) = wv.title().filter(|t| !t.is_empty()) {
                win.set_title(&t);
            }
        });
    }
    // ポップアップからさらにポップアップを開く場合
    {
        let policy = policy.clone();
        wv.connect_create(move |wv, action| {
            let action = action.clone();
            let url = action
                .request()
                .and_then(|r| r.uri())
                .map(|u| u.to_string())?;
            match handle_new_window(&url, wv, &policy) {
                NewWindowResponse::Create { webview } => Some(webview.upcast()),
                _ => None,
            }
        });
    }

    attach_navigation_policy(&wv, policy, Some(win));
    wv
}

/// ページ内の遷移を監視し、外部サイトへのリンククリックは既定のブラウザに回す。
///
/// iframe (埋め込み) の読み込みはユーザー操作ではないので、そのままアプリ内で表示される。
/// `popup` が Some の場合、最初の遷移先が外部サイトならブラウザで開いてポップアップを閉じる
/// (`window.open('')` の後で location を設定するパターンへの対応)。
pub fn attach_navigation_policy(
    wv: &webkit2gtk::WebView,
    policy: Rc<LinkPolicy>,
    popup: Option<gtk::Window>,
) {
    let loaded_real_page = Rc::new(Cell::new(false));
    wv.connect_decide_policy(move |_, decision, decision_type| {
        if decision_type != PolicyDecisionType::NavigationAction {
            return false;
        }
        let Some(nav) = decision.downcast_ref::<NavigationPolicyDecision>() else {
            return false;
        };
        let Some(action) = nav.navigation_action() else {
            return false;
        };
        let Some(uri) = action
            .request()
            .and_then(|r| r.uri())
            .map(|u| u.to_string())
        else {
            return false;
        };
        let kind = policy.classify(&uri);
        let first_real = kind != LinkKind::Internal && !loaded_real_page.replace(true);

        if kind != LinkKind::External {
            return false;
        }
        let is_http = uri.starts_with("http://") || uri.starts_with("https://");
        let user_initiated =
            action.navigation_type() == NavigationType::LinkClicked || action.is_user_gesture();
        let popup_first = popup.is_some() && first_real;

        if !is_http || user_initiated || popup_first {
            links::open_external(&uri);
            decision.ignore();
            if popup_first && let Some(win) = &popup {
                win.close();
            }
            return true;
        }
        false
    });
}
