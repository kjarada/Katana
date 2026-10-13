# Runs the uninstaller's file removal: the functions of cmake/NSIS.template.in that read the
# installed manifest and delete what it lists, compiled with makensis into a throwaway
# installer whose only job is to write an uninstaller, and run over a small tree.
#
#   cmake -DTEMPLATE=<cmake/NSIS.template.in> -DMAKENSIS=<makensis> -DWORK=<folder>
#         [-DMUTATE=<name>] -P run_nsis_uninstaller.cmake
#
# check_nsis_script.cmake reads the template's text; that cannot tell whether the byte reader
# of the manifest finds the paths in it, nor whether the filter in front of a delete keeps a
# hostile entry from deleting. This does: every case below has files that the uninstaller must
# remove and files it must leave (a user's file, and a decoy that a path which got past the
# filter would delete), and the tree that is left is compared with the one worked out here by
# hand.
#
# What runs is the template's own text, cut out between "Function un.<name>" and "FunctionEnd":
# un.StrStr, un.DeleteListedFile, un.DeleteManifestFiles and un.RemoveUpdateState. Nothing is
# paraphrased. The one change is that $LOCALAPPDATA, which an environment variable cannot
# redirect (NSIS asks the shell), is replaced by a folder under WORK, so that no test touches
# the folders of whoever runs it. The registry is not touched at all (the template's
# .onInit, its RunOnce delete and the Settings > Apps entry are in the text checks instead).
#
# -DMUTATE=<name> changes the extracted text first in one way that breaks one rule, and the
# test that uses it expects this script to say the tree differs: a test that cannot see a
# broken reader passes as quietly as one that finds none.

foreach(_required TEMPLATE MAKENSIS WORK)
    if(NOT ${_required})
        message(FATAL_ERROR "Pass -D${_required}=...")
    endif()
endforeach()
file(READ "${TEMPLATE}" _template)
string(REPLACE "\r\n" "\n" _template "${_template}")

# ---- the functions, as the template has them ----------------------------------------------
function(katana_extract name out)
    string(FIND "${_template}" "\nFunction ${name}\n" _start)
    if(_start EQUAL -1)
        message(FATAL_ERROR "${TEMPLATE} has no  Function ${name}")
    endif()
    math(EXPR _start "${_start} + 1")
    string(SUBSTRING "${_template}" ${_start} -1 _from)
    string(FIND "${_from}" "\nFunctionEnd" _end)
    if(_end EQUAL -1)
        message(FATAL_ERROR "Function ${name} does not end")
    endif()
    math(EXPR _end "${_end} + 12")
    string(SUBSTRING "${_from}" 0 ${_end} _body)
    set(${out} "${_body}\n" PARENT_SCOPE)
endfunction()
# The folder the updater stages in is a define of the template (a define comes before its use).
string(REGEX MATCH "\n[ \t]*!define KATANA_UPDATE_STATE[^\n]*" _state_define "${_template}")
if(_state_define STREQUAL "")
    message(FATAL_ERROR "${TEMPLATE} does not define KATANA_UPDATE_STATE")
endif()
set(_functions "${_state_define}\n\n")
foreach(_name IN ITEMS un.StrStr un.DeleteListedFile un.DeleteManifestFiles un.RemoveUpdateState)
    katana_extract(${_name} _one)
    string(APPEND _functions "${_one}\n")
endforeach()

function(katana_mutate old new)
    string(FIND "${_functions}" "${old}" _at)
    if(_at EQUAL -1)
        message(FATAL_ERROR "mutation ${MUTATE} does not apply: the functions have no  ${old}")
    endif()
    string(REPLACE "${old}" "${new}" _changed "${_functions}")
    set(_functions "${_changed}" PARENT_SCOPE)
endfunction()
if(MUTATE STREQUAL "first_folder_length")
    katana_mutate([=[StrCpy $R1 $R0 4]=] [=[StrCpy $R1 $R0 3]=])
