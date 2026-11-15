//! Notion for Linux: notion.so を WebKitGTK で表示する軽量デスクトップアプリ。

mod config;
mod icon;
mod links;
mod notion_icon;
mod popup;
mod single_instance;

use std::cell::RefCell;
use std::rc::Rc;

use config::{Config, WindowState};
use links::{LinkKind, LinkPolicy};
use single_instance::{Instance, Request};

use tao::dpi::{PhysicalPosition, PhysicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::unix::WindowExtUnix;
use tao::window::{Icon as WindowIcon, WindowBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon as TrayIconImage, TrayIcon, TrayIconBuilder};
use webkit2gtk::{
    NotificationExt, NotificationPermissionRequest, PermissionRequestExt, SecurityOrigin,
    SettingsExt, WebContextExt as _, WebViewExt,
};
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};

use gtk::glib::prelude::*;

const APP_NAME: &str = "Notion";
const ZOOM_STEP: f64 = 0.1;

/// ページに注入するスクリプト: キーボードショートカットとマウスの戻る/進むボタン
const INIT_JS: &str = r#"
(() => {
  if (window.__notionLinux) return;
  window.__notionLinux = true;
  const send = (m) => { try { window.ipc.postMessage(m); } catch (_) {} };
  window.addEventListener('keydown', (e) => {
    let cmd = null;
    const ctrl = e.ctrlKey && !e.metaKey && !e.altKey;
    if (ctrl) {
      if (e.key === '=' || e.key === '+' || e.key === ';') cmd = 'zoom-in';
      else if (e.key === '-' && !e.shiftKey) cmd = 'zoom-out';
      else if (e.key === '0' && !e.shiftKey) cmd = 'zoom-reset';
      else if ((e.key === 'q' || e.key === 'Q') && !e.shiftKey) cmd = 'quit';
      else if ((e.key === 'r' || e.key === 'R') && !e.shiftKey) cmd = 'reload';
    }
    if (e.key === 'F5' && !e.ctrlKey && !e.altKey) cmd = 'reload';
    if (e.altKey && !e.ctrlKey && !e.shiftKey) {
      if (e.key === 'ArrowLeft') cmd = 'back';
      else if (e.key === 'ArrowRight') cmd = 'forward';
    }
    if (cmd) { e.preventDefault(); e.stopPropagation(); send(cmd); }
  }, true);
  window.addEventListener('mouseup', (e) => {
    if (e.button === 3) { e.preventDefault(); send('back'); }
    else if (e.button === 4) { e.preventDefault(); send('forward'); }
  }, true);
})();
"#;

#[derive(Debug)]
enum UserEvent {
    Menu(MenuEvent),
    Show,
    Open(String),
    Title(String),
    Ipc(String),
    /// Notion 公式アイコンのダウンロードが完了した (一時ファイルのパス)
    IconDownloaded(std::path::PathBuf),
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
         設定ファイル: {cfg}/config.json\n\
         ショートカット: Ctrl+R/F5 再読み込み, Ctrl+= / Ctrl+- / Ctrl+0 ズーム,\n\
                         Alt+←/→ 戻る/進む, Ctrl+Q 終了",
        ver = env!("CARGO_PKG_VERSION"),
        cfg = config::config_dir().display()
    );
}

