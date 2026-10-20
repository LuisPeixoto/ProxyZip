#!/usr/bin/env python3
"""Fill the package-manager templates with a release's version and checksums.

    python3 packaging/render.py --version 0.4.0 --assets assets/ --out manifests/

Templates use `{{version}}` and `{{sha256:FILE}}`, where FILE is an asset name (it may contain `{{version}}`).
The release workflow runs this after the installers and command-line archives are attached to the release.
"""

import argparse
import hashlib
import re
import sys
from pathlib import Path

ZEROS = "0" * 64


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def render(text: str, version: str, assets: Path, allow_missing: bool, missing: list) -> str:
    text = text.replace("{{version}}", version)

    def checksum(match: re.Match) -> str:
        name = match.group(1)
        path = assets / name
        if path.is_file():
            value = sha256_of(path)
        elif allow_missing:
            value = ZEROS
        else:
            missing.append(name)
            return ZEROS
        # winget wants upper case; the other tools accept either, so keep one spelling per template.
        return value

    return re.sub(r"\{\{sha256:([^}]+)\}\}", checksum, text)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--version", required=True, help="release version without the leading v")
    parser.add_argument("--assets", type=Path, required=True, help="folder with the release files")
    parser.add_argument("--out", type=Path, required=True, help="folder to write the manifests to")
    parser.add_argument("--allow-missing", action="store_true", help="use zero checksums for files that are not there")
    args = parser.parse_args()

    templates = Path(__file__).parent / "templates"
    missing: list = []
    written = 0
    for template in sorted(p for p in templates.rglob("*") if p.is_file()):
        relative = template.relative_to(templates)
        target = args.out / str(relative).replace("{{version}}", args.version)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(render(template.read_text(), args.version, args.assets, args.allow_missing, missing))
        written += 1
    if missing:
        print("missing release files:", *sorted(set(missing)), sep="\n  ", file=sys.stderr)
        return 1
    print(f"wrote {written} manifests to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
