//! キーボードショートカット。WebView より先にウィンドウでキーを捕まえて処理する。

use gtk::gdk::{self, ModifierType, keys::constants as key};

/// キー入力に対応するコマンド名を返す (該当しなければ None で、WebView にそのまま渡す)
pub fn command_for(ev: &gdk::EventKey) -> Option<&'static str> {
    let mods = ev.state()
        & (ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK | ModifierType::MOD1_MASK);
    let ctrl = ModifierType::CONTROL_MASK;
    let shift = ModifierType::SHIFT_MASK;
    let alt = ModifierType::MOD1_MASK;
    let k = ev.keyval().to_lower();

    let cmd = if mods == ctrl {
        match k {
            key::t => "new-tab",
            key::w => "close-tab",
            key::q => "quit",
            key::r => "reload",
            key::Tab | key::Page_Down => "next-tab",
            key::Page_Up => "prev-tab",
            key::equal | key::plus | key::KP_Add | key::semicolon => "zoom-in",
            key::minus | key::KP_Subtract => "zoom-out",
            key::_0 | key::KP_0 => "zoom-reset",
            _ => return None,
        }
    } else if mods == ctrl | shift {
        match k {
            key::t => "reopen-tab",
            // Shift+Tab は ISO_Left_Tab として届く
            key::Tab | key::ISO_Left_Tab => "prev-tab",
            // 日本語キーボードでは Ctrl+Shift+; が「+」
            key::plus | key::semicolon => "zoom-in",
            _ => return None,
        }
    } else if mods == alt {
        match k {
            key::Left => "back",
            key::Right => "forward",
            _ => return None,
        }
    } else if mods.is_empty() && k == key::F5 {
        "reload"
    } else {
        return None;
    };
    Some(cmd)
}
