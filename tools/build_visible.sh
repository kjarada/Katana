#!/usr/bin/env bash
# Builds and tests Katana in a console window of its own, so the owner sees
# every line of the build and the test run as it happens (the owner's standing
# instruction of 2026-11-03).
#
# Why a window: Claude's shell moves a command still running after ten minutes
# to the background, and in the IDE its output is not shown to the owner, so a
# full build or the whole suite was never seen. The window is the owner's view;
# every line also goes to a log Claude reads.
#
# Katana is Rust (docs/rust.md): the build is cargo's, and the process-level
# tests - the command-line tests, the headless window tests, the update and
# repository checks - are CTest's, from a CMake project that compiles nothing
# and runs the programs cargo built (the test runner, docs/building.md).
#
#   tools/build_visible.sh [wsl] [all]               clippy, the build, cargo test, then the
#                                                    runner's whole suite
#   tools/build_visible.sh [wsl] cargo [args...]     the Rust workspace alone: clippy, build,
#                                                    test; with cargo test arguments, only
#                                                    those tests
#   tools/build_visible.sh [wsl] test [ctest args]   the runner's suite only, against the
#                                                    programs the last build made, e.g.
#                                                    test -R PlanContextMenu
#   tools/build_visible.sh close                     close this checkout's windows whose run
#                                                    has ended
#
# With `wsl` a phase runs in the WSL distribution; without it the same phases
# run natively on Windows, in MSYS2 UCRT64 (docs/rust.md, "Windows, once the
# port works"; the owner lifted the WSL-only rule on 2026-11-16). After a
# Windows build, tools/stage_windows.sh puts the programs' DLLs and data where
# a program started from Explorer finds them.
#
# Gone with the C++ (2026-11-15): `build` with its CMake targets, the C++
# configure and its ninja, and the two gates `gate` and `wgate`, which ran the
# C++ suite's cli and window tests against the Rust programs and compared the
# replies with the C++ programs' own. Their oracle, the C++ build, no longer
# exists; what they held is the suite the runner now runs against the Rust
# alone.
#
# A window stays open at its end so that the owner can read it; once its status
# file has been read, Claude closes it with `close` (the owner's instruction of
# 2026-11-05, "make sure to close any finished cmd"). A window whose run is
# still going is never closed: its bash has the run below it, and a finished
# one, waiting at its last prompt, has nothing.
#
# `wsl` runs the phase in the WSL distribution Ubuntu-24.04 (KATANA_WSL_DISTRO
# for another) with the conda-forge prefix of tools/setup_linux_toolchain.py
# (/opt/katana-toolchain, or KATANA_TOOLCHAIN): cargo and the runner both
# building on WSL's own disk, in ~/katana-build/<worktree>/ (cargo/ and
# tests/), because a build on /mnt/d is slow. The window, the log and the
# status file are on the Windows side as for any run, in build/wsl/, so they
# are waited on and read the same way. Git is never run inside WSL: a worktree
# made by Windows git names its repository by a D:/ path that Linux cannot
# open, and the build needs no git.
#
# Run from Claude's shell it opens the window and returns at once, printing
# where the log and the status file are. The status file appears when the run
# ends and holds exit=<code>: wait for it, judge the run by that code, then
# read the log. The window stays open at the end until the owner presses Enter
# or closes it.
#
# The log and the status file are in build/wsl/ for a WSL run and in
# build/windows/ for a native one. KATANA_BUILD_JOBS caps cargo's jobs, e.g. 4
# when agents build in several worktrees at once. A second run of the same kind
# while one is going is refused: two cargos in one target folder queue on its
# lock, and two runners in one directory overwrite each other's results.
# KATANA_TOOLCHAIN_BIN is put first on PATH in a native run, before
# /c/msys64/ucrt64/bin (which is always there, for cargo, cmake and ninja).
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

    local directory=build/windows
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
        all | cargo | test) ;;
        *)
            echo "usage: tools/build_visible.sh [wsl] [all | cargo [cargo test arguments...] | test [ctest arguments...]] | close" >&2
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
        # ( | $ as its own syntax. KATANA_ONLINE_TESTS=1 lets the cases that
        # reach real hosts run (docs/testing.md).
        export WSLENV="KATANA_BUILD_JOBS/u:KATANA_TOOLCHAIN/u:KATANA_SHEET_PNG/p:KATANA_ONLINE_TESTS/u${WSLENV:+:$WSLENV}"
        MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' wsl.exe -d "$distro" --cd "$(cygpath -w "$root")" \
            --exec bash tools/build_visible.sh --inside-wsl "$phase" "$@" 2>&1 | tee -a "$log"
        code=${PIPESTATUS[0]}
    else
        # The toolchain asked for in front of UCRT64, which holds cargo, cmake and
        # ninja. MSYS hands a path-like variable to cmd.exe in Windows form, so it
        # arrives here as C:/msys64/clang64/bin, whose drive colon would split
        # PATH into "C" and a folder that does not exist; cygpath -u takes either
        # form.
        local toolchain=""
        if [ -n "${KATANA_TOOLCHAIN_BIN:-}" ]; then
            toolchain="$(cygpath -u "$KATANA_TOOLCHAIN_BIN")"
        fi
        export PATH="${toolchain:+$toolchain:}/c/msys64/ucrt64/bin:$PATH"
        echo "== Katana $phase in $root, $(date '+%Y-%m-%d %H:%M:%S')" | tee -a "$log"
        echo "== toolchain first on PATH: ${toolchain:-none, UCRT64}" | tee -a "$log"
        # cargo's own target folder in the checkout (target, git-ignored), and
        # the runner's preset folder, build/release; it is handed the programs' folder in
        # Windows form, which cmake.exe reads.
        steps "build/release" "$(cygpath -m "$root/target/release")" "$phase" "$@" 2>&1 |
            tee -a "$log"
        code=${PIPESTATUS[0]}
    fi
    echo "== finished: exit=$code after $((SECONDS - started)) s" | tee -a "$log"
    echo "exit=$code" > "$status"
    rm -f "$running"
    read -r -p "Press Enter to close this window. " _
    return "$code"
}

