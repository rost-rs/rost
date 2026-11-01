#!/usr/bin/env python3
"""Drives the app created by scripts/rost-init.sh in a pseudo-terminal.

    scripts/rost-init.sh /tmp/demo && python3 scripts/demo-test.py /tmp/demo

The app's whole interface is described in app.rost, so the test edits that file (and the
theme files) while the app runs and reads what reaches the screen: a live theme edit, a
broken edit that is rejected with its line, `t` patching the theme, a resized panel, a node
added at runtime, a panel toggled by a key, errors between fields, and a key rebound in
the file itself.
"""
import fcntl
import json
import os
import pty
import re
import select
import struct
import subprocess
import sys
import termios
import time

class Screen:
    """Just enough of a terminal to read what ratatui draws: cursor moves, clears and text."""

    def __init__(self, rows=30, cols=100):
        self.rows, self.cols = rows, cols
        self.grid = [[" "] * cols for _ in range(rows)]
        self.r = self.c = 0
        self.pending = ""

    def feed(self, data):
        text = self.pending + data.decode("utf8", "replace")
        self.pending = ""
        i = 0
        while i < len(text):
            ch = text[i]
            if ch == "\x1b":
                m = re.compile(r"\x1b\[([0-9;?]*)([A-Za-z])").match(text, i)
                if not m:
                    if re.fullmatch(r"\x1b(\[[0-9;?]*)?", text[i:]):
                        self.pending = text[i:]
                        break
                    i += 1
                    continue
                self.csi(m.group(1), m.group(2))
                i = m.end()
                continue
            if ch == "\r":
                self.c = 0
            elif ch == "\n":
                self.r = min(self.r + 1, self.rows - 1)
            elif ch >= " ":
                if self.r < self.rows and self.c < self.cols:
                    self.grid[self.r][self.c] = ch
                self.c += 1
            i += 1

    def csi(self, args, final):
        nums = [int(x) if x.isdigit() else 0 for x in args.lstrip("?").split(";")] if args else []
        if final == "H":
            self.r = (nums[0] or 1) - 1 if nums else 0
            self.c = (nums[1] or 1) - 1 if len(nums) > 1 else 0
        elif final == "J" and (nums[:1] in ([2], [3])):
            self.grid = [[" "] * self.cols for _ in range(self.rows)]
        elif final == "C":
            self.c += nums[0] or 1

    def text(self):
        return "\n".join("".join(row).rstrip() for row in self.grid).rstrip()


demo = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "rost-demo")
os.chdir(demo)
target = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"]))["target_directory"]
binary = os.path.join(target, "debug", "rost-demo")

pid, fd = pty.fork()
if pid == 0:
    os.execv(binary, ["rost-demo"])
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))

screen = Screen(30, 100)
raw = b""


def pump(seconds=1.0):
    global raw
    end = time.time() + seconds
    while time.time() < end:
        ready, _, _ = select.select([fd], [], [], 0.05)
        if ready:
            try:
                data = os.read(fd, 65536)
            except OSError:
                return
            raw += data
            screen.feed(data)


def check(condition, message):
    print(("PASS " if condition else "FAIL ") + message)
    if not condition:
        print(screen.text())
        os.kill(pid, 9)
        sys.exit(1)


def read(path):
    with open(path) as f:
        return f.read()


def write(path, text):
    with open(path, "w") as f:
        f.write(text)


def edit_app(old, new, base):
    assert old in base, old
    write("app.rost", base.replace(old, new, 1))
    pump()


pump(1.2)
check("rost live demo" in screen.text() and "theme Dark" in screen.text(), "first frame is drawn from app.rost: title and theme")
check("try this in app.rost" in screen.text() and "themes/dark.rost" in screen.text() and "hot reload" in screen.text(), "panels, list, palette and status come from the layout in the file")
check("q quit   t next theme" in screen.text(), "the key legend is built from the bindings in the file")
check(b"137;180;250" in raw, "the accent #89b4fa from themes/dark.rost is used")

mark = len(raw)
dark = read("themes/dark.rost")
write("themes/dark.rost", dark.replace("#89b4fa", "#ff0000"))
pump()
check(b"255;0;0" in raw[mark:], "editing dark.rost repaints with the new accent")

write("themes/dark.rost", 'name: "Dark"\nbg: nonsense\n')
pump()
check("themes/dark.rost v" in screen.text() and "kept; rejected at 2:1" in screen.text(), "a broken theme is rejected with its line and the last good one stays")
write("themes/dark.rost", dark)
pump()
check("✓ themes/dark.rost" in screen.text(), "fixing the file recovers")

before = read("app.rost")
os.write(fd, b"t")
pump()
after = read("app.rost")
check("theme: light" in after and after.replace("theme: light", "theme: dark") == before, "t changes only the theme value in app.rost, comments kept")
check("theme Light" in screen.text(), "the screen switched to the Light theme after the reload")
os.write(fd, b"3")
pump()
check("theme Solarized" in screen.text(), "key 3 (theme:solarized, defined in the file) switches to Solarized")
os.write(fd, b"1")
pump()
base = read("app.rost")
check("theme: dark" in base and "theme Dark" in screen.text(), "key 1 goes back to Dark")

def width():
    """Columns of the list panel: from its top-left corner to its top-right corner."""
    row = next(line for line in screen.text().split("\n") if line.startswith("┌try this"))
    return row.index("┐") + 1

wide = width()
edit_app("size: 60%", "size: 30%", base)
narrow = width()
check(narrow < wide * 0.6, f"changing a panel to `size: 30%` in app.rost resizes it live ({wide} -> {narrow} columns)")
write("app.rost", base)
pump()

edit_app("{ kind: keys, size: 1, fg: muted }", '{ kind: text, size: 1, text: "added while running", fg: accent }\n        { kind: keys, size: 1, fg: muted }', base)
check("added while running" in screen.text(), "a text node added to the tree appears on screen")
write("app.rost", base)
pump()
check("added while running" not in screen.text(), "removing it removes it")

check("help" in screen.text() and "reloaded" in screen.text(), "the help panel is visible")
os.write(fd, b"h")
pump(0.6)
check("╔help" not in screen.text(), "key h (toggle:help) hides the help panel")
os.write(fd, b"h")
pump(0.6)
check("╔help" in screen.text(), "key h shows it again")

edit_app("kind: palette", "kind: widget", base)
check("kept; rejected at" in screen.text() and "rost live demo" in screen.text(), "an unknown widget kind is rejected at its line and the UI keeps running")
edit_app('text: "theme {theme_name}', 'text: "theme {themename}', base)
check("kept; rejected" in screen.text(), "an unknown {variable} is rejected by the validate hook")
edit_app('"theme:dark"', '"theme:purple"', base)
check("kept; rejected" in screen.text(), "a key bound to a theme that is not listed is rejected")
edit_app("theme: dark", "theme: purple", base)
check("kept; rejected" in screen.text(), "a theme that is not in `themes` is rejected")
write("app.rost", base)
pump()
check("✓ app.rost" in screen.text(), "restoring app.rost recovers")

edit_app('{ key: "q", action: "quit"', '{ key: "x", action: "quit"', base)
os.write(fd, b"q")
pump(0.6)
check(os.waitpid(pid, os.WNOHANG) == (0, 0), "q no longer quits once app.rost rebinds quit to x")
os.write(fd, b"x")
pump(0.6)
_, status = os.waitpid(pid, 0)
check(os.WIFEXITED(status) and os.WEXITSTATUS(status) == 0, "x (the rebound key) exits cleanly")
