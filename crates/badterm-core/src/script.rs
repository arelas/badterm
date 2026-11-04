//! Scripts: small programs, in [Rhai](https://rhai.rs), that log in to a
//! BBS or find their way around it, as Telix's SALT and Qodem's scripts did.
//!
//! A script runs on its own thread. It reads what the BBS sends (as text,
//! without the ANSI codes) and asks the front end to send, show messages
//! or hang up through channels; the front end wakes up for its requests.
//!
//! ```text
//! if wait("Login:", 20) {          // seconds; false if it never came
//!     send(user() + "\r");
//!     wait("Password:");
//!     send(password() + "\r");
//! }
//! let which = wait_any(["[Y/n]", "Main Menu"], 30);
//! if which == 0 { send("n\r"); }
//! sleep(0.5);
//! notify("Logged in to " + bbs());
//! ```
//!
//! Besides those: `screen()` is the text on the screen, one line per row;
//! `hangup()` ends the call; `print(x)` shows `x` on the status line.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use rhai::{Array, Dynamic, Engine, EvalAltResult};

/// How long `wait` waits when no time is given.
const WAIT: f64 = 30.0;
/// The most text kept for `wait` to search.
const KEEP: usize = 16 * 1024;

/// From the front end to the script.
pub enum ToScript {
    /// What the BBS sent.
    Data(Vec<u8>),
    /// The screen's text, asked for with [`FromScript::WantScreen`].
    Screen(String),
}

/// What a script asks of the front end.
#[derive(Debug, PartialEq, Eq)]
pub enum FromScript {
    /// Send this text (in the terminal's encoding).
    Send(String),
    Notify(String),
    /// Answer with [`Script::screen`].
    WantScreen,
    HangUp,
    /// It finished, or failed with this message.
    Done(Result<(), String>),
}

/// About the call, for `user()`, `password()` and `bbs()`.
#[derive(Clone, Debug, Default)]
pub struct CallInfo {
    pub bbs: String,
    pub user: String,
    pub password: String,
}

/// A script running.
pub struct Script {
    pub name: String,
    tx: Sender<ToScript>,
    rx: Receiver<FromScript>,
    stop: Arc<AtomicBool>,
    done: bool,
}

impl Script {
    /// Start `source`; `wake` is called (from the script's thread) when it
    /// has something for [`Script::poll`].
    pub fn start(name: &str, source: String, info: CallInfo, wake: impl Fn() + Send + 'static) -> Script {
        let (tx, script_rx) = mpsc::channel();
        let (script_tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let host = Host {
            rx: script_rx,
            tx: script_tx,
            wake: Box::new(wake),
            stop: stop.clone(),
            text: String::new(),
            esc: Esc::None,
            info,
        };
        std::thread::Builder::new().name("script".into()).spawn(move || run(source, host)).expect("starting a thread");
        Script { name: name.to_string(), tx, rx, stop, done: false }
    }

    /// What the BBS sent.
    pub fn feed(&self, data: &[u8]) {
        if !data.is_empty() {
            let _ = self.tx.send(ToScript::Data(data.to_vec()));
        }
    }

    pub fn screen(&self, text: String) {
        let _ = self.tx.send(ToScript::Screen(text));
    }

    /// Stop it at its next step.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// What it asks for now.
    pub fn poll(&mut self) -> Vec<FromScript> {
        let out: Vec<FromScript> = self.rx.try_iter().collect();
        if out.iter().any(|m| matches!(m, FromScript::Done(_))) {
            self.done = true;
        }
        out
    }

    pub fn is_done(&self) -> bool {
        self.done
    }
}

impl Drop for Script {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Where an escape sequence being skipped is.
#[derive(Clone, Copy)]
enum Esc {
    None,
    /// After ESC.
    Start,
    /// In a CSI's parameters.
    Csi,
    /// In an OSC, APC or DCS string, until ST or BEL.
    Str,
    StrEnd,
}

struct Host {
    rx: Receiver<ToScript>,
    tx: Sender<FromScript>,
    wake: Box<dyn Fn() + Send>,
    stop: Arc<AtomicBool>,
    /// What arrived since the last match.
    text: String,
    esc: Esc,
    info: CallInfo,
}

type Res<T> = Result<T, Box<EvalAltResult>>;

impl Host {
    fn ask(&self, m: FromScript) {
        let _ = self.tx.send(m);
        (self.wake)();
    }

    fn stopped(&self) -> Res<()> {
        if self.stop.load(Ordering::Relaxed) { Err("stopped".into()) } else { Ok(()) }
    }

