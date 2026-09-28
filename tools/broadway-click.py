#!/usr/bin/env python3
"""Sends real pointer input to a GTK Broadway display, for testing Margin.

    broadway-click.py [--display N] serve FIFO

Connects to the display as its (only) client and stays connected, answering
the server's frame roundtrips, which apps on Broadway wait for once a client
is connected. Each line written to FIFO is a command:

    click X Y     press and release the primary button at X, Y
    focus         click the title bar of the largest window, to focus it
    type TEXT     type ASCII text
    key NAME      press a key (a name from KEYSYMS, a character, or #N for
                  raw code N), optionally with ctrl+/shift+/alt+ prefixes

X and Y are coordinates within the app's main window surface; the harness's
`locate` step writes them to /tmp/margin-locate. Start this before the app
and stop it after.
"""

import base64
import os
import socket
import struct
import sys
import time

EVENT_ENTER, EVENT_MOVE, EVENT_PRESS, EVENT_RELEASE = 0, 2, 3, 4
EVENT_KEY_PRESS, EVENT_KEY_RELEASE = 7, 8
KEYSYMS = {
    "Return": 0xFF0D,
    "Escape": 0xFF1B,
    "Tab": 0xFF09,
    "BackSpace": 0xFF08,
    "Delete": 0xFFFF,
    "Home": 0xFF50,
    "Left": 0xFF51,
    "Up": 0xFF52,
    "Right": 0xFF53,
    "Down": 0xFF54,
    "End": 0xFF57,
    "F10": 0xFFC7,
    "F11": 0xFFC8,
}
EVENT_GRAB_NOTIFY, EVENT_UNGRAB_NOTIFY = 9, 10
EVENT_ROUNDTRIP_NOTIFY = 14
OP_NEW_SURFACE = 2
BUTTON1_MASK = 1 << 8


def connect(port):
    s = socket.create_connection(("127.0.0.1", port))
    key = base64.b64encode(os.urandom(16)).decode()
    s.sendall(
        (
            f"GET /socket HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: broadway\r\n\r\n"
        ).encode()
    )
    head = b""
    while b"\r\n\r\n" not in head:
        head += s.recv(1)
    if b" 101 " not in head.split(b"\r\n")[0]:
        sys.exit(f"websocket handshake failed: {head!r}")
    return s


def read_frame(s):
    def exact(n):
        data = b""
        while len(data) < n:
            chunk = s.recv(n - len(data))
            if not chunk:
                raise EOFError
            data += chunk
        return data

    b0, b1 = exact(2)
    n = b1 & 0x7F
    if n == 126:
        n = struct.unpack(">H", exact(2))[0]
    elif n == 127:
        n = struct.unpack(">Q", exact(8))[0]
    if b1 & 0x80:
        exact(4)
    return b0 & 0x0F, exact(n)


def send_frame(s, payload):
    mask = os.urandom(4)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    n = len(payload)
    header = bytes([0x82]) + (bytes([0x80 | n]) if n < 126 else bytes([0x80 | 126]) + struct.pack(">H", n))
    s.sendall(header + mask + masked)


class Session:
    """A Broadway client: tracks surfaces and answers roundtrips."""

    def __init__(self, sock):
        self.s = sock
        self.surfaces = {}  # id -> [x, y, w, h]
        self.serial = 0
        self.t = int(time.time() * 1000) & 0x7FFFFFFF

    def send(self, cmd, *args):
        self.t += 16
        send_frame(self.s, struct.pack(f">{3 + len(args)}i", cmd, self.serial, self.t, *map(int, args)))

    def handle(self, data):
        pos = 0
        n = len(data)
        u16 = lambda p: struct.unpack_from("<H", data, p)[0]
        i16 = lambda p: struct.unpack_from("<h", data, p)[0]
        u32 = lambda p: struct.unpack_from("<I", data, p)[0]
        while pos < n:
            op = data[pos]
            self.serial = u32(pos + 1)
            pos += 5
            if op == OP_NEW_SURFACE:
                self.surfaces[u16(pos)] = [i16(pos + 2), i16(pos + 4), u16(pos + 6), u16(pos + 8)]
                pos += 10
            elif op in (3, 4, 5, 6, 7, 12):
                pos += 2
            elif op == 8:  # move/resize
                sid, flags = u16(pos), data[pos + 2]
                pos += 3
                surf = self.surfaces.setdefault(sid, [0, 0, 0, 0])
                if flags & 1:
                    surf[0], surf[1] = i16(pos), i16(pos + 2)
                    pos += 4
                if flags & 2:
                    surf[2], surf[3] = u16(pos), u16(pos + 2)
                    pos += 4
            elif op == 9:
                pos += 4
            elif op == 10:
                pass
            elif op == 0:  # grab pointer: acknowledge, as the browser client does
                if os.environ.get("BROADWAY_CLICK_DEBUG"):
                    print(f"grab pointer surface={u16(pos)} owner_events={data[pos + 2]}", flush=True)
                pos += 3
                self.send(EVENT_GRAB_NOTIFY)
            elif op == 1:  # ungrab pointer
                if os.environ.get("BROADWAY_CLICK_DEBUG"):
                    print("ungrab pointer", flush=True)
                self.send(EVENT_UNGRAB_NOTIFY)
            elif op == 13:  # upload texture: id, sized data
                pos += 4
                pos += 4 + u32(pos)
            elif op == 14:
                pos += 4
            elif op == 15:  # set nodes: id, counted 32-bit words
                pos += 2
                pos += 4 + 4 * u32(pos)
            elif op == 16:  # roundtrip: answer it
                sid, tag = u16(pos), u32(pos + 2)
                pos += 6
                self.send(EVENT_ROUNDTRIP_NOTIFY, sid, tag)
            else:
                raise SystemExit(f"unknown broadway op {op}")

    def pump(self, seconds):
        self.s.settimeout(0.05)
        end = time.time() + seconds
        while time.time() < end:
            try:
                op, data = read_frame(self.s)
            except socket.timeout:
                continue
            if op == 2:
                self.handle(data)
        self.s.settimeout(None)

