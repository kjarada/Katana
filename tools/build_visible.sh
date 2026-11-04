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
    local root script build log status running arguments phase
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    script="$root/tools/build_visible.sh"
    local directory="${KATANA_BUILD_DIR:-build/release}"
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
    echo "== Katana $phase in $root/$directory, $(date '+%Y-%m-%d %H:%M:%S')" | tee -a "$log"
    echo "== toolchain first on PATH: ${toolchain:-none, UCRT64}" | tee -a "$log"
    steps "$directory" "$phase" "$@" 2>&1 | tee -a "$log"
    code=${PIPESTATUS[0]}
    echo "== finished: exit=$code after $((SECONDS - started)) s" | tee -a "$log"
    echo "exit=$code" > "$status"
    rm -f "$running"
    read -r -p "Press Enter to close this window. " _
    return "$code"
}

# The build directory, the phase, then its arguments: build targets, or
# ctest's arguments.
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
    if [ "$phase" != test ]; then
        # build/release is the preset's own. Another directory that has a cache
        # is configured again WITHOUT the preset, KATANA_CONFIGURE_ARGS added:
        # the preset sets its compiler and prefix over the cache's, and twice
        # turned the clang directory into a GCC one reading clang's libraries
        # (2026-11-04), the second time for an option that only meant to turn
        # the benchmarks off. The preset is taken for a directory with no cache
        # and for --fresh, which drops one; the options then override it.
        # shellcheck disable=SC2206 # options, split on blanks on purpose
        local extra=(${KATANA_CONFIGURE_ARGS:-})
        local configure=(cmake --preset release)
        if [ "$directory" != build/release ]; then
            if [ -f "$directory/CMakeCache.txt" ] && [[ " ${extra[*]} " != *" --fresh "* ]]; then
                configure=(cmake -S . -B "$directory")
            else
                configure=(cmake --preset release -B "$directory")
            fi
        fi
        configure+=("${extra[@]}")
        echo "== configure: ${configure[*]}"
        "${configure[@]}" || return
        local targets=()
        if [ "$phase" = build ] && [ $# -gt 0 ]; then
            targets=(--target "$@")
        fi
        echo "== build: cmake --build $directory --parallel ${jobs[*]} ${targets[*]} ${keepGoing[*]}"
        cmake --build "$directory" --parallel "${jobs[@]}" "${targets[@]}" "${keepGoing[@]}" ||
            return
    fi
    if [ "$phase" != build ]; then
        echo "== test: ctest --test-dir $directory --parallel 14 --output-on-failure $*"
        QT_QPA_PLATFORM=offscreen ctest --test-dir "$directory" --parallel 14 \
            --output-on-failure "$@"
    fi
}

# One line, so bash has read the exit before main runs.
main "$@"; exit
