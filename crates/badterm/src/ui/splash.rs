//! The splash screen shown for a moment at start: BadTerm's logo, the
//! BADTERM wordmark powering up, with the version and where BadTerm comes
//! from in the corner the logo leaves for them.
//!
//! The logo is an ANSI piece (`assets/logo/badterm.ans`, drawn by
//! `site-next/tools/powerup.py`), played here as a BBS would send it: BAD
//! over TERM, one welded mass under one black keyline, shaded in the
//! negative-land style (a cyan ground, light green and light blue worked over
//! it in vertical streaks), in a gold ki aura with iCE colors. The website's
//! front page shows the same piece with its menu in the corner.
//!
//! The splash is the logo, so it is drawn in the VGA's own colors whatever
//! the color scheme: its cells are resolved to RGB once, which a scheme's
//! palette leaves alone.

use std::sync::OnceLock;
use std::time::Duration;

use badterm_core::Terminal;
use badterm_core::sauce;
use badterm_core::screen::{Cell, Color, Look, Screen};

use super::*;

/// How long it shows (any key closes it sooner).
pub const TIME: Duration = Duration::from_secs(2);

pub const COLS: usize = 80;
pub const ROWS: usize = 25;

/// The logo: 80x25, iCE colors, with SAUCE.
const LOGO: &[u8] = include_bytes!("../../assets/logo/badterm.ans");

/// The corner the logo leaves empty, under BAD and left of TERM: its first
/// column past it, and its top row.
const CORNER_X: usize = 20;
const CORNER_TOP: usize = 14;
/// Where the words in it start.
const TEXT_X: usize = 1;

/// A VGA color, as RGB.
fn vga(i: u8) -> Color {
    Color::Rgb(Look::standard().rgb(Color::Pc(i)))
}

/// The logo's cells, row by row, played once and resolved to RGB.
fn logo() -> &'static [Cell] {
    static CELLS: OnceLock<Vec<Cell>> = OnceLock::new();
    CELLS.get_or_init(|| {
        let (content, _) = sauce::split(LOGO);
        // Rows to spare, so the last row's line feed doesn't scroll it.
        let mut term = Terminal::new(COLS, ROWS + 2, 0);
        term.screen.ice_colors = true;
        term.screen.base_ice = true;
        term.feed(content);
        (0..ROWS)
            .flat_map(|y| term.screen.row(y)[..COLS].to_vec())
            .map(|c| {
                let (fg, bg, _) = c.resolve(true);
                Cell::new(c.ch, Color::Rgb(fg), Color::Rgb(bg))
            })
            .collect()
    })
}

pub fn draw(s: &mut Screen) {
    for (i, cell) in logo().iter().enumerate() {
        if let Some(c) = s.cell_mut(i % COLS, i / COLS) {
            *c = *cell;
        }
    }
    // The corner: what it is, which version, where from.
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let lines = [
        (15, "t h e", 7),
        (16, "b a d a s s", 15),
        (17, "t e r m i n a l", 7),
        (19, version.as_str(), 15),
        (20, "badterm.com", 11),
        (22, "Copyright (C) 2026", 7),
        (23, "Bad Cluster", 7),
        (24, "GNU GPL v3 or later", 8),
    ];
    for (y, text, fg) in lines {
        debug_assert!(y >= CORNER_TOP && TEXT_X + text.chars().count() <= CORNER_X, "{text:?} is outside the corner");
        label(s, TEXT_X, y, text, vga(fg), vga(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(s: &Screen) -> Vec<String> {
        (0..ROWS).map(|y| Screen::row_text(s.row(y))).collect()
    }

    #[test]
    fn logo_and_words() {
        let mut s = Screen::new(COLS, ROWS, 0);
        draw(&mut s);
        let rows = rows(&s);
        assert!(rows[19].contains(env!("CARGO_PKG_VERSION")), "{rows:#?}");
        assert!(rows[20].contains("badterm.com"), "{rows:#?}");
        // The logo fills the screen: blocks and shades on most rows, top to bottom.
        let inked = rows.iter().filter(|r| r.chars().any(|c| "█▀▄░▒▓".contains(c))).count();
        assert!(inked >= 22, "{inked} rows with ink: {rows:#?}");
        // Its aura uses iCE colors (bright backgrounds), in the VGA's own.
        assert!(logo().iter().any(|c| c.bg == vga(14)), "no yellow backgrounds");
        assert!(logo().iter().all(|c| matches!((c.fg, c.bg), (Color::Rgb(_), Color::Rgb(_)))));
    }

    /// The logo leaves its corner empty for the words, and every word fits in it.
    #[test]
    fn corner_is_clear() {
        let cells = logo();
        for y in CORNER_TOP..ROWS {
            for x in 0..CORNER_X {
                let c = cells[y * COLS + x];
                assert!(c.ch == b' ' && c.bg == vga(0), "the logo has ink at {x},{y}");
            }
        }
        let mut s = Screen::new(COLS, ROWS, 0);
        draw(&mut s);
        for y in 0..ROWS {
            for x in 0..COLS {
                let cell = s.row(y)[x];
                let ours = y >= CORNER_TOP && x < CORNER_X;
                if ours {
                    continue;
                }
                assert_eq!(cell, cells[y * COLS + x], "a word spilled out of the corner at {x},{y}");
            }
        }
    }
}
