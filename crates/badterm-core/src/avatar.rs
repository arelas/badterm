//! AVATAR (AVT/0 and AVT/0+): the compact screen codes of Opus, Maximus
//! and other FidoNet-era BBSes, interpreted alongside ANSI.
//!
//! - `^L` clears the screen to cyan on black and homes the cursor.
//! - `^Y c n` repeats the character `c` `n` times.
//! - `^V` starts a command: `^A attr` sets the color (a PC attribute byte),
//!   `^B` blinks, `^C`/`^D`/`^E`/`^F` move up/down/left/right, `^G` clears
//!   to the end of the line, `^H row col` moves the cursor (from 1, 1).
//! - AVT/0+ adds: `^I` insert mode (until the next command), `^J n t l b r`
//!   and `^K ...` scroll an area up or down, `^L attr rows cols` clears an
//!   area at the cursor, `^M attr c rows cols` fills one with a character,
//!   `^N` deletes the character at the cursor, and `^Y n pattern... count`
//!   repeats a pattern.

use crate::screen::{Cell, Color, Screen, flags};

/// The longest a repeated pattern may expand to, so a hostile one cannot
/// make the screen do much work.
const MAX_REPEAT: usize = 64 * 1024;

#[derive(Debug, Default)]
pub struct Avatar {
    /// The bytes of the command so far (after `^V` or `^Y`), or None.
    command: Option<Vec<u8>>,
    /// AVT/0+ insert mode: characters push the rest of the line right.
    pub insert: bool,
}

/// What the terminal should do with a byte.
pub enum Outcome {
    /// Not an Avatar code: handle it as usual.
    Pass,
    /// Taken.
    Done,
    /// Show these bytes (a repeat), as if they had arrived.
    Show(Vec<u8>),
}

impl Avatar {
    /// Whether a command is half read.
    pub fn busy(&self) -> bool {
        self.command.is_some()
    }

    pub fn byte(&mut self, s: &mut Screen, b: u8) -> Outcome {
        let Some(cmd) = self.command.as_mut() else {
            return match b {
                0x0C => {
                    self.insert = false;
                    set_attr(s, 3);
                    s.erase_display(2);
                    s.move_to(0, 0);
                    Outcome::Done
                }
                0x16 | 0x19 => {
                    self.command = Some(vec![b]);
                    Outcome::Done
                }
                _ => Outcome::Pass,
            };
        };
        cmd.push(b);
        let cmd = cmd.clone();
        match self.complete(&cmd) {
            Some(n) if n <= cmd.len() => {
                self.command = None;
                self.run(s, &cmd)
            }
            _ => Outcome::Done,
        }
    }

    /// How long the command is, once enough of it has arrived to tell.
    fn complete(&self, cmd: &[u8]) -> Option<usize> {
        match cmd {
            [0x19, ..] => Some(3),
            [0x16] => None,
            [0x16, op, rest @ ..] => match op {
                0x01 => Some(3),
                0x08 => Some(4),
                0x0A | 0x0B => Some(7),
                0x0C => Some(5),
                0x0D => Some(6),
                // ^V ^Y n pattern(n) count
                0x19 => rest.first().map(|&n| 3 + n as usize + 1),
                _ => Some(2),
            },
            _ => Some(cmd.len()),
        }
    }

    fn run(&mut self, s: &mut Screen, cmd: &[u8]) -> Outcome {
        if cmd[0] == 0x19 {
            return Outcome::Show(vec![cmd[1]; cmd[2] as usize]);
        }
        let (op, a) = (cmd[1], &cmd[2..]);
        // Any command ends insert mode; ^V ^I turns it back on.
        self.insert = op == 0x09;
        let (x, y) = s.cursor();
        match op {
            0x01 => set_attr(s, a[0]),
            0x02 => s.pen.flags |= flags::BLINK,
            0x03 => s.move_by(0, -1),
            0x04 => s.move_by(0, 1),
            0x05 => s.move_by(-1, 0),
            0x06 => s.move_by(1, 0),
            0x07 => s.erase_line(0),
            0x08 => s.move_to((a[1] as usize).saturating_sub(1), (a[0] as usize).saturating_sub(1)),
            0x0A | 0x0B => {
                let (n, top, left, bottom, right) = (a[0] as usize, a[1], a[2], a[3], a[4]);
                let area = Area::from_corners(s, top, left, bottom, right);
                scroll_area(s, area, n, op == 0x0A);
            }
            0x0C => {
                let blank = Cell { ch: b' ', ..attr_cell(s, a[0]) };
                set_attr(s, a[0]);
                fill(s, Area { x, y, w: a[2] as usize, h: a[1] as usize }, blank);
            }
            0x0D => {
                let cell = Cell { ch: a[1], ..attr_cell(s, a[0]) };
                set_attr(s, a[0]);
                fill(s, Area { x, y, w: a[3] as usize, h: a[2] as usize }, cell);
            }
            0x0E => s.delete_chars(1),
            0x19 => {
                let n = a[0] as usize;
                let (pattern, count) = (&a[1..1 + n], a[1 + n] as usize);
                let total = (pattern.len() * count).min(MAX_REPEAT);
                return Outcome::Show(pattern.iter().copied().cycle().take(total).collect());
            }
            _ => log::debug!("unhandled AVATAR ^V {op:#04x}"),
        }
        Outcome::Done
    }
}

