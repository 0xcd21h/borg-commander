#!/usr/bin/env python3
import argparse
import csv
import io
import pathlib
import re
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
TABLE = ROOT / "crates" / "borg-core" / "src" / "network" / "neighbors" / "vendors.tsv"
REGISTRY = "https://standards-oui.ieee.org/oui/oui.csv"
USER_AGENT = "borg-commander vendor table (https://github.com/0xcd21h/borg-commander)"
LARGE_BLOCK = "MA-L"
ASSIGNMENT_DIGITS = 6
SHOUTING_FROM = 5
SUFFIXES = {
    "a/s",
    "ab",
    "ag",
    "as",
    "asa",
    "b.v",
    "bv",
    "co",
    "co.kg",
    "company",
    "corp",
    "corporation",
    "gmbh",
    "inc",
    "incorporated",
    "k.k",
    "kg",
    "kk",
    "l.l.c",
    "lda",
    "limited",
    "llc",
    "ltd",
    "ltda",
    "n.v",
    "nv",
    "oy",
    "plc",
    "pte",
    "pty",
    "s.a",
    "s.a.s",
    "s.l",
    "s.p.a",
    "s.r.l",
    "sa",
    "sarl",
    "sas",
    "sl",
    "spa",
    "srl",
}
SUFFIX = re.compile(r"[\s,.]*([A-Za-z/.&]+)[\s,.]*$")
SPACES = re.compile(r"\s+")


def without_suffixes(name: str) -> str:
    while True:
        match = SUFFIX.search(name)
        if not match or match.start() == 0:
            return name
        word = match.group(1).strip(".").lower()
        if word not in SUFFIXES:
            return name.rstrip(" ,.")
        name = name[: match.start()]


def shouted(name: str) -> bool:
    letters = [c for c in name if c.isalpha()]
    return bool(letters) and all(c.isupper() for c in letters)


def calmed(part: str) -> str:
    if sum(c.isalpha() for c in part) < SHOUTING_FROM:
        return part
    return part[:1].upper() + part[1:].lower()


def short_name(name: str) -> str:
    name = SPACES.sub(" ", name).strip()
    trimmed = without_suffixes(name) or name
    if not shouted(trimmed):
        return trimmed
    return " ".join("-".join(calmed(part) for part in word.split("-")) for word in trimmed.split(" "))


def rows(text: str) -> dict[str, str]:
    table = {}
    for row in csv.DictReader(io.StringIO(text)):
        assignment = row["Assignment"].strip().upper()
        name = row["Organization Name"].strip()
        if row["Registry"] != LARGE_BLOCK or len(assignment) != ASSIGNMENT_DIGITS or not name:
            continue
        int(assignment, 16)
        table[assignment] = short_name(name)
    return table


def main() -> int:
    parser = argparse.ArgumentParser(description="Writes Borg's MAC vendor table from the IEEE MA-L registry.")
    parser.add_argument("registry", nargs="?", type=pathlib.Path, help="a downloaded oui.csv; read from IEEE without it")
    arguments = parser.parse_args()
    if arguments.registry:
        text = arguments.registry.read_text(encoding="utf-8")
    else:
        request = urllib.request.Request(REGISTRY, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request) as response:
            text = response.read().decode("utf-8")
    table = rows(text)
    lines = [f"{assignment}\t{name}\n" for assignment, name in sorted(table.items())]
    TABLE.parent.mkdir(parents=True, exist_ok=True)
    TABLE.write_text("".join(lines), encoding="utf-8")
    print(f"wrote {len(lines)} vendors to {TABLE.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
