// Phosphor - a cross-platform ANSI-BBS telnet client.
// Copyright (C) 2026 The Phosphor contributors
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version. See the LICENSE file for details.

// A GUI program on Windows, so no console window opens alongside it; see
// the console module for how command-line use still gets output.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod console;
mod input;
mod png;
mod render;
mod secrets;
mod selection;
mod ui;

use std::path::PathBuf;
use std::process::ExitCode;

use phosphor_core::dialer::{self, Directory, Entry};
use winit::event_loop::EventLoop;

const USAGE: &str = "\
Usage: phosphor [OPTIONS] [ADDRESS]

Connect to ADDRESS (host, host:port, telnet://host:port, raw://host:port or
ssh://user@host:port), or open the dialing directory if no address is given.

Options:
  --import FILE   Add the entries in FILE to the dialing directory and exit
                  (SyncTERM .lst, .csv, .json or .toml)
  --export FILE   Write the dialing directory to FILE and exit; the format
                  follows the extension (.lst, .csv, .json or .toml)
  --paths         Print where the directory, captures and downloads are
                  stored and exit
  -h, --help      Show this help
  -V, --version   Show the version";

enum Command {
    Run(Option<Entry>),
    Import(PathBuf),
    Export(PathBuf),
    Paths,
}

fn parse_args() -> Result<Command, String> {
    let mut args = std::env::args().skip(1);
    let mut cmd = Command::Run(None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("phosphor {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--import" => cmd = Command::Import(args.next().ok_or("--import needs a file")?.into()),
            "--export" => cmd = Command::Export(args.next().ok_or("--export needs a file")?.into()),
            "--paths" => cmd = Command::Paths,
            a if a.starts_with('-') => return Err(format!("unknown option {a}")),
            a => cmd = Command::Run(Some(Entry::from_address(a).ok_or_else(|| format!("not a valid address: {a}"))?)),
        }
    }
    Ok(cmd)
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
            println!("Captures:          {}", paths.captures.display());
            println!("Downloads:         {}", paths.downloads.display());
            println!("SSH known hosts:   {}", paths.known_hosts.display());
            Ok(())
        }
        Command::Import(file) => app::import_into_directory(&paths, &file).map(|(added, total)| {
            println!("Imported {added} new entries ({} already present).", total - added);
        }),
        Command::Export(file) => Directory::load(&paths.directory)
            .and_then(|d| dialer::export(&file, &d.entries).map(|()| d.entries.len()))
            .map(|n| println!("Exported {n} entries to {}", file.display())),
        Command::Run(startup) => run(paths, startup),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            console::report_error(&format!("{e:#}"), has_console);
            ExitCode::FAILURE
        }
    }
}

fn run(paths: app::Paths, startup: Option<Entry>) -> anyhow::Result<()> {
    let event_loop = EventLoop::<app::UserEvent>::with_user_event().build()?;
    let mut app = app::App::new(event_loop.create_proxy(), paths, startup);
    event_loop.run_app(&mut app)?;
    Ok(())
}
