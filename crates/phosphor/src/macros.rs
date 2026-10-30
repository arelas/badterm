//! Keyboard macros (Alt-1 to Alt-0): text sent to the BBS at a keypress,
//! written as in Telix and QModem. `^M` is Enter (a CR), `^` before a
//! letter or `[ \ ] ^ _` is that control key (`^[` is Esc), `~` waits half
//! a second, and `{user}` and `{password}` are the directory entry's. `^^`
//! and `^~` send a `^` or `~` themselves.

use std::time::Duration;

/// How long a `~` waits.
pub const PAUSE: Duration = Duration::from_millis(500);

/// How many macros there are (Alt-1 to Alt-9, then Alt-0).
pub const COUNT: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Send(String),
    Pause,
}

/// Turn a macro into what to send, and when.
pub fn expand(text: &str, user: &str, password: &str) -> Vec<Step> {
    let text = text.replace("{user}", user).replace("{password}", password);
    let mut steps = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '~' => {
                if !cur.is_empty() {
                    steps.push(Step::Send(std::mem::take(&mut cur)));
                }
                steps.push(Step::Pause);
            }
            '^' => match chars.peek().copied() {
                Some(n @ ('^' | '~')) => {
                    chars.next();
                    cur.push(n);
                }
                Some(n) if n.is_ascii_alphabetic() || "@[\\]_".contains(n) => {
                    chars.next();
                    cur.push(char::from(n.to_ascii_uppercase() as u8 & 0x1F));
                }
                _ => cur.push('^'),
            },
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        steps.push(Step::Send(cur));
    }
    steps
}

/// The key that sends macro `i` (0-based): 1-9, then 0.
pub fn key_name(i: usize) -> char {
    if i == 9 { '0' } else { char::from(b'1' + i as u8) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands() {
        assert_eq!(
            expand("{user}^M~{password}^M", "sysop", "secret"),
            vec![Step::Send("sysop\r".into()), Step::Pause, Step::Send("secret\r".into())]
        );
        assert_eq!(expand("^[[A^^x^~y", "", ""), vec![Step::Send("\x1b[A^x~y".into())]);
        assert_eq!(expand("100^%", "", ""), vec![Step::Send("100^%".into())]);
        assert_eq!(expand("~~", "", ""), vec![Step::Pause, Step::Pause]);
        assert!(expand("", "", "").is_empty());
        assert_eq!((key_name(0), key_name(9)), ('1', '0'));
    }
}
