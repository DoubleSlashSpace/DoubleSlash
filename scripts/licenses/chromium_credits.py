#!/usr/bin/env python3
"""Generate the Chromium third-party credits for the bundled Qt WebEngine.

Qt ships Chromium compiled into Qt WebEngine Core but does not ship Chromium's
credits resource: Qt builds only Chromium's content layer, so the resource a
Chrome build exposes at chrome://credits is not compiled in, and it is not in
any of Qt's .pak files. Qt's per-component attribution pages exist only for
releases doc.qt.io still serves, which excludes 6.8. So the credits have to be
produced from the source tree Qt pins.

This fetches that tree cheaply - a blobless, depth-1, sparse clone that takes
the READMEs and licence files and nothing else, about 20 MB rather than the
several GB a full checkout would cost - and writes the credits file that ships
in packaging/licenses/<target>/client/.

Usage:
    python scripts/licenses/chromium_credits.py --qt-tag v6.8.3 \
        --output packaging/licenses/x86_64-pc-windows-msvc/client/chromium-third-party-credits.txt

The output is ~23,000 lines of licence text, so it is generated rather than
kept in the repository. build_win64.ps1 runs this before it generates the
package notices, and caches the result, regenerating only when the bundled Qt
version changes: both the component set and the Chromium revision move with it.
"""
import argparse, hashlib, io, json, os, posixpath, re, shutil, subprocess, sys, tempfile, urllib.request

CHROMIUM_REPO = "https://github.com/qt/qtwebengine-chromium.git"
KEYS = ("name", "short name", "url", "version", "revision", "license",
        "license file", "shipped", "security critical")


def pinned_revision(qt_tag):
    """The qtwebengine-chromium commit that a qtwebengine tag points its submodule at."""
    url = f"https://api.github.com/repos/qt/qtwebengine/contents/src/3rdparty?ref={qt_tag}"
    with urllib.request.urlopen(url) as r:
        data = json.load(r)
    if data.get("type") != "submodule":
        raise SystemExit(f"src/3rdparty is {data.get('type')}, not a submodule, at {qt_tag}")
    return data["sha"]


def run(args, cwd):
    subprocess.run(args, cwd=cwd, check=True, stdout=subprocess.DEVNULL)


def sparse_clone(work, revision, patterns):
    run(["git", "init", "-q"], work)
    run(["git", "remote", "add", "origin", CHROMIUM_REPO], work)
    run(["git", "config", "core.longpaths", "true"], work)
    run(["git", "sparse-checkout", "init", "--no-cone"], work)
    subprocess.run(["git", "sparse-checkout", "set", "--no-cone", "--stdin"],
                   cwd=work, check=True, input="\n".join(patterns).encode())
    run(["git", "fetch", "--filter=blob:none", "--depth", "1", "origin", revision], work)
    run(["git", "checkout", "-q", "FETCH_HEAD"], work)


def fields(path):
    """First occurrence of each known key at column 0.

    Values wrap onto indented continuation lines - FreeType's License: does -
    and a free-text description follows further down, so neither a blank line
    nor an unmatched line can be treated as the end of the header.
    """
    out = {}
    for line in io.open(path, encoding="utf-8", errors="replace").read().split("\n"):
        m = re.match(r"^([A-Za-z][A-Za-z0-9 _-]*?)\s*:\s*(.*)$", line)
        if not m:
            continue
        key = m.group(1).strip().lower()
        if key in KEYS and key not in out:
            out[key] = m.group(2).strip()
    return out


def collect(root):
    records = []
    for dirpath, _, files in os.walk(root):
        if "README.chromium" not in files:
            continue
        f = fields(os.path.join(dirpath, "README.chromium"))
        raw = f.get("license file", "")
        records.append({
            "dir": os.path.relpath(dirpath, root).replace("\\", "/"),
            "name": f.get("name") or f.get("short name") or os.path.basename(dirpath),
            "url": f.get("url", ""), "version": f.get("version", ""),
            "license": " ".join(f.get("license", "").split()),
            "shipped": f.get("shipped", "").lower(), "license_file_raw": raw,
            "license_paths": [c.strip() for c in re.split(r"[,\n]", raw) if c.strip()],
        })
    return records


def resolve(rec, p):
    """A License File value to a path under chromium/. `//x` is source-root relative."""
    p = p.strip().replace("\\", "/")
    return (posixpath.normpath("chromium/" + p[2:]) if p.startswith("//")
            else posixpath.normpath(f"chromium/{rec['dir']}/{p}"))


def shipped(r):
    return (r["shipped"] not in ("no", "false")
            and "NOT_SHIPPED" not in r["license_file_raw"]
            and not r["dir"].startswith("tools/licenses"))


def entry(r):
    bits = [f"  {r['name']}"]
    for label, key in (("Version", "version"), ("URL", "url"), ("License", "license")):
        if r.get(key):
            bits.append(f"      {label + ':':9}{r[key]}")
    bits.append(f"      {'Path:':9}{r['dir']}")
    return "\n".join(bits)


