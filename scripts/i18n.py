#!/usr/bin/env python3
"""Keeps shellwing's translations in step with the code.

The English is in the code (`tr!("…")`, `tr_n!(n, "…", "…")`); each message's
id is the FNV-1a hash of its English, as crates/i18n computes it.

    scripts/i18n.py update   writes locales/en.ftl from the code, and brings
                             each translation up to date: kept where its
                             English still is, dropped where it is gone, a
                             commented line for each still to translate.
    scripts/i18n.py check    fails if en.ftl or a translation is out of step
                             (stale or broken entries); says how many are left
                             to translate in each.
    scripts/i18n.py todo LANG
                             prints what LANG still lacks: id, then English.
    scripts/i18n.py apply LANG FILE
                             takes translations from FILE, lines of an id, a
                             tab, and the text (`{name}` for a value, as in
                             the English), into LANG's file. A count's forms:
                             `[one] {n} file || [other] {n} files`, in the
                             language's plural categories, `other` last.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LOCALES = ROOT / "crates" / "i18n" / "locales"
# The i18n crate's own examples are not messages, nor what the UI tests say
# (they look words up with tr! to check the app speaks a language).
SOURCES = sorted(
    p
    for p in (ROOT / "crates").glob("*/src/**/*.rs")
    if p.parts[-3] != "i18n" and "ui_tests" not in p.parts
)

STRING = r'"((?:[^"\\]|\\.)*)"'
TR = re.compile(r"\btr!\(\s*" + STRING, re.S)
TR_N = re.compile(r"\btr_n!\(\s*[^;]*?,\s*" + STRING + r"\s*,\s*" + STRING, re.S)
ENTRY = re.compile(r"^(t[0-9a-f]{8})\s*=")


def unescape(s):
    """A Rust string literal's text."""
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c != "\\":
            out.append(c)
            i += 1
            continue
        n = s[i + 1]
        if n == "\n":
            # A line continuation: the newline and the next line's indent go.
            i += 2
            while i < len(s) and s[i] in " \t\n":
                i += 1
            continue
        if n == "u":
            end = s.index("}", i)
            out.append(chr(int(s[i + 3 : end], 16)))
            i = end + 1
            continue
        out.append({"n": "\n", "t": "\t", "r": "\r", "0": "\0"}.get(n, n))
        i += 2
    return "".join(out)


def id_of(english):
    h = 0x811C9DC5
    for b in english.encode("utf-8"):
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return "t%08x" % h


def messages():
    """The code's messages in order of first use: id -> (english, plural)."""
    found = {}
    for path in SOURCES:
        # Comments (docs that show tr! at work) are not code.
        text = "\n".join(
            "" if l.lstrip().startswith("//") else l
            for l in path.read_text(encoding="utf-8").splitlines()
        )
        hits = [(m.start(), unescape(m.group(1)), None) for m in TR.finditer(text)]
        hits += [
            (m.start(), unescape(m.group(1)), unescape(m.group(2)))
            for m in TR_N.finditer(text)
        ]
        for _, one, other in sorted(hits, key=lambda h: h[0]):
            english = one if other is None else one + "\n" + other
            found.setdefault(id_of(english), (one, other))
    return found


def fluent_text(english, indent="    "):
    """English as a Fluent value: `{name}` becomes `{ $name }`."""
    def piece(m):
        return "{ $" + m.group(1) + " }"

    escaped = []
    # Doubled braces are braces (a snippet's {{host}}), not a value.
    english = english.replace("{{", "\x00").replace("}}", "\x01")
    for part in re.split(r"(\{[A-Za-z_][A-Za-z0-9_]*\})", english):
        m = re.fullmatch(r"\{([A-Za-z_][A-Za-z0-9_]*)\}", part)
        if m:
            escaped.append(piece(m))
        else:
            escaped.append(part.replace("{", '{"{"}').replace("}", '{"}"}'))
    value = "".join(escaped)
    value = value.replace("\x00", '{"{{"}').replace("\x01", '{"}}"}')
    lines = value.split("\n")
    if len(lines) == 1:
        return " " + value
    return "\n" + "\n".join(indent + l for l in lines)


def comment(one, other):
    said = one if other is None else one + " | " + other
    return "# " + said.replace("\n", "\\n")


def en_ftl(found):
    out = [
        "# shellwing's English, from the code (scripts/i18n.py update writes it:",
        "# do not edit). The source for translations.",
        "",
    ]
    for mid, (one, other) in found.items():
        if other is None:
            out.append(mid + " =" + fluent_text(one))
        else:
            out.append(mid + " =")
            out.append("    { $n ->")
            out.append("        [one]" + fluent_text(one, " " * 12))
            out.append("       *[other]" + fluent_text(other, " " * 12))
            out.append("    }")
    return "\n".join(out) + "\n"


