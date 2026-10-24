//! Pointer → PTY bytes, through Ghostty's own mouse encoder: X10, normal,
//! button-event and any-event tracking in the X10, UTF-8, SGR, urxvt and
//! SGR-pixels formats, each as the running program asked for them. Also
//! Ghostty's alternate scroll: on the alternate screen, for a program that
//! does not read the mouse, the wheel sends arrow keys (`less`, `git log`).

use ghostty_vt::Terminal;
use ghostty_vt::key::Mods;
pub use ghostty_vt::mouse::{Action, Button};
use ghostty_vt::mouse::{Encoder, EncoderSize, Event, Position};
use ghostty_vt::screen::Screen;
use ghostty_vt::terminal::Mode;
use gpui::Modifiers;

/// The grid a pointer position is measured against: positions are relative
/// to its top-left glyph, in the same units as the cell size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub cell_w: f32,
    pub line_h: f32,
    pub cols: u16,
    pub rows: u16,
}

/// The modes that pick the encoder's tracking and format.
const MOUSE_MODES: [Mode; 8] = [
    Mode::X10_MOUSE,
    Mode::NORMAL_MOUSE,
    Mode::BUTTON_MOUSE,
    Mode::ANY_MOUSE,
    Mode::UTF8_MOUSE,
    Mode::SGR_MOUSE,
    Mode::URXVT_MOUSE,
    Mode::SGR_PIXELS_MOUSE,
];

/// Whether the program on `term` asked for mouse reports.
pub fn captures(term: &Terminal) -> bool {
    term.is_mouse_tracking().unwrap_or(false)
}

/// One terminal's mouse encoder. It lives as long as the terminal does: it
/// keeps the last cell a motion was reported in, so a move reports once per
/// cell crossed, as Ghostty sends them.
pub struct MouseEncoder {
    encoder: Encoder,
    event: Event,
    /// The program's mouse modes and the grid the encoder was last set up
    /// for. Setting it up again forgets the last cell, so it is redone only
    /// when one of them changes.
    applied: Option<([bool; 8], EncoderSize)>,
}

impl Default for MouseEncoder {
    fn default() -> Self {
        let mut encoder = Encoder::new().expect("libghostty-vt mouse encoder");
        encoder.set_track_last_cell(true);
        Self {
            encoder,
            event: Event::new().expect("libghostty-vt mouse event"),
            applied: None,
        }
    }
}

impl MouseEncoder {
    /// Encode a pointer event at `(x, y)` (from the grid's top-left glyph)
    /// for the program on `term`. Empty when the program did not ask for
    /// this one. `held`: a button is down, so a drag out of the grid still
    /// reports.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        term: &Terminal,
        action: Action,
        button: Option<Button>,
        (x, y): (f32, f32),
        mods: &Modifiers,
        held: bool,
        grid: Grid,
    ) -> Vec<u8> {
        if !(grid.cell_w > 0.0 && grid.line_h > 0.0) {
            return Vec::new();
        }
        // Ghostty takes whole-pixel cells; the font's cell is fractional. So
        // the encoder works in units of whole cells the size of the rounded
        // ones, and the pointer is scaled into them: every cell maps exactly,
        // and SGR-pixels reports stay within a pixel of the real position.
        let (cell_w, line_h) = (grid.cell_w.round().max(1.0), grid.line_h.round().max(1.0));
        let size = EncoderSize {
            screen_width: grid.cols as u32 * cell_w as u32,
            screen_height: grid.rows as u32 * line_h as u32,
            cell_width: cell_w as u32,
            cell_height: line_h as u32,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        };
        let modes = MOUSE_MODES.map(|mode| term.mode(mode).unwrap_or(false));
        if self.applied != Some((modes, size)) {
            self.encoder.set_options_from_terminal(term).set_size(size);
            self.applied = Some((modes, size));
        }
        self.encoder.set_any_button_pressed(held);
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
        self.event
            .set_action(action)
            .set_button(button)
            .set_mods(ghostty_mods)
            .set_position(Position {
                x: x / grid.cell_w * cell_w,
                y: y / grid.line_h * line_h,
            });
        let mut out = Vec::new();
        let _ = self.encoder.encode_to_vec(&self.event, &mut out);
        out
    }
}

