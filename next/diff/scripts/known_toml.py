"""Reads the tables of `known-differences.toml` for the scripts.

The gates read the file with a TOML parser. These scripts run on a Python without `tomllib`, so
this reads the subset the file uses: `[[table]]` headers, `key = "basic string"`,
`key = 'literal string'` and `key = <integer>` lines, comments and blank lines. Any other line,
or a key given twice in one table, is an error.
"""
import re

HEADER = re.compile(r"^\[\[([A-Za-z_]+)\]\]$")
ENTRY = re.compile(r"""^([A-Za-z_]+) = (?:"([^"\\]*)"|'([^']*)'|(-?[0-9]+))$""")


def tables(path, name):
    """The `[[name]]` tables of the file at `path`, as dicts, in file order."""
    out, current = [], None
    with open(path) as f:
        for number, raw in enumerate(f, 1):
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            header = HEADER.match(line)
            if header:
                current = {}
                if header.group(1) == name:
                    out.append(current)
                continue
            entry = ENTRY.match(line)
            if entry is None or current is None:
                raise SystemExit("%s:%d: unsupported line: %s" % (path, number, line))
            key, basic, literal, integer = entry.groups()
            if key in current:
                raise SystemExit("%s:%d: `%s` given twice" % (path, number, key))
            if basic is not None:
                current[key] = basic
            elif literal is not None:
                current[key] = literal
            else:
                current[key] = int(integer)
    return out