fn main() {
    let mut arg_url: Option<String> = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => return print_help(),
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
        .then(notion_icon::load_cached)
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

    // ---- WebView ----
    let mut web_context = WebContext::new(Some(config::data_dir().join("webdata")));

    let start_url = arg_url.clone().unwrap_or_else(|| {
        let last = state.borrow().last_url.clone();
        match last {
            Some(u) if cfg.restore_last_page && policy.classify(&u) == LinkKind::Notion => u,
            _ => cfg.start_url.clone(),
        }
    });

    let mut builder = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_clipboard(true)
        .with_devtools(
            cfg!(debug_assertions) || std::env::var_os("NOTION_LINUX_DEVTOOLS").is_some(),
        )
        .with_initialization_script_for_main_only(INIT_JS, true)
        .with_ipc_handler({
            let proxy = proxy.clone();
            move |req| {
                let _ = proxy.send_event(UserEvent::Ipc(req.body().clone()));
            }
        })
        .with_document_title_changed_handler({
            let proxy = proxy.clone();
            move |title| {
                let _ = proxy.send_event(UserEvent::Title(title));
            }
        })
        .with_new_window_req_handler({
            let policy = policy.clone();
            move |url, features| popup::handle_new_window(&url, &features.opener.webview, &policy)
        })
        .with_download_completed_handler(|_url, path, ok| {
            if let (true, Some(path)) = (ok, path) {
                notify("ダウンロード完了", &path.display().to_string());
            }
        });
    if !cfg.user_agent.is_empty() {
        builder = builder.with_user_agent(&cfg.user_agent);
    }

    let vbox = window.default_vbox().expect("GTK vbox");
    let webview = builder.build_gtk(vbox).expect("WebView を作成できません");

    let wk = webview.webview();
    setup_webkit(&wk, &cfg, &policy, &proxy, state.borrow().zoom);
    wk.load_uri(&start_url);

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

    if cfg.use_notion_icon && notion_icon::needs_refresh() {
        let proxy = proxy.clone();
        notion_icon::fetch_in_background(cfg.user_agent.clone(), move |path| {
            let _ = proxy.send_event(UserEvent::IconDownloaded(path));
        });
    }

    // ---- イベントループ ----
    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _keep_tray_alive = &tray;

        let save_state = || {
            let mut st = state.borrow_mut();
            st.maximized = window.is_maximized();
            st.zoom = wk.zoom_level();
            if let Some(uri) = wk.uri()
                && policy.classify(&uri) == LinkKind::Notion
            {
                st.last_url = Some(uri.to_string());
            }
            st.save();
        };
        let show_window = || {
            window.set_visible(true);
            window.set_minimized(false);
            window.set_focus();
        };
        let quit = |cf: &mut ControlFlow| {
            save_state();
            single_instance::cleanup();
            *cf = ControlFlow::Exit;
        };

        match event {
            Event::NewEvents(StartCause::Init) => {}
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    if close_to_tray {
                        save_state();
                        window.set_visible(false);
                    } else {
                        quit(control_flow);
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
                        wk.load_uri(&url);
                        show_window();
                    } else {
                        links::open_external(&url);
                    }
                }
                UserEvent::Title(title) => {
                    let title = title.trim();
                    window.set_title(if title.is_empty() { APP_NAME } else { title });
                }
                UserEvent::Ipc(msg) => match msg.as_str() {
                    "reload" => wk.reload(),
                    "back" => wk.go_back(),
                    "forward" => wk.go_forward(),
                    "zoom-in" => wk.set_zoom_level((wk.zoom_level() + ZOOM_STEP).min(3.0)),
                    "zoom-out" => wk.set_zoom_level((wk.zoom_level() - ZOOM_STEP).max(0.3)),
                    "zoom-reset" => wk.set_zoom_level(1.0),
                    "quit" => quit(control_flow),
                    _ => {}
                },
                UserEvent::IconDownloaded(path) => match notion_icon::install(&path) {
                    Ok(pixbuf) => {
                        let (rgba, w, h) = notion_icon::to_rgba(&pixbuf);
                        window.set_window_icon(WindowIcon::from_rgba(rgba.clone(), w, h).ok());
                        if let (Some(tray), Ok(img)) = (&tray, TrayIconImage::from_rgba(rgba, w, h))
                        {
                            let _ = tray.set_icon(Some(img));
                        }
                    }
                    Err(e) => eprintln!("[notion-linux] アイコンを保存できませんでした: {e}"),
                },
                UserEvent::Menu(e) => {
                    if let Some(ids) = &tray_ids {
                        if e.id == ids.show {
                            if window.is_visible() && window.is_focused() {
                                save_state();
                                window.set_visible(false);
                            } else {
                                show_window();
                            }
                        } else if e.id == ids.reload {
                            wk.reload();
                        } else if e.id == ids.quit {
                            quit(control_flow);
                        }
                    }
                }
            },
            _ => {}
        }
    });
}

/// wry では設定できない WebKitGTK の細かな設定
fn setup_webkit(
    wk: &webkit2gtk::WebView,
    cfg: &Config,
    policy: &Rc<LinkPolicy>,
    proxy: &EventLoopProxy<UserEvent>,
    zoom: f64,
) {
    if let Some(settings) = WebViewExt::settings(wk) {
        settings.set_enable_smooth_scrolling(true);
        settings.set_javascript_can_access_clipboard(true);
        settings.set_enable_back_forward_navigation_gestures(true);
    }
    wk.set_zoom_level(if (0.3..=3.0).contains(&zoom) {
        zoom
    } else {
        1.0
    });

    if let Some(ctx) = wk.context() {
        ctx.set_spell_checking_enabled(cfg.spell_checking);
        let langs: Vec<&str> = cfg
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
        let proxy = proxy.clone();
        wk.connect_show_notification(move |_, notification| {
            let proxy = proxy.clone();
            notification.connect_clicked(move |_| {
                let _ = proxy.send_event(UserEvent::Show);
            });
            false // 表示自体は WebKitGTK の既定処理 (デスクトップ通知) に任せる
        });
    }

    popup::attach_navigation_policy(wk, policy.clone(), None);
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
fn notify(summary: &str, body: &str) {
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
