# Holds the text of Katana's Windows installer script to what makes the installer PER USER
# (decision D1, docs/release.md "The Windows installer", cmake/NSIS.template.in), without
# running it:
#
#   cmake -DTEMPLATE=<cmake/NSIS.template.in> [-DMUTATE=<name>] -P check_nsis_script.cmake
#   cmake -DSCRIPT=<project.nsi> [-DEXPECT_VERSION=0.2.0] [-DEXPECT_FILES=bin/katana.exe;...]
#         [-DMUTATE=<name>] -P check_nsis_script.cmake
#
# TEMPLATE is configured here with sample values for the variables CPack supplies (what
# KatanaPackaging.cmake sets, and what the generator derives from a staged tree), so it runs
# on any host with cmake. SCRIPT is what CPack really generated, from the real configuration
# (build_nsis_package.cmake). The rules are the same for both:
#
#   * no administrator rights asked for, and nothing machine-wide written: no HKLM or any
#     other hive but HKCU, no "all users" shell context, no Program Files or system folder;
#   * the folder is %LOCALAPPDATA%\Programs\Katana, and the registry holds what the updater
#     reads and Settings > Apps shows (include/katana/update/registry.hpp);
#   * the uninstaller removes the updater's working folder .update and every file the
#     installed manifest lists, and never removes the install folder recursively, so what a
#     person put there stays;
#   * a silent run never waits for an answer; the text is Unicode.
#
# Comments are removed before anything is looked at: the template explains in them what it
# does not do, and a word in a comment is not an instruction.
#
# -DMUTATE=<name> changes the text first in one way that breaks one rule (a check that cannot
# see a violation passes as quietly as one that finds none); the tests that use it expect the
# message that names the rule. A mutation that does not apply is an error of its own.

if(TEMPLATE)
    file(READ "${TEMPLATE}" _template)
    # Sample values, as CPack's NSIS generator would supply them for the staged tree of a Katana
    # package (bracket arguments: no escapes, the $ and \ are NSIS's).
    set(CPACK_PACKAGE_VERSION "0.2.0")
    set(CPACK_PACKAGE_VERSION_PATCH "0")
    set(CPACK_TEMPORARY_DIRECTORY "C:/sample/_CPack_Packages/win64/NSIS/Katana-0.2.0-win64")
    set(CPACK_PACKAGE_VENDOR "Jarada")
    set(CPACK_PACKAGE_INSTALL_REGISTRY_KEY "Katana")
    set(CPACK_PACKAGE_INSTALL_DIRECTORY "Katana")
    set(CPACK_NSIS_PACKAGE_NAME "Katana")
    set(CPACK_NSIS_DISPLAY_NAME "Katana 0.2.0")
    set(CPACK_NSIS_UNINSTALL_NAME "Uninstall")
    set(CPACK_NSIS_COMPRESSOR "lzma")
    set(CPACK_NSIS_ENABLE_UNINSTALL_BEFORE_INSTALL "ON")
    set(CPACK_TOPLEVEL_DIRECTORY "C:/sample/_CPack_Packages/win64/NSIS")
    set(CPACK_OUTPUT_FILE_NAME "Katana-0.2.0-win64.exe")
    set(CPACK_NSIS_INSTALL_ROOT [=[$LOCALAPPDATA\Programs]=])
    set(CPACK_NSIS_INSTALLED_ICON_NAME [=[bin\katana.exe]=])
    set(CPACK_NSIS_FULL_INSTALL [=[File /r "${INST_DIR}\*.*"]=])
    set(CPACK_NSIS_CREATE_ICONS [=[CreateShortCut "$SMPROGRAMS\$STARTMENU_FOLDER\Katana.lnk" "$INSTDIR\bin\katana.exe"]=])
    set(CPACK_NSIS_DELETE_ICONS [=[Delete "$SMPROGRAMS\$MUI_TEMP\Katana.lnk"]=])
    set(CPACK_NSIS_DELETE_FILES [=[Delete "$INSTDIR\bin\katana.exe"
  Delete "$INSTDIR\share\katana\install.json"]=])
    set(CPACK_NSIS_DELETE_DIRECTORIES [=[RMDir "$INSTDIR\bin"
  RMDir "$INSTDIR\share\katana"
  RMDir "$INSTDIR\share"]=])
    string(CONFIGURE "${_template}" _text @ONLY)
    set(_what "the template with sample values")
elseif(SCRIPT)
    if(NOT EXISTS "${SCRIPT}")
        message(FATAL_ERROR "${SCRIPT} does not exist: there is no installer script to check")
    endif()
    file(READ "${SCRIPT}" _text)
    set(_what "${SCRIPT}")
else()
    message(FATAL_ERROR "Pass -DTEMPLATE=<NSIS.template.in> or -DSCRIPT=<project.nsi>")
