# Checks the install rules a configure generated, without installing anything.
#
#   cmake -DKATANA_ROOT=<source dir> -DKATANA_BINARY=<build dir> -P tools/check_install_rules.cmake
#
# 1. Every SOURCE file an install rule names exists. `cmake --install` does
#    not skip a missing one: it stops there, after the programs are copied and
#    before the runtime DLLs are, and leaves a tree that does not start. A
#    top-level file was deleted from the repository while a rule still
#    installed it, and every bundle and package failed that way.
# 2. Nothing is installed from the build's third-party inputs: the folder the
#    built-in customisation is compiled from (resources/customisation) and the
#    reference files under docs/. The built-in is tracked, but it is compiled
#    INTO the programs and travels inside them, so no copy of the file is
#    handed on beside them; the reference files are kept on the owner's
#    machine and are not ours to hand on. A bundle is handed on.
#
# 3. The only programs installed are katana, katana_cli, katana_mcp and
#    katana_updater. A program that is installed is one that is handed on, and
#    the converter of the older customisation formats
#    (katana_customisation_convert) carries the readers of those formats: it is
#    built with everything else so that it cannot rot, and is never to ship.
#    The rule names the PROGRAMS, so a developer's tool added next to it is
#    refused until somebody decides it should ship - and says so here.
#    katana_updater is the fourth, the helper that swaps the install's files
#    while Katana is not running (docs/update.md): it is decided. The two
#    executables the update tests build from the same sources with a TEST KEY
#    (katana_updater_test, katana_update_e2e) are never installed, and this
#    check is what refuses one that is: a test key in a shipped program would
#    move the trust root of every update.
# 4. share/katana/install.json, the file that says an install is a RELEASE and
#    names its target, is written ONLY by CPack: by a script CPack runs on the
#    staged tree. That script is <build dir>/KatanaInstallRecord.cmake - no file
#    of the repository: cmake/KatanaPackaging.cmake writes it into the build
#    directory at configure time, and the check below takes its path from
#    CPACK_PRE_BUILD_SCRIPTS. It is listed there first, before the signing
#    script, so what that script signs already holds the record. (A macOS
#    bundle was sealed ad hoc by the install before either ran, and
#    cmake/KatanaSign.cmake seals it again for that reason.) Never by
#    `cmake --install`, the `bundle` target or the build tree: those are
#    development trees, and a development tree that carried it
#    would be offered an update that replaced a developer's build with a
#    release. So: no install script names install.json; CPackConfig.cmake lists
#    the record script before the signing script; that script exists and
#    writes install.json; and no tree this build makes (<build>/share/katana,
#    <build>/dist/Katana/share/katana) holds one.
# 5. The Linux and macOS deploy script (cmake/KatanaDeployUnix.cmake.in, or the file
#    named by -DKATANA_DEPLOY_UNIX_SCRIPT) scans the programs it names for the
#    libraries to copy into lib/, in ONE `foreach(_program ...)` line, and must
#    name every program that is installed. A program missing there is installed
#    with its libraries not copied: it starts on the machine that built it and
#    fails on the user's. The script is configured only on Linux and macOS, so a
#    Windows build never runs it and nothing else would notice. (The macOS signing
#    loop is a different list: macOS installs no katana_updater, so that list
#    is not held to this one.)
#
# Reads <build dir>/cmake_install.cmake, where each rule is one
# `file(INSTALL ... FILES "<path>" ...)` line, and the scripts it includes
# (one for each directory that installs anything), and <build
# dir>/CPackConfig.cmake. -DKATANA_REQUIRE_CPACK=ON makes a missing
# CPackConfig.cmake an error, so the real build's check cannot pass by finding
# nothing to check; the fixtures that have none leave it off.

if(NOT KATANA_ROOT OR NOT KATANA_BINARY)
    message(FATAL_ERROR "Pass -DKATANA_ROOT=<source dir> -DKATANA_BINARY=<build dir>")
endif()
set(_script "${KATANA_BINARY}/cmake_install.cmake")
if(NOT EXISTS "${_script}")
    message(FATAL_ERROR "${_script} does not exist: configure the build first")
endif()

