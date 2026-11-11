#!/usr/bin/env bash
# Builds and tests Katana in a console window of its own, so the owner sees
# every line of the configure, the compile and the test run as it happens (the
# owner's standing instruction of 2026-11-03; CLAUDE.md section 3).
#
# Why a window: Claude's shell moves a command still running after ten minutes
# to the background, and in the IDE its output is not shown to the owner, so a
# full build or the whole suite was never seen. The window is the owner's view;
# every line also goes to a log Claude reads.
#
#   tools/build_visible.sh [all]               configure, build everything, run the whole suite
#   tools/build_visible.sh build [target...]   configure and build (only those targets, if named)
#   tools/build_visible.sh test [ctest args]   the suite only, e.g. test -R PlanContextMenu
#   tools/build_visible.sh cargo               the Rust workspace alone: clippy, build, test
#   tools/build_visible.sh wsl gate [-- ctest args]   step 6's cli gate: the cli tests of the last
#                                              WSL build run against the Rust katana_cli and
#                                              katana_mcp, both runs' replies compared
#                                              (tools/rust_cli_gate.py, whose header says how)
#   tools/build_visible.sh wsl wgate [-- ctest args]  step 7's window gate: the headless window
#                                              tests of the last WSL build run against the
#                                              Rust katana (tools/rust_window_gate.py)
#   tools/build_visible.sh wsl [phase] [...]   any of those in WSL instead (below)
#   tools/build_visible.sh close               close this checkout's windows whose run has ended
#
# A window stays open at its end so that the owner can read it; once its status
# file has been read, Claude closes it with `close` (the owner's instruction of
# 2026-11-05, "make sure to close any finished cmd"). A window whose run is
# still going is never closed: its bash has the run below it, and a finished
# one, waiting at its last prompt, has nothing.
#
# The Rust port (docs/rust.md) is part of every run: after the C++ build, clippy
# and the build of rust/ with warnings as errors; after ctest, cargo test, with
# its counts summed beside ctest's. A narrow run - `build` with targets, `test`
# with ctest's arguments - leaves the Rust out, as it leaves out the rest of the
# C++. On Windows the cargo is MSYS2 UCRT64's, first on PATH as cmake is.
#
# `wsl` runs the phase in the WSL distribution Ubuntu-24.04 (KATANA_WSL_DISTRO
# for another) with the conda-forge prefix of tools/setup_linux_toolchain.py
# (/opt/katana-toolchain, or KATANA_TOOLCHAIN): the linux-release preset and
# cargo, both building on WSL's own disk, in ~/katana-build/<worktree>/, because
# a build on /mnt/d is slow. The window, the log and the status file are on the
# Windows side as for any run, in build/wsl/, so they are waited on and read the
# same way. Git is never run inside WSL: a worktree made by Windows git names its
# repository by a D:/ path that Linux cannot open, and the build needs no git.
#
# Run from Claude's shell it opens the window and returns at once, printing
# where the log and the status file are. The status file appears when the run
# ends and holds exit=<code>: wait for it, judge the run by that code, then
# read the log. The window stays open at the end until the owner presses Enter
# or closes it.
#
# The build directory is the build/release of the checkout or worktree this
# script is in. KATANA_BUILD_JOBS caps the compile, e.g. 4 when agents build in
# several worktrees at once (CLAUDE.md section 8). A second run in the same
# build directory while one is going is refused: two ninjas in one directory
# corrupt .ninja_deps.
#
# Another build of the same preset, such as the clang one that mirrors the
# Windows ARM64 job (docs/building.md):
#   KATANA_BUILD_DIR       the build directory, relative to the checkout
#                          (build/release). One with a cache is configured
#                          again without the preset, so its compilers stay;
#                          one without is made with the preset
#   KATANA_CONFIGURE_ARGS  options for that configure, split on blanks. With
#                          --fresh (which drops the cache) or with no cache,
#                          the preset is taken and they override it: give the
#                          compilers and prefix then; otherwise they are added
#                          to the cache's, as -DKATANA_BUILD_BENCHMARKS=OFF
#   KATANA_TOOLCHAIN_BIN   put first on PATH, before /c/msys64/ucrt64/bin
#                          (which is always there, for cmake and ninja)
#   KATANA_BUILD_KEEP_GOING=1   ninja -k 0: every compile error in one run, not
#                          only the first
# The log and the status file are then in that directory.
#
# Everything is in main(), called from the last line: bash reads a script as it
# runs it, so a window still open while this file was edited resumed at its old
# byte offset in the new text - mid-line, and once a step away from starting a
# second build. Parsed whole before it runs, an open window never reads the file
# again.