endif()

# ---- the text, as instructions ----------------------------------------------------------
string(REPLACE "\r\n" "\n" _text "${_text}")
# Whole-line comments out (a line continuation inside one is not a continuation)...
string(REGEX REPLACE "(^|\n)[ \t]*[;#][^\n]*" "\\1" _text "${_text}")
# ...a backslash that ends a line joins the next one to it, as NSIS reads it...
string(REGEX REPLACE "\\\\\n[ \t]*" " " _text "${_text}")
# ...and a semicolon is only text, not a list separator.
string(REPLACE ";" "<semicolon>" _text "${_text}")

# ---- the mutations, for the tests of this check ---------------------------------------------
function(katana_mutate old new)
    string(FIND "${_text}" "${old}" _at)
    if(_at EQUAL -1)
        message(FATAL_ERROR "mutation ${MUTATE} does not apply: the text has no  ${old}")
    endif()
    string(REPLACE "${old}" "${new}" _changed "${_text}")
    set(_text "${_changed}" PARENT_SCOPE)
endfunction()

if(MUTATE STREQUAL "administrator")
    katana_mutate([=[RequestExecutionLevel user]=] [=[RequestExecutionLevel admin]=])
elseif(MUTATE STREQUAL "second_level")
    katana_mutate([=[RequestExecutionLevel user]=] "RequestExecutionLevel user\nRequestExecutionLevel highest")
elseif(MUTATE STREQUAL "machine_registry")
    katana_mutate([=[WriteRegStr HKCU "${KATANA_UNINSTALL_KEY}" "DisplayName"]=]
                  [=[WriteRegStr HKLM "${KATANA_UNINSTALL_KEY}" "DisplayName"]=])
elseif(MUTATE STREQUAL "shell_context")
    katana_mutate([=[SetShellVarContext current]=] [=[SetShellVarContext all]=])
elseif(MUTATE STREQUAL "program_files")
    katana_mutate([=[InstallDir "$LOCALAPPDATA\Programs\Katana"]=] [=[InstallDir "$PROGRAMFILES64\Katana"]=])
elseif(MUTATE STREQUAL "no_update_folder_removal")
    katana_mutate([=[RMDir /r "$INSTDIR\.update"]=] "")
elseif(MUTATE STREQUAL "no_manifest_removal")
    katana_mutate([=[Call un.DeleteManifestFiles]=] "")
elseif(MUTATE STREQUAL "install_folder_removed_recursively")
    katana_mutate([=[Section "Uninstall"]=] "Section \"Uninstall\"\n  RMDir /r \"$INSTDIR\"")
elseif(MUTATE STREQUAL "install_folder_wildcard_delete")
    katana_mutate([=[Section "Uninstall"]=] "Section \"Uninstall\"\n  Delete \"$INSTDIR\\*.*\"")
elseif(MUTATE STREQUAL "silent_prompt")
    katana_mutate([=[ /SD IDOK]=] "")
elseif(MUTATE STREQUAL "no_install_location")
    katana_mutate([=[WriteRegStr HKCU "${KATANA_UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"]=] "")
elseif(MUTATE STREQUAL "no_folder_record")
    katana_mutate([=[WriteRegStr HKCU "${KATANA_PRODUCT_KEY}" "" "$INSTDIR"]=] "")
elseif(MUTATE STREQUAL "ansi")
    katana_mutate([=[Unicode true]=] "")
elseif(MUTATE STREQUAL "keys_stay_behind")
    katana_mutate([=[DeleteRegKey HKCU "${KATANA_UNINSTALL_KEY}"]=] "")
elseif(MUTATE)
    message(FATAL_ERROR "unknown mutation '${MUTATE}'")
endif()

