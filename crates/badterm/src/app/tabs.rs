//! Tabs: several calls open at once, one of them on screen. A tab is a call
//! and everything that belongs to it (its prompts, its place in the
//! scrollback, its redial, macro and script); the rest of the app (the
//! dialer, settings, the viewer, the window) is shared.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use badterm_core::script::Script;
use badterm_core::session::Session;
use badterm_core::transfer::Direction;

use super::Redial;
use crate::ui::login::LoginPrompt;
use crate::ui::save::SavePrompt;
use crate::ui::transfer::{DownloadPrompt, UploadPrompt};

/// At most this many calls at once: the status bar numbers them 1 to 9.
pub(super) const MAX_TABS: usize = 9;

/// A tab's chip in the status bar is this wide: " 2*".
pub(super) const CHIP_W: usize = 3;

pub(super) struct Tab {
    pub(super) session: Session,
    /// The id its network events (and its script's) come with.
    pub(super) id: u64,
    /// Directory index of the entry it was dialed from.
    pub(super) index: Option<usize>,
    /// The window's title while it is on screen.
    pub(super) title: String,
    /// The SSH login's (or a RIP) question, while one is waiting.
    pub(super) login: Option<LoginPrompt>,
    /// A typed password to remember once the login succeeds.
    pub(super) remember: Option<String>,
    /// Direction of the transfer in progress, for reporting its outcome.
    pub(super) transfer_direction: Option<Direction>,
    pub(super) upload_prompt: Option<UploadPrompt>,
    /// Alt-G: starting an XMODEM or YMODEM download.
    pub(super) download_prompt: Option<DownloadPrompt>,
    /// Alt-S: where to save the scrollback.
    pub(super) save_prompt: Option<SavePrompt>,
    /// Where its last upload came from; empty for the BBS's own folder.
    pub(super) upload_dir: PathBuf,
    /// A keyboard macro being sent: what is left, and when to go on after
    /// a pause.
    pub(super) macro_steps: VecDeque<crate::macros::Step>,
    pub(super) macro_at: Instant,
    /// How many times this call has been redialed.
    pub(super) redials: u32,
    pub(super) redial: Option<Redial>,
    /// Lines scrolled back from the live screen; 0 when live.
    pub(super) scroll: usize,
    pub(super) script: Option<Script>,
    /// Off screen, the BBS has sent something since the tab was last shown.
    pub(super) unseen: bool,
    /// Off screen, it rang, asked something, finished a transfer or hung
    /// up.
    pub(super) attention: bool,
}

impl Tab {
    pub(super) fn new(session: Session, id: u64, index: Option<usize>) -> Tab {
        let title = format!("BadTerm \u{2014} {}", session.entry.name);
        Tab {
            session,
            id,
            index,
            title,
            login: None,
            remember: None,
            transfer_direction: None,
            upload_prompt: None,
            download_prompt: None,
            save_prompt: None,
            upload_dir: PathBuf::new(),
            macro_steps: VecDeque::new(),
            macro_at: Instant::now(),
            redials: 0,
            redial: None,
            scroll: 0,
            script: None,
            unseen: false,
            attention: false,
        }
    }

    /// What its chip says after its number: `!` wanting the user, `*` with
    /// something new to read.
    pub(super) fn mark(&self) -> char {
        mark(self.unseen, self.attention)
    }
}

fn mark(unseen: bool, attention: bool) -> char {
    if attention {
        '!'
    } else if unseen {
        '*'
    } else {
        ' '
    }
}

/// The tab to show after closing tab `closed` (of `len` before it closed)
/// while `active` was on screen: the same call if another one closed, and
/// otherwise the one that takes its place (the one before it, if it was
/// the last).
pub(super) fn after_close(active: usize, closed: usize, len: usize) -> usize {
    let next = if closed < active { active - 1 } else { active };
    // The tabs left are numbered from 0 to len - 2.
    next.min(len.saturating_sub(2))
}

/// The next tab along (`back`: the one before), going round.
pub(super) fn cycle(active: usize, len: usize, back: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if back { (active + len - 1) % len } else { (active + 1) % len }
}

/// The chips at the start of the status bar, one per tab: its number and
/// its mark, e.g. `[" 1 ", " 2*", " 3!"]`.
pub(super) fn chips(marks: &[char]) -> Vec<String> {
    marks.iter().enumerate().map(|(i, m)| format!(" {}{m}", i + 1)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_a_tab_shows_its_neighbour() {
        // The one on screen closes: the next takes its place...
        assert_eq!(after_close(1, 1, 3), 1);
        // ...or, if it was last, the one before it.
        assert_eq!(after_close(2, 2, 3), 1);
        assert_eq!(after_close(0, 0, 2), 0);
        // Another closes: the same call stays, wherever it moved to.
        assert_eq!(after_close(2, 0, 3), 1);
        assert_eq!(after_close(0, 2, 3), 0);
        // The last one: nothing left (the caller shows the idle screen).
        assert_eq!(after_close(0, 0, 1), 0);
    }

    #[test]
    fn switching_goes_round() {
        assert_eq!(cycle(0, 3, false), 1);
        assert_eq!(cycle(2, 3, false), 0);
        assert_eq!(cycle(0, 3, true), 2);
        assert_eq!(cycle(1, 3, true), 0);
        assert_eq!(cycle(0, 1, false), 0);
        assert_eq!(cycle(0, 0, false), 0);
    }

    #[test]
    fn chips_say_what_is_waiting() {
        let marks = [mark(false, false), mark(true, false), mark(true, true)];
        assert_eq!(chips(&marks), [" 1 ", " 2*", " 3!"]);
        // Every chip is the same width, so a click finds its tab; nine
        // (the most) leave the status bar most of its 80 columns.
        let all = chips(&[' '; MAX_TABS]);
        assert!(all.iter().all(|c| c.chars().count() == CHIP_W));
        assert!(all.concat().chars().count() < 28);
    }
}