    /// Take what the BBS sent as text: the ANSI codes and most control
    /// characters go.
    fn take(&mut self, data: &[u8]) {
        for &b in data {
            self.esc = match (self.esc, b) {
                (Esc::None, 0x1B) => Esc::Start,
                (Esc::None, b'\r' | b'\n' | b'\t') => {
                    self.text.push(b as char);
                    Esc::None
                }
                (Esc::None, 0..=0x1F | 0x7F) => Esc::None,
                (Esc::None, _) => {
                    self.text.push(crate::cp437::to_unicode(b));
                    Esc::None
                }
                (Esc::Start, b'[') => Esc::Csi,
                (Esc::Start, b']' | b'_' | b'P' | b'^') => Esc::Str,
                (Esc::Start, _) => Esc::None,
                (Esc::Csi, 0x40..=0x7E) => Esc::None,
                (Esc::Csi, _) => Esc::Csi,
                (Esc::Str, 0x07) => Esc::None,
                (Esc::Str, 0x1B) => Esc::StrEnd,
                (Esc::Str, _) => Esc::Str,
                (Esc::StrEnd, b'\\') => Esc::None,
                (Esc::StrEnd, _) => Esc::Str,
            };
        }
        if self.text.len() > KEEP {
            let mut cut = self.text.len() - KEEP / 2;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text.drain(..cut);
        }
    }

    /// The next message, waiting up to `until`; data is taken in as it
    /// comes. None at the deadline.
    fn next(&mut self, until: Instant) -> Res<Option<String>> {
        loop {
            self.stopped()?;
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            match self.rx.recv_timeout(left.min(Duration::from_millis(100))) {
                Ok(ToScript::Data(d)) => {
                    self.take(&d);
                    return Ok(Some(String::new()));
                }
                Ok(ToScript::Screen(s)) => return Ok(Some(s)),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err("the call ended".into()),
            }
        }
    }

    /// Wait for the first of `texts` to arrive: its index, or -1.
    fn wait_any(&mut self, texts: &[String], secs: f64) -> Res<i64> {
        let until = Instant::now() + Duration::from_secs_f64(secs.clamp(0.0, 86_400.0));
        loop {
            let found = texts
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.is_empty())
                .filter_map(|(i, t)| self.text.find(t.as_str()).map(|at| (at, i, at + t.len())))
                .min();
            if let Some((_, i, end)) = found {
                self.text.drain(..end);
                return Ok(i as i64);
            }
            if self.next(until)?.is_none() {
                return Ok(-1);
            }
        }
    }

    fn screen(&mut self) -> Res<String> {
        self.ask(FromScript::WantScreen);
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match self.next(until)? {
                None => return Ok(String::new()),
                Some(s) if !s.is_empty() => return Ok(s),
                Some(_) => {}
            }
        }
    }

    fn sleep(&mut self, secs: f64) -> Res<()> {
        let until = Instant::now() + Duration::from_secs_f64(secs.clamp(0.0, 86_400.0));
        while self.next(until)?.is_some() {}
        Ok(())
    }
}

fn run(source: String, host: Host) {
    let tx = host.tx.clone();
    let host = Rc::new(RefCell::new(host));
    let result = engine(&host).run(&source).map_err(|e| match *e {
        EvalAltResult::ErrorTerminated(..) => "stopped".to_string(),
        EvalAltResult::ErrorRuntime(ref v, _) if v.to_string() == "stopped" => "stopped".to_string(),
        _ => e.to_string(),
    });
    let result = match result {
        Err(e) if e == "stopped" => Ok(()),
        other => other,
    };
    let h = host.borrow();
    let _ = tx.send(FromScript::Done(result));
    (h.wake)();
}

