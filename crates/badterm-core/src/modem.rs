//! Dialing out with a real modem on a serial port (or anything that acts
//! like one: a USB modem, a WiFi modem, tcpser).
//!
//! The connection thread opens the port, sends the init string and waits
//! for OK, dials the entry's number with the dial command, and waits for
//! the modem's verdict: CONNECT, or BUSY, NO CARRIER, NO DIALTONE, NO
//! ANSWER. What the modem says shows on the terminal as it goes, as it
//! would have in a terminal program of the day. Once connected, the line is
//! passed through untouched until the carrier drops (DCD goes low, or,
//! where the port does not report DCD, the modem says NO CARRIER).
//! Hanging up drops DTR, then tries `+++` and `ATH0`.

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serialport::SerialPort;

use crate::transport::{NetEvent, Writer, WriterKind};

/// How long the modem has to answer the init string.
const INIT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a call may take to connect (S7 is often 50 seconds).
const DIAL_TIMEOUT: Duration = Duration::from_secs(70);
/// The pause around `+++` that tells the modem it is a command.
const GUARD_TIME: Duration = Duration::from_millis(1100);
/// How long a read waits, so the thread notices a hang-up.
const POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlowControl {
    /// RTS/CTS, as most modems want.
    #[default]
    Hardware,
    /// XON/XOFF.
    Software,
    None,
}

impl FlowControl {
    pub const ALL: [FlowControl; 3] = [FlowControl::Hardware, FlowControl::Software, FlowControl::None];

    pub fn name(self) -> &'static str {
        match self {
            FlowControl::Hardware => "RTS/CTS",
            FlowControl::Software => "XON/XOFF",
            FlowControl::None => "none",
        }
    }

    fn serial(self) -> serialport::FlowControl {
        match self {
            FlowControl::Hardware => serialport::FlowControl::Hardware,
            FlowControl::Software => serialport::FlowControl::Software,
            FlowControl::None => serialport::FlowControl::None,
        }
    }
}

/// The modem, as set up in the settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModemConfig {
    /// The serial port: `/dev/ttyUSB0`, `/dev/cu.usbmodem1101`, `COM3`...
    pub device: String,
    /// The speed between the computer and the modem.
    pub speed: u32,
    pub flow: FlowControl,
    /// Sent first, before dialing.
    pub init: String,
    /// Put before the number: `ATDT` for tone, `ATDP` for pulse.
    pub dial: String,
}

impl Default for ModemConfig {
    fn default() -> Self {
        ModemConfig {
            device: String::new(),
            speed: 115_200,
            flow: FlowControl::Hardware,
            init: "ATZ".into(),
            dial: "ATDT".into(),
        }
    }
}

/// The speeds offered for the serial port.
pub const SPEEDS: [u32; 8] = [1200, 2400, 9600, 19_200, 38_400, 57_600, 115_200, 230_400];

/// The serial ports the system lists (where it can).
pub fn ports() -> Vec<String> {
    serialport::available_ports().map(|ps| ps.into_iter().map(|p| p.port_name).collect()).unwrap_or_default()
}

/// What the modem answered a dial with.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Connect,
    Failed(&'static str),
}

/// The result codes a dial can end with, and what they mean.
const RESULTS: [(&str, Option<&str>); 8] = [
    ("CONNECT", None),
    ("BUSY", Some("the line is busy (BUSY)")),
    ("NO CARRIER", Some("no answer from a modem (NO CARRIER)")),
    ("NO DIALTONE", Some("no dial tone (NO DIALTONE)")),
    ("NO DIAL TONE", Some("no dial tone (NO DIAL TONE)")),
    ("NO ANSWER", Some("no answer (NO ANSWER)")),
    ("DELAYED", Some("the modem refuses to dial again so soon (DELAYED)")),
    ("ERROR", Some("the modem did not take the dial command (ERROR)")),
];

/// The result code in what the modem has said so far, once a whole line of
/// it has arrived.
fn verdict(said: &str) -> Option<Verdict> {
    said.split(['\r', '\n']).rev().skip(1).find_map(|line| {
        let line = line.trim();
        RESULTS.iter().find(|(code, _)| line.starts_with(code)).map(|(_, why)| match why {
            None => Verdict::Connect,
            Some(why) => Verdict::Failed(why),
        })
    })
}

/// Dial `number` on the modem in a background thread, reporting through
/// `sink` like the network transports.
pub fn spawn_modem<F>(number: String, config: ModemConfig, sink: F)
where
    F: Fn(NetEvent) + Send + 'static,
{
    thread::spawn(move || {
        let failure = run(&number, &config, &sink).err();
        sink(NetEvent::Closed(failure.map(|e| e.to_string())));
    });
}

