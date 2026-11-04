// BadTerm - a cross-platform ANSI-BBS telnet client.
// Copyright (C) 2026 Bad Cluster
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version. See the LICENSE file for details.

// A GUI program on Windows, so no console window opens alongside it; see
// the console module for how command-line use still gets output.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod bezel;
mod console;
mod crt;
mod input;
#[cfg(target_os = "macos")]
mod macos_urls;
mod macros;
mod png;
mod render;
mod secrets;
mod selection;
mod settings;
mod sound;
mod ui;
mod viewer;

use std::path::PathBuf;
use std::process::ExitCode;

use badterm_core::dialer::{self, Directory, Entry};
use winit::event_loop::EventLoop;

const USAGE: &str = "\
Usage: badterm [OPTIONS] [ADDRESS | FILE]

Connect to ADDRESS (host, host:port, telnet://host:port, raw://host:port or
ssh://user@host:port). With no address, BadTerm starts unconnected; Alt-D
(Cmd-D on macOS) opens the dialing directory. A FILE (ANSI art) opens in
the viewer.

Options:
  --import FILE   Add the entries in FILE to the dialing directory and exit
                  (SyncTERM .lst, NetRunner .phn, Qodem fonebook.txt, magiterm.ini,
                  EtherTerm .xml, .csv such as the Telnet BBS Guide's, .json or .toml)
  --export FILE   Write the dialing directory to FILE and exit; the format
                  follows the extension (.lst, .csv, .json or .toml)
  --paths         Print where the directory, captures and downloads are
                  stored and exit
  --render IN OUT Draw the ANSI file IN to the PNG image OUT and exit. With:
    --screen        only the final 80x25 screen (for a captured session),
                    not the whole file
    --crt MODE      phosphor mode: color, green or amber
    --scanlines S   their strength: off, light, medium or strong
    --glow G        how much bright text glows: off, light, medium or strong
    --curvature C   the glass: off, slight, medium or strong
    --nine-dot      9-dot characters
    --aspect        4:3, as on a CRT
    --font NAME     topaz, topaz-plus, microknight, microknight-plus,
                    p0t-noodle or mosoul (Amiga art)
    --bezel NAME    draw a monitor bezel around it (as in the settings,
                    e.g. 'Beige, 1980s', or one of your own)
  -h, --help      Show this help
  -V, --version   Show the version";

enum Command {
    /// Start, calling an address or showing a file.
    Run(Option<Entry>, Option<PathBuf>),
    Import(PathBuf),
    Export(PathBuf),
    Paths,
    Render(PathBuf, PathBuf, viewer::RenderOptions),
}

fn parse_args() -> Result<Command, String> {
    let mut args = std::env::args().skip(1);
    let mut cmd = Command::Run(None, None);
    // The --render options may come before or after it.
    let mut render = viewer::RenderOptions::default();
    let mut render_files = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("badterm {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--import" => cmd = Command::Import(args.next().ok_or("--import needs a file")?.into()),
            "--export" => cmd = Command::Export(args.next().ok_or("--export needs a file")?.into()),
            "--paths" => cmd = Command::Paths,
            "--render" => {
                let input = args.next().ok_or("--render needs an input and an output file")?;
                let output = args.next().ok_or("--render needs an output file")?;
                render_files = Some((input.into(), output.into()));
            }
            "--screen" => render.screen_only = true,
            "--nine-dot" => render.nine_dot = true,
            "--aspect" => render.aspect = true,
            "--bezel" => {
                let name = args.next().ok_or("--bezel needs a bezel name")?;
                let dir = app::Paths::new().bezels;
                let bezel = bezel::Bezel::find(&name, &dir)
                    .map_err(|e| format!("{e:#} (bezels: {})", bezel::names(&dir).join(", ")))?;
                render.bezel = Some(bezel);
            }
            "--font" => {
                let name = args.next().ok_or("--font needs a font name")?;
                let slug = |n: &str| n.to_ascii_lowercase().replace(' ', "-").replace('\'', "");
                render.font = render::EXTRA_FONTS
                    .iter()
                    .find(|(_, n)| slug(n) == slug(&name))
                    .map(|&(id, _)| id)
                    .ok_or_else(|| format!("unknown font {name}"))?;
            }
            "--glow" => {
                let want = args.next().unwrap_or_default().to_ascii_lowercase();
                render.glow = crt::Glow::ALL
                    .into_iter()
                    .find(|g| g.name().to_ascii_lowercase() == want)
                    .ok_or("--glow needs off, light, medium or strong")?;
            }
            "--curvature" => {
                let want = args.next().unwrap_or_default().to_ascii_lowercase();
                render.curvature = crt::Curvature::ALL
                    .into_iter()
                    .find(|c| c.name().to_ascii_lowercase().starts_with(&want) && !want.is_empty())
                    .ok_or("--curvature needs off, slight, medium or strong")?;
            }
            "--scanlines" => {
                let want = args.next().unwrap_or_default().to_ascii_lowercase();
                render.scanlines = crt::Scanlines::ALL
                    .into_iter()
                    .find(|s| s.name().eq_ignore_ascii_case(&want))
                    .ok_or("--scanlines needs off, light, medium or strong")?;
            }
            "--crt" => {
                render.crt = match args.next().as_deref() {
                    Some("off") => crt::CrtMode::Off,
                    Some("color") => crt::CrtMode::Color,
                    Some("green") => crt::CrtMode::Green,
                    Some("amber") => crt::CrtMode::Amber,
                    _ => return Err("--crt needs off, color, green or amber".into()),
                }
            }
            a if a.starts_with('-') => return Err(format!("unknown option {a}")),
            // An art file to view, or an address to call.
            a if std::path::Path::new(a).is_file() => cmd = Command::Run(None, Some(PathBuf::from(a))),
            a => {
                let entry = Entry::from_address(a).ok_or_else(|| format!("not a valid address: {a}"))?;
                cmd = Command::Run(Some(entry), None);
            }
        }
    }
    Ok(match render_files {
        Some((input, output)) => Command::Render(input, output, render),
        None => cmd,
    })
}

fn main() -> ExitCode {
    let has_console = console::attach();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cmd = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            console::report_error(&format!("{e}\n\n{USAGE}"), has_console);
            return ExitCode::from(2);
        }
    };
    let paths = app::Paths::new();
    let result = match cmd {
        Command::Paths => {
            println!("Dialing directory: {}", paths.directory.display());
            println!("Settings:          {}", paths.settings.display());
            println!("Captures:          {}", paths.captures.display());
            println!("Downloads:         {}", paths.downloads.display());
            println!("Color themes:      {}", paths.themes.display());
            println!("Monitor bezels:    {}", paths.bezels.display());
            println!("SSH known hosts:   {}", paths.known_hosts.display());
            println!("BBS file cache:    {}", paths.bbs_cache.display());
            Ok(())
        }
        Command::Import(file) => app::import_into_directory(&paths, &file).map(|(added, total)| {
            println!("Imported {added} new entries ({} already present).", total - added);
        }),
        Command::Export(file) => Directory::load(&paths.directory)
            .and_then(|d| dialer::export(&file, &d.entries).map(|()| d.entries.len()))
            .map(|n| println!("Exported {n} entries to {}", file.display())),
        Command::Render(input, output, opts) => {
            viewer::render_file(&input, &output, opts).map(|(w, h)| println!("Wrote {} ({w}x{h})", output.display()))
        }
        Command::Run(startup, view) => run(paths, startup, view),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            console::report_error(&format!("{e:#}"), has_console);
            ExitCode::FAILURE
        }
    }
}

fn run(paths: app::Paths, startup: Option<Entry>, view: Option<PathBuf>) -> anyhow::Result<()> {
    let event_loop = EventLoop::<app::UserEvent>::with_user_event().build()?;
    #[cfg(target_os = "macos")]
    {
        let proxy = event_loop.create_proxy();
        macos_urls::install(move |url| {
            // Fails only once the event loop has exited.
            let _ = proxy.send_event(app::UserEvent::OpenUrl(url));
        });
    }
    let mut app = app::App::new(event_loop.create_proxy(), paths, startup, view);
    event_loop.run_app(&mut app)?;
    Ok(())
}
