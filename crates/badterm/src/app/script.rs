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

    /// Run a script in tab `t`'s call.
    pub(super) fn run_script(&mut self, t: usize, path: &Path) {
        let Some(tab) = self.tabs.get_mut(t).filter(|tab| tab.session.state == State::Connected) else {
            return self.notify("Not connected");
        };
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => return self.notify_from(t, format!("Script {}: {e}", path.display())),
        };
        let name = path.file_name().map_or_else(|| "script".into(), |n| n.to_string_lossy().into_owned());
        let e = &tab.session.entry;
        let info = CallInfo { bbs: e.name.clone(), user: e.username.clone(), password: e.password.clone() };
        tab.session.tap = Some(Vec::new());
        let (proxy, id) = (self.proxy.clone(), tab.id);
        tab.script = Some(Script::start(&name, source, info, move || {
            // Fails only once the event loop has exited.
            let _ = proxy.send_event(UserEvent::Script(id));
        }));
        self.notify_from(t, format!("Running {name} ({} stops it)", crate::hotkeys::label("J")));
    }

    /// Stop tab `t`'s script, if it has one running, saying why.
    pub(super) fn stop_script(&mut self, t: usize, why: Option<&str>) {
        let Some(tab) = self.tabs.get_mut(t) else { return };
        let Some(script) = tab.script.take() else { return };
        script.stop();
        tab.session.tap = None;
        if let Some(why) = why {
            self.notify_from(t, format!("{}: {why}", script.name));
        }
    }

    /// Pass tab `t`'s script what its BBS sent.
    pub(super) fn feed_script(&mut self, t: usize) {
        if let Some(tab) = self.tabs.get_mut(t)
            && let Some(script) = &tab.script
            && let Some(tap) = tab.session.tap.as_mut()
            && !tap.is_empty()
        {
            script.feed(&std::mem::take(tap));
        }
    }

    /// Do what the script of the call with this id asks.
    pub(super) fn poll_script(&mut self, id: u64) {
        let Some(t) = self.tab_of(id) else { return };
        let Some(messages) = self.tabs[t].script.as_mut().map(|s| s.poll()) else { return };
        for m in messages {
            let tab = &mut self.tabs[t];
            match m {
                FromScript::Send(text) => {
                    if tab.session.state == State::Connected {
                        let bytes = tab.session.term.encode_text(&text);
                        tab.session.send(&bytes);
                    }
                }
                FromScript::Notify(msg) => self.notify_from(t, msg),
                FromScript::WantScreen => {
                    if let Some(script) = &tab.script {
                        let sc = &tab.session.term.screen;
                        let rows: Vec<String> =
                            (0..sc.rows()).map(|y| Screen::row_text(sc.row(y)).trim_end().to_string()).collect();
                        script.screen(rows.join("\n"));
                    }
                }
                // The tab goes with its call (as Alt-H): nothing more to do.
                FromScript::HangUp => {
                    self.close_tab(t);
                    return self.request_redraw();
                }
                FromScript::Done(result) => {
                    let name = tab.script.take().map_or_else(String::new, |s| s.name.clone());
                    tab.session.tap = None;
                    match result {
                        Ok(()) => self.notify_from(t, format!("{name} finished")),
                        Err(e) => self.notify_from(t, format!("{name}: {e}")),
                    }
                    break;
                }
            }
            if self.tabs[t].script.is_none() {
                break;
            }
        }
        self.session_events(t);
        self.request_redraw();
    }

    /// The script picker, for Alt-J.
    pub(super) fn open_script_picker(&mut self) {
        let _ = std::fs::create_dir_all(&self.paths.scripts);
        self.art_picker = Some(FilePicker::new(self.paths.scripts.clone(), false, &["rhai"]));
        self.picking_script = true;
    }
}
