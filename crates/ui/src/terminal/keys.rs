//! Keystroke → PTY bytes, through Ghostty's own key encoder: legacy
//! sequences, xterm modifyOtherKeys and the kitty keyboard protocol, each as
//! the running program asked for them. On macOS, Ghostty's default
//! line-editing keybinds come first (Cmd+←/→/⌫, Option+←/→).

use ghostty_vt::Terminal;
use ghostty_vt::key::{Action, Encoder, Event, Key, Mods};
use gpui::Modifiers;

/// Encode a keystroke for the program on `term`. `None` means "not ours" —
/// the event should fall through (e.g. the platform-primary shortcuts that
/// drive app actions, or a lone modifier).
pub fn key_bytes(
    term: &Terminal,
    key: &str,
    key_char: Option<&str>,
    mods: &Modifiers,
) -> Option<Vec<u8>> {
    if let Some(bytes) = macos_keybind(key, mods) {
        return Some(bytes);
    }
    // Platform-primary combos (Cmd on macOS, the super key elsewhere) belong to
    // the app keymap, never the PTY.
    if mods.platform {
        return None;
    }
    let mut chars = key.chars();
    let single = chars.next().filter(|_| chars.next().is_none());
    let physical = physical_key(key);
    // The text the layout produced, never a control character (Ghostty derives
    // those from the key and modifiers).
    let text = key_char
        .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
        .or(single.map(|_| key))
        .or((key == "space").then_some(" "));
    if physical == Key::Unidentified && text.is_none() {
        return None;
    }
    let mut ghostty_mods = Mods::empty();
    for (on, bit) in [
        (mods.shift, Mods::SHIFT),
        (mods.alt, Mods::ALT),
        (mods.control, Mods::CTRL),
    ] {
        if on {
            ghostty_mods |= bit;
        }
    }
    // Modifiers the layout used up making the text: Shift for `A`, Option for
    // a composed `∫` (Ghostty's `macos-option-as-alt = false` then sends it).
    let mut consumed = Mods::empty();
    if let (Some(text), Some(base)) = (key_char, single) {
        let composed = !text.chars().eq(std::iter::once(base));
        if composed && mods.shift {
            consumed |= Mods::SHIFT;
        }
        if composed && mods.alt && !text.eq_ignore_ascii_case(key) {
            consumed |= Mods::ALT;
        }
    }

    let mut event = Event::new().ok()?;
    event
        .set_action(Action::Press)
        .set_key(physical)
        .set_mods(ghostty_mods)
        .set_consumed_mods(consumed)
        .set_utf8(text);
    if let Some(base) = single {
        event.set_unshifted_codepoint(base);
    }
    let mut encoder = Encoder::new().ok()?;
    encoder.set_options_from_terminal(term);
    let mut out = Vec::with_capacity(8);
    encoder.encode_to_vec(&event, &mut out).ok()?;
    (!out.is_empty()).then_some(out)
}

/// Ghostty's default macOS keybinds for a shell's line editor: Cmd+← / Cmd+→
/// to the line's start or end (Ctrl+A, Ctrl+E), Cmd+⌫ deletes to its start
/// (Ctrl+U), Option+← / Option+→ a word back or forward (ESC b, ESC f).
fn macos_keybind(key: &str, mods: &Modifiers) -> Option<Vec<u8>> {
    if !cfg!(target_os = "macos") || mods.shift || mods.control {
        return None;
    }
    let bytes: &[u8] = match (mods.platform, mods.alt, key) {
        (true, false, "left") => b"\x01",
        (true, false, "right") => b"\x05",
        (true, false, "backspace") => b"\x15",
        (false, true, "left") => b"\x1bb",
        (false, true, "right") => b"\x1bf",
        _ => return None,
    };
    Some(bytes.to_vec())
}

