#!/usr/bin/env python3
"""The offline signing flow (design section 3, `sign=offline`): the owner signs a DRAFT release by hand.

The workflow with `sign=offline` builds a draft release that has the manifests and the update zips but
no `.sig`. The owner downloads them, runs this tool on the machine that holds the private key, and
uploads the signatures. This tool NEVER runs `gh`: every `gh` command it shows is for the owner to
read and run. (Design section 3 has the tool upload and un-draft; the task that wrote this tool made
that a printed command, because no tool writes to GitHub.) The only program the tool starts is openssl,
to sign and to verify; a test records every program it starts to hold it to that. So the flow is three
steps with this tool in the middle, and the same tool command is run twice:

    python tools/update_release_sign.py --repo kjarada/Katana --tag vX.Y.Z --key update-signing.pem

The first run finds no downloaded draft (the folder release-<tag>, or --dir), prints the exact
`gh release download` command and stops. Run that command, then run the tool command again, unchanged.
Nothing is signed until every step has passed:

  0. The draft's assets are in the folder (the printed `gh release download` fetched them). To see what
     changed, also fetch the release before it and give --previous:
         gh release download vPREV --repo kjarada/Katana --dir release-vPREV --pattern 'Katana-update-*.json'
     (Whether `gh` finds a DRAFT by its tag is one of the first-release checks of docs/update.md.)
  1. Recheck every asset with tools/update_verify_release.py's code, which re-derives every hash from the
     zip members and the signed byte ranges (the producer's own word is not taken). The manifests are
     judged as the very bytes hashed before this step (see 4), and those are the bytes signed in step 5.
  2. Show what changed against --previous: per target, the files added, removed and changed, and the bytes an
     installed copy of the previous release would download.
  3. Check the key: it must be one the tree pins (cmake/KatanaUpdateKeys.cmake), because an installed copy
     rejects anything else, and it must not be a published test key.
  4. Make you TYPE the version. A mistyped or empty answer, or end of input, signs nothing. Every
     manifest and zip is hashed before step 1 and again after the answer: a file that changed while you
     read the report is refused, so what is signed is exactly what was checked and shown.
  5. Sign each manifest (openssl pkeyutl -sign -rawin) into `<manifest>.sig`, from the very bytes that
     were checked, then verify the signatures with the openssl command line against the pinned keys and
     recheck the directory. On any failure the signatures this run wrote are removed.
  6. Print the exact `gh release upload` and `gh release edit` commands. They are NOT executed.

The full form: --repo kjarada/Katana --tag vX.Y.Z --key update-signing.pem --previous release-vPREV
[--dir release-vX.Y.Z]

--report-only stops after step 3. --test-key (tests) requires a published RFC 8032 test key and trusts
TEST 1 and TEST 2, as the engine's tests do; without it such a key is refused.

Exit status: 0 signed (or --report-only passed), 1 refused or aborted, 2 a usage error.
"""

from __future__ import annotations

import argparse
import hashlib
import shlex
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

import update_verify_release as verifier
from update_make_manifest import (
    DEFAULT_KEYS_CMAKE,
    DEFAULT_REPO,
    ToolError,
    check_signing_key,
    find_openssl,
    key_id,
    public_key_of,
    sign_bytes,
    signature_line,
)

LIST_LIMIT = 12
CHUNK = 1 << 20


@dataclass
class Diff:
    added: list[str]
    removed: list[str]
    changed: list[str]
    unchanged: list[str]
    download_bytes: int  # compressed bytes an installed copy of the previous release would fetch


def compare_manifests(old: Optional[dict], new: dict) -> Diff:
    """Which files are new, gone, different (by SHA-256) or the same, between two parsed manifests."""
    before = {f["path"]: f for f in old["files"]} if old else {}
    after = {f["path"]: f for f in new["files"]}
    added = sorted(set(after) - set(before))
    removed = sorted(set(before) - set(after))
    changed = sorted(p for p in set(after) & set(before) if after[p]["sha256"] != before[p]["sha256"])
    unchanged = sorted(p for p in set(after) & set(before) if after[p]["sha256"] == before[p]["sha256"])
    download = sum(after[p]["csize"] for p in added + changed)
    return Diff(added, removed, changed, unchanged, download)


def _size(count: int) -> str:
    return f"{count / (1 << 20):.1f} MiB" if count >= 1 << 20 else f"{count} bytes"


def _listing(marker: str, paths: list[str], classes: dict[str, str]) -> list[str]:
    lines = [f"      {marker} {p}  [{classes.get(p, '?')}]" for p in paths[:LIST_LIMIT]]
    if len(paths) > LIST_LIMIT:
        lines.append(f"      ... and {len(paths) - LIST_LIMIT} more")
    return lines


