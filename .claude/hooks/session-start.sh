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

# The GPU tests' display and device: a virtual X server and Mesa's software
# Vulkan (docs/gpu.md, "Testing"). Without them those cases skip rather than
# fail, so a failed install is reported and not fatal. Before the configure,
# which looks for xvfb-run.
if ! command -v xvfb-run > /dev/null || [ ! -e /usr/share/vulkan/icd.d/lvp_icd.json ]; then
    if command -v apt-get > /dev/null && [ "$(id -u)" = 0 ]; then
        DEBIAN_FRONTEND=noninteractive apt-get install -y -qq xvfb xauth mesa-vulkan-drivers \
            libvulkan1 > /dev/null 2>&1 ||
            echo "could not install xvfb and mesa-vulkan-drivers: the GPU tests will skip" >&2
    fi
fi

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
- The GPU renderer (src/katana_qt/gpu) is built here too, on Vulkan (docs/gpu.md). Its tests run on xcb under Xvfb, on Mesa's lavapipe: this hook installs both (xvfb, mesa-vulkan-drivers); without them those cases skip. Offscreen, the 3D view uses the software rasteriser.
- How work is done in this repository - measuring before claiming, derived test expectations, the SIMD kernel rules - is docs/architecture.md, "Working rules", and docs/performance.md. Read those before changing code.
MSG