fn engine(host: &Rc<RefCell<Host>>) -> Engine {
    let mut e = Engine::new();
    // A script that loops with no waits still stops when asked.
    let h = host.clone();
    e.on_progress(move |_| h.borrow().stop.load(Ordering::Relaxed).then_some(Dynamic::UNIT));
    let h = host.clone();
    e.on_print(move |s| h.borrow().ask(FromScript::Notify(s.to_string())));
    let h = host.clone();
    e.register_fn("send", move |s: &str| -> Res<()> {
        let h = h.borrow();
        h.stopped()?;
        h.ask(FromScript::Send(s.to_string()));
        Ok(())
    });
    let h = host.clone();
    e.register_fn("notify", move |s: &str| h.borrow().ask(FromScript::Notify(s.to_string())));
    let h = host.clone();
    e.register_fn("hangup", move || h.borrow().ask(FromScript::HangUp));
    let h = host.clone();
    e.register_fn("wait", move |s: &str, secs: f64| -> Res<bool> {
        Ok(h.borrow_mut().wait_any(&[s.to_string()], secs)? == 0)
    });
    let h = host.clone();
    e.register_fn("wait", move |s: &str, secs: i64| -> Res<bool> {
        Ok(h.borrow_mut().wait_any(&[s.to_string()], secs as f64)? == 0)
    });
    let h = host.clone();
    e.register_fn("wait", move |s: &str| -> Res<bool> { Ok(h.borrow_mut().wait_any(&[s.to_string()], WAIT)? == 0) });
    let texts = |a: Array| a.into_iter().map(|d| d.to_string()).collect::<Vec<_>>();
    let h = host.clone();
    e.register_fn("wait_any", move |a: Array, secs: f64| -> Res<i64> { h.borrow_mut().wait_any(&texts(a), secs) });
    let h = host.clone();
    e.register_fn("wait_any", move |a: Array, secs: i64| -> Res<i64> {
        h.borrow_mut().wait_any(&texts(a), secs as f64)
    });
    let h = host.clone();
    e.register_fn("wait_any", move |a: Array| -> Res<i64> { h.borrow_mut().wait_any(&texts(a), WAIT) });
    let h = host.clone();
    e.register_fn("sleep", move |secs: f64| -> Res<()> { h.borrow_mut().sleep(secs) });
    let h = host.clone();
    e.register_fn("sleep", move |secs: i64| -> Res<()> { h.borrow_mut().sleep(secs as f64) });
    let h = host.clone();
    e.register_fn("screen", move || -> Res<String> { h.borrow_mut().screen() });
    let h = host.clone();
    e.register_fn("user", move || h.borrow().info.user.clone());
    let h = host.clone();
    e.register_fn("password", move || h.borrow().info.password.clone());
    let h = host.clone();
    e.register_fn("bbs", move || h.borrow().info.bbs.clone());
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run a script against a pretend BBS: each time the script sends
    /// something, the reply for it (if any) comes back.
    fn talk(source: &str, greeting: &[u8], replies: &[(&str, &[u8])]) -> (Vec<FromScript>, Result<(), String>) {
        let info = CallInfo { bbs: "Test BBS".into(), user: "sysop".into(), password: "secret".into() };
        let mut s = Script::start("test", source.to_string(), info, || {});
        s.feed(greeting);
        let mut seen = Vec::new();
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            for m in s.poll() {
                match m {
                    FromScript::Done(r) => return (seen, r),
                    FromScript::Send(ref t) => {
                        if let Some((_, r)) = replies.iter().find(|(k, _)| k == t) {
                            s.feed(r);
                        }
                        seen.push(m);
                    }
                    FromScript::WantScreen => s.screen("Row one\nRow two".into()),
                    m => seen.push(m),
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the script did not finish");
    }

    #[test]
    fn logs_in_through_ansi() {
        let script = r#"
            if wait("Login:", 5) { send(user() + "\r"); }
            wait("Password:");
            send(password() + "\r");
            let i = wait_any(["Bad password", "Main Menu"], 5);
            if i == 1 { notify("in " + bbs()); }
            if !wait("never", 0.2) { print("timed out"); }
            if screen().contains("Row two") { send("ok"); }
        "#;
        let (seen, done) = talk(
            script,
            b"\x1b[2J\x1b[1;33mWelcome\r\nLogin: ",
            &[("sysop\r", b"\x1b[0mPass\x1b[1mword: "), ("secret\r", b"\x1b]0;title\x07\r\n\x1b[32mMain Menu\r\n")],
        );
        assert_eq!(done, Ok(()));
        assert_eq!(
            seen,
            [
                FromScript::Send("sysop\r".into()),
                FromScript::Send("secret\r".into()),
                FromScript::Notify("in Test BBS".into()),
                FromScript::Notify("timed out".into()),
                FromScript::Send("ok".into()),
            ]
        );
    }

    #[test]
    fn the_example_login_script() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extras/scripts/login.rhai");
        let Ok(source) = std::fs::read_to_string(path) else { return };
        let (seen, done) = talk(
            &source,
            b"Welcome! Press any key to continue\r\n",
            &[
                ("\r", b"Login: "),
                ("sysop\r", b"Password: "),
                ("secret\r", b"Read new mail? [Y/n] "),
                ("n", b"\x1b[1mMain Menu\x1b[0m\r\nCommand: "),
            ],
        );
        assert_eq!(done, Ok(()));
        assert_eq!(seen.last(), Some(&FromScript::Notify("Logged in to Test BBS".into())));
        assert_eq!(seen.len(), 5, "{seen:?}");
    }

    #[test]
    fn errors_and_stopping() {
        let (_, done) = talk("let x = ;", b"", &[]);
        assert!(done.is_err());
        let (_, done) = talk("wait(\"x\", 0.1); hangup(); throw \"no login\";", b"", &[]);
        assert!(done.unwrap_err().contains("no login"));
        // A loop that never waits is stopped too.
        let mut s = Script::start("loop", "loop { }".into(), CallInfo::default(), || {});
        std::thread::sleep(Duration::from_millis(50));
        s.stop();
        let until = Instant::now() + Duration::from_secs(5);
        while !s.is_done() && Instant::now() < until {
            s.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(s.is_done());
    }
}