# The runner's build directory, the folder of the programs cargo built, the
# phase, then its arguments: cargo test's, or ctest's. KATANA_LINUX_BUILD (set
# by inside_wsl) makes it the Linux run: the runner's suite under an X server,
# where the window's tests draw through Vulkan as in the release job.
steps() {
    local runner="$1" programs="$2" phase="$3"
    shift 3
    if [ "$phase" = cargo ]; then
        # With arguments, only those tests: `cargo -p katana_window --test codes NAME`
        # (cargo test's own arguments, after --locked --release), shown in full.
        if [ $# -gt 0 ]; then
            echo "== cargo test --locked --release $*"
            RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-D warnings" cargo test \
                --locked --release "$@" -- --nocapture
            return
        fi
        cargo_steps build && cargo_steps test
        return
    fi
    if [ "$phase" = test ]; then
        runner_steps "$runner" "$programs" "$@"
        return
    fi
    # all: the build first, and nothing after a build that failed - a suite run
    # against the programs of an earlier build would answer for code that is not
    # the checkout's. Then both suites, the runner's after a failed cargo test as
    # well, so that one run gives both answers.
    cargo_steps build || return
    local tested=0
    cargo_steps test || tested=$?
    if ! runner_steps "$runner" "$programs" && [ "$tested" -eq 0 ]; then
        tested=1
    fi
    return "$tested"
}

# The test runner (the root CMakeLists.txt; docs/building.md): configured with
# its preset - linux-release in WSL, release on Windows - and the folder of the
# programs it runs, then the whole of its suite, or the ctest arguments given.
# The configure compiles nothing and takes seconds, so it runs every time: the
# tests it registers follow the checkout.
runner_steps() {
    local runner="$1" programs="$2" preset=release
    shift 2
    if [ -n "${KATANA_LINUX_BUILD:-}" ]; then
        preset=linux-release
    fi
    local configure=(cmake --preset "$preset" -B "$runner" "-DKATANA_PROGRAMS=$programs")
    echo "== runner: ${configure[*]}"
    "${configure[@]}" || return
    local display=()
    if [ -n "${KATANA_LINUX_BUILD:-}" ]; then
        display=(xvfb-run -a)
    fi
    echo "== test: ${display[*]} ctest --test-dir $runner --parallel 14 --output-on-failure $*"
    "${display[@]}" ctest --test-dir "$runner" --parallel 14 --output-on-failure "$@"
}

# The Rust workspace: `build` is clippy and the build with warnings as errors,
# `test` is cargo test with the counts of every test binary summed. Every cargo
# here is given the same flags - RUSTFLAGS as the caller set it, then -D
# warnings - or each step would rebuild what the one before it built. Every
# cargo is --locked: the committed Cargo.lock is what is built, so a
# build never takes a newer crate on its own (docs/rust.md, "Third-party
# libraries": a crate is at least two weeks old, and listed in the notices).
cargo_steps() {
    local step="$1" manifest=Cargo.toml
    local flags="${RUSTFLAGS:+$RUSTFLAGS }-D warnings"
    local jobs=()
    if [ -n "${KATANA_BUILD_JOBS:-}" ]; then
        jobs=(--jobs "$KATANA_BUILD_JOBS")
    fi
    if [ "$step" = build ]; then
        export CLIPPY_CONF_DIR="$PWD"
        echo "== cargo: $(command -v cargo), $(cargo --version), $(rustc --version)"
        echo "== cargo clippy --locked --workspace --all-targets --release --keep-going ${jobs[*]} -- -D warnings"
        RUSTFLAGS="$flags" cargo clippy --manifest-path "$manifest" --locked --workspace --all-targets \
            --release --keep-going "${jobs[@]}" -- -D warnings || return
        rust_float_guard "$flags" || return
        echo "== cargo build --locked --workspace --release ${jobs[*]}"
        RUSTFLAGS="$flags" cargo build --manifest-path "$manifest" --locked --workspace --release "${jobs[@]}"
        return
    fi
    # --no-fail-fast: a failing test binary must not hide the binaries after
    # it, or the counts summed below would read as the whole suite's.
    echo "== cargo test --locked --workspace --release --no-fail-fast ${jobs[*]}"
    local results code
    results="$(mktemp)"
    RUSTFLAGS="$flags" cargo test --manifest-path "$manifest" --locked --workspace --release --no-fail-fast \
        "${jobs[@]}" 2>&1 | tee "$results"
    code=${PIPESTATUS[0]}
    awk '/^test result:/ { for (i = 1; i < NF; i++) { if ($(i + 1) ~ /^passed;/) p += $i
                                                      if ($(i + 1) ~ /^failed;/) f += $i
                                                      if ($(i + 1) ~ /^ignored;/) s += $i } }
         END { printf "== cargo test: %d passed, %d failed, %d ignored\n", p, f, s }' "$results"
    rm -f "$results"
    return "$code"
}