# Every `TYPE EXECUTABLE` rule of the top script and the scripts it includes,
# in the order they are met. Followed through the `include(...)` lines CMake
# wrote, not by searching the build tree: only what `cmake --install` would run.
set(KATANA_INSTALLED_PROGRAMS katana katana_cli katana_mcp katana_updater)
list(JOIN KATANA_INSTALLED_PROGRAMS ", " _allowed_names)
set(_pending "${_script}")
set(_seen "")
set(_programs "")
set(_violations "")
while(_pending)
    list(POP_FRONT _pending _current)
    if(_current IN_LIST _seen OR NOT EXISTS "${_current}")
        continue()
    endif()
    list(APPEND _seen "${_current}")
    file(STRINGS "${_current}" _lines REGEX "include[(]\"|TYPE EXECUTABLE|install[.]json")
    foreach(_line IN LISTS _lines)
        if(_line MATCHES "^[ \t]*include[(]\"([^\"]+)\"[)]")
            list(APPEND _pending "${CMAKE_MATCH_1}")
        elseif(_line MATCHES " TYPE EXECUTABLE .*FILES (\"[^)]*)")
            string(REGEX MATCHALL "\"[^\"]+\"" _quoted "${CMAKE_MATCH_1}")
            foreach(_path IN LISTS _quoted)
                string(REPLACE "\"" "" _path "${_path}")
                get_filename_component(_program "${_path}" NAME_WE)
                list(APPEND _programs "${_program}")
                if(NOT _program IN_LIST KATANA_INSTALLED_PROGRAMS)
                    list(APPEND _violations
                        "installs the program ${_program}, which is not one of ${_allowed_names}")
                endif()
            endforeach()
        elseif(_line MATCHES "install[.]json")
            list(APPEND _violations
                "a plain install writes install.json (${_line}): only CPack's pre-build script may, so that a build tree, a bundle and a `cmake --install` stay development trees")
        endif()
    endforeach()
endwhile()
list(LENGTH _programs _program_count)
if(_program_count EQUAL 0)
    message(FATAL_ERROR "No install rule for a program was found in ${_script}, or in the "
                        "scripts it includes: the check would pass with nothing to check")
endif()

file(TO_CMAKE_PATH "${KATANA_ROOT}" _root)
file(STRINGS "${_script}" _rules REGEX "file[(]INSTALL ")
set(_checked 0)
foreach(_rule IN LISTS _rules)
    # The quoted paths after FILES, up to the first keyword that follows them.
    if(NOT _rule MATCHES " FILES (\"[^)]*)")
        continue()
    endif()
    string(REGEX MATCHALL "\"[^\"]+\"" _quoted "${CMAKE_MATCH_1}")
    foreach(_path IN LISTS _quoted)
        string(REPLACE "\"" "" _path "${_path}")
        # Built files (the programs) exist only after a build; a rule for one
        # is the build's to satisfy. Only what is taken from the sources is
        # checked here.
        string(FIND "${_path}" "${_root}/" _at)
        if(NOT _at EQUAL 0)
            continue()
        endif()
        file(RELATIVE_PATH _rel "${_root}" "${_path}")
        if(_rel MATCHES "^build/")
            continue()
        endif()
        math(EXPR _checked "${_checked} + 1")
        if(NOT EXISTS "${_path}")
            list(APPEND _violations "installs ${_rel}, which does not exist")
        endif()
        if(_rel MATCHES "^(resources/customisation|docs)(/|$)" OR _rel MATCHES "^(resources|docs)/?$")
            list(APPEND _violations "installs ${_rel}, a third-party input of the build")
        endif()
    endforeach()
endforeach()

if(_checked EQUAL 0)
    message(FATAL_ERROR "No install rule taking files from ${_root} was found in ${_script}")
endif()

