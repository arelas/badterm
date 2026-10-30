//! Telnet protocol (RFC 854) with the option negotiation a BBS client needs:
//! BINARY (RFC 856), ECHO (857), SGA (858), TERMINAL-TYPE (1091) and
//! NAWS (1073).
//!
//! Negotiation follows the spirit of RFC 1143: we only answer requests that
//! change an option's state, which prevents negotiation loops.

pub const IAC: u8 = 255;
pub const DONT: u8 = 254;
pub const DO: u8 = 253;
pub const WONT: u8 = 252;
pub const WILL: u8 = 251;
pub const SB: u8 = 250;
pub const SE: u8 = 240;

pub mod opt {
    pub const BINARY: u8 = 0;
    pub const ECHO: u8 = 1;
    pub const SGA: u8 = 3;
    pub const TTYPE: u8 = 24;
    pub const NAWS: u8 = 31;
}

const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;
const MAX_SB: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Data,
    Iac,
    /// After IAC WILL/WONT/DO/DONT, waiting for the option byte.
    Negotiate(u8),
    Sb,
    SbIac,
    /// Saw CR in the data stream; a following NUL is dropped (NVT CR NUL).
    Cr,
}

pub struct Telnet {
    state: State,
    /// Options enabled on our side (we WILL).
    local: [bool; 256],
    /// Options enabled on the server side (it WILL).
    remote: [bool; 256],
    sb: Vec<u8>,
    pub term_type: String,
    cols: u16,
    rows: u16,
}

impl Telnet {
    pub fn new(term_type: &str, cols: u16, rows: u16) -> Self {
        Telnet {
            state: State::Data,
            local: [false; 256],
            remote: [false; 256],
            sb: Vec::new(),
            term_type: term_type.to_string(),
            cols,
            rows,
        }
    }

    /// Whether the server has agreed to echo our input.
    pub fn remote_echo(&self) -> bool {
        self.remote[opt::ECHO as usize]
    }

    /// Process bytes from the network. Application data is appended to
    /// `data`; protocol replies to send back are appended to `reply`.
    pub fn receive(&mut self, input: &[u8], data: &mut Vec<u8>, reply: &mut Vec<u8>) {
        for &b in input {
            self.state = match self.state {
                State::Data | State::Cr => {
                    let after_cr = self.state == State::Cr;
                    match b {
                        IAC => State::Iac,
                        0 if after_cr => State::Data,
                        b'\r' => {
                            data.push(b);
                            State::Cr
                        }
                        _ => {
                            data.push(b);
                            State::Data
                        }
                    }
                }
                State::Iac => match b {
                    IAC => {
                        data.push(IAC);
                        State::Data
                    }
                    WILL | WONT | DO | DONT => State::Negotiate(b),
                    SB => {
                        self.sb.clear();
                        State::Sb
                    }
                    // NOP, GA, AYT and friends carry no data for us.
                    _ => State::Data,
                },
                State::Negotiate(cmd) => {
                    self.negotiate(cmd, b, reply);
                    State::Data
                }
                State::Sb => {
                    if b == IAC {
                        State::SbIac
                    } else {
                        if self.sb.len() < MAX_SB {
                            self.sb.push(b);
                        }
                        State::Sb
                    }
                }
                State::SbIac => match b {
                    SE => {
                        self.subnegotiation(reply);
                        State::Data
                    }
                    IAC => {
                        if self.sb.len() < MAX_SB {
                            self.sb.push(IAC);
                        }
                        State::Sb
                    }
                    _ => State::Sb,
                },
            };
        }
    }

    fn supports_local(o: u8) -> bool {
        matches!(o, opt::BINARY | opt::SGA | opt::TTYPE | opt::NAWS)
    }

    fn supports_remote(o: u8) -> bool {
        matches!(o, opt::BINARY | opt::SGA | opt::ECHO)
    }

    fn negotiate(&mut self, cmd: u8, o: u8, reply: &mut Vec<u8>) {
        let i = o as usize;
        match cmd {
            WILL => {
                if !Self::supports_remote(o) {
                    reply.extend_from_slice(&[IAC, DONT, o]);
                } else if !self.remote[i] {
                    self.remote[i] = true;
                    reply.extend_from_slice(&[IAC, DO, o]);
                }
            }
            WONT if self.remote[i] => {
                self.remote[i] = false;
                reply.extend_from_slice(&[IAC, DONT, o]);
            }
            DO => {
                if !Self::supports_local(o) {
                    reply.extend_from_slice(&[IAC, WONT, o]);
                } else if !self.local[i] {
                    self.local[i] = true;
                    reply.extend_from_slice(&[IAC, WILL, o]);
                    if o == opt::NAWS {
                        self.naws(reply);
                    }
                } else if o == opt::NAWS {
                    // Some servers re-ask; resending the size is harmless.
                    self.naws(reply);
                }
            }
            DONT if self.local[i] => {
                self.local[i] = false;
                reply.extend_from_slice(&[IAC, WONT, o]);
            }
            _ => {}
        }
    }

