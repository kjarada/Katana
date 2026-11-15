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
if ! python3 tools/setup_linux_toolchain.py --prefix "$KATANA_TOOLCHAIN" --check 2> /dev/null; then
    python3 tools/setup_linux_toolchain.py --prefix "$KATANA_TOOLCHAIN" >&2
    # A new toolchain invalidates what the test runner's configure found out
    # about the old one.
    rm -f build/linux-release/CMakeCache.txt
fi

# Only the build tools on PATH, not the whole prefix: its bin/ also holds a
# python3 that would shadow the system one.
tools_bin="$KATANA_TOOLCHAIN/katana-bin"
mkdir -p "$tools_bin"
for tool in cargo rustc cmake ctest cpack ninja; do
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

# The programs are built with cargo (target/ at the checkout's root unless
# CARGO_TARGET_DIR says otherwise); CMake only runs the process-level tests
# against them. Configure that runner (seconds); the first cargo build of the
# session is the long step.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
export RUSTFLAGS="${RUSTFLAGS:--C link-arg=-Wl,-rpath,$KATANA_TOOLCHAIN/lib}"
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
    {
        echo "export CARGO_TARGET_DIR=\"$CARGO_TARGET_DIR\""
        echo "export RUSTFLAGS=\"$RUSTFLAGS\""
    } >> "$CLAUDE_ENV_FILE"
fi
cmake --preset linux-release -DKATANA_PROGRAMS="$CARGO_TARGET_DIR/release" > /dev/null

cat <<'MSG'
Katana cloud session: the Linux toolchain is installed and the test runner (build/linux-release) is configured.

- Katana is Rust: one Cargo workspace at the checkout's root (crates/katana_<layer>), the window in egui and wgpu. There is no C++ but three small shims inside crates/ that GDAL's algorithm framework and PDAL need.
- Toolchain: rustc, cargo, GCC (for the shims), PROJ, GDAL and PDAL from conda-forge, in $KATANA_TOOLCHAIN (tools/setup_linux_toolchain.py); cargo, cmake and ctest from it are on PATH.
- Build: cargo build --locked --workspace --release
- Lint:  cargo clippy --locked --workspace --all-targets --release -- -D warnings
- Test:  cargo test --locked --workspace --release   (one crate: -p katana_<layer>)
- Process tests (the cli.* and headless window tests, against the built programs): xvfb-run -a ctest --test-dir build/linux-release -j 4 --output-on-failure
- github.com downloads are blocked here; crates come through crates.io as Cargo.lock pins them.
- How work is done in this repository: CLAUDE.md, docs/rust.md and docs/architecture.md, "Working rules". Read those before changing code.
MSG
