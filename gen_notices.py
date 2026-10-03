import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.abspath(__file__))
NOTICES = os.path.join(ROOT, "THIRD_PARTY_NOTICES.txt")
META = json.loads(subprocess.run(
    ["cargo", "metadata", "--format-version", "1", "--locked", "--all-features"],
    cwd=ROOT, check=True, stdout=subprocess.PIPE,
).stdout)
RULE = "-" * 80


def section(title):
    return ["", RULE, title, RULE, ""]


def read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read().replace("\r\n", "\n").replace("\r", "\n")


def norm_text(text):
    lines = [line.rstrip() for line in text.split("\n")]
    out = []
    for line in lines:
        if line == "" and (not out or out[-1] == ""):
            continue
        out.append(line)
    while out and out[-1] == "":
        out.pop()

    return "\n".join(out)


lock = read(os.path.join(ROOT, "Cargo.lock"))
locked = re.findall(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"(?:\nsource = "([^"]+)")?', lock)
by_key = {(p["name"], p["version"]): p for p in META["packages"]}
crates = []
local = []
for name, version, source in locked:
    if not source:
        local.append(name)
        continue
    if (name, version) not in by_key:
        sys.exit("missing metadata for %s %s" % (name, version))
    crates.append(by_key[(name, version)])
crates.sort(key=lambda p: (p["name"].lower(), [(0, int(x), "") if x.isdigit() else (1, 0, x) for x in re.split(r"[.+-]", p["version"])]))


def crate(name):
    for p in crates:
        if p["name"] == name:
            return p
    sys.exit("Cargo.lock no longer contains %s; update gen_notices.py" % name)


def crate_dir(p):
    return os.path.dirname(p["manifest_path"])


def label(p):
    return "%s %s" % (p["name"], p["version"])


def expr(p):
    lic = p["license"] or ""
    lic = re.sub(r"\s*/\s*", " OR ", lic)

    return lic


def choices(p):
    lic = expr(p)
    if " AND " in lic:
        return None

    return [c.strip("() ") for c in lic.split(" OR ")]


LICENSE_NAME = re.compile(r"(?i)^(licen[cs]e|copying|copyright|notice|unlicense)")


def license_files(p):
    d = crate_dir(p)
    files = [f for f in os.listdir(d) if LICENSE_NAME.match(f) and os.path.isfile(os.path.join(d, f))]
    rank = lambda f: (0 if "MIT" in f.upper() else 1 if f.upper() in ("LICENSE", "LICENSE.MD", "LICENSE.TXT") else 2, f)

    return [os.path.join(d, f) for f in sorted(files, key=rank)]


TEMPLATE = re.compile(
    r"(?i)\[yyyy\]|\{yyyy\}|<year>|\[year\]|\{year\}|<copyright|\[name of|\{name of|"
    r"copyright (notice|license|owner|holder|statement|and permission|and license|and related)|"
    r"copyright notices|copyright,? patent|copyright law|copyright interest|subject to copyright|"
    r"copyright assignment|copyright to\b|copyright in\b"
)
COPYRIGHT_LINE = re.compile(r"(?i)^[\s#*/;!-]*((copyright\b|©).*)$")


def copyright_lines(paths):
    out = []
    for path in paths:
        try:
            text = read(path)
        except OSError:
            continue
        for line in text.split("\n"):
            m = COPYRIGHT_LINE.match(line)
            if not m:
                continue
            s = re.sub(r"\s+", " ", m.group(1)).strip().rstrip(",;")
            s = re.sub(r"\s+or$", "", s)
            if TEMPLATE.search(s) or len(s) < 12:
                continue
            if not re.search(r"\d{4}|[A-Z][a-z]", s[9:]):
                continue
            if s not in out:
                out.append(s)

    return out


def authors(p):
    names = [re.sub(r"\s*<[^>]*>", "", a).strip().rstrip(".") for a in p.get("authors") or []]

    return [n for n in names if n]


NOTES = {
    "android-activity": "Rust code is used under the MIT terms. The bundled GameActivity C/C++\n  sources are Apache-2.0; see the Android GameActivity section.",
    "dpi": "Both licenses apply. The Apache-2.0 text is above; the MIT notice for\n  code taken from rust-lang/libm is in the copyright lines here.",
    "epaint_default_fonts": "Crate code is used under the MIT terms. The bundled fonts are listed\n  in the egui default fonts section.",
    "lua-src": "Bundles Lua 5.1 to 5.5 sources, MIT, Copyright (C) 1994-2026 Lua.org, PUC-Rio.",
    "luajit-src": "Bundles LuaJIT; see the LuaJIT section.",
    "moxcms": "Used under the Apache-2.0 terms above.",
    "pxfm": "Used under the Apache-2.0 terms above.",
    "r-efi": "Used under the MIT terms. The LGPL-2.1-or-later option is not used.",
    "steamworks-sys": "Links the prebuilt Steam API library; see the Steamworks SDK section.",
    "unicode-ident": "Crate code is used under the MIT terms. The Unicode data is under the\n  Unicode License V3 in the section above.",
    "zstd-sys": "Bundles the Zstandard library; see the Zstandard section.",
    "hexf-parse": "CC0-1.0 is a public domain dedication and requires no notice.",
    "lzma": "WTFPL requires no notice. The package ships no license file.",
}


def needs_full_text(p):
    c = choices(p)
    if c is None:
        parts = re.split(r"\s+(?:AND|OR)\s+", expr(p).replace("(", "").replace(")", ""))

        return any(x not in ("MIT", "Apache-2.0") for x in parts)
    if "MIT" in c or all(x.startswith("Apache-2.0") for x in c):
        return False
    if "Apache-2.0" in c:
        return False

    return c[0] not in ("CC0-1.0", "WTFPL", "Unlicense")


def full_text_files(p):
    out = []
    for path in license_files(p):
        text = read(path)
        name = os.path.basename(path).upper()
        if "APACHE" in name or re.match(r"\s*Apache License", text):
            continue
        if "MIT" in name and p["name"] in ("unicode-ident", "dpi"):
            continue
        out.append(path)

    return out


EXTRA_TEXT = {
    "clipboard-win": [os.path.join(crate_dir(crate("error-code")), "LICENSE")],
}

groups = []
for p in crates:
    if p["name"] == "epaint_default_fonts" or not needs_full_text(p):
        continue
    if p["name"].startswith("zstd"):
        files = [f for f in full_text_files(p) if not f.endswith("LICENSE.BSD-3-Clause")]
    else:
        files = full_text_files(p) or EXTRA_TEXT.get(p["name"], [])
    if not files:
        sys.exit("no license text for %s" % label(p))
    text = "\n\n".join(norm_text(read(f)) for f in files)
    for g in groups:
        if g["text"] == text:
            g["crates"].append(p)
            break
    else:
        groups.append({"text": text, "crates": [p], "expr": expr(p)})


def between(path, start, end):
    text = read(os.path.join(ROOT, path))
    begin = text.find(start)
    if begin < 0:
        sys.exit("%s no longer contains %r" % (path, start))
    stop = text.find(end, begin)

    return norm_text(text[begin:stop])


def apache_text():
    counts = {}
    for p in crates:
        for path in license_files(p):
            if "APACHE" not in os.path.basename(path).upper():
                continue
            text = norm_text(read(path))
            stop = text.find("END OF TERMS AND CONDITIONS")
            if text.lstrip().startswith("Apache License") and stop >= 0:
                text = text[: stop + len("END OF TERMS AND CONDITIONS")]
                counts[text] = counts.get(text, 0) + 1
    if not counts:
        sys.exit("no crate in Cargo.lock ships the Apache-2.0 text")

    return max(counts, key=lambda text: (counts[text], text))


def join_names(names):
    if len(names) == 1:
        return names[0]

    return ", ".join(names[:-1]) + " and " + names[-1]


def wrap(text, width=80, indent=""):
    words = text.split()
    lines = []
    cur = indent
    for w in words:
        if len(cur) + len(w) + (0 if cur.strip() == "" else 1) > width:
            lines.append(cur.rstrip())
            cur = indent + w
        else:
            cur = cur + ("" if cur.strip() == "" else " ") + w
    if cur.strip():
        lines.append(cur.rstrip())

    return lines


luajit = crate("luajit-src")
mlua = crate("mlua")
zstd_sys = crate("zstd-sys")
steam = crate("steamworks-sys")
fonts = crate("epaint_default_fonts")
activities = [p for p in crates if p["name"] == "android-activity"]

out = [
    "Third-party notices",
    "",
    "Distribute this file with binaries of this engine and with games built from it.",
    "It collects the license notices for code and the fonts that are compiled into",
    "the binary. The engine's own license is also reproduced so this file can stand",
    "alone next to an executable.",
    "",
    "Crate versions below are the ones pinned in Cargo.lock. License texts were read",
    "from those crate versions.",
]

out += section("This engine")
out += [
    "Source: this repository",
    "License: MIT",
    "",
    norm_text(read(os.path.join(ROOT, "LICENSE"))),
]

out += section("OpenVR")
out += [
    "Source: game/base/third_party/openvr",
    "Copyright (c) 2015, Valve Corporation",
    "License: BSD-3-Clause",
    "",
    norm_text(read(os.path.join(ROOT, "game/base/third_party/openvr/LICENSE"))),
]

out += section("miniaudio")
out += [
    "Source: game/base/third_party/miniaudio/miniaudio.h",
    "Author: David Reid",
    "License: Public Domain (Unlicense) or MIT No Attribution, at your option",
    "",
    between("game/base/third_party/miniaudio/miniaudio.h", "This software is available as a choice of the following licenses.", "*/"),
]

out += section("JsonCpp")
out += [
    "Source: game/base/third_party/openvr/src/jsoncpp.cpp",
    "Copyright (c) 2007-2010 Baptiste Lepilleur",
    "License: Public domain, or MIT where public domain is not recognized",
    "",
    between("game/base/third_party/openvr/src/jsoncpp.cpp", "The JsonCpp library's source code", "*/"),
]

out += section("Roboto")
out += [
    "Source: game/base/src/ui/font_default.ttf",
    "Name: Roboto Regular, version 2.001101 (2014)",
    "Copyright 2011 Google Inc. All Rights Reserved.",
    "Designer: Christian Robertson",
    "Trademark: Roboto is a trademark of Google.",
    "License: Apache License, Version 2.0",
    "http://www.apache.org/licenses/LICENSE-2.0",
    "",
    "The Apache License, Version 2.0, is reproduced in the next section. It also",
    "covers the other Apache-2.0 components listed further down.",
]

out += section("The Apache License, Version 2.0")
out.append(apache_text())

out += section("Qwen")
out += [
    "Source: models/qwen.gguf",
    "Packed into base.pak as models/qwen.gguf.",
    "Name: Qwen2.5-0.5B-Instruct, Q4_K_M GGUF",
    "Copyright 2024 Alibaba Cloud",
    "License: Apache License, Version 2.0, reproduced above.",
    "http://www.apache.org/licenses/LICENSE-2.0",
]

def wiki_font_parts(path):
    text = read(path)
    marker = "SIL OPEN FONT LICENSE"
    stop = text.find(marker)
    if stop < 0:
        sys.exit("%s has no OFL text" % path)
    head = []
    for line in text[:stop].split("\n"):
        line = line.strip()
        if not line or set(line) <= set("-"):
            continue
        head.append(line)
    body = "\n".join(line.rstrip() for line in text[stop:].split("\n")).strip()

    return head, body

wiki_font_dir = os.path.join(ROOT, "game/wiki/fonts")
wiki_fonts = [
    (
        "Newsreader",
        "Regular and Italic. A Latin subset is embedded in the scripting wiki.",
        "newsreader-OFL.txt",
    ),
    (
        "Manrope",
        "Regular and SemiBold. A Latin subset is embedded in the scripting wiki.",
        "manrope-OFL.txt",
    ),
    (
        "JetBrains Mono",
        "Regular, version 2.304. A Latin subset is embedded in the scripting wiki.",
        "jetbrains-OFL.txt",
    ),
]
wiki_entries = []
wiki_body = None
for title, note, filename in wiki_fonts:
    head, body = wiki_font_parts(os.path.join(wiki_font_dir, filename))
    if wiki_body is None:
        wiki_body = body
    elif body != wiki_body:
        sys.exit("wiki font OFL texts diverged; update gen_notices.py")
    wiki_entries.append((title, note, head))

out += section("Scripting wiki fonts")
out += [
    "Source: game/wiki/fonts",
    "Embedded in the scripting wiki (wiki.html, and wiki/index.html inside base.pak).",
    "The shipped files are Latin subsets. Reserved font names are unchanged.",
    "License: SIL Open Font License, Version 1.1",
    "",
]
for title, note, head in wiki_entries:
    out += [title, note] + head + [""]
out.append(wiki_body)

out += section("LuaJIT")
out += [
    "Source: LuaJIT, vendored by the %s crate (features luajit52, vendored)" % label(mlua),
    "         through %s" % label(luajit),
    "License file reproduced from luajit2/COPYRIGHT:",
    "",
    norm_text(read(os.path.join(crate_dir(luajit), "luajit2", "COPYRIGHT"))),
]

out += section("Zstandard")
out += [
    "Bundled in %s. The library is BSD. The zstd command-line program is not" % label(zstd_sys),
    "compiled into this engine.",
    "",
]
zstd_license = norm_text(read(os.path.join(crate_dir(zstd_sys), "zstd", "LICENSE")))
out.append(zstd_license)

out += section("Android GameActivity")
ga_lines = []
for p in activities:
    for r, ds, fs in os.walk(crate_dir(p)):
        for f in fs:
            if f.endswith((".c", ".cpp", ".h")):
                for line in copyright_lines([os.path.join(r, f)]):
                    if line not in ga_lines:
                        ga_lines.append(line)
out += wrap(
    "Bundled in %s as C/C++ sources from the Android Games SDK. Only compiled for Android targets."
    % join_names([label(p) for p in activities])
)
out += [
    "License: Apache License, Version 2.0 (text above)",
    "",
] + sorted(ga_lines)

out += section("Steamworks SDK")
out += [
    "Linked by %s when the engine is built with the steam feature." % label(steam),
    "The steam_api redistributable libraries are Copyright (c) Valve Corporation.",
    "They are not open source and are distributed under the Steamworks SDK Access",
    "Agreement: https://partner.steamgames.com/documentation/sdk_access_agreement",
]

out += section("egui default fonts")
out += [
    "Bundled in %s and compiled into the launcher." % label(fonts),
    "",
]
font_files = [
    ("Hack Regular", "Hack-Regular.txt"),
    ("Ubuntu Light, Ubuntu Font Licence 1.0", "UFL.txt"),
    ("Noto Emoji, SIL Open Font License 1.1", "OFL.txt"),
    ("emoji-icon-font, MIT", "emoji-icon-font-mit-license.txt"),
]
for idx, (title, f) in enumerate(font_files):
    if idx:
        out.append("")
    out += ["=" * 79, title, "=" * 79, "", norm_text(read(os.path.join(crate_dir(fonts), "fonts", f)))]

out += section("Other license texts required by Cargo.lock")
out += [
    "These texts are in addition to the MIT and Apache-2.0 texts above. A crate is",
    "named under the text that applies to it.",
]
for g in groups:
    out += [""] + wrap(join_names([label(p) for p in g["crates"]])) + ["  " + g["expr"], "", g["text"]]

out += section("Rust crates from Cargo.lock")
local_names = sorted(set(local))
out += wrap(
    "Every registry crate in Cargo.lock is listed below, for every target platform. "
    "Local crates %s are the engine itself, covered by the MIT license at the top." % join_names(local_names)
)
out += [""]
out += wrap(
    "Where a crate offers a choice, this distribution uses MIT if MIT is one of the choices, "
    "and otherwise the license noted for that crate. MIT-licensed crates use the MIT terms "
    "reproduced at the top with the copyright lines given here in place of the engine's. "
    "Apache-2.0 crates use the Apache License text above."
)
out += ["", "%d crates." % len(crates)]

for p in crates:
    files = license_files(p)
    lines = copyright_lines(files)
    if p["name"] == "zstd-sys":
        lines = [l for l in lines if "Meta Platforms" not in l and "Facebook" not in l]
    out += ["", label(p), "  " + expr(p)]
    if lines:
        out += ["  " + l for l in lines]
    else:
        names = authors(p)
        if names:
            out += wrap("Authors: " + ", ".join(names) + ". The package has no separate copyright line.", indent="  ")
        else:
            out.append("  %s contributors. The package has no separate copyright line." % p["name"])
    if p["name"] in NOTES:
        out.append("  " + NOTES[p["name"]])

text = "\n".join(out).rstrip("\n") + "\n"
open(NOTICES, "w", encoding="utf-8").write(text)
print(len(crates), "crates,", len(groups), "text groups,", text.count("\n"), "lines")