elseif(MUTATE STREQUAL "etc_not_accepted")
    katana_mutate([=[StrCmp $R1 "etc\" listed_folder_ok]=] "")
elseif(MUTATE STREQUAL "no_dotdot_rule")
    katana_mutate([=[Push "\..\"]=] [=[Push "\.x.\"]=])
elseif(MUTATE STREQUAL "no_dot_rule")
    katana_mutate([=[Push "\.\"]=] [=[Push "\.x\"]=])
elseif(MUTATE STREQUAL "no_empty_component_rule")
    katana_mutate([=[Push "\\"]=] [=[Push "\x\"]=])
elseif(MUTATE STREQUAL "slash_kept")
    katana_mutate([=[StrCpy $3 "$3\"]=] [=[StrCpy $3 "$3/"]=])
elseif(MUTATE STREQUAL "wildcard_allowed")
    katana_mutate([=[StrCmp $1 42 manifest_bad_byte]=] "")
elseif(MUTATE STREQUAL "high_byte_allowed")
    katana_mutate([=[IntCmp $1 126 manifest_character_ok_too manifest_character_ok_too manifest_bad_byte]=] "")
elseif(MUTATE STREQUAL "state_json_removed")
    katana_mutate([=[StrCmp $1 "state.json" state_skip]=] "")
elseif(MUTATE)
    message(FATAL_ERROR "unknown mutation '${MUTATE}'")
endif()

# ---- the throwaway installer ------------------------------------------------------------------
file(REMOVE_RECURSE "${WORK}")
file(MAKE_DIRECTORY "${WORK}")
file(TO_NATIVE_PATH "${WORK}" _work)
file(TO_NATIVE_PATH "${WORK}/localappdata" _localappdata)
string(REPLACE [=[$LOCALAPPDATA]=] "${_localappdata}" _functions "${_functions}")
string(FIND "${_functions}" [=[$LOCALAPPDATA]=] _real_folder)
if(NOT _real_folder EQUAL -1)
    message(FATAL_ERROR "the functions still name the real LOCALAPPDATA; refusing to run them")
endif()

# The target line is the template's own (cmake/NSIS.template.in, "Which NSIS this is").
set(_nsi [=[Unicode true
!if /FileExists "${NSISDIR}\Plugins\unicode\nsDialogs.dll"
  Target amd64-unicode
!endif
RequestExecutionLevel user
SilentInstall silent
]=])
string(APPEND _nsi "OutFile \"${_work}\\installer.exe\"\n")
string(APPEND _nsi [=[
Section
  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd
]=])
string(APPEND _nsi "${_functions}")
string(APPEND _nsi [=[
Section "Uninstall"
  Call un.DeleteManifestFiles
  Call un.RemoveUpdateState
SectionEnd
]=])
file(WRITE "${WORK}/uninstaller_functions.nsi" "${_nsi}")
execute_process(COMMAND "${MAKENSIS}" /V2 "${WORK}/uninstaller_functions.nsi"
    RESULT_VARIABLE _status OUTPUT_VARIABLE _output ERROR_VARIABLE _error)
if(NOT _status EQUAL 0)
    message(FATAL_ERROR "makensis did not accept the uninstaller's functions (${_status}):\n${_output}\n${_error}")
endif()

# ---- one case: a tree, a manifest, the uninstaller, the tree that is left -------------------------
set(_failures "")
set(_dirs_to_compare "")
# A file with content, made with its folders. Bytes outside ASCII are written as they are.
function(katana_make_file root relative)
    file(WRITE "${root}/${relative}" "x")
endfunction()
# One manifest entry as the producer writes it (tools/update_make_manifest.py: sorted keys,
# compact, one per line); the path is inserted as given.
function(katana_entry out path)
    set(${out} "{\"class\":\"lib\",\"path\":\"${path}\",\"sha256\":\"0000000000000000000000000000000000000000000000000000000000000000\",\"size\":1}" PARENT_SCOPE)
endfunction()

# case_name: the folder under WORK. files, entries and expect are lists (the paths have no
# semicolons). MANIFEST_VARIABLE, instead of ENTRIES, names a variable that holds the whole file. expect lists, for
# the folder of the case, every file and folder that must be left, relative to it, sorted.
function(katana_case case_name)
    cmake_parse_arguments(ARG "NO_MANIFEST;CHECK_LOCAL" "MANIFEST_VARIABLE" "FILES;ENTRIES;EXPECT;LOCAL_FILES;LOCAL_EXPECT" ${ARGN})
    set(_case "${WORK}/${case_name}")
    set(_root "${_case}/install")
    file(MAKE_DIRECTORY "${_root}")
    file(REMOVE_RECURSE "${WORK}/localappdata")
    file(TO_NATIVE_PATH "${_root}" _native_root)
    execute_process(COMMAND "${WORK}/installer.exe" /S "/D=${_native_root}"
        RESULT_VARIABLE _made OUTPUT_VARIABLE _o ERROR_VARIABLE _e)
    if(NOT _made EQUAL 0 OR NOT EXISTS "${_root}/Uninstall.exe")
        message(FATAL_ERROR "${case_name}: the throwaway installer did not write the uninstaller (${_made})")
    endif()
    foreach(_file IN LISTS ARG_FILES)
        katana_make_file("${_root}" "${_file}")
    endforeach()
    # Files outside the install folder that no listed path may reach.
    foreach(_outside IN ITEMS outside.txt outside2.txt)
        katana_make_file("${_case}" "${_outside}")
    endforeach()
    foreach(_file IN LISTS ARG_LOCAL_FILES)
        katana_make_file("${WORK}/localappdata" "${_file}")
    endforeach()
    if(NOT ARG_NO_MANIFEST)
        if(ARG_MANIFEST_VARIABLE)
            set(_manifest "${${ARG_MANIFEST_VARIABLE}}")
        else()
            set(_manifest "{\n\"applier\":1,\n\"files\":[\n")
            set(_first TRUE)
            foreach(_entry IN LISTS ARG_ENTRIES)
                if(NOT _first)
                    string(APPEND _manifest ",\n")
                endif()
                set(_first FALSE)
                string(APPEND _manifest "${_entry}")
            endforeach()
            string(APPEND _manifest "\n],\n\"version\":\"0.3.0\"\n}\n")
        endif()
        file(WRITE "${_root}/share/katana/installed.manifest.json" "${_manifest}")
    endif()

    execute_process(COMMAND "${_root}/Uninstall.exe" /S "_?=${_native_root}"
        RESULT_VARIABLE _ran OUTPUT_VARIABLE _o ERROR_VARIABLE _e TIMEOUT 120)
    if(NOT _ran EQUAL 0)
        set(_failures "${_failures}${case_name}: the uninstaller exited with ${_ran}\n" PARENT_SCOPE)
        return()
    endif()

    file(GLOB_RECURSE _left LIST_DIRECTORIES true RELATIVE "${_case}" "${_case}/*")
    list(SORT _left)
    set(_wanted ${ARG_EXPECT})
    list(SORT _wanted)
    if(NOT "${_left}" STREQUAL "${_wanted}")
        set(_extra ${_left})
        list(REMOVE_ITEM _extra ${_wanted})
        set(_missing ${_wanted})
        list(REMOVE_ITEM _missing ${_left})
        string(REPLACE ";" "\n      " _extra "${_extra}")
        string(REPLACE ";" "\n      " _missing "${_missing}")
        string(APPEND _failures "${case_name}: the tree that is left is not the one expected.\n"
            "    left that should have gone:\n      ${_extra}\n    gone that should have stayed:\n      ${_missing}\n")
    endif()
    if(ARG_CHECK_LOCAL)
        file(GLOB_RECURSE _local_left LIST_DIRECTORIES true RELATIVE "${WORK}/localappdata" "${WORK}/localappdata/*")
        set(_local_wanted ${ARG_LOCAL_EXPECT})
        list(SORT _local_left)
        list(SORT _local_wanted)
        if(NOT "${_local_left}" STREQUAL "${_local_wanted}")
            string(REPLACE ";" "\n      " _shown "${_local_left}")
            string(REPLACE ";" "\n      " _wanted_shown "${_local_wanted}")
            string(APPEND _failures "${case_name}: what is left under LOCALAPPDATA is not what was expected.\n"
                "    left:\n      ${_shown}\n    expected:\n      ${_wanted_shown}\n")
        endif()
    endif()
    set(_failures "${_failures}" PARENT_SCOPE)
endfunction()

# ---- case 1: a manifest of an updated install, with hostile entries among the good ------------------
# Good entries (their files must go, with the folders they leave empty): normal names, a space,
# a deep folder, etc\ (the certificates), a literal $R0 in a name, and a file after an over-long
# entry (the reader must find its way back to the next entry).
# Hostile entries (every one has a decoy file that the path would reach if it got past the filter,
# or would be a delete through the wildcard, so the decoy must still be there afterwards):
set(_long "")
foreach(_i RANGE 1 30)
    string(APPEND _long "abcdefghij")   # 300 characters
endforeach()
string(ASCII 233 _high)          # the byte 0xE9, which is the character U+00E9 as NSIS reads it
string(ASCII 195 169 _e_acute)   # the same character as the UTF-8 that a file name is written in
foreach(_pair IN ITEMS
        "good1|bin/katana.exe" "good2|bin/with space.dll" "good3|bin/plugins/styles/a.dll"
        "good4|etc/ssl/certs/ca-bundle.crt" "good5|lib/libx.a" "good6|share/katana/samples/s1/project.json"
        "good7|bin/$R0.dat" "good9|bin/missing.dll"
        "bad1|bin/../victim-dotdot.txt" "bad2|bin/../../outside.txt" "bad3|../outside.txt"
        "bad4|bin/./dot-target.dll" "bad5|bin//double.dll" "bad6|/abs.txt"
        "bad7|${WORK}/case_1/outside2.txt" "bad8|bin/wild*.txt" "bad9|bin/q?.txt"
        "bad11|other/foreign.dll" "bad12|binx/f.dll"
        "bad13|bin/hi${_high}.txt")
    string(REGEX REPLACE "^([a-z0-9]+)[|].*$" "\\1" _key "${_pair}")
    string(REGEX REPLACE "^[a-z0-9]+[|]" "" _path "${_pair}")
    katana_entry(_entry_${_key} "${_path}")
endforeach()
# Paths with a backslash or a JSON escape in them: bracket arguments, which have no escapes.
katana_entry(_entry_bad10 [=[bin\bs-target.txt]=])
katana_entry(_entry_bad14 [=[bin/caf\u00e9.txt]=])
katana_entry(_entry_long "bin/${_long}.dll")
katana_entry(_entry_good8 "bin/after-long.dll")

katana_case(case_1
    FILES
        bin/katana.exe "bin/with space.dll" bin/plugins/styles/a.dll etc/ssl/certs/ca-bundle.crt lib/libx.a
        share/katana/samples/s1/project.json "bin/$R0.dat" bin/after-long.dll
        "bin/user notes.txt" "my project.katana"
        victim-dotdot.txt bin/dot-target.dll bin/double.dll abs.txt bin/wild1.txt bin/wild2.txt bin/q1.txt
        bin/bs-target.txt bin/cafu00e9.txt "bin/hi${_e_acute}.txt" other/foreign.dll binx/f.dll
    ENTRIES
        "${_entry_good1}" "${_entry_good2}" "${_entry_good3}" "${_entry_good4}" "${_entry_good5}"
        "${_entry_good6}" "${_entry_good7}" "${_entry_long}" "${_entry_good8}" "${_entry_good9}"
        "${_entry_bad1}" "${_entry_bad2}" "${_entry_bad3}" "${_entry_bad4}" "${_entry_bad5}" "${_entry_bad6}"
        "${_entry_bad7}" "${_entry_bad8}" "${_entry_bad9}" "${_entry_bad10}" "${_entry_bad11}"
        "${_entry_bad12}" "${_entry_bad13}" "${_entry_bad14}"
    CHECK_LOCAL
    LOCAL_FILES
        Katana/update/state.json Katana/update/Katana-0.3.0-win64.exe Katana/update/staged/payload/x.bin
        Katana/update/helper/katana_updater.exe Katana/Katana/cache/c.dat
    LOCAL_EXPECT
        Katana Katana/update Katana/update/state.json Katana/Katana Katana/Katana/cache Katana/Katana/cache/c.dat
    EXPECT
        install install/Uninstall.exe install/abs.txt install/victim-dotdot.txt "install/my project.katana"
        install/bin "install/bin/user notes.txt" install/bin/dot-target.dll install/bin/double.dll
        install/bin/wild1.txt install/bin/wild2.txt install/bin/q1.txt install/bin/bs-target.txt
        install/bin/cafu00e9.txt "install/bin/hi${_e_acute}.txt"
        install/binx install/binx/f.dll install/other install/other/foreign.dll
        install/share install/share/katana install/share/katana/installed.manifest.json
        outside.txt outside2.txt)
# (The expectations are worked out by hand from the lists above: the good entries' files and the
# folders only they used (plugins, etc, lib, samples) are gone, bin and share/katana stay because
# a user's file and the manifest are in them; the installed manifest itself is the Section's to
# remove, not these functions'; under LOCALAPPDATA only state.json and the cache stay.)
# Case 1's decoys are under case_1/, so the hostile absolute path above names case_1/outside2.txt.

# ---- case 2: a manifest cut off in the middle of an entry --------------------------------------------
# (By variable: a [ in a list argument groups everything after it into one element.)
set(_truncated "{\n\"files\":[\n{\"path\":\"bin/gone.dll\",\"size\":1},\n{\"path\":\"bin/trunc")
katana_case(case_2
    FILES bin/gone.dll bin/trunc bin/keep.txt
    MANIFEST_VARIABLE _truncated
    EXPECT
        install install/Uninstall.exe install/bin install/bin/trunc install/bin/keep.txt
        install/share install/share/katana install/share/katana/installed.manifest.json
        outside.txt outside2.txt)

# ---- case 3: no manifest (a copy that has never been updated); the staged download alone ------------
katana_case(case_3
    NO_MANIFEST
    FILES bin/katana.exe
    CHECK_LOCAL
    LOCAL_FILES Katana/update/Katana-0.3.0-win64.exe
    EXPECT install install/Uninstall.exe install/bin install/bin/katana.exe outside.txt outside2.txt)

if(_failures)
    message(FATAL_ERROR "The uninstaller's file removal is not what was expected:\n${_failures}")
endif()
message(STATUS "Uninstaller file removal${MUTATE}: the manifest's files and the updater's staged files went; "
    "hostile entries, user files and everything outside the folder stayed, in 3 cases.")
