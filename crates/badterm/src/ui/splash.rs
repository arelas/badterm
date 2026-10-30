//! The splash screen shown for a moment at start: the name in block
//! letters, with the version and where BadTerm comes from.

use std::time::Duration;

use badterm_core::screen::{Cell, Color, Screen};

use super::*;

/// How long it shows (any key closes it sooner).
pub const TIME: Duration = Duration::from_secs(2);

pub const COLS: usize = 80;
pub const ROWS: usize = 25;

/// Five rows per letter, `#` for a block.
const LETTERS: [(char, [&str; 5]); 7] = [
    ('B', ["####.", "#...#", "####.", "#...#", "####."]),
    ('A', [".###.", "#...#", "#####", "#...#", "#...#"]),
    ('D', ["####.", "#...#", "#...#", "#...#", "####."]),
    ('T', ["#####", "..#..", "..#..", "..#..", "..#.."]),
    ('E', ["#####", "#....", "####.", "#....", "#####"]),
    ('R', ["####.", "#...#", "####.", "#..#.", "#...#"]),
    ('M', ["#...#", "##.##", "#.#.#", "#...#", "#...#"]),
];

const BLOCK: u8 = 0xDB;

pub fn draw(s: &mut Screen) {
    s.fill_rect(0, 0, COLS, ROWS, Cell::new(b' ', Color::Pc(7), Color::Pc(0)));
    let word = "BADTERM";
    // Each letter is five blocks wide, a column apart.
    let width = word.len() * 6 - 1;
    let (x0, y0) = ((COLS - width) / 2, 5);
    // Bright at the top, deeper green below, as a phosphor glows.
    let shades = [Color::Pc(10), Color::Pc(10), Color::Pc(10), Color::Pc(2), Color::Pc(2)];
    for (i, ch) in word.chars().enumerate() {
        let rows = LETTERS.iter().find(|(c, _)| *c == ch).map(|(_, r)| r).expect("a letter of the name");
        for (dy, row) in rows.iter().enumerate() {
            for (dx, b) in row.bytes().enumerate() {
                if b == b'#' {
                    s.draw_text(x0 + i * 6 + dx, y0 + dy, &[BLOCK], shades[dy], Color::Pc(0));
                }
            }
        }
    }
    let lines: [(String, Color); 4] = [
        (format!("Version {}", env!("CARGO_PKG_VERSION")), Color::Pc(14)),
        ("The badass terminal, with the glow of a real CRT".into(), Color::Pc(11)),
        ("Copyright (C) 2026 Bad Cluster.  Free software under the GNU GPL v3+".into(), Color::Pc(7)),
        ("badterm.com".into(), Color::Pc(10)),
    ];
    for (i, (text, fg)) in lines.iter().enumerate() {
        let x = (COLS - text.chars().count()) / 2;
        label(s, x, 13 + i * 2, text, *fg, Color::Pc(0));
    }
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
        // The name's blocks are on screen, inside the edges.
        assert!(rows[5].contains('\u{2588}') && !rows[5].starts_with('\u{2588}'));
    }
}