def render(records, base, qt_tag, revision):
    with_text, chromium_own, absent = [], [], []
    for r in sorted((r for r in records if shipped(r)),
                    key=lambda x: (x["name"].lower(), x["dir"])):
        have = [p for p in (resolve(r, q) for q in r["license_paths"])
                if os.path.isfile(os.path.join(base, p))]
        if have:
            with_text.append((r, have))
        elif not r["license_paths"] and "/third_party/" not in "/" + r["dir"]:
            chromium_own.append(r)
        else:
            absent.append(r)

    # One copy of each distinct licence text, referenced by the components using it:
    # 237 components share 183 texts, and repeating them would double the file.
    blocks, order = {}, []
    for r, have in with_text:
        for p in have:
            text = io.open(os.path.join(base, p), encoding="utf-8", errors="replace").read().strip()
            digest = hashlib.sha256(text.encode()).hexdigest()[:12]
            if digest not in blocks:
                blocks[digest] = text
                order.append(digest)
            r.setdefault("_blocks", []).append(digest)

    out = [f"""Chromium third-party components in Qt WebEngine ({qt_tag})
{'=' * (48 + len(qt_tag))}

This is the third-party credits set for the Chromium snapshot compiled into
the Qt WebEngine that DoubleSlash bundles - the equivalent of the list a
Chrome build shows at chrome://credits.

Generated by scripts/licenses/chromium_credits.py from the tree Qt pins:

  repository  https://code.qt.io/cgit/qt/qtwebengine-chromium.git
  revision    {revision}
  pinned by   qtwebengine {qt_tag}, src/3rdparty

Every component is listed with the metadata Chromium records for it in its
README.chromium and, in Part 1, the full text of its licence. Components
marked "Shipped: no" upstream are excluded, as Chromium's own generator
excludes them.

Chromium itself, and the directories in this tree that are Chromium's own code
rather than a third-party import, are covered by the Chromium licence in
chromium-122-LICENSE.txt beside this file.

Contents
--------
  Part 1  {len(with_text):>3} components with their licence texts
  Part 2  {len(chromium_own):>3} Chromium-owned directories, under the Chromium licence
  Part 3  {len(absent):>3} components whose licence file is not present in Qt's snapshot
"""]

    rule = "=" * 78
    out.append(f"\n{rule}\nPart 1 - components and licence texts\n{rule}\n")
    for r, _ in with_text:
        out.append(entry(r))
        out.append(f"      Licence:  block {' '.join(r.get('_blocks', []))}\n")

    out.append(f"\n{rule}\nPart 2 - Chromium's own directories\n{rule}\n\n"
               "These record no separate third-party licence: they are Chromium source,\n"
               "under the Chromium licence in chromium-122-LICENSE.txt.\n")
    for r in chromium_own:
        out.append(f"  {r['name']}  ({r['dir']})")

    out.append(f"\n{rule}\nPart 3 - licence text not present in Qt's snapshot\n{rule}\n\n"
               "Each component below declares a licence file that Chromium's tree carries\n"
               "but Qt's qtwebengine-chromium snapshot does not - mostly nested checkouts\n"
               "(the vendored Rust crates under third_party/rust/chromium_crates_io, and\n"
               "the `src/` subtrees of openscreen, openxr and several ANGLE dependencies)\n"
               "that Qt strips.\n\n"
               "A component whose sources are absent from the snapshot is not compiled\n"
               "into Qt WebEngine, so these are listed for completeness rather than\n"
               "because their code is in the binary. Each is named with the licence\n"
               "Chromium declares for it and its upstream URL.\n")
    for r in absent:
        out.append(entry(r))
        out.append(f"      Declared: {r['license_file_raw']}\n")

    out.append(f"\n{rule}\nLicence texts\n{rule}\n")
    for digest in order:
        out.append(f"\n---- block {digest} " + "-" * (60 - len(digest)) + "\n")
        out.append(blocks[digest])
        out.append("")

    return "\n".join(out) + "\n", (len(with_text), len(chromium_own), len(absent), len(blocks))


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--qt-tag", default="v6.8.3", help="qtwebengine tag, e.g. v6.8.3")
    ap.add_argument("--output", required=True, help="credits file to write")
    ap.add_argument("--revision", help="skip the lookup and use this chromium revision")
    ap.add_argument("--work", help="reuse this clone directory instead of a temporary one")
    args = ap.parse_args()

    # Windows consoles default to a legacy codepage, and a path this cannot
    # encode would otherwise kill the build on a progress message.
    for stream in (sys.stdout, sys.stderr):
        try: stream.reconfigure(encoding="utf-8", errors="replace")
        except AttributeError: pass

    revision = args.revision or pinned_revision(args.qt_tag)
    print(f"qtwebengine {args.qt_tag} pins chromium {revision}")

    keep = args.work is not None
    work = args.work or tempfile.mkdtemp(prefix="qtwe-chromium-")
    os.makedirs(work, exist_ok=True)
    root = os.path.join(work, "chromium")

    if not os.path.isdir(root):
        print("fetching READMEs (blobless, depth 1) ...")
        sparse_clone(work, revision, ["/chromium/**/README.chromium", "/chromium/LICENSE"])

    records = collect(root)
    print(f"{len(records)} README.chromium files, {sum(1 for r in records if shipped(r))} shipped")

    # Second pass: now that the READMEs say which licence files matter, fetch exactly those.
    wanted = {"/chromium/**/README.chromium", "/chromium/LICENSE"}
    for r in records:
        if shipped(r):
            wanted.update("/" + resolve(r, p) for p in r["license_paths"])
    print(f"fetching {len(wanted) - 2} licence files ...")
    subprocess.run(["git", "sparse-checkout", "set", "--no-cone", "--stdin"],
                   cwd=work, check=True, input="\n".join(sorted(wanted)).encode())
    run(["git", "checkout", "-q", "FETCH_HEAD"], work)

    text, (n1, n2, n3, blocks) = render(records, work, args.qt_tag, revision)
    os.makedirs(os.path.dirname(os.path.abspath(args.output)), exist_ok=True)
    io.open(args.output, "w", encoding="utf-8", newline="\n").write(text)
    print(f"wrote {args.output}: {len(text):,} bytes")
    print(f"  part 1 {n1} with text, part 2 {n2} chromium-owned, part 3 {n3} absent")
    print(f"  {blocks} distinct licence texts")
    if not keep:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