# The float-to-text guard. A real becomes text through katana_core's number
# formatters, never through to_string, whose text is not the shortest
# round-trip text the replies are held to; clippy.toml forbids the call
# (disallowed-methods, KATANA_REAL_FORMATTER_REQUIRED). This proves the lint
# still fires, on a crate whose one function makes the forbidden call
# (tests/rust_check_fixtures/float_to_string): a lint that stopped firing would
# pass every build and find nothing.
rust_float_guard() {
    local flags="$1" results code
    results="$(mktemp)"
    echo "== Rust float-to-text canary: the forbidden call must be diagnosed"
    RUSTFLAGS="$flags" cargo clippy \
        --manifest-path tests/rust_check_fixtures/float_to_string/Cargo.toml \
        --target-dir "${CARGO_TARGET_DIR:-target}/float-format-canary" \
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
# system's), cargo linking with the prefix's GCC and sysroot, and both the
# cargo target and the runner on WSL's own disk; then steps() as on Windows.
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
    # A Linux PATH of its own. WSL appends the Windows one (/mnt/c/...), and a
    # program that searches PATH would find MSYS2's Windows tools through
    # /mnt/c/msys64: on 2026-11-05 a CMake configure took MSYS2's Windows CURL
    # package that way.
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
    steps "$home/tests" "$CARGO_TARGET_DIR/release" "$phase" "$@"
}

# Closes the windows of this script - this checkout's, by its full path - whose
# run has ended. A window is the bash that `start` gave a console (its parent
# is no window bash), with the script's own bash below it; at its last prompt
# nothing runs under either but those two and the console host, while a
# running one has cargo, cmake or wsl.exe somewhere below it. Counting only the
# window's direct children took that second bash for a run, and no finished
# window was ever closed (2026-11-16).
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
            \$below = @(\$w.ProcessId); \$busy = @()
            while (\$below.Count -gt 0) {
                \$kids = @(\$all | Where-Object { \$below -contains \$_.ParentProcessId })
                \$busy += @(\$kids | Where-Object { \$_.Name -ne 'conhost.exe' -and \$_.Name -ne 'bash.exe' })
                \$below = @(\$kids | ForEach-Object { \$_.ProcessId })
            }
            if (\$busy.Count -eq 0) { Stop-Process -Id \$w.ProcessId -Force; \$closed++ }
        }
        \"closed \$closed finished window(s); \$(@(\$windows | Where-Object { \$windows.ProcessId -notcontains \$_.ParentProcessId }).Count - \$closed) still running\"
    "
}

# One line, so bash has read the exit before main runs.
main "$@"; exit
