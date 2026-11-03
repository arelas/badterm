//! The splash screen shown for a moment at start: the name as ANSI art,
//! with the version and where BadTerm comes from.
//!
//! The art is drawn the way the BBS scene's logos were: on a canvas of
//! half-block "pixels" (each text cell is two of them, stacked: `▀` in one
//! color over another), so letters can have rounded corners and a color
//! ramp down them. The letters fade from white through cyan to blue, like
//! ice lit from above, and cast a dark shadow down and to the right; thin
//! rules above and below fade out to dots, in the same colors.

use std::time::Duration;

use badterm_core::screen::{Cell, Color, Screen};

use super::*;

/// How long it shows (any key closes it sooner).
pub const TIME: Duration = Duration::from_secs(2);

pub const COLS: usize = 80;
pub const ROWS: usize = 25;

/// The canvas: two pixels per text row.
const PW: usize = COLS;
const PH: usize = ROWS * 2;

/// Letters, 9 pixels wide and 12 high (`#` is lit).
const LETTERS: [(char, [&str; 12]); 7] = [
    (
        'B',
        [
            "#######..",
            "########.",
            "###...###",
            "###...###",
            "###..###.",
            "#######..",
            "########.",
            "###...###",
            "###...###",
            "###...###",
            "########.",
            "#######..",
        ],
    ),
    (
        'A',
        [
            "..#####..",
            ".#######.",
            "###...###",
            "###...###",
            "###...###",
            "#########",
            "#########",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
        ],
    ),
    (
        'D',
        [
            "#######..",
            "########.",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
            "########.",
            "#######..",
        ],
    ),
    (
        'T',
        [
            "#########",
            "#########",
            "...###...",
            "...###...",
            "...###...",
            "...###...",
            "...###...",
            "...###...",
            "...###...",
            "...###...",
            "...###...",
            "...###...",
        ],
    ),
    (
        'E',
        [
            "#########",
            "#########",
            "###......",
            "###......",
            "###......",
            "#######..",
            "#######..",
            "###......",
            "###......",
            "###......",
            "#########",
            "#########",
        ],
    ),
    (
        'R',
        [
            "#######..",
            "########.",
            "###...###",
            "###...###",
            "###...###",
            "########.",
            "#######..",
            "###.###..",
            "###..###.",
            "###...###",
            "###...###",
            "###...###",
        ],
    ),
    (
        'M',
        [
            "##.....##",
            "###...###",
            "####.####",
            "###.#.###",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
            "###...###",
        ],
    ),
];

/// The letters' color by pixel row, ice: a white highlight at the top,
/// light cyan, cyan, then deep blue at the bottom.
const RAMP: [u8; 12] = [15, 11, 11, 11, 11, 11, 3, 3, 3, 1, 1, 1];
/// Where the logo's top pixel row is (an even one: a text row's top).
const LOGO_Y: usize = 10;
const SHADOW: u8 = 8;

/// A canvas of pixels, each a VGA color (0, black, for none).
struct Canvas([[u8; PW]; PH]);

impl Canvas {
    fn set(&mut self, x: usize, y: usize, c: u8) {
        if x < PW && y < PH {
            self.0[y][x] = c;
        }
    }

    /// Onto the screen: each cell shows its two pixels with `▀`, its
    /// foreground over its background (or a space, or a full block).
    fn draw(&self, s: &mut Screen) {
        for y in 0..ROWS {
            for x in 0..COLS {
                let (top, bottom) = (self.0[y * 2][x], self.0[y * 2 + 1][x]);
                let cell = match (top, bottom) {
                    (0, 0) => Cell::new(b' ', Color::Pc(7), Color::Pc(0)),
                    (t, b) if t == b => Cell::new(0xDB, Color::Pc(t), Color::Pc(0)),
                    (0, b) => Cell::new(0xDC, Color::Pc(b), Color::Pc(0)),
                    (t, b) => Cell::new(0xDF, Color::Pc(t), Color::Pc(b)),
                };
                if let Some(c) = s.cell_mut(x, y) {
                    *c = cell;
                }
            }
        }
    }
}