def describe_changes(report: verifier.Report, previous: Optional[Path]) -> list[str]:
    lines = []
    for token, new in sorted(report.manifests.items()):
        old = verifier.previous_manifest(previous, token) if previous is not None else None
        heading = f"  {token}: Katana {new['version']}, sequence {new['sequence']}"
        if old:
            heading += f"  (was {old['version']}, sequence {old['sequence']})"
        lines.append(heading)
        if new.get("notify_only"):
            lines.append("    notify-only: no files")
            continue
        diff = compare_manifests(old, new)
        classes = {f["path"]: f["class"] for f in new["files"]}
        if old:
            classes.update({f["path"]: f["class"] for f in old["files"] if f["path"] not in classes})
            lines.append(f"    {len(new['files'])} files: {len(diff.added)} added, {len(diff.removed)} removed, "
                         f"{len(diff.changed)} changed, {len(diff.unchanged)} unchanged")
            lines.append(f"    an installed copy of the previous release downloads {_size(diff.download_bytes)}")
        else:
            lines.append(f"    {len(new['files'])} files, all new to this comparison (no --previous given)")
            lines.append(f"    a fresh download is {_size(diff.download_bytes)}")
        programs_changed = [p for p in diff.changed + diff.added + diff.removed if classes.get(p) == "program"]
        if old and programs_changed:
            lines.append("    PROGRAMS: " + ", ".join(sorted(programs_changed)))
        lines += _listing("+", diff.added, classes) if old else []
        lines += _listing("-", diff.removed, classes)
        lines += _listing("~", diff.changed, classes)
    return lines


@dataclass
class Snapshot:
    """What the directory held when it was read: a SHA-256 per release file, and the manifests' bytes."""

    digests: dict[str, str] = field(default_factory=dict)
    manifests: dict[str, bytes] = field(default_factory=dict)


def take_snapshot(directory: Path) -> Snapshot:
    """Hash every Katana-* file of the directory (manifests, zips, signatures); keep the manifests' bytes."""
    snapshot = Snapshot()
    for path in sorted(directory.iterdir()):
        if not path.name.startswith("Katana-") or not path.is_file():
            continue
        if path.name.startswith("Katana-update-") and path.name.endswith(".json"):
            data = path.read_bytes()
            snapshot.manifests[path.name] = data
            snapshot.digests[path.name] = hashlib.sha256(data).hexdigest()
            continue
        digest = hashlib.sha256()
        with open(path, "rb") as handle:
            while chunk := handle.read(CHUNK):
                digest.update(chunk)
        snapshot.digests[path.name] = digest.hexdigest()
    return snapshot


def gh_download_commands(directory: Path, tag: str, repo: str) -> list[str]:
    return [
        f"gh release download {shlex.quote(tag)} --repo {shlex.quote(repo)} --dir {shlex.quote(directory.as_posix())} "
        "--pattern 'Katana-update-*.json' --pattern 'Katana-*.update.zip'",
    ]


def gh_commands(directory: Path, tag: str, repo: str, signatures: list[Path]) -> list[str]:
    files = " ".join(shlex.quote(s.resolve().as_posix()) for s in signatures)
    return [
        f"gh release upload {shlex.quote(tag)} {files} --repo {shlex.quote(repo)}",
        f"gh release edit {shlex.quote(tag)} --repo {shlex.quote(repo)} --draft=false --latest",
    ]