    fn subnegotiation(&mut self, reply: &mut Vec<u8>) {
        if self.sb.first() == Some(&opt::TTYPE)
            && self.sb.get(1) == Some(&TTYPE_SEND)
            && self.local[opt::TTYPE as usize]
        {
            reply.extend_from_slice(&[IAC, SB, opt::TTYPE, TTYPE_IS]);
            push_escaped(reply, self.term_type.as_bytes());
            reply.extend_from_slice(&[IAC, SE]);
        }
    }

    fn naws(&self, reply: &mut Vec<u8>) {
        reply.extend_from_slice(&[IAC, SB, opt::NAWS]);
        push_escaped(reply, &self.cols.to_be_bytes());
        push_escaped(reply, &self.rows.to_be_bytes());
        reply.extend_from_slice(&[IAC, SE]);
    }

    /// Update the window size, returning a NAWS report if the option is active.
    pub fn set_window_size(&mut self, cols: u16, rows: u16) -> Vec<u8> {
        self.cols = cols;
        self.rows = rows;
        let mut out = Vec::new();
        if self.local[opt::NAWS as usize] {
            self.naws(&mut out);
        }
        out
    }

    /// Encode application data for the wire: IAC is doubled and, outside of
    /// binary mode, a bare CR becomes CR NUL as the NVT requires.
    pub fn encode(&self, data: &[u8]) -> Vec<u8> {
        let binary = self.local[opt::BINARY as usize];
        let mut out = Vec::with_capacity(data.len() + 4);
        for (i, &b) in data.iter().enumerate() {
            out.push(b);
            if b == IAC {
                out.push(IAC);
            } else if b == b'\r' && !binary && data.get(i + 1) != Some(&b'\n') {
                out.push(0);
            }
        }
        out
    }
}

fn push_escaped(out: &mut Vec<u8>, bytes: &[u8]) {
    for &b in bytes {
        out.push(b);
        if b == IAC {
            out.push(IAC);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(t: &mut Telnet, input: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let (mut d, mut r) = (Vec::new(), Vec::new());
        t.receive(input, &mut d, &mut r);
        (d, r)
    }

    #[test]
    fn data_and_escaped_iac() {
        let mut t = Telnet::new("ANSI", 80, 25);
        let (d, r) = run(&mut t, &[b'a', IAC, IAC, b'b']);
        assert_eq!(d, [b'a', 255, b'b']);
        assert!(r.is_empty());
    }

    #[test]
    fn negotiation_is_answered_once() {
        let mut t = Telnet::new("ANSI", 80, 25);
        let (_, r) = run(&mut t, &[IAC, WILL, opt::ECHO, IAC, WILL, opt::ECHO]);
        assert_eq!(r, [IAC, DO, opt::ECHO]);
        assert!(t.remote_echo());
        let (_, r) = run(&mut t, &[IAC, DO, 42]);
        assert_eq!(r, [IAC, WONT, 42]);
    }

    #[test]
    fn naws_and_ttype() {
        let mut t = Telnet::new("ANSI", 80, 25);
        let (_, r) = run(&mut t, &[IAC, DO, opt::NAWS]);
        assert_eq!(r, [IAC, WILL, opt::NAWS, IAC, SB, opt::NAWS, 0, 80, 0, 25, IAC, SE]);
        let (_, r) = run(&mut t, &[IAC, DO, opt::TTYPE, IAC, SB, opt::TTYPE, 1, IAC, SE]);
        let mut want = vec![IAC, WILL, opt::TTYPE, IAC, SB, opt::TTYPE, 0];
        want.extend_from_slice(b"ANSI");
        want.extend_from_slice(&[IAC, SE]);
        assert_eq!(r, want);
    }

    #[test]
    fn split_across_reads() {
        let mut t = Telnet::new("ANSI", 80, 25);
        let (d, _) = run(&mut t, &[b'x', IAC]);
        assert_eq!(d, b"x");
        let (d, r) = run(&mut t, &[WILL, opt::SGA, b'y']);
        assert_eq!(d, b"y");
        assert_eq!(r, [IAC, DO, opt::SGA]);
    }

    #[test]
    fn encode_cr_and_iac() {
        let t = Telnet::new("ANSI", 80, 25);
        assert_eq!(t.encode(&[b'\r', 255]), [b'\r', 0, 255, 255]);
        assert_eq!(t.encode(b"\r\n"), b"\r\n");
    }

    #[test]
    fn cr_nul_from_server_is_stripped() {
        let mut t = Telnet::new("ANSI", 80, 25);
        let (d, _) = run(&mut t, b"a\r\0b");
        assert_eq!(d, b"a\rb");
    }
}