# 4. install.json: CPack's, and nobody else's.
set(_cpack "${KATANA_BINARY}/CPackConfig.cmake")
set(_cpack_checked "no CPackConfig.cmake")
if(EXISTS "${_cpack}")
    set(_cpack_checked "CPack writes it before it signs")
    file(STRINGS "${_cpack}" _pre REGEX "^set[(]CPACK_PRE_BUILD_SCRIPTS ")
    if(NOT _pre MATCHES "KatanaInstallRecord[.]cmake")
        list(APPEND _violations
            "CPACK_PRE_BUILD_SCRIPTS does not run KatanaInstallRecord.cmake, the script that writes install.json into CPack's staged tree: no package would say it is a release, and none could update")
    else()
        if(_pre MATCHES "KatanaSign[.]cmake.*KatanaInstallRecord[.]cmake")
            list(APPEND _violations
                "CPACK_PRE_BUILD_SCRIPTS runs KatanaSign.cmake before KatanaInstallRecord.cmake: install.json would be added to a tree that is already signed, which breaks the seal of a macOS bundle")
        endif()
        if(_pre MATCHES "\"([^;\"]*KatanaInstallRecord[.]cmake)")
            set(_record_script "${CMAKE_MATCH_1}")
        elseif(_pre MATCHES ";([^;\"]*KatanaInstallRecord[.]cmake)")
            set(_record_script "${CMAKE_MATCH_1}")
        endif()
        if(NOT _record_script OR NOT EXISTS "${_record_script}")
            list(APPEND _violations
                "the record script ${_record_script} named by CPACK_PRE_BUILD_SCRIPTS does not exist")
        else()
            file(READ "${_record_script}" _record_text)
            if(NOT _record_text MATCHES "install[.]json")
                list(APPEND _violations "${_record_script} does not write install.json")
            endif()
        endif()
    endif()
elseif(KATANA_REQUIRE_CPACK)
    list(APPEND _violations
        "${_cpack} was not found: the check of CPack's install.json would pass with nothing to check")
endif()
# 5. The Unix deploy script scans every program that is installed.
if(NOT KATANA_DEPLOY_UNIX_SCRIPT)
    set(KATANA_DEPLOY_UNIX_SCRIPT "${_root}/cmake/KatanaDeployUnix.cmake.in")
endif()
if(NOT EXISTS "${KATANA_DEPLOY_UNIX_SCRIPT}")
    list(APPEND _violations "${KATANA_DEPLOY_UNIX_SCRIPT} does not exist: the check of the programs it scans would pass with nothing to check")
else()
    file(STRINGS "${KATANA_DEPLOY_UNIX_SCRIPT}" _scan_lines REGEX "^foreach[(]_program ")
    list(LENGTH _scan_lines _scan_count)
    if(NOT _scan_count EQUAL 1)
        list(APPEND _violations
            "${KATANA_DEPLOY_UNIX_SCRIPT} has ${_scan_count} foreach(_program ...) lines where exactly one is expected: the list of programs it scans for libraries could not be read")
    else()
        string(REGEX REPLACE "^foreach[(]_program +| *[)] *$" "" _scan_names "${_scan_lines}")
        string(REGEX REPLACE " +" ";" _scan_names "${_scan_names}")
        list(JOIN _scan_names ", " _scan_text)
        foreach(_program IN LISTS KATANA_INSTALLED_PROGRAMS)
            if(NOT _program IN_LIST _scan_names)
                list(APPEND _violations
                    "the Unix deploy script does not scan ${_program} for libraries (it scans: ${_scan_text}): a Linux or macOS package would install it without the libraries it loads")
            endif()
        endforeach()
    endif()
endif()

# Trees this build makes that are development trees by definition.
foreach(_tree IN ITEMS "share/katana/install.json" "dist/Katana/share/katana/install.json")
    if(EXISTS "${KATANA_BINARY}/${_tree}")
        list(APPEND _violations
            "the build tree carries ${_tree}: a development tree, which an update would replace with a release, must not")
    endif()
endforeach()

if(_violations)
    list(JOIN _violations "\n  " _text)
    message(FATAL_ERROR "Install rule problems:\n  ${_text}")
endif()
list(REMOVE_DUPLICATES _programs)
list(JOIN _programs ", " _program_names)
message(STATUS "Install rules checked: ${_checked} source paths, all present, none third-party. "
               "Programs installed: ${_program_names}. "
               "Install record: no plain install writes install.json (${_cpack_checked}). "
               "Unix deploy script scans: ${_scan_text}.")
