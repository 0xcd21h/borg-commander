#!/usr/bin/env python3
import sys
from pathlib import Path
from xml.etree import ElementTree

BODY_LIMIT = 8000
STREAMS = ("system-out", "system-err")
FAILURES = ("failure", "error")
FLAKY = "flakyFailure"
DATA_ESCAPES = (("%", "%25"), ("\r", "%0D"), ("\n", "%0A"))
PROPERTY_ESCAPES = DATA_ESCAPES + ((":", "%3A"), (",", "%2C"))


def escape(text, escapes):
    for plain, escaped in escapes:
        text = text.replace(plain, escaped)
    return text


def annotate(level, title, body):
    print(f"::{level} title={escape(title, PROPERTY_ESCAPES)}::{escape(body[:BODY_LIMIT], DATA_ESCAPES)}")


def captured(element):
    return "\n".join(text for text in (element.findtext(stream) for stream in STREAMS) if text)


def report(case):
    title = f"{case.get('classname')} {case.get('name')}"
    failure = next((found for found in map(case.find, FAILURES) if found is not None), None)
    if failure is not None:
        annotate("error", title, captured(case) or failure.get("message", ""))
        return
    attempts = case.findall(FLAKY)
    if attempts:
        annotate("warning", f"{title} passed on attempt {len(attempts) + 1}", captured(attempts[-1]))


def main(path):
    if not path.is_file():
        return
    for case in ElementTree.parse(path).iter("testcase"):
        report(case)


if __name__ == "__main__":
    main(Path(sys.argv[1]))