def read_catalog(path):
    """A translation file: its header (the comments it starts with, up to the
    first blank line), and its entries by id, each its lines."""
    lines = path.read_text(encoding="utf-8").splitlines()
    header = []
    for line in lines:
        if not line.startswith("#"):
            break
        header.append(line)
    entries, current = {}, None
    for line in lines[len(header):]:
        m = ENTRY.match(line)
        if m:
            current = m.group(1)
            entries[current] = [line]
        elif current and line[:1] in (" ", "\t") and line.strip():
            entries[current].append(line)
        else:
            current = None
    return header, entries


def write_catalog(path, found, header, entries):
    out = list(header) + [""]
    for mid, (one, other) in found.items():
        out.append(comment(one, other))
        if mid in entries:
            out.extend(entries[mid])
        else:
            out.append("# " + mid + " =")
        out.append("")
    path.write_text("\n".join(out).rstrip("\n") + "\n", encoding="utf-8")


def translations():
    return sorted(p for p in LOCALES.glob("*.ftl") if p.stem != "en")


def update():
    found = messages()
    (LOCALES / "en.ftl").write_text(en_ftl(found), encoding="utf-8")
    for path in translations():
        header, entries = read_catalog(path)
        write_catalog(path, found, header, entries)
        left = sum(1 for m in found if m not in entries)
        print(f"{path.name}: {len(found) - left} of {len(found)} translated")


def check():
    found = messages()
    ok = True
    en = LOCALES / "en.ftl"
    if not en.exists() or en.read_text(encoding="utf-8") != en_ftl(found):
        print("en.ftl is out of step with the code: run scripts/i18n.py update")
        ok = False
    for path in translations():
        _, entries = read_catalog(path)
        stale = [m for m in entries if m not in found]
        if stale:
            print(f"{path.name}: {len(stale)} entries no longer in the code: run update")
            ok = False
        for mid, lines in entries.items():
            if mid in found and found[mid][1] is None:
                english = found[mid][0].replace("{{", "").replace("}}", "")
                names = set(re.findall(r"\{([A-Za-z_][A-Za-z0-9_]*)\}", english))
                used = set(re.findall(r"\$([A-Za-z_][A-Za-z0-9_-]*)", "\n".join(lines)))
                if used - names:
                    print(f"{path.name}: {mid} uses {sorted(used - names)}, which its English has not")
                    ok = False
        left = sum(1 for m in found if m not in entries)
        print(f"{path.name}: {len(found) - left} of {len(found)} translated")
    return ok


def todo(lang):
    found = messages()
    _, entries = read_catalog(LOCALES / f"{lang}.ftl")
    for mid, (one, other) in found.items():
        if mid not in entries:
            print(mid + "\t" + (one if other is None else one + " | " + other).replace("\n", "\\n"))


def plural_entry(mid, text):
    """A count's forms, `[one] … || [few] … || [other] …`, as a Fluent
    select on `$n` (the last is the default)."""
    forms = []
    for part in text.split("||"):
        m = re.match(r"\s*\[([a-z]+)\]\s*(.*)$", part, re.S)
        if not m:
            raise SystemExit(f"{mid}: a plural form must start with [one], [few], [many] or [other]: {part!r}")
        forms.append((m.group(1), m.group(2).strip()))
    lines = [mid + " =", "    { $n ->"]
    for i, (cat, form) in enumerate(forms):
        star = "*" if i == len(forms) - 1 else " "
        lines.append("       " + star + "[" + cat + "]" + fluent_text(form, " " * 12))
    lines.append("    }")
    return lines


def apply(lang, tsv):
    found = messages()
    path = LOCALES / f"{lang}.ftl"
    header, entries = read_catalog(path)
    taken = 0
    for line in pathlib.Path(tsv).read_text(encoding="utf-8").splitlines():
        if not line.strip() or "\t" not in line:
            continue
        mid, text = line.split("\t", 1)
        mid = mid.strip()
        if mid not in found:
            print(f"{mid}: not a message of the code; skipped")
            continue
        text = text.replace("\\n", "\n")
        entries[mid] = plural_entry(mid, text) if text.lstrip().startswith("[") else [
            mid + " =" + fluent_text(text)
        ]
        taken += 1
    write_catalog(path, found, header, entries)
    print(f"{path.name}: {taken} taken")


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "check"
    if what == "update":
        update()
    elif what == "check":
        sys.exit(0 if check() else 1)
    elif what == "todo":
        todo(sys.argv[2])
    elif what == "apply":
        apply(sys.argv[2], sys.argv[3])
    else:
        print(__doc__)
        sys.exit(2)