/// The logo's pixels: which are lit, at (x, y) from its top left.
fn logo() -> Vec<(usize, usize)> {
    let mut lit = Vec::new();
    for (i, ch) in "BADTERM".chars().enumerate() {
        let rows = LETTERS.iter().find(|(c, _)| *c == ch).map(|(_, r)| r).expect("a letter of the name");
        for (y, row) in rows.iter().enumerate() {
            for (x, b) in row.bytes().enumerate() {
                if b == b'#' {
                    lit.push((i * 10 + x, y));
                }
            }
        }
    }
    lit
}

/// A thin rule across the screen, bright in the middle and fading out
/// to dots at the edges, in the logo's colors.
fn rule(s: &mut Screen, y: usize) {
    for x in 0..COLS {
        let d = ((x as f64 + 0.5) - COLS as f64 / 2.0).abs() / (COLS as f64 / 2.0);
        let (ch, fg) = match d {
            d if d < 0.18 => (0xC4, 11),
            d if d < 0.42 => (0xC4, 3),
            d if d < 0.66 => (0xC4, 1),
            d if d < 0.80 => (0xC4, 8),
            d if d < 0.88 => (0xF9, 8),
            d if d < 0.95 && x % 2 == 0 => (0xFA, 8),
            _ => continue,
        };
        s.draw_text(x, y, &[ch], Color::Pc(fg), Color::Pc(0));
    }
}

/// Text in pieces of different colors, centered on row `y`.
fn centered(s: &mut Screen, y: usize, parts: &[(&str, u8)]) {
    let len: usize = parts.iter().map(|(t, _)| t.chars().count()).sum();
    let mut x = (COLS - len) / 2;
    for (text, fg) in parts {
        label(s, x, y, text, Color::Pc(*fg), Color::Pc(0));
        x += text.chars().count();
    }
}

pub fn draw(s: &mut Screen) {
    s.fill_rect(0, 0, COLS, ROWS, Cell::new(b' ', Color::Pc(7), Color::Pc(0)));
    let mut c = Canvas([[0; PW]; PH]);
    let lit = logo();
    let width = lit.iter().map(|&(x, _)| x + 1).max().unwrap_or(0);
    let x0 = (PW - width) / 2;
    // The shadow, then the letters over it.
    for &(x, y) in &lit {
        c.set(x0 + x + 1, LOGO_Y + y + 1, SHADOW);
    }
    for &(x, y) in &lit {
        c.set(x0 + x, LOGO_Y + y, RAMP[y]);
    }
    c.draw(s);

    // The rules a row clear of the logo (and its shadow), then the words.
    let (top, bottom) = (LOGO_Y / 2 - 2, (LOGO_Y + 13) / 2 + 2);
    rule(s, top);
    rule(s, bottom);
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    centered(s, bottom + 3, &[("t h e   b a d a s s   t e r m i n a l", 7)]);
    centered(s, bottom + 6, &[(version.as_str(), 15), ("  \u{00B7}  ", 8), ("badterm.com", 11)]);
    let copyright = [("Copyright (C) 2026 Bad Cluster", 7), ("  \u{00B7}  ", 8), ("free software, GNU GPL v3+", 7)];
    centered(s, bottom + 8, &copyright);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits() {
        let mut s = Screen::new(COLS, ROWS, 0);
        draw(&mut s);
        let rows: Vec<String> = (0..ROWS).map(|y| Screen::row_text(s.row(y))).collect();
        assert!(rows.iter().any(|r| r.contains(env!("CARGO_PKG_VERSION"))), "{rows:#?}");
        // The logo is on screen, inside the edges.
        let logo_row = &rows[LOGO_Y / 2 + 1];
        assert!(logo_row.contains('\u{2588}') && !logo_row.starts_with('\u{2588}'), "{rows:#?}");
        // Every letter is the same width.
        for (ch, rows) in LETTERS {
            assert!(rows.iter().all(|r| r.len() == 9), "{ch}");
        }
    }
}
