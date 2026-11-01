#!/usr/bin/env python3
"""Drives the app created by scripts/rost-init.sh in a pseudo-terminal.

    scripts/rost-init.sh /tmp/demo && python3 scripts/demo-test.py /tmp/demo

Edits files while the app runs and checks what reaches the screen: a live edit, a broken
edit (rejected with its line), the recovery, `t` patching app.rost, a rejected theme name
and a clean exit.
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

demo = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "rost-demo")
os.chdir(demo)
target = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"]))["target_directory"]
binary = os.path.join(target, "debug", "rost-demo")

pid, fd = pty.fork()
if pid == 0:
    os.execv(binary, ["rost-demo"])
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))

buf = b""


def pump(seconds):
    global buf
    end = time.time() + seconds
    while time.time() < end:
        ready, _, _ = select.select([fd], [], [], 0.05)
        if ready:
            try:
                buf += os.read(fd, 65536)
            except OSError:
                return


def plain(data):
    return re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]", b"", data).decode("utf8", "replace")


def check(condition, message):
    print(("PASS " if condition else "FAIL ") + message)
    if not condition:
        os.kill(pid, 9)
        sys.exit(1)


def read(path):
    with open(path) as f:
        return f.read()


def write(path, text):
    with open(path, "w") as f:
        f.write(text)


pump(1.0)
check("rost live demo" in plain(buf) and "Dark" in plain(buf), "first frame shows the title and the Dark theme")
check(b"137;180;250" in buf, "the accent #89b4fa from themes/dark.rost is used")

mark = len(buf)
dark = read("themes/dark.rost")
write("themes/dark.rost", dark.replace("#89b4fa", "#ff0000"))
pump(1.0)
check(b"255;0;0" in buf[mark:], "editing dark.rost repaints with the new accent")

mark = len(buf)
write("themes/dark.rost", 'name: "Dark"\nbg: nonsense\n')
pump(1.0)
shown = plain(buf[mark:])
check("kept; rejected at 2:1" in shown, "a broken edit is rejected with its line and the last good theme stays")

write("themes/dark.rost", dark.replace("#89b4fa", "#00ff00"))
pump(1.0)
check(b"0;255;0" in buf, "fixing the file recovers")

before = read("app.rost")
os.write(fd, b"t")
pump(1.0)
after = read("app.rost")
check("theme: light" in after and after.replace("theme: light", "theme: dark") == before, "t changes only the theme value in app.rost, comments kept")
check("Light" in plain(buf[-20000:]), "the screen switched to the Light theme after the reload")

mark = len(buf)
write("app.rost", after.replace("theme: light", "theme: purple"))
pump(1.0)
check("no theme named `purple`" in plain(buf[mark:]), "a theme name that does not exist is rejected")

os.write(fd, b"q")
pump(0.5)
_, status = os.waitpid(pid, 0)
check(os.WIFEXITED(status) and os.WEXITSTATUS(status) == 0, "q exits cleanly")
