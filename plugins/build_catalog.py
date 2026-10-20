#!/usr/bin/env python3
"""Rebuild plugins/catalog.json from the plugin folders next to it.

    python3 plugins/build_catalog.py

Every folder with a plugin.json becomes an entry with the SHA-256 of each file. ProxyZip downloads the files from
`base_url`, checks the hashes and only then installs. Run it whenever a plugin changes; CI fails if it is stale.
"""

import hashlib
import json
from pathlib import Path

BASE = "https://raw.githubusercontent.com/LuisPeixoto/ProxyZip/main/plugins"
ROOT = Path(__file__).parent


def main() -> None:
    entries = []
    for manifest_path in sorted(ROOT.glob("*/plugin.json")):
        folder = manifest_path.parent
        manifest = json.loads(manifest_path.read_text())
        files = [
            {"path": p.relative_to(folder).as_posix(), "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
            for p in sorted(folder.rglob("*"))
            if p.is_file()
        ]
        contributes = manifest.get("contributes", {})
        provides = []
        if contributes.get("hooks"):
            provides.append("hook")
        if contributes.get("viewers"):
            provides.append("viewer")
        if contributes.get("copy_as"):
            provides.append("copy as")
        if contributes.get("tabs"):
            provides.append("tab")
        entries.append(
            {
                "id": manifest["id"],
                "name": manifest["name"],
                "version": manifest["version"],
                "description": manifest.get("description", ""),
                "author": manifest.get("author", ""),
                "provides": provides,
                "settings": bool(manifest.get("settings")),
                "base_url": f"{BASE}/{folder.name}",
                "files": files,
            }
        )
    (ROOT / "catalog.json").write_text(json.dumps({"version": 1, "plugins": entries}, indent=2) + "\n")
    print(f"wrote {len(entries)} plugins to plugins/catalog.json")


if __name__ == "__main__":
    main()