/// gpui's key name → the US-layout physical key Ghostty encodes by.
// ponytail: gpui gives no keycode, so a key another layout types (`ж`) is
// Unidentified and goes as its text; kitty's base-layout key for it is lost.
fn physical_key(key: &str) -> Key {
    use Key::*;
    const LETTERS: [Key; 26] = [
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    ];
    const DIGITS: [Key; 10] = [
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    ];
    const FUNCTION: [Key; 24] = [
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18,
        F19, F20, F21, F22, F23, F24,
    ];
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return match c.to_ascii_lowercase() {
            c @ 'a'..='z' => LETTERS[(c as u8 - b'a') as usize],
            c @ '0'..='9' => DIGITS[(c as u8 - b'0') as usize],
            '`' => Backquote,
            '\\' => Backslash,
            '[' => BracketLeft,
            ']' => BracketRight,
            ',' => Comma,
            '=' => Equal,
            '-' => Minus,
            '.' => Period,
            '\'' => Quote,
            ';' => Semicolon,
            '/' => Slash,
            ' ' => Space,
            _ => Unidentified,
        };
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<usize>().ok())
        && (1..=FUNCTION.len()).contains(&n)
    {
        return FUNCTION[n - 1];
    }
    match key {
        "enter" => Enter,
        "tab" => Tab,
        "backspace" => Backspace,
        "escape" => Escape,
        "space" => Space,
        "up" => ArrowUp,
        "down" => ArrowDown,
        "left" => ArrowLeft,
        "right" => ArrowRight,
        "home" => Home,
        "end" => End,
        "pageup" => PageUp,
        "pagedown" => PageDown,
        "insert" => Insert,
        "delete" => Delete,
        _ => Unidentified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::emulator::Emulator;

    fn mods() -> Modifiers {
        Modifiers::default()
    }

    /// Encode against a fresh terminal, after feeding it `setup`.
    fn enc(setup: &[u8], key: &str, key_char: Option<&str>, mods: Modifiers) -> Option<Vec<u8>> {
        let mut e = Emulator::new(20, 4);
        e.feed(setup);
        key_bytes(e.terminal(), key, key_char, &mods)
    }

    fn plain(key: &str, key_char: Option<&str>, mods: Modifiers) -> Option<Vec<u8>> {
        enc(b"", key, key_char, mods)
    }

    #[test]
    fn printables_prefer_key_char() {
        assert_eq!(plain("a", Some("a"), mods()), Some(b"a".to_vec()));
        let shift = Modifiers { shift: true, ..mods() };
        assert_eq!(plain("a", Some("A"), shift), Some(b"A".to_vec()));
        // Multi-byte characters pass through as UTF-8.
        assert_eq!(plain("e", Some("é"), mods()), Some("é".as_bytes().to_vec()));
        assert_eq!(plain("ж", Some("ж"), mods()), Some("ж".as_bytes().to_vec()));
        // Named single-char keys fall back to the key name.
        assert_eq!(plain("/", None, mods()), Some(b"/".to_vec()));
        // Unknown multi-char keys are not ours.
        assert_eq!(plain("capslock", None, mods()), None);
    }

    #[test]
    fn control_keys_and_sequences() {
        assert_eq!(plain("enter", None, mods()), Some(b"\r".to_vec()));
        assert_eq!(plain("backspace", None, mods()), Some(vec![0x7f]));
        assert_eq!(plain("tab", None, mods()), Some(b"\t".to_vec()));
        let shift = Modifiers { shift: true, ..mods() };
        assert_eq!(plain("tab", None, shift), Some(b"\x1b[Z".to_vec()));
        assert_eq!(plain("escape", None, mods()), Some(vec![0x1b]));
        assert_eq!(plain("space", None, mods()), Some(b" ".to_vec()));
        assert_eq!(plain("delete", None, mods()), Some(b"\x1b[3~".to_vec()));
        assert_eq!(plain("pageup", None, mods()), Some(b"\x1b[5~".to_vec()));
        assert_eq!(plain("f1", None, mods()), Some(b"\x1bOP".to_vec()));
        assert_eq!(plain("f5", None, mods()), Some(b"\x1b[15~".to_vec()));
    }

    #[test]
    fn arrows_respect_app_cursor_mode() {
        assert_eq!(plain("up", None, mods()), Some(b"\x1b[A".to_vec()));
        assert_eq!(enc(b"\x1b[?1h", "up", None, mods()), Some(b"\x1bOA".to_vec()));
        assert_eq!(plain("home", None, mods()), Some(b"\x1b[H".to_vec()));
        assert_eq!(enc(b"\x1b[?1h", "end", None, mods()), Some(b"\x1bOF".to_vec()));
        // Modified arrows carry their modifiers, as xterm and Ghostty send them.
        let shift = Modifiers { shift: true, ..mods() };
        assert_eq!(plain("up", None, shift), Some(b"\x1b[1;2A".to_vec()));
    }

    #[test]
    fn ctrl_combos_map_to_control_bytes() {
        let ctrl = Modifiers { control: true, ..mods() };
        assert_eq!(plain("c", Some("c"), ctrl), Some(vec![0x03]));
        assert_eq!(plain("z", None, ctrl), Some(vec![0x1a]));
        assert_eq!(plain("space", None, ctrl), Some(vec![0x00]));
        assert_eq!(plain("\\", None, ctrl), Some(vec![0x1c]));
        assert_eq!(plain("]", None, ctrl), Some(vec![0x1d]));
        // Ghostty follows fixterms here: Ctrl+[ (and Ctrl+I, Ctrl+M) go as
        // CSI u, so a program can tell them from Escape, Tab and Enter.
        assert_eq!(plain("[", None, ctrl), Some(b"\x1b[91;5u".to_vec()));
    }

    /// Alt is Meta, an ESC prefix; on macOS Option is not (Ghostty's
    /// `macos-option-as-alt = false`): the layout's text goes as typed.
    #[test]
    fn alt_prefixes_escape_but_not_macos_option() {
        let alt = Modifiers { alt: true, ..mods() };
        let alt_ctrl = Modifiers { alt: true, control: true, ..mods() };
        if cfg!(target_os = "macos") {
            assert_eq!(plain("b", Some("b"), alt), Some(b"b".to_vec()));
        } else {
            assert_eq!(plain("b", Some("b"), alt), Some(b"\x1bb".to_vec()));
        }
        // A control byte has no layout text, so Alt prefixes it everywhere.
        assert_eq!(plain("c", None, alt_ctrl), Some(vec![0x1b, 0x03]));
        // Modified arrows carry Alt as a parameter everywhere.
        assert_eq!(plain("up", None, alt), Some(b"\x1b[1;3A".to_vec()));
    }

    /// GPUI hands Option chords their composed key_char (Option+B → "∫" on a
    /// US layout); like Ghostty's default (macos-option-as-alt = false), that
    /// character is what the user typed.
    #[cfg(target_os = "macos")]
    #[test]
    fn option_sends_the_character_macos_composed() {
        let alt = Modifiers { alt: true, ..mods() };
        assert_eq!(plain("b", Some("∫"), alt), Some("∫".as_bytes().to_vec()));
        assert_eq!(plain("l", Some("@"), alt), Some(b"@".to_vec()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ghostty_macos_line_editing_keybinds() {
        let cmd = Modifiers { platform: true, ..mods() };
        let alt = Modifiers { alt: true, ..mods() };
        assert_eq!(plain("left", None, cmd), Some(vec![0x01]), "the line's start");
        assert_eq!(plain("right", None, cmd), Some(vec![0x05]), "the line's end");
        assert_eq!(plain("backspace", None, cmd), Some(vec![0x15]));
        assert_eq!(plain("left", None, alt), Some(b"\x1bb".to_vec()), "a word back");
        assert_eq!(plain("right", None, alt), Some(b"\x1bf".to_vec()), "a word forward");
    }

    #[test]
    fn platform_primary_combos_fall_through() {
        let cmd = Modifiers { platform: true, ..mods() };
        assert_eq!(plain("j", Some("j"), cmd), None);
        assert_eq!(plain("enter", None, cmd), None);
    }

    /// Once the program pushes kitty's disambiguate flag, keys the legacy
    /// encoding has no room for get their own codes: Claude Code's
    /// Shift+Enter newline, Ctrl+Tab, Ctrl+Enter.
    #[test]
    fn kitty_keyboard_protocol_when_the_program_asks() {
        let kitty = b"\x1b[>1u";
        let shift = Modifiers { shift: true, ..mods() };
        let ctrl = Modifiers { control: true, ..mods() };
        assert_eq!(enc(kitty, "enter", None, shift), Some(b"\x1b[13;2u".to_vec()));
        assert_eq!(enc(kitty, "enter", None, ctrl), Some(b"\x1b[13;5u".to_vec()));
        assert_eq!(enc(kitty, "tab", None, ctrl), Some(b"\x1b[9;5u".to_vec()));
        assert_eq!(enc(kitty, "c", Some("c"), ctrl), Some(b"\x1b[99;5u".to_vec()));
        assert_eq!(enc(kitty, "escape", None, mods()), Some(b"\x1b[27u".to_vec()));
        // Plain text stays text.
        assert_eq!(enc(kitty, "a", Some("a"), mods()), Some(b"a".to_vec()));
        assert_eq!(enc(kitty, "enter", None, mods()), Some(b"\r".to_vec()));
        // Without the protocol, Ghostty still sets Shift+Enter apart from
        // Enter, in xterm's CSI 27 form.
        assert_eq!(plain("enter", None, shift), Some(b"\x1b[27;2;13~".to_vec()));
    }
}
