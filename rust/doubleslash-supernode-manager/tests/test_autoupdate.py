"""Run the shipped updater with fake systemd; Linux, Python 3, no root/network.

    python3 -m unittest discover -s tests -p test_autoupdate.py -v
"""

import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "crates/snm-supernode/src/autoupdate.rs"
SCRIPT = SOURCE.read_text().split('pub const UPDATER_SCRIPT: &str = r#"', 1)[1].split('"#;', 1)[0]
UNIT = "doubleslash-supernode@test.service"
STOPPED = "doubleslash-supernode@stopped.service"

# Model selection, start limits and a binary that crashes after a restart.
SYSTEMCTL = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
p = pathlib.Path(os.environ["FAKE_STATE"])
s = json.loads(p.read_text())
args = sys.argv[1:]
s["calls"].append(args)
code = 0
if args[0] == "list-units":
    code = s.get("list_error", 0)
    states = next(a.split("=", 1)[1].split(",") for a in args if a.startswith("--state="))
    if not code:
        for name, unit in s["units"].items():
            if unit["state"] in states:
                print(name, "loaded", unit["state"], "service")
elif args[0] == "reset-failed":
    s["units"][args[1]]["limited"] = False
elif args[0] == "restart":
    unit = s["units"][args[1]]
    current = pathlib.Path(os.environ["FAKE_CURRENT"]).resolve()
    unit["starts"] += 1
    crash = s.get("crash_new", False) and current.name != "old"
    code = int(unit.get("limited", False) or crash)
    unit["state"] = "failed" if code else "active"
    unit["limited"] = bool(code)
elif args[0] == "is-active":
    code = int(s["units"][args[-1]]["state"] != "active")
elif args[0] == "show":
    print(s["units"][args[-1]]["starts"])
else:
    raise RuntimeError(args)
p.write_text(json.dumps(s))
sys.exit(code)
'''


class UpdaterTests(unittest.TestCase):
    def run_update(self, initial="failed", crash=False, list_error=0):
        with tempfile.TemporaryDirectory(prefix="doubleslash-update-test-") as directory:
            root = Path(directory)
            bindir = root / "install/bin"
            bindir.mkdir(parents=True)
            old = bindir / "old"
            old.write_text("old binary")
            old.chmod(0o755)
            current = bindir / "current"
            current.symlink_to(old)
            state_dir = root / "state"
            state_dir.mkdir()
            (state_dir / "package.sha256").write_text("a" * 64)
            package = root / "package.tar.gz"
            with tarfile.open(package, "w:gz") as archive:
                for name in ["doubleslash-supernode", "licenses/rust-licenses.html",
                             "licenses/DoubleSlash-LICENSE.txt", "licenses/inventory.json"]:
                    data = b"test release content"
                    info = tarfile.TarInfo("release/" + name)
                    info.size = len(data)
                    archive.addfile(info, io.BytesIO(data))
            digest = hashlib.sha256(package.read_bytes()).hexdigest()
            env_file = root / "update.env"
            env_file.write_text(f"CHANNEL=nightly\nINSTALL_ROOT={root}/install\nSTATE_DIR={state_dir}\n")
            script = root / "updater"
            script.write_text(SCRIPT.replace("/etc/doubleslash/update.env", str(env_file))
                              .replace("/run/doubleslash-update.lock", str(root / "lock")))
            fake = root / "fake"
            fake.mkdir()
            for name, contents in [("systemctl", SYSTEMCTL), ("sleep", "#!/bin/sh\nexit 0\n")]:
                path = fake / name
                path.write_text(contents)
                path.chmod(0o755)
            state = root / "systemd.json"
            state.write_text(json.dumps({"calls": [], "crash_new": crash, "list_error": list_error,
                "units": {UNIT: {"state": initial, "limited": True, "starts": 0},
                          STOPPED: {"state": "inactive", "starts": 0}}}))
            env = dict(os.environ, PATH=f"{fake}:{os.environ['PATH']}",
                       FAKE_STATE=str(state), FAKE_CURRENT=str(current))
            result = subprocess.run(["sh", str(script), "local", str(package), digest],
                                    env=env, capture_output=True, text=True, timeout=15)
            return (result, json.loads(state.read_text()), current.resolve().name,
                    (state_dir / "package.sha256").read_text(), digest,
                    (bindir / f"doubleslash-supernode-nightly-{digest[:12]}").exists())

    def test_failed_and_restarting_instances_are_recovered(self):
        for initial in ["active", "failed", "activating", "reloading"]:
            with self.subTest(initial=initial):
                result, state, current, installed, digest, exists = self.run_update(initial)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(state["units"][UNIT]["state"], "active")
                self.assertEqual(state["units"][UNIT]["starts"], 1)
                self.assertEqual(state["units"][STOPPED]["starts"], 0)
                self.assertEqual(installed, digest)
                self.assertTrue(exists)
                self.assertNotEqual(current, "old")

    def test_crashing_update_rolls_back_even_after_start_limit(self):
        result, state, current, installed, _, exists = self.run_update(crash=True)
        self.assertEqual(result.returncode, 5, result.stderr)
        self.assertIn("rolling back", result.stderr)
        self.assertEqual(current, "old")
        self.assertEqual(state["units"][UNIT]["state"], "active")
        self.assertEqual(state["units"][UNIT]["starts"], 2)
        self.assertEqual(state["units"][STOPPED]["starts"], 0)
        self.assertEqual(installed, "a" * 64)
        self.assertFalse(exists)

    def test_systemd_listing_failure_cannot_record_success(self):
        result, state, current, installed, _, _ = self.run_update(list_error=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(current, "old")
        self.assertEqual(installed, "a" * 64)
        self.assertEqual(state["units"][UNIT]["starts"], 0)


if __name__ == "__main__":
    unittest.main()
