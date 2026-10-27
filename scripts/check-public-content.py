#!/usr/bin/env python3
import argparse
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
MAX_TEXT_BYTES = 4 * 1024 * 1024
PATTERNS = {
    "personal macOS home path": re.compile(r"/Users/[A-Za-z0-9_.-]+/"),
    "private macOS temporary path": re.compile(r"/(?:private/)?var/folders/[A-Za-z0-9_-]+/"),
    "personal Windows home path": re.compile(r"[A-Za-z]:\\Users\\[A-Za-z0-9_.-]+\\"),
    "private key": re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----"),
    "GitHub token": re.compile(r"\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})\b"),
    "AWS access key": re.compile(r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b"),
    "Slack token": re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b"),
    "OpenAI project key": re.compile(r"\bsk-proj-[A-Za-z0-9_-]{40,}\b"),
}
PRIVATE_NAMES = {"id_rsa", "id_ed25519", "id_ecdsa", "credentials.json", ".netrc"}
TEXT_SUFFIXES = {".html", ".css", ".js", ".mjs", ".json", ".xml", ".txt", ".svg", ".md", ".mdx"}


def findings(path, name):
    issues = []
    filename = path.name
    example = filename.endswith(".example") or ".example." in filename
    if not example and (filename in PRIVATE_NAMES or filename == ".env" or filename.startswith(".env.")):
        issues.append(f"{name}: private configuration filename")
    if not path.is_file() or path.stat().st_size > MAX_TEXT_BYTES:
        return issues
    data = path.read_bytes()
    if b"\0" in data:
        return issues
    content = data.decode("utf-8", errors="replace")
    for label, pattern in PATTERNS.items():
        for match in pattern.finditer(content):
            line = content.count("\n", 0, match.start()) + 1
            issues.append(f"{name}:{line}: {label}")
    return issues


def main():
    parser = argparse.ArgumentParser(description="Check public source and generated pages without printing sensitive values.")
    parser.add_argument("--site", type=pathlib.Path)
    args = parser.parse_args()
    names = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=ROOT).decode().split("\0")
    issues = []
    checked = 0
    for name in filter(None, names):
        issues.extend(findings(ROOT / name, name))
        checked += 1
    if args.site:
        if not args.site.is_dir():
            parser.error("site output directory does not exist")
        for path in args.site.rglob("*"):
            if path.is_file() and path.suffix in TEXT_SUFFIXES:
                issues.extend(findings(path, str(path.relative_to(args.site))))
                checked += 1
    if issues:
        print("Public content check failed:\n" + "\n".join(issues), file=sys.stderr)
        return 1
    print(f"Public content check passed ({checked} files; binary contents and files over 4 MiB excluded).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
