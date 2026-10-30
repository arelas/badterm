//! Mouse selection of terminal text: a run of cells from where the drag
//! started to where it is now, wrapping across lines like selected text in
//! an editor.

use phosphor_core::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Where the drag started, as (column, row) on screen.
    pub anchor: (usize, usize),
    /// Where it is now.
    pub head: (usize, usize),
}

impl Selection {
    pub fn at(cell: (usize, usize)) -> Selection {
        Selection { anchor: cell, head: cell }
    }

    /// First and last selected cells, in reading order.
    fn ends(&self) -> ((usize, usize), (usize, usize)) {
        let key = |(x, y): (usize, usize)| (y, x);
        if key(self.anchor) <= key(self.head) { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    /// Nothing selected yet: a click without a drag.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn contains(&self, x: usize, y: usize) -> bool {
        let ((x0, y0), (x1, y1)) = self.ends();
        (y0..=y1).contains(&y) && (y > y0 || x >= x0) && (y < y1 || x <= x1)
    }

    /// The selected part of each row in `rows`.
    pub fn lines<'a>(&self, rows: &[&'a [Cell]]) -> Vec<&'a [Cell]> {
        let ((x0, y0), (x1, y1)) = self.ends();
        (y0..=y1.min(rows.len().saturating_sub(1)))
            .map(|y| {
                let row = rows[y];
                let start = if y == y0 { x0.min(row.len()) } else { 0 };
                let end = if y == y1 { (x1 + 1).min(row.len()) } else { row.len() };
                &row[start..end.max(start)]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use phosphor_core::Terminal;
    use phosphor_core::export;

    use super::*;

    #[test]
    fn selects_a_run_across_lines() {
        let mut t = Terminal::new(10, 3, 0);
        t.feed(b"first line\r\nsecond\r\nthird");
        let rows: Vec<&[Cell]> = (0..3).map(|y| t.screen.row(y)).collect();

        // Dragged backwards, from "th" of "third" up to "line".
        let s = Selection { anchor: (1, 2), head: (6, 0) };
        assert!(s.contains(6, 0) && s.contains(0, 1) && s.contains(9, 1) && s.contains(1, 2));
        assert!(!s.contains(5, 0) && !s.contains(2, 2));
        assert_eq!(export::text(&s.lines(&rows)), "line\nsecond\nth\n");

        assert!(Selection::at((3, 1)).is_empty());
        let one = Selection { anchor: (0, 1), head: (2, 1) };
        assert_eq!(export::text(&one.lines(&rows)), "sec\n");
    }
}