main() {
    local root script build log status running arguments phase host=windows
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    script="$root/tools/build_visible.sh"

    # ---- inside WSL, started by a window below: run the steps, nothing else ---------------
    if [ "${1:-}" = "--inside-wsl" ]; then
        shift
        inside_wsl "$root" "$@"
        return
    fi
    if [ "${1:-}" = close ]; then
        close_finished "$script"
        return
    fi
    # `wsl` on Claude's command line; KATANA_BUILD_HOST in the window it opens,
    # which is given the phase alone.
    if [ "${1:-}" = wsl ]; then
        host=wsl
        shift
    elif [ "${KATANA_BUILD_HOST:-}" = wsl ]; then
        host=wsl
    fi

    local directory="${KATANA_BUILD_DIR:-build/release}"
    if [ "$host" = wsl ]; then
        directory=build/wsl # the window's log and status; the build itself is on WSL's disk
    fi
    build="$root/$directory"
    log="$build/visible_build.log"
    status="$build/visible_build.status"
    running="$build/visible_build.running"
    # The phase's arguments, NUL-separated: they reach the window through this
    # file, not through cmd.exe's command line, which would read a | in a ctest
    # pattern (-R "a|b") as a pipe and a & or ^ as its own.
    arguments="$build/visible_build.args"

    phase="${1:-all}"
    case "$phase" in
        all | build | test | cargo | gate | wgate) ;;
        *)
            echo "usage: tools/build_visible.sh [wsl] [all | build [target...] | test [ctest arguments...] | cargo]" >&2
            return 2
            ;;
    esac
    [ $# -gt 0 ] && shift

    # ---- in Claude's shell: open the window and return -----------------------------------
    if [ "${KATANA_BUILD_WINDOW:-}" != "1" ]; then
        mkdir -p "$build"
        if [ -f "$running" ] && kill -0 "$(cat "$running" 2> /dev/null)" 2> /dev/null; then
            echo "refused: a run is still going in $build (pid $(cat "$running")); wait for $status" >&2
            return 1
        fi
        rm -f "$status" "$running"
        : > "$log"
        if [ $# -gt 0 ]; then
            printf '%s\0' "$@" > "$arguments"
        else
            : > "$arguments"
        fi
        local title
        title="Katana $phase - $(basename "$root")"
        if [ "$host" = wsl ]; then
            title="Katana WSL $phase - $(basename "$root")"
        fi
        # start gives the window a console of its own on the owner's desktop; the
        # environment, KATANA_BUILD_WINDOW with it, goes with it. Only the phase
        # and two paths with no cmd.exe metacharacters are on its command line.
        KATANA_BUILD_WINDOW=1 KATANA_BUILD_HOST="$host" cmd.exe //c start "$title" \
            "$(cygpath -w "$BASH")" --login "$(cygpath -w "$script")" "$phase" || return
        echo "window=\"$title\" log=$log status=$status"
        return 0
    fi

    # ---- in the window: run, show, log -----------------------------------------------------
    # The arguments the launcher wrote, in place of the command line's.
    local given=()
    if [ -s "$arguments" ]; then
        mapfile -d '' given < "$arguments"
    fi
    set -- "${given[@]}"

    cd "$root" || return 1
    # The toolchain asked for in front of UCRT64, which holds cmake and ninja:
    # CLANG64 has its compilers but neither (a clang run found cmake missing).
    # MSYS hands a path-like variable to cmd.exe in Windows form, so it arrives
    # here as C:/msys64/clang64/bin, whose drive colon split PATH into "C" and
    # a folder that does not exist: the clang suite's first listing then could
    # not find libc++.dll. cygpath -u takes either form.
    local toolchain=""
    if [ -n "${KATANA_TOOLCHAIN_BIN:-}" ]; then
        toolchain="$(cygpath -u "$KATANA_TOOLCHAIN_BIN")"
    fi
    export PATH="${toolchain:+$toolchain:}/c/msys64/ucrt64/bin:$PATH"
    echo $$ > "$running"
    # shellcheck disable=SC2064 # the path is fixed now, on purpose
    trap "rm -f '$running'" EXIT

    local started code
    started=$SECONDS
    if [ "$host" = wsl ]; then
        local distro="${KATANA_WSL_DISTRO:-Ubuntu-24.04}"
        echo "== Katana $phase in WSL ($distro), sources $root, $(date '+%Y-%m-%d %H:%M:%S')" | tee -a "$log"
        # The caps and switches reach the Linux side through WSLENV; the phase's
        # arguments on wsl.exe's command line, with MSYS's path conversion off so
        # that a Linux path or a ctest pattern arrives as it was written. --exec,
        # not --, because -- hands the line to a shell, which takes a pattern's
        # ( | $ as its own syntax.
        # KATANA_ONLINE_TESTS=1 lets the cases that reach real hosts run
        # (CLAUDE.md section 4), in both programs: step 5's gate has an
        # online half (docs/rust.md, "The order of the work").
        export WSLENV="KATANA_BUILD_JOBS/u:KATANA_BUILD_KEEP_GOING/u:KATANA_TOOLCHAIN/u:KATANA_ONLINE_TESTS/u${WSLENV:+:$WSLENV}"
        MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' wsl.exe -d "$distro" --cd "$(cygpath -w "$root")" \
            --exec bash tools/build_visible.sh --inside-wsl "$phase" "$@" 2>&1 | tee -a "$log"
        code=${PIPESTATUS[0]}
    else
        echo "== Katana $phase in $root/$directory, $(date '+%Y-%m-%d %H:%M:%S')" | tee -a "$log"
        echo "== toolchain first on PATH: ${toolchain:-none, UCRT64}" | tee -a "$log"
        steps "$directory" "$phase" "$@" 2>&1 | tee -a "$log"
        code=${PIPESTATUS[0]}
    fi
    echo "== finished: exit=$code after $((SECONDS - started)) s" | tee -a "$log"
    echo "exit=$code" > "$status"
    rm -f "$running"
    read -r -p "Press Enter to close this window. " _
    return "$code"
}

# The build directory, the phase, then its arguments: build targets, or
# ctest's arguments. KATANA_LINUX_BUILD (set by inside_wsl) makes it the Linux
# build: the linux-release preset into that directory, and the suite under an
# X server, where the gpu. suite runs Vulkan as in the release job.
steps() {
    local directory="$1" phase="$2"
    shift 2
    local jobs=() keepGoing=()
    if [ -n "${KATANA_BUILD_JOBS:-}" ]; then
        jobs=("$KATANA_BUILD_JOBS")
    fi
    if [ "${KATANA_BUILD_KEEP_GOING:-}" = 1 ]; then
        keepGoing=(-- -k 0)
    fi
    local oracle="$directory/bin/tests/katana_number_format_oracle"
    if [[ "$oracle" != /* ]]; then
        oracle="$PWD/$oracle"
    fi
    if [ -n "${KATANA_LINUX_BUILD:-}" ]; then
        export KATANA_NUMBER_FORMAT_ORACLE="$oracle"
    else
        export KATANA_NUMBER_FORMAT_ORACLE="$(cygpath -w "$oracle.exe")"
    fi
    # The C++ katana_cli of the same build, for step 3's round-trip gate
    # (rust/crates/katana_app/tests/round_trip.rs): it runs the program beside
    # the Rust katana_cli on every file of the corpus, and fails, as the
    # oracle's test does, when this is not set.
    local cli="$directory/bin/katana_cli"
    if [[ "$cli" != /* ]]; then
        cli="$PWD/$cli"
    fi
    if [ -n "${KATANA_LINUX_BUILD:-}" ]; then
        export KATANA_CPP_CLI="$cli"
    else
        export KATANA_CPP_CLI="$(cygpath -w "$cli.exe")"
    fi
    if [ "$phase" = cargo ]; then
        cargo_steps build && cargo_steps test
        return
    fi
    # Step 6's cli gate, in WSL only: the C++ build tree's cli tests, rewritten
    # to run the Rust programs cargo built, beside a rerun of the C++ ones, and
    # the two runs' replies compared. It reads the build tree of the last full
    # run and runs after it, never beside it: the cli tests write into that tree.
    if [ "$phase" = gate ]; then
        if [ -z "${KATANA_LINUX_BUILD:-}" ]; then
            echo "the gate runs in WSL: tools/build_visible.sh wsl gate" >&2
            return 2
        fi
        cli_gate "$directory" "$@"
        return
    fi
    if [ "$phase" = wgate ]; then
        if [ -z "${KATANA_LINUX_BUILD:-}" ]; then
            echo "the window gate runs in WSL: tools/build_visible.sh wsl wgate" >&2
            return 2
        fi
        window_gate "$directory" "$@"
        return
    fi
    if [ "$phase" != test ]; then
        local configure
        if [ -n "${KATANA_LINUX_BUILD:-}" ]; then
            configure=(cmake --preset linux-release -B "$directory")
        else
            # build/release is the preset's own. Another directory that has a
            # cache is configured again WITHOUT the preset, KATANA_CONFIGURE_ARGS
            # added: the preset sets its compiler and prefix over the cache's,
            # and twice turned the clang directory into a GCC one reading
            # clang's libraries (2026-11-04), the second time for an option that
            # only meant to turn the benchmarks off. The preset is taken for a
            # directory with no cache and for --fresh, which drops one; the
            # options then override it.
            # shellcheck disable=SC2206 # options, split on blanks on purpose
            local extra=(${KATANA_CONFIGURE_ARGS:-})
            configure=(cmake --preset release)
            if [ "$directory" != build/release ]; then
                if [ -f "$directory/CMakeCache.txt" ] && [[ " ${extra[*]} " != *" --fresh "* ]]; then
                    configure=(cmake -S . -B "$directory")
                else
                    configure=(cmake --preset release -B "$directory")
                fi
            fi
            configure+=("${extra[@]}")
        fi
        echo "== configure: ${configure[*]}"
        "${configure[@]}" || return
        local targets=()
        if [ "$phase" = build ] && [ $# -gt 0 ]; then
            targets=(--target "$@")
        fi
        echo "== build: cmake --build $directory --parallel ${jobs[*]} ${targets[*]} ${keepGoing[*]}"
        cmake --build "$directory" --parallel "${jobs[@]}" "${targets[@]}" "${keepGoing[@]}" ||
            return
        # The Rust is part of a whole build, not of one that names targets.
        if [ ${#targets[@]} -eq 0 ]; then
            cargo_steps build || return
        fi
    fi
    if [ "$phase" != build ]; then
        local runner=()
        if [ -n "${KATANA_LINUX_BUILD:-}" ]; then
            runner=(xvfb-run -a)
        fi
        echo "== test: ${runner[*]} ctest --test-dir $directory --parallel 14 --output-on-failure $*"
        local tested=0
        QT_QPA_PLATFORM=offscreen "${runner[@]}" ctest --test-dir "$directory" --parallel 14 \
            --output-on-failure "$@" || tested=$?
        # The Rust suite too, unless ctest was given a selection: the run is then
        # about those tests alone. It runs after a failed ctest as well, so that
        # one run gives both answers.
        if [ $# -eq 0 ]; then
            if ! cargo_steps test && [ "$tested" -eq 0 ]; then
                tested=1
            fi
            # Step 6's cli gate is part of the whole run in WSL, after both
            # suites, whose cli tests it reruns: a reply of the Rust programs
            # that differs from the C++'s fails the run as a failed test does.
            # On Windows it waits for step 8.
            if [ -n "${KATANA_LINUX_BUILD:-}" ] && ! cli_gate "$directory" &&
                [ "$tested" -eq 0 ]; then
                tested=1
            fi
        fi
        return "$tested"
    fi
}

# Step 6's cli gate (tools/rust_cli_gate.py, whose header says how): the cli
# tests of the build tree in $1, rewritten to run the Rust programs cargo
# built, beside a rerun of the C++ ones, and the two runs' replies compared.
# The rest of the arguments go to ctest.
cli_gate() {
    local directory="$1"
    shift
    echo "== gate: xvfb-run -a python3 tools/rust_cli_gate.py --build $directory --programs $CARGO_TARGET_DIR/release --rerun-cpp $*"
    QT_QPA_PLATFORM=offscreen xvfb-run -a python3 tools/rust_cli_gate.py --build "$directory" \
        --programs "$CARGO_TARGET_DIR/release" --rerun-cpp "$@"
}

# Step 7's window gate (tools/rust_window_gate.py, whose header says how): the
# headless window tests of the build tree in $1, run against the Rust katana
# beside a rerun of the C++ window, both programs' streams and exit codes
# compared, both screenshots kept side by side for a person. The rest of the
# arguments go to ctest. Its own phase until step 7's last wave builds the Rust
# katana; then it joins the whole run as the cli gate did.
window_gate() {
    local directory="$1"
    shift
    echo "== window gate: xvfb-run -a python3 tools/rust_window_gate.py --build $directory --programs $CARGO_TARGET_DIR/release --rerun-cpp $*"
    QT_QPA_PLATFORM=offscreen xvfb-run -a python3 tools/rust_window_gate.py --build "$directory" \
        --programs "$CARGO_TARGET_DIR/release" --rerun-cpp "$@"
}

# The Rust workspace: `build` is clippy and the build with warnings as errors,
# `test` is cargo test with the counts of every test binary summed. Every cargo
# here is given the same flags - RUSTFLAGS as the caller set it, then -D
# warnings - or each step would rebuild what the one before it built. Every
# cargo is --locked: the committed rust/Cargo.lock is what is built, so a
# build never takes a newer crate on its own (docs/rust.md, "Third-party
# libraries": a crate is at least two weeks old, and listed in the notices).
cargo_steps() {
    local step="$1" manifest=rust/Cargo.toml
    local flags="${RUSTFLAGS:+$RUSTFLAGS }-D warnings"
    if [ "$step" = build ]; then
        export CLIPPY_CONF_DIR="$PWD/rust"
        echo "== cargo: $(command -v cargo), $(cargo --version), $(rustc --version)"
        echo "== cargo clippy --locked --workspace --all-targets --release --keep-going -- -D warnings"
        RUSTFLAGS="$flags" cargo clippy --manifest-path "$manifest" --locked --workspace --all-targets \
            --release --keep-going -- -D warnings || return
        rust_float_guard "$flags" || return
        echo "== cargo build --locked --workspace --release"
        RUSTFLAGS="$flags" cargo build --manifest-path "$manifest" --locked --workspace --release
        return
    fi
    # --no-fail-fast: a failing test binary must not hide the binaries after
    # it, or the counts summed below would read as the whole suite's.
    echo "== cargo test --locked --workspace --release --no-fail-fast"
    local results code
    results="$(mktemp)"
    RUSTFLAGS="$flags" cargo test --manifest-path "$manifest" --locked --workspace --release --no-fail-fast 2>&1 |
        tee "$results"
    code=${PIPESTATUS[0]}
    awk '/^test result:/ { for (i = 1; i < NF; i++) { if ($(i + 1) ~ /^passed;/) p += $i
                                                      if ($(i + 1) ~ /^failed;/) f += $i
                                                      if ($(i + 1) ~ /^ignored;/) s += $i } }
         END { printf "== cargo test: %d passed, %d failed, %d ignored\n", p, f, s }' "$results"
    rm -f "$results"
    return "$code"
}

rust_float_guard() {
    local flags="$1" results code
    results="$(mktemp)"
    echo "== Rust float-to-text canary: the forbidden call must be diagnosed"
    RUSTFLAGS="$flags" cargo clippy \
        --manifest-path tests/rust_check_fixtures/float_to_string/Cargo.toml \
        --target-dir "${CARGO_TARGET_DIR:-rust/target}/float-format-canary" \
        --release -- -D warnings 2>&1 | tee "$results"
    code=${PIPESTATUS[0]}
    if [ "$code" -eq 0 ] || ! grep -q 'KATANA_REAL_FORMATTER_REQUIRED' "$results"; then
        rm -f "$results"
        echo "Rust float-to-text guard failed: the forbidden call was not diagnosed" >&2
        return 1
    fi
    rm -f "$results"
    echo "== Rust float-to-text guard: forbidden call rejected"
}

# In WSL, from a window's wsl.exe: the toolchain prefix's build tools first on
# PATH (only those: its bin/ also holds a python3 that would shadow the
# system's), cargo linking with the prefix's GCC and sysroot as CMake builds
# with them, and both builds on WSL's own disk; then steps() as on Windows.
inside_wsl() {
    local root="$1" phase="$2"
    shift 2
    local prefix="${KATANA_TOOLCHAIN:-/opt/katana-toolchain}"
    if [ ! -x "$prefix/bin-tools/cargo" ] || [ ! -x "$prefix/bin-tools/cmake" ]; then
        echo "no toolchain at $prefix/bin-tools: install it with tools/setup_linux_toolchain.py" \
            "and link its build tools there (docs/rust.md, \"WSL\")" >&2
        return 1
    fi
    export KATANA_TOOLCHAIN="$prefix" KATANA_LINUX_BUILD=1
    # A Linux PATH of its own. WSL appends the Windows one (/mnt/c/...), and
    # CMake's find_package searches the prefixes of PATH: on 2026-11-05 it took
    # MSYS2's Windows CURL package through /mnt/c/msys64/ucrt64 and the
    # configure failed.
    export PATH="$prefix/bin-tools:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    local machine home
    machine="$(uname -m)"
    export "CARGO_TARGET_${machine^^}_UNKNOWN_LINUX_GNU_LINKER=$prefix/bin/$machine-conda-linux-gnu-gcc"
    # A program finds the prefix's libraries without LD_LIBRARY_PATH.
    export RUSTFLAGS="-C link-arg=-Wl,-rpath,$prefix/lib"
    home="$HOME/katana-build/$(basename "$root")"
    export CARGO_TARGET_DIR="$home/cargo"
    mkdir -p "$home"
    cd "$root" || return 1
    echo "== WSL: $(. /etc/os-release && echo "$PRETTY_NAME"), toolchain $prefix, builds in $home"
    steps "$home/linux-release" "$phase" "$@"
}

# Closes the windows of this script - this checkout's, by its full path - whose
# run has ended. A window is the bash that `start` gave a console (its parent
# is no window bash); at its last prompt it has no child but its console host,
# while a running one has the run's subshell, cmake or wsl.exe below it.
close_finished() {
    local script
    script="$(cygpath -w "$1")"
    powershell.exe -NoProfile -Command "
        \$script = '$script'
        \$all = Get-CimInstance Win32_Process
        \$windows = @(\$all | Where-Object { \$_.Name -eq 'bash.exe' -and \$_.CommandLine -and \$_.CommandLine.Contains(\$script) })
        \$closed = 0
        foreach (\$w in \$windows) {
            if (\$windows.ProcessId -contains \$w.ParentProcessId) { continue }
            \$busy = @(\$all | Where-Object { \$_.ParentProcessId -eq \$w.ProcessId -and \$_.Name -ne 'conhost.exe' })
            if (\$busy.Count -eq 0) { Stop-Process -Id \$w.ProcessId -Force; \$closed++ }
        }
        \"closed \$closed finished window(s); \$(@(\$windows | Where-Object { \$windows.ProcessId -notcontains \$_.ParentProcessId }).Count - \$closed) still running\"
    "
}

# One line, so bash has read the exit before main runs.
main "$@"; exit
