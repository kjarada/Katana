#!/bin/bash
# SessionStart hook for Claude Code on the web: makes a fresh cloud container
# ready to build and test Katana, and tells Claude how (docs/building.md,
# "Linux"). Local sessions are left alone - a developer's machine has its own
# toolchain.
#
# Idempotent and quick once done: the container is cached after this hook
# finishes, so the toolchain (about 3 GB, a few minutes) is installed once and
# every later session finds it current in about a second.
#
# Progress goes to stderr; stdout is what Claude reads at the start of the
# session.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
    exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel)}"

export KATANA_TOOLCHAIN="${KATANA_TOOLCHAIN:-/opt/katana-toolchain}"
python3 tools/setup_linux_toolchain.py --prefix "$KATANA_TOOLCHAIN" >&2

# Only the build tools on PATH, not the whole prefix: its bin/ also holds a
# python3 that would shadow the system one.
tools_bin="$KATANA_TOOLCHAIN/katana-bin"
mkdir -p "$tools_bin"
for tool in cmake ctest cpack ninja clang-format; do
    ln -sf "$KATANA_TOOLCHAIN/bin/$tool" "$tools_bin/$tool"
done
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
    {
        echo "export KATANA_TOOLCHAIN=\"$KATANA_TOOLCHAIN\""
        echo "export PATH=\"$tools_bin:\$PATH\""
    } >> "$CLAUDE_ENV_FILE"
fi
export PATH="$tools_bin:$PATH"

# Configure (seconds), not build (minutes of four cores): the first `cmake
# --build` of the session does that, incrementally from here on.
cmake --preset linux-release > /dev/null

cat <<'MSG'
Katana cloud session: the Linux toolchain is installed and build/linux-release is configured.

- Toolchain: GCC 16.2, Qt 6, CGAL 6, PROJ, GDAL, PDAL, GoogleTest and Benchmark from conda-forge, in $KATANA_TOOLCHAIN (tools/setup_linux_toolchain.py). cmake, ctest, ninja and clang-format from it are on PATH. The system g++ is too old for this code; never build with it.
- Build: cmake --build --preset linux-release   (all targets take about 10 minutes on this machine; name a target, e.g. --target katana_render_tests, when one suite will do)
- Test:  ctest --preset linux-release -j 4       (or one suite: build/linux-release/bin/tests/katana_<module>_tests --gtest_filter=...)
- Other presets: linux-debug, linux-relwithdebinfo, linux-sanitize. Format: cmake --build --preset linux-release --target format-check
- github.com downloads are blocked here: never add a FetchContent/URL download to the Linux path; the toolchain file finds GoogleTest and Benchmark in the prefix.
- The GPU renderer (src/katana_qt/gpu) is Direct3D 11 only and is not built on Linux; the 3D view uses the software rasteriser here.
- How work is done in this repository - measuring before claiming, derived test expectations, the SIMD kernel rules - is docs/architecture.md, "Working rules", and docs/performance.md. Read those before changing code.
MSG
