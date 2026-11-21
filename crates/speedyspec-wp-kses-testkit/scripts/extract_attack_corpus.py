"""Copies the payloads of each fetched source into corpus/attacks/<source>.json (a JSON list of base64 strings), with
the source's license and a SOURCE.md naming the repository, commit and files.

Every downloaded file is read as data and parsed as text; nothing from the downloads is executed (the JavaScript
sources are parsed for their string literals, never imported). PortSwigger-derived lists are skipped: their owners
ask that the data not be republished.

Usage: python3 -I scripts/extract_attack_corpus.py <target/attack-sources> <corpus/attacks>
"""

import base64
import json
import os
import re
import subprocess
import sys

SOURCES, OUT = sys.argv[1], sys.argv[2]
os.makedirs(OUT, exist_ok=True)


def commit(name):
    return subprocess.run(["git", "-C", os.path.join(SOURCES, name), "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()


def read(path):
    with open(path, "rb") as f:
        return f.read()


def lines(path):
    return [l.rstrip(b"\r") for l in read(path).split(b"\n") if l.strip()]


ESCAPES = {b"n": b"\n", b"t": b"\t", b"r": b"\r", b"b": b"\b", b"f": b"\f", b"v": b"\v", b"0": b"\0"}


def js_string(text, at):
    """Parses one JavaScript string literal at text[at] (a quote); returns (bytes, end)."""
    quote = text[at:at + 1]
    out = bytearray()
    i = at + 1
    while i < len(text):
        c = text[i:i + 1]
        if c == b"\\":
            n = text[i + 1:i + 2]
            if n == b"u" and re.match(rb"[0-9a-fA-F]{4}", text[i + 2:i + 6]):
                out += chr(int(text[i + 2:i + 6], 16)).encode("utf-8", "surrogatepass")
                i += 6
                continue
            if n == b"x" and re.match(rb"[0-9a-fA-F]{2}", text[i + 2:i + 4]):
                out += chr(int(text[i + 2:i + 4], 16)).encode("utf-8")
                i += 4
                continue
            if n == b"\n":
                i += 2
                continue
            out += ESCAPES.get(n, n)
            i += 2
            continue
        if c == quote:
            return bytes(out), i + 1
        out += c
        i += 1
    raise ValueError("unterminated string")


def js_concatenation(text, at):
    """A string literal possibly joined with `+` to more literals; returns (bytes, end)."""
    value, end = js_string(text, at)
    while True:
        m = re.match(rb"\s*\+\s*(['\"])", text[end:])
        if not m:
            return value, end
        more, end = js_string(text, end + m.end() - 1)
        value += more


def write(id, repo, name, license, spdx, files, payloads):
    unique = list(dict.fromkeys(p for p in payloads if p))
    with open(os.path.join(OUT, id + ".json"), "w") as f:
        json.dump([base64.b64encode(p).decode() for p in unique], f)
        f.write("\n")
    with open(os.path.join(OUT, id + ".LICENSE"), "wb") as f:
        f.write(read(os.path.join(SOURCES, name, license)))
    with open(os.path.join(OUT, id + ".SOURCE.md"), "w") as f:
        f.write(f"# {id}\n\nFrom https://github.com/{repo} at commit {commit(name)} ({spdx}; see {id}.LICENSE).\n\n")
        f.write("Files: " + ", ".join(f"`{x}`" for x in files) + ".\n\n")
        f.write(f"{len(unique)} payloads, used only as test input for the kses port.\n")
    print(id, len(unique))


# Cure53 HTML5 Security Cheatsheet: every vector's `data`, and the XML/SVG/XSL/DTD/HTML attachments.
items = read(os.path.join(SOURCES, "H5SC", "items.js"))
payloads = []
for m in re.finditer(rb"['\"]data['\"]\s*:\s*(['\"])", items):
    payloads.append(js_concatenation(items, m.end() - 1)[0])
attachments = sorted(f for f in os.listdir(os.path.join(SOURCES, "H5SC", "attachments")) if re.search(r"\.(svg|xml|xsl|dtd|html|xhtml|mml)$", f))
payloads += [read(os.path.join(SOURCES, "H5SC", "attachments", f)) for f in attachments]
write("h5sc", "cure53/H5SC", "H5SC", "LICENSE", "MPL-2.0", ["items.js"] + ["attachments/" + f for f in attachments], payloads)

# DOMPurify's test fixtures: every case's `payload` string literal.
fixture = read(os.path.join(SOURCES, "DOMPurify", "test", "fixtures", "expect.mjs"))
payloads = [js_concatenation(fixture, m.end() - 1)[0] for m in re.finditer(rb"['\"]?payload['\"]?\s*:\s*(['\"])", fixture)]
write("dompurify", "cure53/DOMPurify", "DOMPurify", "LICENSE", "Apache-2.0 (dual-licensed MPL-2.0)", ["test/fixtures/expect.mjs"], payloads)

# PayloadsAllTheThings: the XSS Intruders lists.
d = os.path.join(SOURCES, "PayloadsAllTheThings", "XSS Injection", "Intruders")
files = sorted(f for f in os.listdir(d) if f.endswith(".txt") and not re.search("port_?swigger", f, re.I))
write("payloadsallthethings", "swisskyrepo/PayloadsAllTheThings", "PayloadsAllTheThings", "LICENSE", "MIT",
      ["XSS Injection/Intruders/" + f for f in files], [l for f in files for l in lines(os.path.join(d, f))])

# SecLists: the robot-friendly XSS lists and the polyglots.
base = os.path.join(SOURCES, "SecLists", "Fuzzing", "XSS")
files = [s + "/" + f for s in ("robot-friendly", "Polyglots") for f in sorted(os.listdir(os.path.join(base, s)))
         if f.endswith(".txt") and not re.search("portswigger", f, re.I)]
write("seclists", "danielmiessler/SecLists", "SecLists", "LICENSE", "MIT", ["Fuzzing/XSS/" + f for f in files],
      [l for f in files for l in lines(os.path.join(base, f))])

# OWASP's XSS Filter Evasion Cheat Sheet: every fenced code block.
file = "cheatsheets/XSS_Filter_Evasion_Cheat_Sheet.md"
text = read(os.path.join(SOURCES, "CheatSheetSeries", file))
write("owasp-evasion", "OWASP/CheatSheetSeries", "CheatSheetSeries", "LICENSE.md", "CC-BY-SA-4.0", [file],
      [m.group(1) for m in re.finditer(rb"^```[a-z]*\n(.*?)\n```", text, re.M | re.S)])
