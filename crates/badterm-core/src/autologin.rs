//! Logging in with the entry's stored user name and password: answering the
//! BBS's prompts by itself, or one step at a time on the user's request
//! (Alt-L).
//!
//! A prompt is the text left of the cursor on its line once the BBS stops
//! sending. "Login:", "User name:", "Handle?", Synchronet's "NN:" and the
//! like get the user name; "Password:" (or "PW:") gets the password, but
//! only after the user name, so a new-user form asking for a password is
//! never answered. Each is sent once, and only in the first few minutes of a
//! call.

use std::time::{Duration, Instant};

use crate::dialer::{Entry, Protocol};

/// How long after connecting prompts are still answered.
const WINDOW: Duration = Duration::from_secs(120);

/// What to send next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    User(String),
    Password(String),
}

#[derive(Debug)]
pub struct AutoLogin {
    user: String,
    password: String,
    /// Answer prompts without being asked.
    automatic: bool,
    sent_user: bool,
    sent_password: bool,
    until: Option<Instant>,
}

impl AutoLogin {
    pub fn new(entry: &Entry) -> AutoLogin {
        AutoLogin {
            user: entry.username.clone(),
            password: entry.password.clone(),
            // SSH and RLogin log in as part of the protocol.
            automatic: entry.auto_login && (entry.protocol.is_telnet() || entry.protocol == Protocol::Raw),
            sent_user: false,
            sent_password: false,
            until: None,
        }
    }

    /// The call is connected: start the time window.
    pub fn connected(&mut self) {
        self.until = Some(Instant::now() + WINDOW);
    }

    /// The answer for `prompt`, if it is a login prompt this login answers.
    pub fn answer(&mut self, prompt: &str) -> Option<Step> {
        if !self.automatic || self.until.is_none_or(|t| Instant::now() > t) {
            return None;
        }
        if !self.sent_user && !self.user.is_empty() && is_user_prompt(prompt) {
            return self.next_step();
        }
        if self.sent_user && !self.sent_password && !self.password.is_empty() && is_password_prompt(prompt) {
            return self.next_step();
        }
        None
    }

    /// The next step, whatever is on the screen: Alt-L. `None` once both
    /// have been sent (or there is nothing stored).
    pub fn next_step(&mut self) -> Option<Step> {
        if !self.sent_user && !self.user.is_empty() {
            self.sent_user = true;
            return Some(Step::User(self.user.clone()));
        }
        self.sent_user = true;
        if !self.sent_password && !self.password.is_empty() {
            self.sent_password = true;
            return Some(Step::Password(self.password.clone()));
        }
        None
    }

    /// Whether there is anything stored to log in with.
    pub fn has_login(&self) -> bool {
        !self.user.is_empty() || !self.password.is_empty()
    }
}

/// The prompt, lower-cased and without the colon (or question mark, or
/// angle bracket) it ends with; `None` if it does not end like a prompt.
fn prompt_text(prompt: &str) -> Option<String> {
    let p = prompt.trim_end();
    let p = p.strip_suffix([':', '?', '>'])?;
    Some(p.trim().to_ascii_lowercase())
}

fn is_user_prompt(prompt: &str) -> bool {
    let Some(p) = prompt_text(prompt) else { return false };
    if p.contains("password") || p.contains("pass word") {
        return false;
    }
    // The last words matter: "Enter your user name or NEW", "Login",
    // "Handle", "NN" (Synchronet: name or number).
    let words = ["login", "user", "username", "name", "handle", "alias", "nn", "logon", "callsign"];
    let last: Vec<&str> = p.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let tail = &last[last.len().saturating_sub(4)..];
    tail.iter().any(|w| words.contains(w)) && !tail.contains(&"real") && !tail.contains(&"first")
}

fn is_password_prompt(prompt: &str) -> bool {
    let Some(p) = prompt_text(prompt) else { return false };
    p.ends_with("password") || p.ends_with("pw") || p.ends_with("passwd") || p.ends_with("pass")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(user: &str, password: &str) -> AutoLogin {
        let mut a = AutoLogin::new(&Entry { username: user.into(), password: password.into(), ..Entry::default() });
        a.connected();
        a
    }

    #[test]
    fn prompts() {
        for p in ["Login: ", "Username:", "Enter your user name or 'NEW':", "Handle? ", "NN: ", "Alias>", "logon:"] {
            assert!(is_user_prompt(p), "{p}");
        }
        for p in ["Password: ", "PW: ", "Enter your password:", "passwd:"] {
            assert!(is_password_prompt(p), "{p}");
            assert!(!is_user_prompt(p), "{p}");
        }
        for p in ["Press any key", "Login", "Your real name:", "First name?", "Choose a password for your new user:"] {
            assert!(!is_user_prompt(p), "{p}");
        }
        assert!(!is_password_prompt("Choose a password for your new user name:"));
    }

    #[test]
    fn answers_in_order_once() {
        let mut a = login("sysop", "secret");
        // A password prompt before the user name is not answered.
        assert_eq!(a.answer("Password:"), None);
        assert_eq!(a.answer("Login: "), Some(Step::User("sysop".into())));
        assert_eq!(a.answer("Login: "), None);
        assert_eq!(a.answer("Password: "), Some(Step::Password("secret".into())));
        assert_eq!(a.answer("Password: "), None);
        assert_eq!(a.next_step(), None);
    }

    #[test]
    fn manual_steps_and_settings() {
        let mut a = login("sysop", "secret");
        assert_eq!(a.next_step(), Some(Step::User("sysop".into())));
        assert_eq!(a.next_step(), Some(Step::Password("secret".into())));
        assert_eq!(a.next_step(), None);

        // Not before connecting, not when turned off, not over SSH.
        let mut a = AutoLogin::new(&Entry { username: "u".into(), ..Entry::default() });
        assert_eq!(a.answer("Login:"), None);
        let off = Entry { username: "u".into(), auto_login: false, ..Entry::default() };
        let mut a = AutoLogin::new(&off);
        a.connected();
        assert_eq!(a.answer("Login:"), None);
        let ssh = Entry { username: "u".into(), protocol: Protocol::Ssh, ..Entry::default() };
        let mut a = AutoLogin::new(&ssh);
        a.connected();
        assert_eq!(a.answer("Login:"), None);
        assert!(a.has_login());
    }
}