def main():
    import selectors

    args = sys.argv[1:]
    display = 7
    if args[:1] == ["--display"]:
        display = int(args[1])
        args = args[2:]
    if len(args) != 2 or args[0] != "serve":
        sys.exit(__doc__)
    fifo_path = args[1]
    if not os.path.exists(fifo_path):
        os.mkfifo(fifo_path)
    session = Session(connect(8080 + display))
    # Opened read-write so the FIFO never reports end-of-file.
    fifo = os.open(fifo_path, os.O_RDWR | os.O_NONBLOCK)
    sel = selectors.DefaultSelector()
    sel.register(session.s, selectors.EVENT_READ, "socket")
    sel.register(fifo, selectors.EVENT_READ, "fifo")
    pending = b""
    while True:
        for key, _ in sel.select():
            if key.data == "socket":
                try:
                    op, data = read_frame(session.s)
                except EOFError:
                    return
                if op == 2:
                    session.handle(data)
                elif op == 8:
                    return
                continue
            pending += os.read(fifo, 4096)
            while b"\n" in pending:
                line, pending = pending.split(b"\n", 1)
                words = line.decode().split()
                if words[:1] == ["click"] and len(words) == 3:
                    click(session, float(words[1]), float(words[2]))
                elif words[:1] == ["focus"] and session.surfaces:
                    _, (_, _, w, _) = max(session.surfaces.items(), key=lambda kv: kv[1][2] * kv[1][3])
                    click(session, w / 2, 14)
                elif words[:1] == ["type"]:
                    for ch in line.decode().split(" ", 1)[1]:
                        press_key(session, ord(ch))
                elif words[:1] == ["key"] and len(words) == 2:
                    *mods, name = words[1].split("+")
                    state = sum({"ctrl": 4, "shift": 1, "alt": 8}[m] for m in mods)
                    # "#N" sends raw code N (Broadway uses the keysym as the keycode).
                    sym = KEYSYMS.get(name) or (int(name[1:]) if name[:1] == "#" and name[1:] else ord(name))
                    press_key(session, sym, state)


def press_key(session, keysym, state=0):
    session.send(EVENT_KEY_PRESS, keysym, state)
    session.pump(0.03)
    session.send(EVENT_KEY_RELEASE, keysym, state)
    session.pump(0.03)


def click(session, x, y):
    if not session.surfaces:
        print("no surfaces yet", flush=True)
        return
    sid, (sx, sy, _, _) = max(session.surfaces.items(), key=lambda kv: kv[1][2] * kv[1][3])
    rx, ry = sx + x, sy + y
    send = session.send
    send(EVENT_ENTER, sid, sid, rx, ry, x, y, 0, 0)
    send(EVENT_MOVE, sid, sid, rx, ry, x, y, 0)
    session.pump(0.05)
    # Like the browser client: the state already includes the button on
    # press and no longer does on release. Broadway synthesizes motion events
    # from the last state it saw, and GTK cancels a click on buttonless motion.
    send(EVENT_PRESS, sid, sid, rx, ry, x, y, BUTTON1_MASK, 1)
    session.pump(0.08)
    send(EVENT_RELEASE, sid, sid, rx, ry, x, y, 0, 1)
    session.pump(0.05)
    print(f"clicked surface {sid} at {x:.0f},{y:.0f}", flush=True)


if __name__ == "__main__":
    main()
