#!/usr/bin/env python3
"""Install the Linux or macOS toolchain Katana builds with, into one prefix.

The Windows build takes every tool and library from MSYS2 UCRT64. Linux
distributions lag behind that - Ubuntu 24.04 ships GCC 14, CGAL 5.6 and PROJ
9.4, and Katana needs GCC 15 or later (#embed, C++26 library additions),
CGAL 6 (Constraint_id::index) and PROJ 9.5 - so the Linux build takes the same
set from conda-forge instead, into a prefix of its own that touches nothing
else on the machine (docs/building.md, "Linux").

The environment is solved with py-rattler, from PyPI, rather than conda or
mamba: it needs no installer of its own, and it reaches the channel over plain
HTTPS - which is all a Claude Code cloud session's egress allows
(conda.anaconda.org and pypi.org; github.com and api.anaconda.org are blocked).

Reproducible: the solve is frozen at SNAPSHOT, so the same versions come back
on every machine and in every session until SNAPSHOT or SPECS are changed
here. Idempotent: a prefix whose stamp matches SNAPSHOT and SPECS is left
alone, so running this again costs a second.

macOS takes the same libraries from conda-forge for osx-arm64, with clang and
libc++ instead of GCC: every conda-forge C++ library for macOS - Qt, PDAL,
GDAL - is built against libc++, and GCC's libstdc++ cannot link with them
(docs/building.md, "macOS"). The script keeps its name because the cloud
session's start-up hook and the documentation name it.

    python3 tools/setup_linux_toolchain.py            # into $KATANA_TOOLCHAIN or /opt/katana-toolchain
    python3 tools/setup_linux_toolchain.py --prefix ~/katana-toolchain
    python3 tools/setup_linux_toolchain.py --check    # exit 1 if it would install
"""

from __future__ import annotations

import argparse
import asyncio
import datetime
import hashlib
import importlib
import json
import os
import subprocess
import sys
from pathlib import Path

DEFAULT_PREFIX = "/opt/katana-toolchain"

# The channel as it stood on this date. Moving it is how the toolchain is
# upgraded; the versions it resolved to are recorded in docs/building.md.
SNAPSHOT = datetime.datetime(2026, 9, 26, tzinfo=datetime.timezone.utc)

# The same versions MSYS2 gives the Windows build where the two can match
# (GCC 16.2); the libraries at the newest release the snapshot has. Eigen is
# held at 3.4 because the build asks for Eigen3.
COMMON_SPECS = [
    "cmake >=3.31",
    "ninja",
    "clang-format",
    "qt6-main >=6.9",
    "cgal-cpp >=6",
    "eigen >=3.4,<3.5",
    "proj >=9.5",
    "gdal",
    "pdal",
    "nlohmann_json",
    "libsqlite",
    "gmp",
    "mpfr",
    "libboost-headers",
    "gtest",
    "benchmark",
]

def is_arm64() -> bool:
    import platform
    return platform.machine().lower() in ("aarch64", "arm64")


# conda-forge names its Linux compilers and sysroots by target: linux-64 for
# x86-64, linux-aarch64 for 64-bit ARM, each built to run on that machine.
_LINUX_TARGET = "linux-aarch64" if is_arm64() else "linux-64"

LINUX_SPECS = [
    f"gxx_{_LINUX_TARGET} 16.2.*",
    f"gcc_{_LINUX_TARGET} 16.2.*",
    # The C library the programs are linked against, and so the oldest one they
    # run on: glibc 2.28 is Debian 10, Ubuntu 20.04 and RHEL 8. Left free, the
    # solver takes the newest sysroot (2.39 here), and a release built on
    # Ubuntu 24.04 would refuse to start on anything older.
    f"sysroot_{_LINUX_TARGET} 2.28.*",
    "libgl-devel",
    # Qt declares QVulkanInstance only where vulkan/vulkan.h is; the loader
    # itself comes with the machine's graphics driver.
    "libvulkan-headers",
]

