#!/usr/bin/env python3
"""Lists every function and every definition-file key of the Wintermute
Engine 1.x core, one row each, into the tables of this directory.

    python3 docs/port/inventory.py <wintermute-engine checkout>
    python3 docs/port/inventory.py --check

The first form reads `src/engine_core/wme_base` and `wme_ad` of the MIT
sources (https://github.com/MnemonicWME/wintermute-engine) and rewrites
the tables. The list comes from the sources, never from memory: a
function is every definition `Class::Method(` at the start of a line, a
key is every entry of a `TOKEN_TABLE` block. The status and the note of
a row already in a table are kept, so the tables can be regenerated at
any time; a row that is new gets the status `unreviewed`.

The second form reads the tables only and prints the counts per status;
it fails when a status is not one of the known ones.
"""

import re
import sys
from collections import OrderedDict
from pathlib import Path

HERE = Path(__file__).resolve().parent

STATUSES = OrderedDict(
    [
        ("unreviewed", "nobody has compared it with Luma yet"),
        ("done", "Luma does the same; the note says where"),
        ("partial", "Luma does part of it; the note says what is missing"),
        ("todo", "Luma does not do it and should"),
        ("replaced", "Luma reaches the same result another way; the note says how"),
        ("n/a", "has no meaning outside the original program; the note says why"),
        ("excluded", "left out by a decision of the maintainer; the note says which and when"),
    ]
)

# Which table a source file goes to: the first rule that matches its name.
GROUPS = [
    ("adventure", "The adventure game: scenes, actors, entities, items, dialogue", r"^(Ad|UIEntity)"),
    ("platform", "Windows, settings dialogs, hardware detection, plugins, network, debugger",
     r"^(Dlg|Win[A-Z]|HW|PlatformWin|ThemeLibEx|BDebugger|BNetworkMgr|BSocket|BPlugin|SXPluginObject|SXTcpClient|utils_getdxver|wme_plugin|StdAfx|BRegistry)"),
    ("render", "Renderers, surfaces, 3D models and scenes",
     r"^(BRender|BSurface|BImage|3D|X[A-Z]|SkinMeshHelper)"),
    ("sound-video", "Sound and video", r"^(BSound|Vid)"),
    ("ui-text", "Windows and controls, fonts, text", r"^(UI|BFont|BStringTable|BStringUtil|BTextUtils)"),
    ("script", "The script engine, its classes, saved games",
     r"^(Sc[A-Z]|SX|SxObject|Sys|BPersistMgr|BScriptHolder|BScriptable|BSaveThumb)"),
    ("files", "Packages, files, the definition-file parser, utilities",
     r"^(BFile|BDiskFile|BPackage|BPkgFile|BResourceFile|BDynBuffer|BParser|BParamSet|crc|utils|jpeg)"),
    ("game", "The game, its objects, sprites, regions, effects", r""),
]

METHOD = re.compile(
    r"^(?!\s)(?!(?:return|if|else|case|while|for|switch|delete|new|typedef|using|namespace|#)\b)"
    r"(?:[\w:\*&<>,\s]*?[\s\*&])?(~?\w+(?:::\w+)*)::(~?\w+|operator\s*\S+?)\s*\("
)
# A function that belongs to no class: a return type, a name, an opening
# parenthesis, at the start of a line that is not a declaration.
FREE = re.compile(
    r"^(?!\s)(?!(?:return|if|else|case|while|for|switch|delete|new|typedef|using|namespace|extern|#)\b)"
    r"(?:[\w\*&<>,\s]+?[\s\*&])(\w+)\s*\([^;]*$"
)
TOKEN = re.compile(r"^\s*TOKEN_TABLE\s*\(\s*(\w+)\s*\)")
ROW = re.compile(r"^\| `([^`]+)` \| ([^|]+?) \| (.*?) \|$")


def group_of(stem):
    for name, _, rule in GROUPS:
        if re.search(rule, stem):
            return name
    raise AssertionError(stem)


def uncommented(text):
    """The text without its comments (old code lives in them), strings
    and line breaks kept, so that `//*` or a `/*` inside a string does
    not swallow the code that follows."""
    out = []
    i, n = 0, len(text)
    while i < n:
        two = text[i : i + 2]
        if two == "//":
            while i < n and text[i] != "\n":
                i += 1
        elif two == "/*":
            end = text.find("*/", i + 2)
            end = n if end < 0 else end + 2
            out.append("\n" * text.count("\n", i, end))
            i = end
        elif text[i] in "\"'":
            quote = text[i]
            j = i + 1
            while j < n and text[j] != quote and text[j] != "\n":
                j += 2 if text[j] == "\\" else 1
            out.append(quote + quote)
            i = min(j + 1, n)
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def scan(path):
    """The functions of a source file in order, each with the keys of
    its `TOKEN_TABLE` block."""
    functions = []
    text = uncommented(path.read_text(encoding="latin-1"))
    depth = 0
    for line in text.splitlines():
        line = line.rstrip()
        # Definitions stand outside every block; what is inside one is a
        # call or a declaration.
        if depth == 0 and not line.endswith(";"):
            found = METHOD.match(line)
            free = None if found else FREE.match(line)
            if found:
                functions.append((f"{found.group(1)}::{found.group(2)}", []))
            elif free:
                functions.append((free.group(1), []))
        depth = max(0, depth + line.count("{") - line.count("}"))
        key = TOKEN.match(line)
        if key and functions and key.group(1) not in functions[-1][1]:
            functions[-1][1].append(key.group(1))
    return functions