# ---- the rules ------------------------------------------------------------------------
# The problems found, collected where every function can add to them.
function(katana_problem text)
    set_property(GLOBAL APPEND_STRING PROPERTY KATANA_NSIS_PROBLEMS "  - ${text}
")
endfunction()
function(katana_require what literal)
    string(FIND "${_text}" "${literal}" _at)
    if(_at EQUAL -1)
        katana_problem("${what}: the script has no  ${literal}")
    endif()
endfunction()

# 1. No elevation: exactly one RequestExecutionLevel, and it is user.
string(REGEX MATCHALL "(^|\n)[ \t]*RequestExecutionLevel[^\n]*" _levels "${_text}")
list(LENGTH _levels _level_count)
if(NOT _level_count EQUAL 1)
    katana_problem("the script has ${_level_count} RequestExecutionLevel instructions, and must have exactly one, user")
elseif(NOT _levels MATCHES "RequestExecutionLevel[ \t]+user[ \t]*$")
    string(STRIP "${_levels}" _level)
    katana_problem("${_level}: the installer must run as the user (RequestExecutionLevel user), with no UAC prompt")
endif()

# 2. Nothing machine-wide: no hive but HKCU, no "all users" shell context, no Program Files.
string(REGEX MATCHALL "(^|[^A-Za-z0-9_])(HK[A-Z]+|HKEY_[A-Z_]+|SHCTX|SHELL_CONTEXT)" _hives "${_text}")
set(_hive_count 0)
set(_foreign_hives "")
foreach(_hive IN LISTS _hives)
    string(REGEX REPLACE "^[^A-Za-z0-9_]+" "" _name "${_hive}")
    math(EXPR _hive_count "${_hive_count} + 1")
    if(NOT _name STREQUAL "HKCU" AND NOT _name STREQUAL "HKEY_CURRENT_USER")
        list(APPEND _foreign_hives "${_name}")
    endif()
endforeach()
list(REMOVE_DUPLICATES _foreign_hives)
foreach(_name IN LISTS _foreign_hives)
    katana_problem("${_name}: the installer writes machine-wide; the registry is HKCU only")
endforeach()
if(_hive_count EQUAL 0)
    katana_problem("the script names no registry hive at all: the checks of its keys would pass with nothing to check")
endif()
if(_text MATCHES "SetShellVarContext[ \t]+all")
    katana_problem("SetShellVarContext all: the Start menu and shortcuts would be the all-users ones")
endif()
if(_text MATCHES "[$](PROGRAMFILES|PROGRAMFILES32|PROGRAMFILES64|COMMONFILES|COMMONFILES32|COMMONFILES64|WINDIR|SYSDIR)")
    string(REGEX MATCH "[$](PROGRAMFILES|PROGRAMFILES32|PROGRAMFILES64|COMMONFILES|COMMONFILES32|COMMONFILES64|WINDIR|SYSDIR)" _folder "${_text}")
    katana_problem("${_folder}: a folder only an administrator can write to")
endif()

# 3. Where it installs, and what it records.
katana_require("default folder" [=[InstallDir "$LOCALAPPDATA\Programs\Katana"]=])
katana_require("the product key" [=[!define KATANA_PRODUCT_KEY "Software\Jarada\Katana"]=])
katana_require("the uninstall key" [=[!define KATANA_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Katana"]=])
katana_require("the install folder, for the updater" [=[WriteRegStr HKCU "${KATANA_PRODUCT_KEY}" "" "$INSTDIR"]=])
foreach(_value IN ITEMS DisplayName DisplayVersion UninstallString InstallLocation DisplayIcon)
    katana_require("the entry in Settings > Apps" "WriteRegStr HKCU \"\${KATANA_UNINSTALL_KEY}\" \"${_value}\"")
endforeach()
katana_require("the publisher" [=[WriteRegStr HKCU "${KATANA_UNINSTALL_KEY}" "Publisher" "Jarada"]=])
katana_require("no Modify button" [=[WriteRegDWORD HKCU "${KATANA_UNINSTALL_KEY}" "NoModify" 1]=])
katana_require("no Repair button" [=[WriteRegDWORD HKCU "${KATANA_UNINSTALL_KEY}" "NoRepair" 1]=])
katana_require("the uninstaller" [=[WriteUninstaller "$INSTDIR\Uninstall.exe"]=])
if(EXPECT_VERSION)
    katana_require("the version in Settings > Apps"
        "WriteRegStr HKCU \"\${KATANA_UNINSTALL_KEY}\" \"DisplayVersion\" \"${EXPECT_VERSION}\"")
endif()

# 4. Silent: every MessageBox has a default answer (/SD), or a silent install waits for ever.
string(REGEX MATCHALL "(^|\n)[ \t]*MessageBox[^\n]*" _boxes "${_text}")
list(LENGTH _boxes _box_count)
foreach(_box IN LISTS _boxes)
    if(NOT _box MATCHES "/SD[ \t]+ID[A-Z]+")
        string(STRIP "${_box}" _box)
        katana_problem("${_box}: no /SD, so a silent install (/S) would wait for an answer")
    endif()
endforeach()

# 5. Unicode: the default folder is under the user's profile, whose name can hold any character.
katana_require("a Unicode installer" "Unicode true")

# 6. The uninstaller.
string(FIND "${_text}" [=[Section "Uninstall"]=] _uninstall_start)
if(_uninstall_start EQUAL -1)
    katana_problem("the script has no Uninstall section")
    set(_uninstall "")
else()
    string(SUBSTRING "${_text}" ${_uninstall_start} -1 _uninstall)
    string(FIND "${_uninstall}" "SectionEnd" _uninstall_end)
    if(_uninstall_end EQUAL -1)
        katana_problem("the Uninstall section does not end")
    else()
        string(SUBSTRING "${_uninstall}" 0 ${_uninstall_end} _uninstall)
    endif()
endif()
function(katana_require_in_uninstaller what literal)
    string(FIND "${_uninstall}" "${literal}" _at)
    if(_at EQUAL -1)
        katana_problem("the uninstaller does not ${what}: it has no  ${literal}")
    endif()
endfunction()
katana_require_in_uninstaller("remove the updater's working folder" [=[RMDir /r "$INSTDIR\.update"]=])
katana_require_in_uninstaller("remove the files the installed manifest lists" [=[Call un.DeleteManifestFiles]=])
katana_require_in_uninstaller("remove the updater's records" [=[Delete "$INSTDIR\share\katana\installed.json"]=])
katana_require_in_uninstaller("remove the updater's records" [=[Delete "$INSTDIR\share\katana\installed.manifest.json"]=])
katana_require_in_uninstaller("remove the updater's records" [=[Delete "$INSTDIR\share\katana\installed.manifest.json.sig"]=])
katana_require_in_uninstaller("remove its own entry in Settings > Apps" [=[DeleteRegKey HKCU "${KATANA_UNINSTALL_KEY}"]=])
katana_require_in_uninstaller("remove the folder record" [=[DeleteRegKey HKCU "${KATANA_PRODUCT_KEY}"]=])
katana_require_in_uninstaller("remove the folder only if it is empty" [=[RMDir "$INSTDIR"]=])
katana_require_in_uninstaller("remove itself" [=[Delete "$INSTDIR\Uninstall.exe"]=])
katana_require("the installed manifest, read by the uninstaller" [=["$INSTDIR\share\katana\installed.manifest.json" r]=])
# The path rules of the manifest reader (paths.hpp, in the part that keeps a delete inside the folder).
foreach(_rule IN ITEMS [=[StrCmp $R1 "bin\" listed_folder_ok]=] [=[StrCmp $R1 "share\" listed_folder_ok listed_done]=]
        [=[Push "\..\"]=] [=[Push "\.\"]=] [=[Push "\\"]=])
    katana_require("the manifest reader's path rules" "${_rule}")
endforeach()

# Recursive removal is for the updater's own folders and nothing else: the install folder
# may hold what the person put there.
string(REGEX MATCHALL "(^|\n)[ \t]*RMDir[ \t]+/r[^\n]*" _recursive "${_text}")
foreach(_removal IN LISTS _recursive)
    string(STRIP "${_removal}" _removal)
    if(NOT _removal STREQUAL [=[RMDir /r "$INSTDIR\.update"]=]
       AND NOT _removal STREQUAL [=[RMDir /r "${KATANA_UPDATE_STATE}\$1"]=])
        katana_problem("${_removal}: recursive removal of anything but the updater's own folders (.update and its state) would take what the person put in the install folder with it")
    endif()
endforeach()
string(REGEX MATCHALL "(^|\n)[ \t]*Delete[^\n]*[$]INSTDIR[^\n]*[*?][^\n]*" _wildcards "${_text}")
foreach(_wildcard IN LISTS _wildcards)
    string(STRIP "${_wildcard}" _wildcard)
    katana_problem("${_wildcard}: a wildcard delete in the install folder")
endforeach()

# What the manifest lists goes before the folders are removed, or an update-added file keeps
# its folder alive: the first folder removal of CPack's list must come after the call.
string(FIND "${_uninstall}" [=[Call un.DeleteManifestFiles]=] _manifest_call)
string(REGEX MATCH "\n[ \t]*RMDir \"[$]INSTDIR\\\\[^\n]*" _first_folder "${_uninstall}")
if(NOT _first_folder STREQUAL "" AND NOT _manifest_call EQUAL -1)
    string(FIND "${_uninstall}" "${_first_folder}" _first_folder_at)
    if(_first_folder_at LESS _manifest_call)
        string(STRIP "${_first_folder}" _first_folder)
        katana_problem("${_first_folder} comes before Call un.DeleteManifestFiles: a folder an update filled would be left behind")
    endif()
endif()

# What the build said goes in (CPack's own lists, from the real staged tree).
foreach(_file IN LISTS EXPECT_FILES)
    string(REPLACE "/" "\\" _windows "${_file}")
    katana_require_in_uninstaller("delete ${_file}, which the installer puts down" "Delete \"$INSTDIR\\${_windows}\"")
endforeach()

get_property(_problems GLOBAL PROPERTY KATANA_NSIS_PROBLEMS)
if(_problems)
    message(FATAL_ERROR "The installer script (${_what}) is not a per-user installer:\n${_problems}")
endif()
message(STATUS "Installer script (${_what}): per user. RequestExecutionLevel user; ${_hive_count} registry hive "
    "references, all HKCU; ${_box_count} MessageBox, all with /SD; the uninstaller removes .update and the "
    "files the installed manifest lists, and no folder recursively but the updater's.")
