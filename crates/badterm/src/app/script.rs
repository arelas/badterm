//! Scripts (see [`badterm_core::script`]): starting one, stopping it, and
//! doing what it asks.

use std::path::Path;

use badterm_core::script::{CallInfo, FromScript, Script};

use super::*;

impl App {
    /// A script's file: a name or relative path is in the scripts folder.
    pub(super) fn script_path(&self, name: &str) -> PathBuf {
        let p = ui::expand_home(name.trim());
        if p.is_absolute() { p } else { self.paths.scripts.join(p) }
    }

    pub(super) fn run_script(&mut self, path: &Path) {
        let Some(session) = self.session.as_mut().filter(|s| s.state == State::Connected) else {
            return self.notify("Not connected");
        };
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => return self.notify(format!("Script {}: {e}", path.display())),
        };
        let name = path.file_name().map_or_else(|| "script".into(), |n| n.to_string_lossy().into_owned());
        let e = &session.entry;
        let info = CallInfo { bbs: e.name.clone(), user: e.username.clone(), password: e.password.clone() };
        session.tap = Some(Vec::new());
        let proxy = self.proxy.clone();
        self.script = Some(Script::start(&name, source, info, move || {
            // Fails only once the event loop has exited.
            let _ = proxy.send_event(UserEvent::Script);
        }));
        self.notify(format!("Running {name} ({} stops it)", crate::hotkeys::label("J")));
    }

    /// Stop the script running, if any, saying why.
    pub(super) fn stop_script(&mut self, why: Option<&str>) {
        let Some(script) = self.script.take() else { return };
        script.stop();
        if let Some(s) = self.session.as_mut() {
            s.tap = None;
        }
        if let Some(why) = why {
            self.notify(format!("{}: {why}", script.name));
        }
    }

    /// Pass the script what the BBS sent.
    pub(super) fn feed_script(&mut self) {
        if let (Some(script), Some(session)) = (&self.script, self.session.as_mut())
            && let Some(tap) = session.tap.as_mut()
            && !tap.is_empty()
        {
            script.feed(&std::mem::take(tap));
        }
    }

    /// Do what the script asks.
    pub(super) fn poll_script(&mut self) {
        let Some(script) = self.script.as_mut() else { return };
        for m in script.poll() {
            match m {
                FromScript::Send(text) => {
                    if let Some(s) = self.session.as_mut().filter(|s| s.state == State::Connected) {
                        let bytes = s.term.encode_text(&text);
                        s.send(&bytes);
                    }
                }
                FromScript::Notify(msg) => self.notify(msg),
                FromScript::WantScreen => {
                    if let (Some(script), Some(s)) = (&self.script, &self.session) {
                        let sc = &s.term.screen;
                        let rows: Vec<String> =
                            (0..sc.rows()).map(|y| Screen::row_text(sc.row(y)).trim_end().to_string()).collect();
                        script.screen(rows.join("\n"));
                    }
                }
                FromScript::HangUp => self.hang_up(),
                FromScript::Done(result) => {
                    let name = self.script.as_ref().map_or_else(String::new, |s| s.name.clone());
                    self.script = None;
                    if let Some(s) = self.session.as_mut() {
                        s.tap = None;
                    }
                    match result {
                        Ok(()) => self.notify(format!("{name} finished")),
                        Err(e) => self.notify(format!("{name}: {e}")),
                    }
                    break;
                }
            }
            if self.script.is_none() {
                break;
            }
        }
        self.session_events();
        self.request_redraw();
    }

    /// The script picker, for Alt-J.
    pub(super) fn open_script_picker(&mut self) {
        let _ = std::fs::create_dir_all(&self.paths.scripts);
        self.art_picker = Some(FilePicker::new(self.paths.scripts.clone(), false, &["rhai"]));
        self.picking_script = true;
    }
}
