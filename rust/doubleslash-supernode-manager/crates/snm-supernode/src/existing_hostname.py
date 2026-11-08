"""Explicit hostname Apply: compare, atomically replace, preserve file metadata."""
import hashlib
import json
import os
import stat
import sys
import tempfile

path, stage, expected_hash, expected_identity = sys.argv[1:]
temporary = None
try:
    metadata = os.lstat(path)
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
        raise ValueError("shared or linked manifest")
    with open(os.path.join(os.path.dirname(path), "identity.json"), "r", encoding="utf-8") as source:
        identity = json.load(source)
    if (identity.get("public_key") or identity.get("public_id", "")).rstrip("=") != expected_identity:
        raise ValueError("identity changed")
    with open(path, "rb") as source:
        original = source.read(262145)
    if hashlib.sha256(original).hexdigest() != expected_hash:
        raise ValueError("manifest changed")
    with open(stage, "rb") as source:
        patch = source.read(262145)
    if len(patch) > 262144:
        raise ValueError("patch too large")
    descriptor, temporary = tempfile.mkstemp(prefix=".hostname-", dir=os.path.dirname(path))
    with os.fdopen(descriptor, "wb") as target:
        os.fchown(target.fileno(), metadata.st_uid, metadata.st_gid)
        os.fchmod(target.fileno(), stat.S_IMODE(metadata.st_mode))
        target.write(patch)
        target.flush()
        os.fsync(target.fileno())
    with open(path, "rb") as source:
        if hashlib.sha256(source.read(262145)).hexdigest() != expected_hash:
            raise ValueError("manifest changed")
    os.replace(temporary, path)
    temporary = None
    directory = os.open(os.path.dirname(path), os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
    print("hostname saved")
except Exception:
    print("hostname edit refused or failed")
    raise SystemExit(1)
finally:
    for leftover in [temporary, stage]:
        if leftover is not None:
            try:
                os.unlink(leftover)
            except FileNotFoundError:
                pass
