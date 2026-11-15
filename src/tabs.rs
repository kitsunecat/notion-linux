//! アプリ内タブ。gtk::Notebook の各ページに WebView を 1 つずつ置く。
//! タブが 1 つだけのときはタブバーを隠す。

use std::rc::Rc;

use gtk::prelude::*;
use tao::event_loop::EventLoopProxy;
use webkit2gtk::{
    NotificationExt, NotificationPermissionRequest, PermissionRequestExt, SecurityOrigin,
    SettingsExt, WebContextExt as _, WebViewExt,
};
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};

use crate::config::Config;
use crate::links::{LinkKind, LinkPolicy};
use crate::{UserEvent, notify, popup};

pub type TabId = u32;

/// ページに注入するスクリプト:
/// マウスの戻る/進むボタン、Ctrl+クリック / 中クリックで新しいタブ
/// (キーボードショートカットは shortcuts.rs で GTK レベルで処理する)
const INIT_JS: &str = r#"
(() => {
  if (window.__notionLinux) return;
  window.__notionLinux = true;
  const send = (m) => { try { window.ipc.postMessage(m); } catch (_) {} };

  window.addEventListener('mouseup', (e) => {
    if (e.button === 3) { e.preventDefault(); send('back'); }
    else if (e.button === 4) { e.preventDefault(); send('forward'); }
  }, true);

  // Ctrl+クリック / 中クリックでリンクを新しいタブに
  let lastNewTabGesture = 0;
  const isNewTabGesture = (e) => (e.button === 0 && e.ctrlKey && !e.altKey) || e.button === 1;
  const markGesture = (e) => { if (isNewTabGesture(e)) lastNewTabGesture = Date.now(); };
  window.addEventListener('pointerdown', markGesture, true);
  window.addEventListener('mousedown', markGesture, true);
  const onClick = (e) => {
    if (!isNewTabGesture(e)) return;
    lastNewTabGesture = Date.now();
    const a = e.target && e.target.closest ? e.target.closest('a[href]') : null;
    if (!a) return; // リンク以外は Notion 側の処理に任せる (window.open を下で捕まえる)
    const href = a.href;
    if (!/^https?:/i.test(href)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    send('new-tab ' + href);
  };
  window.addEventListener('click', onClick, true);
  window.addEventListener('auxclick', onClick, true);

  // Ctrl+クリック直後に Notion が window.open(url) した場合も新しいタブにする
  const origOpen = window.open;
  window.open = function (url, target, features) {
    if (url && !features && Date.now() - lastNewTabGesture < 1500) {
      try {
        const abs = new URL(url, location.href).href;
        if (/^https?:/i.test(abs)) { lastNewTabGesture = 0; send('new-tab ' + abs); return null; }
      } catch (_) {}
    }
    return origOpen.apply(this, arguments);
  };
})();
"#;

const TAB_TITLE_MAX_CHARS: i32 = 24;

pub struct Tab {
    pub id: TabId,
    webview: wry::WebView,
    page: gtk::Box,
    label: gtk::Label,
}

impl Tab {
    pub fn wk(&self) -> webkit2gtk::WebView {
        self.webview.webview()
    }

    pub fn title(&self) -> String {
        self.wk()
            .title()
            .map(|t| t.to_string())
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| crate::APP_NAME.to_string())
    }
}

pub struct Tabs {
    notebook: gtk::Notebook,
    tabs: Vec<Tab>,
    next_id: TabId,
    closed: Vec<String>,
    web_context: WebContext,
    cfg: Rc<Config>,
    policy: Rc<LinkPolicy>,
    proxy: EventLoopProxy<UserEvent>,
    zoom: f64,
    context_ready: bool,
}

