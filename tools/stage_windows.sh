#!/bin/bash
# tools/stage_windows.sh [programs-folder] [stage-folder]
#
# A runnable copy of Katana's programs on Windows, from cargo's MSYS2 UCRT64 build
# (docs/building.md): the programs link GDAL, PDAL, PROJ, SQLite, curl and the GCC runtime as
# DLLs of C:\msys64\ucrt64\bin, which is on no PATH a program started from Explorer has. The
# stage holds, as an installed Katana does:
#
#   <stage>/bin/      katana.exe, katana_cli.exe, katana_mcp.exe, katana_updater.exe and every
#                     DLL they import from UCRT64, followed import by import (objdump -p);
#   <stage>/share/    gdal/ and proj/, found beside the libraries (katana_core::library_data,
#                     katana_gis::gdal_registry: <folder of the DLL>/../share/...);
#   <stage>/etc/ssl/certs/ca-bundle.crt
#                     the certificate bundle curl checks https:// against; without it every
#                     web request fails.
#
# Defaults: target/release (cargo's output at the checkout's root) into build/windows/Katana.
# With --beside, the runtime is put beside cargo's own output instead, so that
# target/release/katana.exe itself starts from Explorer: the DLLs into target/release, the
# data into target/share and target/etc (one folder up from the DLLs, where they are looked for).
# Run from the checkout's root in an MSYS2 or Git bash. Copies only; a stage made before is
# refreshed in place. GDAL's and PDAL's run-time plugins are not copied (none is needed by the
# formats Katana's tests use).
set -euo pipefail

if [ "${1:-}" = "--beside" ]; then
    stage=build/windows/Katana
    bash "$0" target/release "$stage" >/dev/null
    cp -f "$stage"/bin/*.dll target/release/
    mkdir -p target/share target/etc/ssl/certs
    cp -rf "$stage/share/gdal" "$stage/share/proj" target/share/
    cp -f "$stage/etc/ssl/certs/ca-bundle.crt" target/etc/ssl/certs/
    echo "the runtime is beside target/release/katana.exe (and in $stage)"
    exit 0
fi

ucrt="${KATANA_UCRT64:-/c/msys64/ucrt64}"
programs="${1:-target/release}"
stage="${2:-build/windows/Katana}"
objdump="$ucrt/bin/objdump.exe"
[ -x "$objdump" ] || objdump="$(command -v objdump)"

mkdir -p "$stage/bin"
queue=()
for program in katana katana_cli katana_mcp katana_updater; do
    if [ ! -f "$programs/$program.exe" ]; then
        echo "no $programs/$program.exe: build first (bash tools/build_visible.sh cargo)" >&2
        exit 1
    fi
    cp -f "$programs/$program.exe" "$stage/bin/"
    queue+=("$stage/bin/$program.exe")
done

# Every DLL imported, followed through what each DLL imports in turn; a DLL that is not in
# UCRT64 is the system's (kernel32, the CRT's api-ms-win-*) and stays where Windows has it.
declare -A seen=()
copied=0
while [ "${#queue[@]}" -gt 0 ]; do
    file="${queue[0]}"
    queue=("${queue[@]:1}")
    while IFS= read -r dll; do
        key="${dll,,}"
        [ -n "$key" ] || continue
        [ -z "${seen[$key]:-}" ] || continue
        seen[$key]=1
        if [ -f "$ucrt/bin/$dll" ]; then
            cp -f "$ucrt/bin/$dll" "$stage/bin/"
            copied=$((copied + 1))
            queue+=("$stage/bin/$dll")
        fi
    done < <("$objdump" -p "$file" | sed -n 's/^[[:space:]]*DLL Name: //p' | tr -d '\r')
done

mkdir -p "$stage/share" "$stage/etc/ssl/certs"
cp -rf "$ucrt/share/gdal" "$stage/share/"
cp -rf "$ucrt/share/proj" "$stage/share/"
cp -f "$ucrt/etc/ssl/certs/ca-bundle.crt" "$stage/etc/ssl/certs/"

echo "staged: $stage/bin - 4 programs and $copied DLLs from $ucrt/bin; share/gdal, share/proj, etc/ssl/certs/ca-bundle.crt"
echo "run: $stage/bin/katana.exe [project-folder]"