/// The pen for a PC attribute byte: foreground in the low nibble, the
/// background in the next three bits, and blink in the top one.
fn attr_cell(s: &Screen, attr: u8) -> Cell {
    let mut flags_ = s.pen.flags & !(flags::BOLD | flags::BLINK | flags::REVERSE | flags::FAINT);
    if attr & 0x80 != 0 {
        flags_ |= flags::BLINK;
    }
    Cell { ch: b' ', fg: Color::Pc(attr & 0x0F), bg: Color::Pc((attr >> 4) & 0x07), flags: flags_, tile: 0 }
}

fn set_attr(s: &mut Screen, attr: u8) {
    s.pen = attr_cell(s, attr);
}

/// A rectangle of cells, clipped to the screen.
#[derive(Clone, Copy, Debug)]
struct Area {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl Area {
    /// From 1-based corners, as AVT/0+ gives them.
    fn from_corners(s: &Screen, top: u8, left: u8, bottom: u8, right: u8) -> Area {
        let (t, l) = ((top as usize).max(1) - 1, (left as usize).max(1) - 1);
        let (b, r) = ((bottom as usize).clamp(1, s.rows()) - 1, (right as usize).clamp(1, s.cols()) - 1);
        Area { x: l, y: t, w: (r + 1).saturating_sub(l), h: (b + 1).saturating_sub(t) }
    }

    fn clip(self, s: &Screen) -> Area {
        let x = self.x.min(s.cols());
        let y = self.y.min(s.rows());
        Area { x, y, w: self.w.min(s.cols() - x), h: self.h.min(s.rows() - y) }
    }
}

fn fill(s: &mut Screen, area: Area, cell: Cell) {
    let a = area.clip(s);
    s.fill_rect(a.x, a.y, a.w, a.h, cell);
}

/// Scroll the cells of `area` up (or down) `n` lines, blanking the lines
/// that open up in the current color. 0 lines clears the area.
fn scroll_area(s: &mut Screen, area: Area, n: usize, up: bool) {
    let a = area.clip(s);
    let blank = Cell { ch: b' ', ..s.pen };
    let n = if n == 0 { a.h } else { n.min(a.h) };
    let rows: Vec<Vec<Cell>> = (a.y..a.y + a.h).map(|y| s.row(y)[a.x..a.x + a.w].to_vec()).collect();
    for i in 0..a.h {
        let from = if up { i.checked_add(n).filter(|&f| f < a.h) } else { i.checked_sub(n) };
        let line = from.map(|f| rows[f].clone()).unwrap_or_else(|| vec![blank; a.w]);
        for (dx, cell) in line.into_iter().enumerate() {
            if let Some(c) = s.cell_mut(a.x + dx, a.y + i) {
                *c = cell;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ansi::{Encoding, Terminal};
    use crate::screen::Color;

    fn term(data: &[u8]) -> Terminal {
        let mut t = Terminal::new(80, 25, 0);
        t.encoding = Encoding::Avatar;
        t.feed(data);
        t
    }

    fn line(t: &Terminal, y: usize) -> String {
        crate::screen::Screen::row_text(t.screen.row(y)).trim_end().to_string()
    }

    #[test]
    fn avt0_codes() {
        // Clear, yellow on blue, a repeat, goto row 3 col 5, then text.
        let t = term(b"junk\x0c\x16\x01\x1eHi\x19-\x05\x16\x08\x03\x05X");
        assert_eq!(line(&t, 0), "Hi-----");
        let c = t.screen.row(0)[0];
        assert_eq!((c.fg, c.bg), (Color::Pc(14), Color::Pc(1)));
        assert_eq!(t.screen.row(2)[4].ch, b'X');
        // ^L leaves cyan on black.
        let t = term(b"\x0cA");
        assert_eq!(t.screen.row(0)[0].fg, Color::Pc(3));
    }

    #[test]
    fn avt0_plus_codes() {
        // Insert mode pushes the line right; any command ends it.
        let t = term(b"ABC\x16\x08\x01\x01\x16\x09XY\x16\x06Z");
        assert_eq!(line(&t, 0), "XYAZC");
        // A pattern repeat, and ANSI still works alongside.
        let t = term(b"\x16\x19\x02ab\x03\x1b[2;1HOK");
        assert_eq!((line(&t, 0), line(&t, 1)), ("ababab".into(), "OK".into()));
        // Fill an area, then scroll it up a line.
        let t = term(b"\x16\x08\x02\x02\x16\x0d\x07#\x02\x03\x16\x0a\x01\x02\x02\x03\x04");
        assert_eq!((line(&t, 1), line(&t, 2)), (" ###".into(), "".into()));
        // Delete a character.
        let t = term(b"ABC\x16\x08\x01\x01\x16\x0e");
        assert_eq!(line(&t, 0), "BC");
    }
}
