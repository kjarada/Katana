#!/usr/bin/env python3
"""Pin the update signing keys: the owner's one-time key steps, as far as a script may do them.

There is NO `gen`. This tool never creates a key and never writes a private key anywhere: it reads two
private-key PEMs that the owner made, asks openssl for their PUBLIC halves, and rewrites only the
block between the two marker lines of cmake/KatanaUpdateKeys.cmake. Public keys are public data; that
file is committed. The private halves stay offline (*.pem and update-signing* are git-ignored).

The owner's steps, once (design section 3, docs/update.md):

  1. Make the two keys, the active one that CI signs with and the spare that stays offline:
         openssl genpkey -algorithm ed25519 -out update-signing.pem
         openssl genpkey -algorithm ed25519 -out update-spare.pem
     Back both up offline. Lose both and installed copies can never update again.
  2. Pin their public halves in the tree (this tool):
         python tools/update_keytool.py pub update-signing.pem update-spare.pem
     then commit cmake/KatanaUpdateKeys.cmake.
  3. On GitHub, make the environment `release`, restricted to the default branch (docs/release.md,
     "Update signing"), and give that environment the active key only (never the spare):
         gh secret set UPDATE_SIGNING_KEY --env release < update-signing.pem
     Make the environment first. Without `--env release` the secret would be the repository's, which
     every workflow there can read; a secret of the environment reaches only the Publish job.
  4. Install the first updater-bearing release by hand; the next one is the first live update.

Until step 2 the file holds PLACEHOLDER keys that no private key matches, and KATANA_REQUIRE_UPDATE_KEYS
(ON in release.yml) refuses to build a release with them.

`pub` refuses: a PEM openssl cannot read or that is not an Ed25519 private key; the same key twice
(a spare that is the active key is no way to recover from losing it); a published RFC 8032 test key
(anyone can sign with it; --allow-test-keys exists for the tool's own tests, on a scratch copy given
with --cmake, never on the tree's own file); a target that is a .pem or holds a private key; a target
without the two marker lines; and a replacement of BOTH keys the file already pins (--replace-both
overrides): every installed copy trusts only those two, so a rotation keeps one of them -- ship a build
that pins a new spare beside the active key, or, if the active key leaked, a build that pins the old
spare as the active one beside a new spare -- or those copies can never update again.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import tempfile
from pathlib import Path
from typing import Optional

from update_make_manifest import (
    DEFAULT_KEYS_CMAKE,
    TEST_PUBLIC_KEYS,
    ToolError,
    find_openssl,
    key_id,
    public_key_of,
)

# `[^\r\n]*` and not `.*`: a file with CRLF line ends must not have its CR taken into the marker.
START_MARKER = re.compile(r"^# >>> keys\b[^\r\n]*", re.MULTILINE)
END_MARKER = re.compile(r"^# <<< keys\b[^\r\n]*", re.MULTILINE)


def rewrite_keys_block(text: str, active_hex: str, spare_hex: str) -> str:
    """`text` with only the block between the marker lines replaced; line endings kept as the file has them."""
    starts = list(START_MARKER.finditer(text))
    ends = list(END_MARKER.finditer(text))
    if len(starts) != 1 or len(ends) != 1:
        raise ToolError("the file must hold exactly one '# >>> keys' line and one '# <<< keys' line")
    if ends[0].start() < starts[0].end():
        raise ToolError("the '# <<< keys' line comes before the '# >>> keys' line")
    newline = "\r\n" if "\r\n" in text else "\n"
    block = newline.join([
        "set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS OFF)",
        f'set(KATANA_UPDATE_KEY_ACTIVE "{active_hex}")',
        f'set(KATANA_UPDATE_KEY_SPARE "{spare_hex}")',
    ])
    return text[: starts[0].end()] + newline + block + newline + text[ends[0].start():]


_PIN_FLAG = re.compile(r"set\(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS\s+(ON|OFF)\)")
_PIN_KEY = re.compile(r'set\(KATANA_UPDATE_KEY_(?:ACTIVE|SPARE)\s+"([0-9a-f]{64})"\)')


def pinned_now(text: str) -> set[str]:
    """The keys the file pins as real ones; empty while it holds placeholders (or no settings at all)."""
    flag = _PIN_FLAG.search(text)
    if flag is None or flag.group(1) == "ON":
        return set()
    return set(_PIN_KEY.findall(text))


def pin_keys(active_pem: Path, spare_pem: Path, cmake_file: Path, *, allow_test_keys: bool = False,
             replace_both: bool = False, openssl: Optional[str] = None) -> tuple[str, str]:
    """Pin the public halves of two private keys in `cmake_file`; return their key ids."""
    if cmake_file.suffix.lower() == ".pem":
        raise ToolError(f"{cmake_file} is a .pem file: this tool writes only cmake/KatanaUpdateKeys.cmake")
    if allow_test_keys and cmake_file.resolve() == DEFAULT_KEYS_CMAKE.resolve():
        raise ToolError("--allow-test-keys writes published keys that anyone can sign with, so it never writes the "
                        "tree's own cmake/KatanaUpdateKeys.cmake: give --cmake a scratch copy")
    original = cmake_file.read_bytes().decode("utf-8")
    if "PRIVATE KEY" in original:
        raise ToolError(f"{cmake_file} holds a private key: this is not the keys file")
    tool = find_openssl(openssl)
    active = public_key_of(tool, active_pem)
    spare = public_key_of(tool, spare_pem)
    if active == spare:
        raise ToolError("the active and the spare key are the same key: a spare that is the active key is no way to recover from losing it")
    for name, key in (("active", active), ("spare", spare)):
        if key.hex() in TEST_PUBLIC_KEYS and not allow_test_keys:
            raise ToolError(f"the {name} key is the published test key {TEST_PUBLIC_KEYS[key.hex()]}: anyone can sign with it")
    before = pinned_now(original)
    if before and not (before & {active.hex(), spare.hex()}) and not replace_both:
        raise ToolError(
            f"the file pins {' and '.join(sorted(k[:16] for k in before))}, and neither is among the new keys: every "
            "installed copy trusts only those two, so replacing both strands them all (they could never update "
            "again). Keep one of them, or pass --replace-both if no copy was ever released with them"
        )
    updated = rewrite_keys_block(original, active.hex(), spare.hex())
    if "PRIVATE" in updated and "PRIVATE" not in original:
        raise ToolError("internal error: the new text would hold a private key")
    handle, scratch = tempfile.mkstemp(prefix=cmake_file.name + ".", dir=cmake_file.parent)
    try:
        with os.fdopen(handle, "wb") as out:
            out.write(updated.encode("utf-8"))
            out.flush()
            os.fsync(out.fileno())
        os.replace(scratch, cmake_file)
    except BaseException:
        if os.path.exists(scratch):
            os.unlink(scratch)
        raise
    return key_id(active), key_id(spare)


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        prog="update_keytool.py",
        description="Pin the update signing keys in cmake/KatanaUpdateKeys.cmake.",
        epilog=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    commands = parser.add_subparsers(dest="command", required=True, metavar="pub")
    pub = commands.add_parser("pub", help="pin the public halves of two private-key PEMs",
                              description="Rewrite the keys block of cmake/KatanaUpdateKeys.cmake from two private-key PEMs.")
    pub.add_argument("active", type=Path, help="the active private key (update-signing.pem): CI signs with it")
    pub.add_argument("spare", type=Path, help="the spare private key (update-spare.pem): kept offline")
    pub.add_argument("--cmake", type=Path, default=DEFAULT_KEYS_CMAKE, help="the file to rewrite (default cmake/KatanaUpdateKeys.cmake)")
    pub.add_argument("--allow-test-keys", action="store_true", help="accept a published RFC 8032 test key (the tool's own tests, on a scratch copy given with --cmake only)")
    pub.add_argument("--replace-both", action="store_true", help="allow replacing BOTH keys the file pins (strands every installed copy that trusts them)")
    pub.add_argument("--openssl", help="the openssl executable (default: KATANA_OPENSSL, else PATH)")
    args = parser.parse_args(argv)
    try:
        active_id, spare_id = pin_keys(args.active, args.spare, args.cmake,
                                       allow_test_keys=args.allow_test_keys,
                                       replace_both=args.replace_both, openssl=args.openssl)
    except (ToolError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"pinned in {args.cmake}")
    print(f"  active key {active_id}")
    print(f"  spare  key {spare_id}")
    print("Next: commit that file (public data). Keep both .pem files offline and never commit them;")
    # As a secret of the environment `release`, never of the repository: only the Publish job runs in that
    # environment, while a repository secret can be read by every workflow (docs/release.md, "Update signing").
    print("give CI only the active one:  gh secret set UPDATE_SIGNING_KEY --env release < " + str(args.active))
    print("(make the GitHub environment `release` first: docs/release.md, \"Update signing\", step 3)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
