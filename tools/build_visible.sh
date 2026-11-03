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
# Everything is in main(), called from the last line: bash reads a script as it
# runs it, so a window still open while this file was edited resumed at its old
# byte offset in the new text - mid-line, and once a step away from starting a
# second build. Parsed whole before it runs, an open window never reads the file
# again.

main() {
    local root script build log status running arguments phase
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    script="$root/tools/build_visible.sh"
    build="$root/build/release"
    log="$build/visible_build.log"
    status="$build/visible_build.status"
    running="$build/visible_build.running"
    # The phase's arguments, NUL-separated: they reach the window through this
    # file, not through cmd.exe's command line, which would read a | in a ctest
    # pattern (-R "a|b") as a pipe and a & or ^ as its own.
    arguments="$build/visible_build.args"

    phase="${1:-all}"
    case "$phase" in
        all | build | test) ;;
        *)
            echo "usage: tools/build_visible.sh [all | build [target...] | test [ctest arguments...]]" >&2
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
        # start gives the window a console of its own on the owner's desktop; the
        # environment, KATANA_BUILD_WINDOW with it, goes with it. Only the phase
        # and two paths with no cmd.exe metacharacters are on its command line.
        KATANA_BUILD_WINDOW=1 cmd.exe //c start "$title" "$(cygpath -w "$BASH")" --login \
            "$(cygpath -w "$script")" "$phase" || return
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
    export PATH="/c/msys64/ucrt64/bin:$PATH"
    echo $$ > "$running"
    # shellcheck disable=SC2064 # the path is fixed now, on purpose
    trap "rm -f '$running'" EXIT

    local started code
    started=$SECONDS
    echo "== Katana $phase in $root, $(date '+%Y-%m-%d %H:%M:%S')" | tee -a "$log"
    steps "$phase" "$@" 2>&1 | tee -a "$log"
    code=${PIPESTATUS[0]}
    echo "== finished: exit=$code after $((SECONDS - started)) s" | tee -a "$log"
    echo "exit=$code" > "$status"
    rm -f "$running"
    read -r -p "Press Enter to close this window. " _
    return "$code"
}

# The phase, then its arguments: build targets, or ctest's arguments.
steps() {
    local phase="$1"
    shift
    local jobs=()
    if [ -n "${KATANA_BUILD_JOBS:-}" ]; then
        jobs=("$KATANA_BUILD_JOBS")
    fi
    if [ "$phase" != test ]; then
        echo "== configure: cmake --preset release"
        cmake --preset release || return
        local targets=()
        if [ "$phase" = build ] && [ $# -gt 0 ]; then
            targets=(--target "$@")
        fi
        echo "== build: cmake --build build/release --parallel ${jobs[*]} ${targets[*]}"
        cmake --build build/release --parallel "${jobs[@]}" "${targets[@]}" || return
    fi
    if [ "$phase" != build ]; then
        echo "== test: ctest --test-dir build/release --parallel 14 --output-on-failure $*"
        QT_QPA_PLATFORM=offscreen ctest --test-dir build/release --parallel 14 \
            --output-on-failure "$@"
    fi
}

# One line, so bash has read the exit before main runs.
main "$@"; exit
