#!/usr/bin/env python3
"""Call The Vortex BBS, and draw its welcome screen with BadTerm, for the
website's "Call in" section.

    python3 -I site-next/tools/vortex.py BADTERM OUT.png [seconds]

Calls vortexbbs.com over telnet as an 80x25 ANSI terminal would: it offers
the terminal type ANSI and the window size, and answers the board's
cursor-position check. It records what the board sends for `seconds` (about
two minutes covers the intro animations up to the welcome screen with its
"ANY KEY" prompt), then hangs up without pressing a key, so it never logs in.
The capture then goes to `BADTERM --render CAPTURE OUT --screen`, which draws
the final 80x25 screen as BadTerm shows it. The art is the board's own, with
its artist's tag in the corner.
"""

import os
import re
import socket
import subprocess
import sys
import tempfile
import time

HOST, PORT = "vortexbbs.com", 23
IAC, DONT, DO, WONT, WILL, SB, SE = 255, 254, 253, 252, 251, 250, 240
BINARY, ECHO, SGA, TTYPE, NAWS = 0, 1, 3, 24, 31
WE_DO = {BINARY, TTYPE, NAWS, SGA}
THEY_DO = {BINARY, ECHO, SGA}


def capture(seconds, idle=10):
    """The screen bytes the board sends (telnet stripped)."""
    s = socket.create_connection((HOST, PORT), timeout=15)
    s.settimeout(0.5)
    data, buf = bytearray(), b""
    start = last = time.time()
    while time.time() - start < seconds and time.time() - last < idle:
        try:
            chunk = s.recv(65536)
        except socket.timeout:
            continue
        if not chunk:
            break
        last = time.time()
        buf += chunk
        i = 0
        while i < len(buf):
            b = buf[i]
            if b != IAC:
                data.append(b)
                if data[-4:] == b"\x1b[6n":        # where is the cursor? (the board's ANSI check)
                    s.sendall(b"\x1b[25;80R")
                i += 1
                continue
            if i + 1 >= len(buf):
                break
            cmd = buf[i + 1]
            if cmd == IAC:
                data.append(IAC)
                i += 2
            elif cmd in (DO, DONT, WILL, WONT):
                if i + 2 >= len(buf):
                    break
                opt = buf[i + 2]
                if cmd == DO:
                    s.sendall(bytes([IAC, WILL if opt in WE_DO else WONT, opt]))
                    if opt == NAWS:
                        s.sendall(bytes([IAC, SB, NAWS, 0, 80, 0, 25, IAC, SE]))
                elif cmd == WILL:
                    s.sendall(bytes([IAC, DO if opt in THEY_DO else DONT, opt]))
                i += 3
            elif cmd == SB:
                end = buf.find(bytes([IAC, SE]), i)
                if end < 0:
                    break
                if buf[i + 2:i + 4] == bytes([TTYPE, 1]):
                    s.sendall(bytes([IAC, SB, TTYPE, 0]) + b"ANSI" + bytes([IAC, SE]))
                i = end + 2
            else:
                i += 2
        buf = buf[i:]
    s.close()
    return bytes(data)


def main():
    badterm, out = sys.argv[1], sys.argv[2]
    seconds = float(sys.argv[3]) if len(sys.argv) > 3 else 120
    data = capture(seconds)
    text = re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", data).replace(b" ", b"")
    if b"letgo" not in text:
        print("warning: no 'ANY KEY' prompt seen; the welcome screen may not have come yet", file=sys.stderr)
    with tempfile.NamedTemporaryFile(suffix=".ans", delete=False) as f:
        f.write(data)
    try:
        subprocess.run([badterm, "--render", f.name, out, "--screen"], check=True)
    finally:
        os.unlink(f.name)
    print(f"captured {len(data)} bytes; wrote {out}")


if __name__ == "__main__":
    main()