# clang 23 is the newest conda-forge has for osx-arm64 at SNAPSHOT; the
# compiler package brings libc++, ld64 and cctools with it.
MACOS_SPECS = [
    "clangxx_osx-arm64 23.*",
    "clang_osx-arm64 23.*",
]


def is_macos() -> bool:
    return sys.platform == "darwin"


SPECS = COMMON_SPECS + (MACOS_SPECS if is_macos() else LINUX_SPECS)

# The C++ compiler the toolchain files (cmake/toolchains/) expect in bin/.
if is_macos():
    COMPILER = "arm64-apple-darwin20.0.0-clang++"
elif is_arm64():
    COMPILER = "aarch64-conda-linux-gnu-g++"
else:
    COMPILER = "x86_64-conda-linux-gnu-g++"

# py-rattler's API has changed between releases; this is the one the script
# is written against.
RATTLER = "py-rattler==0.26.0"

STAMP = ".katana-toolchain.json"


def stamp_text() -> str:
    digest = hashlib.sha256("\n".join([SNAPSHOT.isoformat(), *SPECS]).encode()).hexdigest()
    return json.dumps({"snapshot": SNAPSHOT.isoformat(), "specs": SPECS, "digest": digest}, indent=2)


def is_current(prefix: Path) -> bool:
    stamp = prefix / STAMP
    compiler = prefix / "bin" / COMPILER
    try:
        return compiler.exists() and json.loads(stamp.read_text()) == json.loads(stamp_text())
    except (OSError, ValueError):
        return False


def import_rattler():
    try:
        return importlib.import_module("rattler")
    except ImportError:
        pass
    print(f"installing {RATTLER} from PyPI", file=sys.stderr)
    subprocess.run(
        [sys.executable, "-m", "pip", "install", "--quiet", "--disable-pip-version-check",
         "--root-user-action=ignore", RATTLER],
        check=True,
    )
    importlib.invalidate_caches()
    return importlib.import_module("rattler")


async def install(prefix: Path) -> None:
    rattler = import_rattler()
    platforms = [rattler.Platform.current(), rattler.Platform("noarch")]
    print(f"solving {len(SPECS)} specs against conda-forge as of {SNAPSHOT.date()}", file=sys.stderr)
    records = await rattler.solve(
        sources=["conda-forge"],
        specs=SPECS,
        platforms=platforms,
        virtual_packages=rattler.VirtualPackage.detect(),
        exclude_newer=SNAPSHOT,
    )
    print(f"installing {len(records)} packages into {prefix}", file=sys.stderr)
    await rattler.install(records, target_prefix=str(prefix), show_progress=False)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--prefix", default=os.environ.get("KATANA_TOOLCHAIN") or DEFAULT_PREFIX,
                        help=f"where to install (default: $KATANA_TOOLCHAIN or {DEFAULT_PREFIX})")
    parser.add_argument("--check", action="store_true",
                        help="report whether the prefix is current; install nothing")
    args = parser.parse_args()
    prefix = Path(args.prefix).expanduser().resolve()

    if is_current(prefix):
        print(f"toolchain at {prefix} is current", file=sys.stderr)
        return 0
    if args.check:
        print(f"toolchain at {prefix} is missing or out of date", file=sys.stderr)
        return 1

    prefix.mkdir(parents=True, exist_ok=True)
    (prefix / STAMP).unlink(missing_ok=True)  # an interrupted install must not look current
    asyncio.run(install(prefix))
    (prefix / STAMP).write_text(stamp_text() + "\n")
    compiler = prefix / "bin" / COMPILER
    if not compiler.exists():
        print(f"{compiler} is missing after the install", file=sys.stderr)
        return 1
    version = subprocess.run([str(compiler), "--version"], check=True,
                             capture_output=True, text=True).stdout.splitlines()[0]
    print(f"toolchain at {prefix} installed: {version}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