def rows_of(functions):
    """One row name per function (a repeated name, an overload, gets a
    number) and one per key, `Class::Method KEY`."""
    seen = {}
    rows = []
    for name, keys in functions:
        seen[name] = seen.get(name, 0) + 1
        shown = name if seen[name] == 1 else f"{name} #{seen[name]}"
        rows.append((shown, False))
        for key in keys:
            rows.append((f"{shown} {key}", True))
    return rows


def read_tables():
    """What the tables hold: {table: {file: {row: (status, note)}}}."""
    kept = {}
    for name, _, _ in GROUPS:
        path = HERE / f"{name}.md"
        files = kept.setdefault(name, OrderedDict())
        if not path.exists():
            continue
        current = None
        for line in path.read_text(encoding="utf-8").splitlines():
            if line.startswith("## "):
                current = files.setdefault(line[3:].strip().strip("`"), OrderedDict())
            row = ROW.match(line)
            if row and current is not None:
                current[row.group(1)] = (row.group(2).strip(), row.group(3).strip())
    return kept


def is_key(row):
    return bool(re.search(r" [A-Z][A-Z0-9_]*$", row))


def write(tables, titles):
    for name, files in tables.items():
        out = [f"# Port inventory: {titles[name].lower()}", ""]
        out.append(
            "Every function and definition-file key of these Wintermute 1.x source "
            "files. Generated by `inventory.py`; edit only the status and the note "
            "of a row. See [README.md](README.md) for the statuses."
        )
        for file, rows in files.items():
            out += ["", f"## `{file}`", "", "| Function or key | Status | Luma, or what is missing |", "|---|---|---|"]
            for row, (status, note) in rows.items():
                out.append(f"| `{row}` | {status} | {note} |")
        (HERE / f"{name}.md").write_text("\n".join(out) + "\n", encoding="utf-8")


def summary(tables, titles):
    lines = ["| Table | Functions | Keys | " + " | ".join(STATUSES) + " |", "|---|---|---|" + "---|" * len(STATUSES)]
    grand = [0, 0] + [0] * len(STATUSES)
    for name, files in tables.items():
        functions = keys = 0
        by_status = OrderedDict((status, 0) for status in STATUSES)
        for rows in files.values():
            for row, (status, _) in rows.items():
                if status not in STATUSES:
                    sys.exit(f"unknown status '{status}' on `{row}` in {name}.md")
                by_status[status] += 1
                if is_key(row):
                    keys += 1
                else:
                    functions += 1
        cells = [functions, keys] + list(by_status.values())
        grand = [a + b for a, b in zip(grand, cells)]
        lines.append(f"| [{titles[name]}]({name}.md) | " + " | ".join(str(c) for c in cells) + " |")
    lines.append("| **Total** | " + " | ".join(f"**{c}**" for c in grand) + " |")
    return lines


def main():
    titles = {name: title for name, title, _ in GROUPS}
    kept = read_tables()
    if sys.argv[1:] == ["--check"]:
        print("\n".join(summary(kept, titles)))
        return
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    core = Path(sys.argv[1]) / "src" / "engine_core"
    if not (core / "wme_base").is_dir() or not (core / "wme_ad").is_dir():
        sys.exit(f"no Wintermute sources under {core}: refusing to rewrite the tables")
    tables = OrderedDict((name, OrderedDict()) for name, _, _ in GROUPS)
    for directory in ("wme_ad", "wme_base"):
        for path in sorted((core / directory).glob("*.cpp"), key=lambda p: p.name.lower()):
            functions = scan(path)
            if not functions:
                continue
            table = group_of(path.stem)
            file = f"{directory}/{path.name}"
            old = kept.get(table, {}).get(file, {})
            # A file may have moved to another table: look everywhere.
            if not old:
                for files in kept.values():
                    old = files.get(file, old)
            tables[table][file] = OrderedDict(
                (row, old.get(row, ("unreviewed", ""))) for row, _ in rows_of(functions)
            )
    if sum(len(rows) for files in tables.values() for rows in files.values()) == 0:
        sys.exit("found no functions in the sources: refusing to rewrite the tables")
    write(tables, titles)
    start, end = "<!-- counts -->", "<!-- /counts -->"
    readme = HERE / "README.md"
    text = readme.read_text(encoding="utf-8")
    head, rest = text.split(start, 1)
    _, tail = rest.split(end, 1)
    readme.write_text(head + start + "\n" + "\n".join(summary(tables, titles)) + "\n" + end + tail, encoding="utf-8")
    print("\n".join(summary(tables, titles)))


if __name__ == "__main__":
    main()
