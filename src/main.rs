//! Notion for Linux: notion.so を WebKitGTK で表示する軽量デスクトップアプリ。

mod config;
mod icon;
mod links;
mod notion_icon;
mod popup;
mod shortcuts;
mod single_instance;
mod tabs;

use std::cell::RefCell;
use std::rc::Rc;

use config::{Config, WindowState};
use links::{LinkKind, LinkPolicy};
use single_instance::{Instance, Request};

use gtk::prelude::WidgetExt;
use tabs::Tabs;
use tao::dpi::{PhysicalPosition, PhysicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::unix::WindowExtUnix;
use tao::window::{Icon as WindowIcon, WindowBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon as TrayIconImage, TrayIcon, TrayIconBuilder};
use webkit2gtk::WebViewExt;

pub const APP_NAME: &str = "Notion";
const ZOOM_STEP: f64 = 0.1;

#[derive(Debug)]
pub enum UserEvent {
    Menu(MenuEvent),
    Show,
    Open(String),
    Title {
        tab: tabs::TabId,
        title: String,
    },
    Ipc {
        tab: tabs::TabId,
        msg: String,
    },
    /// GTK レベルで捕まえたキーボードショートカット (アクティブなタブに対して実行)
    Shortcut(&'static str),
    CloseTab(tabs::TabId),
    TabSwitched,
}

struct TrayMenuIds {
    show: tray_icon::menu::MenuId,
    reload: tray_icon::menu::MenuId,
    quit: tray_icon::menu::MenuId,
}

fn print_help() {
    println!(
        "notion-linux {ver}\n\n\
         使い方: notion-linux [URL]\n\n\
         URL を渡すと、そのページを開きます (起動済みなら既存ウィンドウで開きます)。\n\n\
         notion-linux --fetch-icon [--system]\n\
         \x20 notion.so から Notion のアイコンを取得して保存します\n\
         \x20 (既定は ~/.local/share/icons、--system は /usr/share/icons。通常はインストール時に自動実行)\n\n\
         設定ファイル: {cfg}/config.json\n\
         ショートカット: Ctrl+R/F5 再読み込み, Ctrl+= / Ctrl+- / Ctrl+0 ズーム,\n\
                         Alt+←/→ 戻る/進む, Ctrl+Q 終了\n\
         タブ:           Ctrl+クリック / 中クリック リンクを新しいタブで開く,\n\
                         Ctrl+T 新しいタブ, Ctrl+W タブを閉じる, Ctrl+Shift+T 閉じたタブを開き直す,\n\
                         Ctrl+Tab / Ctrl+Shift+Tab タブの切り替え",
        ver = env!("CARGO_PKG_VERSION"),
        cfg = config::config_dir().display()
    );
}

fn main() {
    let mut arg_url: Option<String> = None;
    let (mut fetch_icon, mut system) = (false, false);
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => return print_help(),
            "--fetch-icon" => fetch_icon = true,
            "--system" => system = true,
            "-V" | "--version" => {
                println!("notion-linux {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            a if a.starts_with("http://") || a.starts_with("https://") => arg_url = Some(a.into()),
            a if a.starts_with("notion://") => {
                // notion://www.notion.so/xxx 形式 → https に変換
                arg_url = Some(format!("https://{}", a.trim_start_matches("notion://")))
            }
            other => eprintln!("[notion-linux] 不明な引数を無視します: {other}"),
        }
    }

    if fetch_icon {
        // インストール時に呼ばれる: Notion 公式アイコンを取得して保存し、終了する
        let target = if system {
            notion_icon::Target::System
        } else {
            notion_icon::Target::User
        };
        match notion_icon::fetch_and_install(target) {
            Ok(path) => println!("Notion のアイコンを保存しました: {}", path.display()),
            Err(e) => {
                eprintln!("Notion のアイコンを取得できませんでした: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    let cfg = Config::load();
    if cfg.disable_dmabuf_renderer && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: スレッドを起動する前 (シングルスレッドの段階) に設定している
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }

    let listener = match single_instance::acquire(arg_url.as_deref()) {
        Instance::Primary(l) => l,
        Instance::Secondary => return,
    };

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    {
        let proxy = proxy.clone();
        single_instance::listen(listener, move |req| {
            let _ = proxy.send_event(match req {
                Request::Show => UserEvent::Show,
                Request::Open(url) => UserEvent::Open(url),
            });
        });
    }

    let policy = Rc::new(LinkPolicy::new(&cfg.extra_internal_hosts));
    let state = Rc::new(RefCell::new(WindowState::load()));

    // ---- ウィンドウ ----
    // Notion 公式アイコン (キャッシュ済みなら) か、同梱のオリジナルアイコン
    let (icon_rgba, icon_w, icon_h) = cfg
        .use_notion_icon
        .then(notion_icon::load_installed)
        .flatten()
        .map(|pb| notion_icon::to_rgba(&pb))
        .unwrap_or_else(|| (icon::rgba(128), 128, 128));
    let window_icon = WindowIcon::from_rgba(icon_rgba.clone(), icon_w, icon_h).ok();
    let mut wb = WindowBuilder::new()
        .with_title(APP_NAME)
        .with_window_icon(window_icon)
        .with_min_inner_size(PhysicalSize::new(480, 360))
        .with_maximized(state.borrow().maximized);
    {
        let st = state.borrow();
        wb = wb.with_inner_size(PhysicalSize::new(st.width.max(480), st.height.max(360)));
        if let (Some(x), Some(y)) = (st.x, st.y) {
            wb = wb.with_position(PhysicalPosition::new(x, y));
        }
    }
    let window = wb.build(&event_loop).expect("ウィンドウを作成できません");

    // ---- タブと WebView ----
    let cfg = Rc::new(cfg);
    let vbox = window.default_vbox().expect("GTK vbox");
    let mut tabs = Tabs::new(
        vbox,
        cfg.clone(),
        policy.clone(),
        proxy.clone(),
        state.borrow().zoom,
    );
    {
        let st = state.borrow();
        let saved: Vec<String> = st
            .tabs
            .iter()
            .filter(|u| policy.classify(u) == LinkKind::Notion)
            .cloned()
            .collect();
        if let Some(url) = &arg_url {
            tabs.open(url, true);
        } else if cfg.restore_last_page && !saved.is_empty() {
            tabs.restore(&saved, st.active_tab);
        } else {
            match &st.last_url {
                Some(u) if cfg.restore_last_page && policy.classify(u) == LinkKind::Notion => {
                    tabs.open(u, true)
                }
                _ => tabs.open(&cfg.start_url, true),
            };
        }
    }

    // キーボードショートカットはウィンドウで先に捕まえる (エラーページ表示中でも効くように)
    {
        let proxy = proxy.clone();
        window.gtk_window().connect_key_press_event(move |_, ev| {
            match shortcuts::command_for(ev) {
                Some(cmd) => {
                    let _ = proxy.send_event(UserEvent::Shortcut(cmd));
                    gtk::glib::Propagation::Stop
                }
                None => gtk::glib::Propagation::Proceed,
            }
        });
    }

    // ---- トレイ ----
    let (tray, tray_ids) = if cfg.tray_icon {
        match build_tray(&proxy, icon_rgba, icon_w, icon_h) {
            Ok((t, ids)) => (Some(t), Some(ids)),
            Err(e) => {
                eprintln!("[notion-linux] トレイアイコンを作成できません: {e}");
                (None, None)
            }
        }
    } else {
        (None, None)
    };
    let close_to_tray = cfg.close_to_tray && tray.is_some();

    // ---- イベントループ ----
    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _keep_tray_alive = &tray;

        let save_state = |tabs: &Tabs| {
            let mut st = state.borrow_mut();
            st.maximized = window.is_maximized();
            st.zoom = tabs.zoom();
            let (urls, active) = tabs.session();
            st.last_url = urls.get(active).cloned();
            st.tabs = urls;
            st.active_tab = active;
            st.save();
        };
        let update_title = |tabs: &Tabs| {
            let title = tabs
                .active()
                .map(|t| t.title())
                .unwrap_or_else(|| APP_NAME.to_string());
            window.set_title(&title);
        };
        let show_window = || {
            window.set_visible(true);
            window.set_minimized(false);
            window.set_focus();
        };
        let quit = |tabs: &Tabs, cf: &mut ControlFlow| {
            save_state(tabs);
            single_instance::cleanup();
            *cf = ControlFlow::Exit;
        };

        match event {
            Event::NewEvents(StartCause::Init) => {}
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    if close_to_tray {
                        save_state(&tabs);
                        window.set_visible(false);
                    } else {
                        quit(&tabs, control_flow);
                    }
                }
                WindowEvent::Resized(size) => {
                    if !window.is_maximized() && !window.is_minimized() && size.width > 0 {
                        let mut st = state.borrow_mut();
                        st.width = size.width;
                        st.height = size.height;
                    }
                }
                WindowEvent::Moved(pos) if !window.is_maximized() && !window.is_minimized() => {
                    let mut st = state.borrow_mut();
                    st.x = Some(pos.x);
                    st.y = Some(pos.y);
                }
                _ => {}
            },
            Event::UserEvent(ev) => match ev {
                UserEvent::Show => show_window(),
                UserEvent::Open(url) => {
                    if policy.is_in_app(&url) {
                        tabs.open(&url, true);
                        show_window();
                    } else {
                        links::open_external(&url);
                    }
                }
                UserEvent::Title { tab, title } => {
                    if tabs.set_title(tab, &title) {
                        update_title(&tabs);
                    }
                }
                UserEvent::TabSwitched => update_title(&tabs),
                UserEvent::CloseTab(tab) => {
                    tabs.close(tab);
                    update_title(&tabs);
                }
                UserEvent::Shortcut(cmd) => {
                    if let Some(tab) = tabs.active().map(|t| t.id) {
                        let _ = proxy.send_event(UserEvent::Ipc {
                            tab,
                            msg: cmd.to_string(),
                        });
                    }
                }
                UserEvent::Ipc { tab, msg } => {
                    let wk = tabs.get(tab).map(|t| t.wk());
                    let (cmd, arg) = msg.split_once(' ').unwrap_or((msg.as_str(), ""));
                    match (cmd, wk) {
                        ("reload", Some(wk)) => wk.reload(),
                        ("back", Some(wk)) => wk.go_back(),
                        ("forward", Some(wk)) => wk.go_forward(),
                        ("zoom-in", _) => tabs.set_zoom(tabs.zoom() + ZOOM_STEP),
                        ("zoom-out", _) => tabs.set_zoom(tabs.zoom() - ZOOM_STEP),
                        ("zoom-reset", _) => tabs.set_zoom(1.0),
                        ("new-tab", _) if arg.is_empty() => {
                            tabs.open(&cfg.start_url, true);
                        }
                        ("new-tab", _) => {
                            // Ctrl+クリック: Notion 内のリンクは背景の新しいタブ、外部はブラウザ
                            if policy.is_in_app(arg) {
                                tabs.open(arg, false);
                            } else {
                                links::open_external(arg);
                            }
                        }
                        ("close-tab", _) => {
                            tabs.close(tab);
                            update_title(&tabs);
                        }
                        ("reopen-tab", _) => tabs.reopen_closed(),
                        ("next-tab", _) => tabs.cycle(1),
                        ("prev-tab", _) => tabs.cycle(-1),
                        ("quit", _) => quit(&tabs, control_flow),
                        _ => {}
                    }
                }
                UserEvent::Menu(e) => {
                    if let Some(ids) = &tray_ids {
                        if e.id == ids.show {
                            if window.is_visible() && window.is_focused() {
                                save_state(&tabs);
                                window.set_visible(false);
                            } else {
                                show_window();
                            }
                        } else if e.id == ids.reload {
                            if let Some(t) = tabs.active() {
                                t.wk().reload();
                            }
                        } else if e.id == ids.quit {
                            quit(&tabs, control_flow);
                        }
                    }
                }
            },
            _ => {}
        }
    });
}

fn build_tray(
    proxy: &EventLoopProxy<UserEvent>,
    icon_rgba: Vec<u8>,
    icon_w: u32,
    icon_h: u32,
) -> Result<(TrayIcon, TrayMenuIds), Box<dyn std::error::Error>> {
    let show = MenuItem::new("Notion を表示 / 隠す", true, None);
    let reload = MenuItem::new("再読み込み", true, None);
    let quit = MenuItem::new("終了", true, None);
    let menu = Menu::new();
    menu.append_items(&[&show, &reload, &PredefinedMenuItem::separator(), &quit])?;

    let ids = TrayMenuIds {
        show: show.id().clone(),
        reload: reload.id().clone(),
        quit: quit.id().clone(),
    };

    let proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Menu(e));
    }));

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(APP_NAME)
        .with_title(APP_NAME)
        .with_icon(TrayIconImage::from_rgba(icon_rgba, icon_w, icon_h)?)
        .build()?;
    Ok((tray, ids))
}

/// notify-send があればデスクトップ通知を出す (無ければ何もしない)
pub fn notify(summary: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args([
            "--app-name",
            APP_NAME,
            "--icon",
            config::APP_ID,
            summary,
            body,
        ])
        .spawn();
}