/// The wheel as arrow keys, one per line (positive: up), on the alternate
/// screen of a program that does not read the mouse, while it leaves mode
/// 1007 on (Ghostty's default). `None` when the wheel is not for keys.
pub fn alternate_scroll(term: &Terminal, lines: i32) -> Option<Vec<u8>> {
    if lines == 0
        || captures(term)
        || term.active_screen().ok()? != Screen::Alternate
        || !term.mode(Mode::ALT_SCROLL).unwrap_or(false)
    {
        return None;
    }
    let app = term.mode(Mode::DECCKM).unwrap_or(false);
    let key: &[u8] = match (lines > 0, app) {
        (true, false) => b"\x1b[A",
        (false, false) => b"\x1b[B",
        (true, true) => b"\x1bOA",
        (false, true) => b"\x1bOB",
    };
    Some(key.repeat(lines.unsigned_abs() as usize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::emulator::Emulator;

    /// 10×20 cells, 20×4.
    const GRID: Grid = Grid {
        cell_w: 10.0,
        line_h: 20.0,
        cols: 20,
        rows: 4,
    };

    fn mods() -> Modifiers {
        Modifiers::default()
    }

    /// A terminal whose program sent `setup`, and an encoder for it.
    fn term(setup: &[u8]) -> (Emulator, MouseEncoder) {
        let mut e = Emulator::new(GRID.cols, GRID.rows);
        e.feed(setup);
        (e, MouseEncoder::default())
    }

    /// The middle of cell (col, row).
    fn at(col: usize, row: usize) -> (f32, f32) {
        (col as f32 * 10.0 + 5.0, row as f32 * 20.0 + 10.0)
    }

    fn enc(
        e: &Emulator,
        m: &mut MouseEncoder,
        action: Action,
        button: Option<Button>,
        pos: (f32, f32),
    ) -> String {
        let held = button.is_some() && action != Action::Release;
        String::from_utf8_lossy(&m.encode(e.terminal(), action, button, pos, &mods(), held, GRID))
            .into_owned()
    }

    #[test]
    fn nothing_without_tracking() {
        let (e, mut m) = term(b"");
        assert!(!captures(e.terminal()));
        assert_eq!(
            enc(&e, &mut m, Action::Press, Some(Button::Left), at(2, 1)),
            ""
        );
    }

    #[test]
    fn sgr_press_release_and_wheel() {
        let (e, mut m) = term(b"\x1b[?1000h\x1b[?1006h");
        assert!(captures(e.terminal()));
        let press = enc(&e, &mut m, Action::Press, Some(Button::Left), at(2, 1));
        assert_eq!(press, "\x1b[<0;3;2M");
        let release = enc(&e, &mut m, Action::Release, Some(Button::Left), at(2, 1));
        assert_eq!(release, "\x1b[<0;3;2m");
        let right = enc(&e, &mut m, Action::Press, Some(Button::Right), at(0, 0));
        assert_eq!(right, "\x1b[<2;1;1M");
        let middle = enc(&e, &mut m, Action::Press, Some(Button::Middle), at(19, 3));
        assert_eq!(middle, "\x1b[<1;20;4M");
        // The wheel at the pointer's cell: up is button 4, down button 5.
        let up = enc(&e, &mut m, Action::Press, Some(Button::Four), at(5, 2));
        assert_eq!(up, "\x1b[<64;6;3M");
        let down = enc(&e, &mut m, Action::Press, Some(Button::Five), at(5, 2));
        assert_eq!(down, "\x1b[<65;6;3M");
        // Normal tracking reports no motion, dragged or not.
        assert_eq!(
            enc(&e, &mut m, Action::Motion, Some(Button::Left), at(3, 1)),
            ""
        );
        assert_eq!(enc(&e, &mut m, Action::Motion, None, at(3, 1)), "");
    }

    #[test]
    fn modifiers_add_to_the_button_code() {
        let (e, mut m) = term(b"\x1b[?1000h\x1b[?1006h");
        let ctrl_alt = Modifiers {
            control: true,
            alt: true,
            ..mods()
        };
        let bytes = m.encode(
            e.terminal(),
            Action::Press,
            Some(Button::Left),
            at(0, 0),
            &ctrl_alt,
            true,
            GRID,
        );
        assert_eq!(bytes, b"\x1b[<24;1;1M");
    }

    /// Button-event tracking (1002): drags report, once per cell crossed;
    /// a move with no button does not.
    #[test]
    fn button_tracking_reports_drags_per_cell() {
        let (e, mut m) = term(b"\x1b[?1002h\x1b[?1006h");
        enc(&e, &mut m, Action::Press, Some(Button::Left), at(2, 1));
        let drag = enc(&e, &mut m, Action::Motion, Some(Button::Left), at(3, 1));
        assert_eq!(drag, "\x1b[<32;4;2M");
        let (x, y) = at(3, 1);
        assert_eq!(
            enc(&e, &mut m, Action::Motion, Some(Button::Left), (x + 2.0, y)),
            ""
        );
        let right_drag = enc(&e, &mut m, Action::Motion, Some(Button::Right), at(4, 1));
        assert_eq!(right_drag, "\x1b[<34;5;2M");
        assert_eq!(enc(&e, &mut m, Action::Motion, None, at(6, 1)), "");
    }

    /// Any-event tracking (1003): a move with no button reports too, for
    /// hover in TUIs.
    #[test]
    fn any_tracking_reports_hover_per_cell() {
        let (e, mut m) = term(b"\x1b[?1003h\x1b[?1006h");
        assert_eq!(
            enc(&e, &mut m, Action::Motion, None, at(2, 1)),
            "\x1b[<35;3;2M"
        );
        assert_eq!(enc(&e, &mut m, Action::Motion, None, at(2, 1)), "");
        assert_eq!(
            enc(&e, &mut m, Action::Motion, None, at(2, 2)),
            "\x1b[<35;3;3M"
        );
        // Out of the grid with no button held: nothing.
        assert_eq!(enc(&e, &mut m, Action::Motion, None, (-5.0, 10.0)), "");
    }

    /// The fractional cell of a real font still maps every position to its cell.
    #[test]
    fn fractional_cells_map_exactly() {
        let (e, mut m) = term(b"\x1b[?1000h\x1b[?1006h");
        let grid = Grid {
            cell_w: 7.8,
            line_h: 17.6,
            cols: 100,
            rows: 30,
        };
        // The right edge of col 89, row 27: rounding the cell to 8 px first
        // would land on col 87.
        let bytes = m.encode(
            e.terminal(),
            Action::Press,
            Some(Button::Left),
            (7.8 * 90.0 - 0.1, 17.6 * 28.0 - 0.1),
            &mods(),
            true,
            grid,
        );
        assert_eq!(bytes, b"\x1b[<0;90;28M");
    }

    /// The default (X10) format: three bytes offset by 32, releases as button
    /// 3, and nothing past column 223.
    #[test]
    fn x10_format_and_its_limits() {
        let (e, mut m) = term(b"\x1b[?1000h");
        let press = m.encode(
            e.terminal(),
            Action::Press,
            Some(Button::Left),
            at(2, 1),
            &mods(),
            true,
            GRID,
        );
        assert_eq!(press, [0x1b, b'[', b'M', 32, 32 + 3, 32 + 2]);
        let release = m.encode(
            e.terminal(),
            Action::Release,
            Some(Button::Left),
            at(2, 1),
            &mods(),
            false,
            GRID,
        );
        assert_eq!(release, [0x1b, b'[', b'M', 32 + 3, 32 + 3, 32 + 2]);
        let wide = Grid { cols: 300, ..GRID };
        let far = m.encode(
            e.terminal(),
            Action::Press,
            Some(Button::Left),
            (250.0 * 10.0, 10.0),
            &mods(),
            true,
            wide,
        );
        assert!(far.is_empty());
    }

    /// X10 tracking (mode 9): presses only, and no modifiers.
    #[test]
    fn x10_tracking_reports_presses_only() {
        let (e, mut m) = term(b"\x1b[?9h");
        let ctrl = Modifiers {
            control: true,
            ..mods()
        };
        let press = m.encode(
            e.terminal(),
            Action::Press,
            Some(Button::Left),
            at(0, 0),
            &ctrl,
            true,
            GRID,
        );
        assert_eq!(press, [0x1b, b'[', b'M', 32, 33, 33]);
        assert_eq!(
            enc(&e, &mut m, Action::Release, Some(Button::Left), at(0, 0)),
            ""
        );
        assert_eq!(
            enc(&e, &mut m, Action::Press, Some(Button::Four), at(0, 0)),
            ""
        );
    }

    #[test]
    fn urxvt_and_sgr_pixel_formats() {
        let (e, mut m) = term(b"\x1b[?1000h\x1b[?1015h");
        let urxvt = enc(&e, &mut m, Action::Press, Some(Button::Left), at(2, 1));
        assert_eq!(urxvt, "\x1b[32;3;2M");
        let (e, mut m) = term(b"\x1b[?1000h\x1b[?1016h");
        let pixels = enc(&e, &mut m, Action::Press, Some(Button::Left), at(2, 1));
        assert_eq!(pixels, "\x1b[<0;25;30M");
    }

    #[test]
    fn alternate_scroll_sends_arrows_on_the_alternate_screen() {
        let (e, _) = term(b"");
        assert_eq!(alternate_scroll(e.terminal(), 1), None, "primary screen");
        let (e, _) = term(b"\x1b[?1049h");
        assert_eq!(
            alternate_scroll(e.terminal(), 2),
            Some(b"\x1b[A\x1b[A".to_vec())
        );
        assert_eq!(alternate_scroll(e.terminal(), -1), Some(b"\x1b[B".to_vec()));
        assert_eq!(alternate_scroll(e.terminal(), 0), None);
        let (e, _) = term(b"\x1b[?1049h\x1b[?1h");
        assert_eq!(alternate_scroll(e.terminal(), 1), Some(b"\x1bOA".to_vec()));
        // A program that reads the mouse gets the wheel itself.
        let (e, _) = term(b"\x1b[?1049h\x1b[?1000h");
        assert_eq!(alternate_scroll(e.terminal(), 1), None);
        let (e, _) = term(b"\x1b[?1049h\x1b[?1007l");
        assert_eq!(alternate_scroll(e.terminal(), 1), None);
    }
}