def run(args: argparse.Namespace) -> int:
    directory: Path = args.dir if args.dir is not None else Path(f"release-{args.tag}")
    if directory.exists() and not directory.is_dir():
        raise ToolError(f"{directory} is not a directory")
    manifests = sorted(directory.glob("Katana-update-*.json")) if directory.is_dir() else []
    if not manifests:
        print(f"{directory} holds no Katana-update-<token>.json: the draft's assets are not downloaded yet.")
        print("This tool does not run gh. Run this command yourself, then run the same tool command again:")
        for command in gh_download_commands(directory, args.tag, args.repo):
            print(f"  {command}")
        return 1
    already = [m.name + ".sig" for m in manifests if (directory / (m.name + ".sig")).exists()]
    if already:
        raise ToolError("already signed, so not signed again: " + ", ".join(already))

    print(f"Offline signing of {args.tag} ({args.repo}) from {directory}")
    print("Step 1: recomputing every hash from the update zips")
    reviewed = take_snapshot(directory)
    report = verifier.verify_release(directory, allow_unsigned=True, previous=args.previous,
                                     manifest_bytes=reviewed.manifests)
    for warning in report.warnings:
        print(f"  warning: {warning}")
    if not report.ok:
        for error in report.errors:
            print(f"  ERROR {error}")
        print(f"Refused: {len(report.errors)} problem(s); nothing was signed.")
        return 1
    versions = {m["version"] for m in report.manifests.values()}
    tags = {m["tag"] for m in report.manifests.values()}
    if tags != {args.tag}:
        print(f"Refused: the manifests are for {sorted(tags)}, not {args.tag}; nothing was signed.")
        return 1
    version = versions.pop()
    print(f"  {len(report.manifests)} manifest(s) recomputed, all consistent")

    print("Step 2: what this release changes")
    for line in describe_changes(report, args.previous):
        print(line)

    print("Step 3: the key")
    openssl = find_openssl(args.openssl)
    public_key = public_key_of(openssl, args.key)
    check_signing_key(public_key, test_key=args.test_key, keys_cmake=args.keys_cmake)
    trust = dict(verifier.TEST_TRUST) if args.test_key else verifier.trust_from_cmake(args.keys_cmake)
    if public_key.hex() not in trust:
        raise ToolError(f"key {key_id(public_key)} is not one of the keys this build pins")
    print(f"  key {key_id(public_key)} ({trust[public_key.hex()]}) is pinned; its private half is read from {args.key} and is not copied")
    if args.report_only:
        print("Report only: nothing was signed.")
        return 0

    print("Step 4: confirm")
    sys.stdout.write(f"Type the version to sign ({version}): ")
    sys.stdout.flush()
    answer = sys.stdin.readline()
    if answer.strip() != version:
        print()
        print(f"Refused: {answer.strip()!r} is not {version!r}; nothing was signed.")
        return 1

    after = take_snapshot(directory)
    if after.digests != reviewed.digests:
        names = sorted(n for n in set(reviewed.digests) | set(after.digests) if reviewed.digests.get(n) != after.digests.get(n))
        print(f"Refused: {', '.join(names)} changed after it was checked; nothing was signed.")
        return 1

    print("Step 5: signing")
    written: list[Path] = []
    try:
        for manifest in manifests:
            if manifest.name[len("Katana-update-"):-len(".json")] not in report.manifests:
                continue
            signature = sign_bytes(openssl, args.key, reviewed.manifests[manifest.name])
            target = manifest.with_name(manifest.name + ".sig")
            target.write_bytes(signature_line(public_key, signature).encode("ascii"))
            written.append(target)
            print(f"  wrote {target}")
        final = verifier.verify_release(directory, trust=trust, openssl=openssl, previous=args.previous)
        if not final.ok:
            for error in final.errors:
                print(f"  ERROR {error}")
            raise ToolError("the signed directory does not verify")
    except BaseException:
        for path in written:
            path.unlink(missing_ok=True)
        raise
    print("  every signature verifies with the openssl command line, and the directory rechecks clean")

    print("Step 6: upload (NOT run by this tool; read them, then run them yourself)")
    for command in gh_commands(directory, args.tag, args.repo, written):
        print(f"  {command}")
    return 0


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        prog="update_release_sign.py",
        description="Sign a draft release's update manifests offline, after recomputing every hash.",
        epilog=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--dir", type=Path, help="the downloaded draft's assets: manifests and update zips (default release-<tag>)")
    parser.add_argument("--tag", required=True, help="the release tag, vX.Y.Z")
    parser.add_argument("--repo", default=DEFAULT_REPO, help=f"owner/name, for the printed commands (default {DEFAULT_REPO})")
    parser.add_argument("--key", type=Path, required=True, help="the private key PEM (update-signing.pem or the spare)")
    parser.add_argument("--previous", type=Path, help="the previous release's directory (or one manifest) to compare with")
    parser.add_argument("--report-only", action="store_true", help="recheck and report, then stop before asking to sign")
    parser.add_argument("--test-key", action="store_true", help="the key is a published RFC 8032 test key (tests only)")
    parser.add_argument("--keys-cmake", type=Path, default=DEFAULT_KEYS_CMAKE, help="the file that pins the production keys")
    parser.add_argument("--openssl", help="the openssl executable (default: KATANA_OPENSSL, else PATH)")
    args = parser.parse_args(argv)
    for stream in (sys.stdout, sys.stderr):  # a path with non-ASCII letters must not kill the report
        try:
            stream.reconfigure(errors="backslashreplace")  # type: ignore[union-attr]
        except (AttributeError, ValueError):
            pass
    try:
        return run(args)
    except (ToolError, verifier.UsageError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