impl Tabs {
    pub fn new(
        container: &gtk::Box,
        cfg: Rc<Config>,
        policy: Rc<LinkPolicy>,
        proxy: EventLoopProxy<UserEvent>,
        zoom: f64,
    ) -> Self {
        let notebook = gtk::Notebook::new();
        notebook.set_scrollable(true);
        notebook.set_show_border(false);
        notebook.set_show_tabs(false);
        container.pack_start(&notebook, true, true, 0);
        notebook.show();
        {
            let proxy = proxy.clone();
            notebook.connect_switch_page(move |_, _, _| {
                let _ = proxy.send_event(UserEvent::TabSwitched);
            });
        }
        Self {
            notebook,
            tabs: vec![],
            next_id: 1,
            closed: vec![],
            web_context: WebContext::new(Some(crate::config::data_dir().join("webdata"))),
            cfg,
            policy,
            proxy,
            zoom: if (0.3..=3.0).contains(&zoom) {
                zoom
            } else {
                1.0
            },
            context_ready: false,
        }
    }

    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// 新しいタブで `url` を開く。`focus` が false なら背景で開く (ブラウザの Ctrl+クリックと同じ)。
    pub fn open(&mut self, url: &str, focus: bool) -> TabId {
        let id = self.next_id;
        self.next_id += 1;

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let webview = self.build_webview(id, &page);
        let wk = webview.webview();
        self.setup_webkit(&wk);

        // タブの見出し: タイトル + 閉じるボタン
        let label = gtk::Label::new(Some(crate::APP_NAME));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(TAB_TITLE_MAX_CHARS);
        label.set_width_chars(8);
        let close = gtk::Button::from_icon_name(Some("window-close-symbolic"), gtk::IconSize::Menu);
        close.set_relief(gtk::ReliefStyle::None);
        close.set_focus_on_click(false);
        close.set_tooltip_text(Some("タブを閉じる (Ctrl+W)"));
        {
            let proxy = self.proxy.clone();
            close.connect_clicked(move |_| {
                let _ = proxy.send_event(UserEvent::CloseTab(id));
            });
        }
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        header.pack_start(&label, true, true, 0);
        header.pack_start(&close, false, false, 0);
        // 中クリックでタブを閉じる
        let header_events = gtk::EventBox::new();
        header_events.add(&header);
        {
            let proxy = self.proxy.clone();
            header_events.connect_button_release_event(move |_, ev| {
                if ev.button() == 2 {
                    let _ = proxy.send_event(UserEvent::CloseTab(id));
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
        }
        header_events.show_all();

        let position = self.notebook.current_page().map(|p| p + 1);
        self.notebook
            .insert_page(&page, Some(&header_events), position);
        self.notebook.set_tab_reorderable(&page, true);
        page.show_all();

        self.tabs.push(Tab {
            id,
            webview,
            page: page.clone(),
            label,
        });
        self.update_tab_bar();
        if focus || self.tabs.len() == 1 {
            if let Some(n) = self.notebook.page_num(&page) {
                self.notebook.set_current_page(Some(n));
            }
            wk.grab_focus();
        }
        wk.load_uri(url);
        id
    }

    fn build_webview(&mut self, id: TabId, page: &gtk::Box) -> wry::WebView {
        let proxy = self.proxy.clone();
        let proxy_title = self.proxy.clone();
        let policy = self.policy.clone();
        let mut builder = WebViewBuilder::new_with_web_context(&mut self.web_context)
            .with_clipboard(true)
            .with_devtools(
                cfg!(debug_assertions) || std::env::var_os("NOTION_LINUX_DEVTOOLS").is_some(),
            )
            .with_initialization_script_for_main_only(INIT_JS, true)
            .with_ipc_handler(move |req| {
                let _ = proxy.send_event(UserEvent::Ipc {
                    tab: id,
                    msg: req.body().clone(),
                });
            })
            .with_document_title_changed_handler(move |title| {
                let _ = proxy_title.send_event(UserEvent::Title { tab: id, title });
            })
            .with_new_window_req_handler(move |url, features| {
                popup::handle_new_window(&url, &features.opener.webview, &policy)
            })
            .with_download_completed_handler(|_url, path, ok| {
                if let (true, Some(path)) = (ok, path) {
                    notify("ダウンロード完了", &path.display().to_string());
                }
            });
        if !self.cfg.user_agent.is_empty() {
            builder = builder.with_user_agent(&self.cfg.user_agent);
        }
        builder.build_gtk(page).expect("WebView を作成できません")
    }

    /// wry では設定できない WebKitGTK の細かな設定
    fn setup_webkit(&mut self, wk: &webkit2gtk::WebView) {
        if let Some(settings) = WebViewExt::settings(wk) {
            settings.set_enable_smooth_scrolling(true);
            settings.set_javascript_can_access_clipboard(true);
            settings.set_enable_back_forward_navigation_gestures(true);
        }
        wk.set_zoom_level(self.zoom);

        // WebContext (全タブ共通) の設定は 1 回だけ
        if !self.context_ready
            && let Some(ctx) = wk.context()
        {
            self.context_ready = true;
            ctx.set_spell_checking_enabled(self.cfg.spell_checking);
            let langs: Vec<&str> = self
                .cfg
                .spell_checking_languages
                .iter()
                .map(String::as_str)
                .collect();
            ctx.set_spell_checking_languages(&langs);

            // Notion からのデスクトップ通知を既定で許可する
            let init_perms = |ctx: &webkit2gtk::WebContext| {
                let origins = [
                    SecurityOrigin::new("https", "www.notion.so", 0),
                    SecurityOrigin::new("https", "notion.so", 0),
                    SecurityOrigin::new("https", "www.notion.com", 0),
                ];
                let refs: Vec<&SecurityOrigin> = origins.iter().collect();
                ctx.initialize_notification_permissions(&refs, &[]);
            };
            init_perms(&ctx);
            ctx.connect_initialize_notification_permissions(init_perms);
        }

        // 権限リクエスト: 通知は許可、それ以外は既定 (拒否)
        wk.connect_permission_request(|_, req| {
            if req.is::<NotificationPermissionRequest>() {
                req.allow();
                true
            } else {
                false
            }
        });

        // 通知をクリックしたらウィンドウを前面に
        {
            let proxy = self.proxy.clone();
            wk.connect_show_notification(move |_, notification| {
                let proxy = proxy.clone();
                notification.connect_clicked(move |_| {
                    let _ = proxy.send_event(UserEvent::Show);
                });
                false // 表示自体は WebKitGTK の既定処理 (デスクトップ通知) に任せる
            });
        }

        popup::attach_navigation_policy(wk, self.policy.clone(), None);
    }

    fn update_tab_bar(&self) {
        self.notebook.set_show_tabs(self.tabs.len() > 1);
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.index_of(id).map(|i| &self.tabs[i])
    }

    pub fn active(&self) -> Option<&Tab> {
        let n = self.notebook.current_page()?;
        let widget = self.notebook.nth_page(Some(n))?;
        self.tabs
            .iter()
            .find(|t| t.page.upcast_ref::<gtk::Widget>() == &widget)
    }

    pub fn is_active(&self, id: TabId) -> bool {
        self.active().is_some_and(|t| t.id == id)
    }

    /// タブを閉じる。最後の 1 つは閉じない (false を返す)。
    pub fn close(&mut self, id: TabId) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }
        let Some(i) = self.index_of(id) else {
            return false;
        };
        let tab = self.tabs.remove(i);
        if let Some(uri) = tab.wk().uri() {
            self.closed.push(uri.to_string());
            if self.closed.len() > 20 {
                self.closed.remove(0);
            }
        }
        if let Some(n) = self.notebook.page_num(&tab.page) {
            self.notebook.remove_page(Some(n));
        }
        drop(tab);
        self.update_tab_bar();
        if let Some(t) = self.active() {
            t.wk().grab_focus();
        }
        true
    }

    /// Ctrl+Shift+T: 最後に閉じたタブを開き直す
    pub fn reopen_closed(&mut self) {
        if let Some(url) = self.closed.pop() {
            self.open(&url, true);
        }
    }

    /// 隣のタブへ移動 (delta = 1 で右、-1 で左。端では反対側へ回る)
    pub fn cycle(&self, delta: i32) {
        let n = self.notebook.n_pages() as i32;
        if n < 2 {
            return;
        }
        let cur = self.notebook.current_page().unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(n) as u32;
        self.notebook.set_current_page(Some(next));
        if let Some(t) = self.active() {
            t.wk().grab_focus();
        }
    }

    /// タブの見出しを更新し、そのタブがアクティブなら true を返す
    pub fn set_title(&self, id: TabId, title: &str) -> bool {
        let Some(tab) = self.get(id) else {
            return false;
        };
        let title = title.trim();
        let title = if title.is_empty() {
            crate::APP_NAME
        } else {
            title
        };
        tab.label.set_text(title);
        tab.label.set_tooltip_text(Some(title));
        self.is_active(id)
    }

    pub fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom.clamp(0.3, 3.0);
        for t in &self.tabs {
            t.wk().set_zoom_level(self.zoom);
        }
    }

    /// 保存用: タブの並び順どおりの Notion の URL と、アクティブなタブの位置
    pub fn session(&self) -> (Vec<String>, usize) {
        let active = self.active().map(|t| t.id);
        let mut urls = vec![];
        let mut active_index = 0;
        for n in 0..self.notebook.n_pages() {
            let Some(widget) = self.notebook.nth_page(Some(n)) else {
                continue;
            };
            let Some(tab) = self
                .tabs
                .iter()
                .find(|t| t.page.upcast_ref::<gtk::Widget>() == &widget)
            else {
                continue;
            };
            let Some(uri) = tab.wk().uri() else { continue };
            if self.policy.classify(&uri) != LinkKind::Notion {
                continue;
            }
            if Some(tab.id) == active {
                active_index = urls.len();
            }
            urls.push(uri.to_string());
        }
        (urls, active_index)
    }

    /// 保存しておいたタブを開き直す
    pub fn restore(&mut self, urls: &[String], active: usize) {
        for url in urls {
            self.open(url, false);
        }
        let n = active.min(self.tabs.len().saturating_sub(1)) as u32;
        self.notebook.set_current_page(Some(n));
        if let Some(t) = self.active() {
            t.wk().grab_focus();
        }
    }
}
