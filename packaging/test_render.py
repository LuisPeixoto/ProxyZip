"""Checks that the templates render into well-formed manifests. Run: python3 packaging/test_render.py"""

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).parent
VERSION = "9.9.9"
ASSETS = [
    f"ProxyZip_{VERSION}_aarch64.dmg",
    f"ProxyZip_{VERSION}_x64.dmg",
    f"ProxyZip_{VERSION}_x64-setup.exe",
    f"ProxyZip_{VERSION}_amd64.deb",
    f"proxyzip-{VERSION}-aarch64-apple-darwin.tar.gz",
    f"proxyzip-{VERSION}-x86_64-apple-darwin.tar.gz",
    f"proxyzip-{VERSION}-x86_64-unknown-linux-musl.tar.gz",
    f"proxyzip-{VERSION}-aarch64-unknown-linux-gnu.tar.gz",
    f"proxyzip-{VERSION}-x86_64-pc-windows-msvc.zip",
]


def run(assets: Path, out: Path, *extra: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(HERE / "render.py"), "--version", VERSION, "--assets", str(assets), "--out", str(out), *extra],
        capture_output=True,
        text=True,
    )


class RenderTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.assets = Path(self.tmp.name) / "assets"
        self.assets.mkdir()
        self.out = Path(self.tmp.name) / "out"
        for name in ASSETS:
            (self.assets / name).write_bytes(name.encode())

    def tearDown(self):
        self.tmp.cleanup()

    def digest(self, name: str) -> str:
        return hashlib.sha256(name.encode()).hexdigest()

    def test_fills_versions_and_checksums(self):
        result = run(self.assets, self.out)
        self.assertEqual(result.returncode, 0, result.stderr)
        cask = (self.out / "homebrew/proxyzip.rb").read_text()
        self.assertIn(f'version "{VERSION}"', cask)
        self.assertIn(self.digest(f"ProxyZip_{VERSION}_aarch64.dmg"), cask)
        self.assertIn(self.digest(f"ProxyZip_{VERSION}_x64.dmg"), cask)
        scoop = json.loads((self.out / "scoop/proxyzip.json").read_text())
        self.assertEqual(scoop["version"], VERSION)
        self.assertEqual(scoop["architecture"]["64bit"]["hash"], self.digest(f"proxyzip-{VERSION}-x86_64-pc-windows-msvc.zip"))
        winget = (self.out / "winget/ProxyZip.ProxyZip.installer.yaml").read_text()
        self.assertIn(self.digest(f"ProxyZip_{VERSION}_x64-setup.exe"), winget)

    def test_nothing_is_left_unfilled(self):
        run(self.assets, self.out)
        for path in self.out.rglob("*"):
            if path.is_file():
                self.assertNotIn("{{", path.read_text(), path)

    def test_a_missing_file_fails_unless_allowed(self):
        (self.assets / ASSETS[0]).unlink()
        failed = run(self.assets, self.out)
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn(ASSETS[0], failed.stderr)
        allowed = run(self.assets, self.out, "--allow-missing")
        self.assertEqual(allowed.returncode, 0, allowed.stderr)


if __name__ == "__main__":
    unittest.main()