fn run<F: Fn(NetEvent)>(number: &str, config: &ModemConfig, sink: &F) -> Result<(), String> {
    let device = config.device.trim();
    if device.is_empty() {
        return Err("choose the modem's serial port first: Settings (Alt-O), the Modem tab".into());
    }
    if number.trim().is_empty() {
        return Err("the entry has no phone number: put it in \"Address\"".into());
    }
    let mut port = serialport::new(device, config.speed)
        .flow_control(config.flow.serial())
        .timeout(POLL)
        .open()
        .map_err(|e| format!("opening the modem on {device}: {e}"))?;
    let _ = port.write_data_terminal_ready(true);
    let _ = port.write_request_to_send(true);

    let mut said = String::new();
    if !config.init.trim().is_empty() {
        send(&mut port, &format!("{}\r", config.init.trim()))?;
        let answer =
            wait(&mut port, sink, INIT_TIMEOUT, &mut said, |s| s.lines().any(|l| matches!(l.trim(), "OK" | "ERROR")))?;
        if !answer {
            return Err(format!("the modem on {device} did not answer \"{}\"", config.init.trim()));
        }
        if said.lines().any(|l| l.trim() == "ERROR") {
            return Err(format!("the modem did not take the init string \"{}\" (ERROR)", config.init.trim()));
        }
    }
    sink(NetEvent::Notice(format!("Dialing {number}...")));
    said.clear();
    send(&mut port, &format!("{}{}\r", config.dial.trim(), number.trim()))?;
    let answered = wait(&mut port, sink, DIAL_TIMEOUT, &mut said, |s| verdict(s).is_some())?;
    match verdict(&said) {
        Some(Verdict::Connect) => {}
        Some(Verdict::Failed(why)) => return Err(format!("connecting to {number}: {why}")),
        None if !answered => {
            // Stop the dial before giving up on it.
            let _ = port.write_all(b"\r");
            return Err(format!("connecting to {number}: no answer in {} seconds", DIAL_TIMEOUT.as_secs()));
        }
        None => unreachable!("waited for a verdict"),
    }

    let stop = Arc::new(AtomicBool::new(false));
    let writer = port.try_clone().map_err(|e| format!("using the modem: {e}"))?;
    sink(NetEvent::Connected(Writer(WriterKind::Serial(std::sync::Mutex::new(writer), stop.clone()))));
    pass_through(&mut port, sink, &stop)
}

fn send(port: &mut Box<dyn SerialPort>, text: &str) -> Result<(), String> {
    port.write_all(text.as_bytes()).map_err(|e| format!("talking to the modem: {e}"))
}

/// Read what the modem says, showing it, until `done` says so (true) or
/// the time is up (false).
fn wait<F: Fn(NetEvent)>(
    port: &mut Box<dyn SerialPort>,
    sink: &F,
    limit: Duration,
    said: &mut String,
    done: impl Fn(&str) -> bool,
) -> Result<bool, String> {
    let deadline = Instant::now() + limit;
    let mut buf = [0u8; 1024];
    while Instant::now() < deadline {
        match port.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                sink(NetEvent::Data(buf[..n].to_vec()));
                said.push_str(&String::from_utf8_lossy(&buf[..n]));
                if done(said) {
                    return Ok(true);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("reading from the modem: {e}")),
        }
    }
    Ok(false)
}

/// The call itself: everything the modem passes on, until the carrier
/// drops or the user hangs up.
fn pass_through<F: Fn(NetEvent)>(port: &mut Box<dyn SerialPort>, sink: &F, stop: &AtomicBool) -> Result<(), String> {
    // Ports that report DCD show the carrier's end; others only say so in
    // words.
    let mut carrier_seen = port.read_carrier_detect().unwrap_or(false);
    let mut tail: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        if stop.load(Ordering::Relaxed) {
            hang_up(port);
            return Ok(());
        }
        match port.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                sink(NetEvent::Data(buf[..n].to_vec()));
                if !carrier_seen {
                    tail.extend_from_slice(&buf[..n]);
                    if crate::transfer::find(&tail, b"NO CARRIER").is_some() {
                        return Ok(());
                    }
                    tail.drain(..tail.len().saturating_sub(16));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("the modem: {e}")),
        }
        match port.read_carrier_detect() {
            Ok(true) => carrier_seen = true,
            Ok(false) if carrier_seen => return Ok(()),
            _ => {}
        }
    }
}

/// Drop DTR (most modems hang up at once), then, if the carrier is still
/// up or cannot be told, escape to command mode and hang up.
fn hang_up(port: &mut Box<dyn SerialPort>) {
    let _ = port.write_data_terminal_ready(false);
    thread::sleep(Duration::from_millis(500));
    if matches!(port.read_carrier_detect(), Ok(false)) {
        return;
    }
    let _ = port.write_data_terminal_ready(true);
    thread::sleep(GUARD_TIME);
    let _ = port.write_all(b"+++");
    thread::sleep(GUARD_TIME);
    let _ = port.write_all(b"ATH0\r");
    thread::sleep(Duration::from_millis(200));
}

/// The writing side of a modem call (see [`WriterKind::Serial`]).
pub(crate) fn write(port: &std::sync::Mutex<Box<dyn SerialPort>>, data: &[u8]) -> io::Result<()> {
    port.lock().unwrap_or_else(|e| e.into_inner()).write_all(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_codes() {
        assert_eq!(verdict("ATDT5551234\r\r\nCONNECT 33600\r\n"), Some(Verdict::Connect));
        assert_eq!(verdict("ATDT5551234\r\r\nBUSY\r\n"), Some(Verdict::Failed("the line is busy (BUSY)")));
        // Not until the line is whole.
        assert_eq!(verdict("ATDT5551234\r\r\nCONNE"), None);
        assert_eq!(verdict("ATDT5551234\r\r\n"), None);
        assert!(matches!(verdict("\r\nNO DIALTONE\r\n"), Some(Verdict::Failed(_))));
    }
}
