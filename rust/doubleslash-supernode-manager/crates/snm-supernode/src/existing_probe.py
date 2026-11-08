"""Read-only systemd discovery. Return public identity fields only."""
import json
import os
import re
import shlex
import subprocess

MAX_FILE = 262144


def run(*args):
    return subprocess.check_output(args, text=True, timeout=5)


def read(path):
    with open(path, "r", encoding="utf-8", newline="") as source:
        value = source.read(MAX_FILE + 1)
    if len(value.encode("utf-8")) > MAX_FILE:
        raise ValueError("configuration exceeds the import limit")
    return value


try:
    units = set()
    for command in [
        ["systemctl", "list-units", "--all", "--type=service", "--plain", "--no-legend", "--no-pager", "doubleslash-supernode@*.service"],
        ["systemctl", "list-unit-files", "--type=service", "--no-legend", "--no-pager", "doubleslash-supernode@*.service"],
    ]:
        for line in run(*command).splitlines():
            name = line.split()[0] if line.split() else ""
            if re.fullmatch(r"doubleslash-supernode@[A-Za-z0-9_.-]+\.service", name):
                units.add(name)
    if len(units) > 64:
        raise ValueError("too many instances on this host")
    result = []
    for unit in sorted(units):
        values = dict(line.split("=", 1) for line in run(
            "systemctl", "show", "--no-pager",
            "--property=Id,Environment,User,ExecStart,ActiveState,MainPID", unit,
        ).splitlines() if "=" in line)
        env = dict(part.split("=", 1) for part in shlex.split(values.get("Environment", "")) if "=" in part)
        pid = int(values.get("MainPID", "0"))
        if pid:
            with open("/proc/%d/environ" % pid, "rb") as source:
                process_env = source.read(1048576).decode("utf-8", errors="replace")
            env.update(dict(part.split("=", 1) for part in process_env.split("\0") if "=" in part))
        home = env.get("DOUBLESLASH_HOME", "")
        if not os.path.isabs(home):
            raise ValueError("instance has no absolute DOUBLESLASH_HOME")
        home = os.path.realpath(home)
        # The identity file also holds the private key. Never return that file.
        identity = json.loads(read(os.path.join(home, "identity.json")))
        public_key = identity.get("public_key") or identity.get("public_id")
        invite = None
        invite_path = os.path.join(home, "reusable_invite.json")
        if os.path.isfile(invite_path):
            raw = json.loads(read(invite_path)).get("invite", {})
            allowed = ["inviter_peer_id", "inviter_identity_pub", "invite_id", "expires_at",
                       "inviter_ephemeral_pub", "relay_hint", "inviter_handle", "is_supernode",
                       "turn_hints", "supernode_relay_hints", "signature"]
            invite = {key: raw[key] for key in allowed if key in raw}
        executable = re.search(r"\bpath=([^;]+)", values.get("ExecStart", ""))
        result.append({
            "unit": unit, "home": home, "public_key": public_key,
            "public_host": env.get("supernode_host", ""),
            "relay_port": env.get("supernode_port", "3478"),
            "ws_port": env.get("supernode_signaling_port", "34935"),
            "user": values.get("User", ""),
            "executable": executable.group(1).strip() if executable else "",
            "active": values.get("ActiveState") == "active",
            "manifest": read(os.path.join(home, "supernode.toml")),
            "invite": invite,
        })
    snapshot = json.dumps({"instances": result}, separators=(",", ":"))
    if len(snapshot.encode("utf-8")) > 4 * 1024 * 1024:
        raise ValueError("snapshot too large")
    print(snapshot)
except Exception:
    # No tracebacks or arbitrary file content (including private identities).
    print("Could not inspect existing instances; check systemd, Python 3 and permission to read their data directories.")
    raise SystemExit(1)
