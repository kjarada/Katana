# Holds the text of Katana's Windows installer script to what makes the installer PER USER
# (decision D1, docs/release.md "The Windows installer", cmake/NSIS.template.in), without
# running it:
#
#   cmake -DTEMPLATE=<cmake/NSIS.template.in> [-DREGISTRY_HEADER=<include/katana/update/registry.hpp>]
#         [-DMUTATE=<name>] -P check_nsis_script.cmake
#   cmake -DSCRIPT=<project.nsi> [-DEXPECT_VERSION=0.2.0] [-DEXPECT_FILES=bin/katana.exe;...]
#         [-DREGISTRY_HEADER=...] [-DMUTATE=<name>] -P check_nsis_script.cmake
#
# TEMPLATE is configured here with sample values for the variables CPack supplies (what
# KatanaPackaging.cmake sets, and what the generator derives from a staged tree), so it runs
# on any host with cmake. SCRIPT is what CPack really generated, from the real configuration
# (build_nsis_package.cmake). The rules are the same for both:
#
#   * no administrator rights asked for, and nothing machine-wide written: no HKLM or any
#     other hive but HKCU, no "all users" shell context, no Program Files or system folder;
#   * the folder is %LOCALAPPDATA%\Programs\Katana, and the registry holds what the updater
#     reads and Settings > Apps shows (include/katana/update/registry.hpp); a copy that is
#     already installed is offered as the folder again; no folder at all, or one that cannot
#     be written to, is refused before anything is put down, by a silent run too;
#   * installed over a copy of Katana, it clears the updater's record of that copy's version
#     and the working folder of an update that was cut short, and a file that is in use stops
#     it: none is skipped; an entry in Settings > Apps whose uninstaller is gone does not
#     stop it;
#   * the uninstaller removes the updater's working folder .update and every file the
#     installed manifest lists, reading that list before it deletes it, and never removes the
#     install folder recursively, so what a person put there stays; it stops when its folder
#     is not known; outside the folder it clears the updater's state folder and no other,
#     keeps the person's choices about updating, and removes the helper's RunOnce value under
#     the name the helper gives it (REGISTRY_HEADER, when given, is where that name is read);
#   * a silent run never waits for an answer; the text is Unicode.
#
# That list is all this check holds the text to. What only a run can show (the exit code of a
# refused folder, the size in Settings > Apps, the shortcuts) is not in it.
#
# The check reads text, and NSIS does not compile all of its text. Two things take an
# instruction away from NSIS, and each is dealt with before a rule looks at anything:
#
#   * comments. Whole-line comments are removed: the template explains in them what it does
#     not do, and a word in a comment is not an instruction. NSIS continues a line that ends
#     in a backslash onto the next one, a comment as well as a command (its manual, "Long
#     commands"), so the line after such a comment is part of the comment to NSIS and would
#     be an instruction here; a line that has a comment character in it and ends in a
#     backslash is therefore refused, not interpreted. So is a block comment (a slash and a
#     star), anywhere: the template has none, and none is read here;
#   * conditional compilation. The template has two kinds, and no other is accepted. The test
#     for MSYS2's NSIS (!if /FileExists) cannot be evaluated here, so it may hold the Target
#     line and nothing else. A comparison of two literal strings, which is what a value CPack
#     filled in makes, is evaluated as NSIS evaluates it (== and !=, without regard to case),
#     and the text inside a false one is removed, as NSIS leaves it out. One condition inside
#     another, an !else, an !ifdef and every other form are refused.
#
# What it cannot see is an instruction that is compiled and never reached: one in a function
# that nothing calls, or behind a jump. The rules about the installer's and the uninstaller's
# sections look inside the section, so an instruction moved out of one is missed there; beyond
# that, this is for whoever reads a change to the template.
# Instruction names are read in the case the template writes them; NSIS itself takes any.
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

# ---- the mutations, for the tests of this check ---------------------------------------------
# Every occurrence of `old`.
function(katana_mutate old new)
    string(FIND "${_text}" "${old}" _at)
    if(_at EQUAL -1)
        message(FATAL_ERROR "mutation ${MUTATE} does not apply: the text has no  ${old}")
    endif()
    string(REPLACE "${old}" "${new}" _changed "${_text}")
    set(_text "${_changed}" PARENT_SCOPE)
endfunction()
# The first occurrence only. The installer's section comes before the uninstaller's, so of an
# instruction both have, this changes the installer's.
function(katana_mutate_first old new)
    string(FIND "${_text}" "${old}" _at)
    if(_at EQUAL -1)
        message(FATAL_ERROR "mutation ${MUTATE} does not apply: the text has no  ${old}")
    endif()
    string(LENGTH "${old}" _length)
    math(EXPR _rest "${_at} + ${_length}")
    string(SUBSTRING "${_text}" 0 ${_at} _before)
    string(SUBSTRING "${_text}" ${_rest} -1 _after)
    set(_text "${_before}${new}${_after}" PARENT_SCOPE)
endfunction()

# ---- the text, as instructions ----------------------------------------------------------
string(REPLACE "\r\n" "\n" _text "${_text}")
# One mutation is of the text as it is written, comments and all, because the comment is what
# it breaks: the folder's removal, behind a comment that takes it.
if(MUTATE STREQUAL "comment_takes_the_next_line")
    katana_mutate([=[  RMDir /r "$INSTDIR\.update"]=]
                  "  ; the updater's working folder \\\n  RMDir /r \"$INSTDIR\\.update\"")
endif()
# A line with a comment character in it that ends in a backslash (see the top of this file):
# kept here, reported with the other problems below.
set(_continued_comment "")
if("${_text}\n" MATCHES "(^|\n)([^\n]*[;#][^\n]*\\\\)[ \t]*\n")
    set(_continued_comment "${CMAKE_MATCH_2}")
endif()
# Whole-line comments out...
string(REGEX REPLACE "(^|\n)[ \t]*[;#][^\n]*" "\\1" _text "${_text}")
# ...a backslash that ends a line joins the next one to it, as NSIS reads it...
string(REGEX REPLACE "\\\\\n[ \t]*" " " _text "${_text}")
# ...and a semicolon is only text, not a list separator.
string(REPLACE ";" "<semicolon>" _text "${_text}")

if(MUTATE STREQUAL "comment_takes_the_next_line")
    # Applied above, to the text with its comments.
elseif(MUTATE STREQUAL "administrator")
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
elseif(MUTATE STREQUAL "stale_record_survives_a_reinstall")
    katana_mutate_first([=[Delete "$INSTDIR\share\katana\installed.json"]=] "")
elseif(MUTATE STREQUAL "stale_journal_survives_a_reinstall")
    katana_mutate_first([=[RMDir /r "$INSTDIR\.update"]=] "")
elseif(MUTATE STREQUAL "state_folder_is_another_folder")
    katana_mutate([=[!define KATANA_UPDATE_STATE "$LOCALAPPDATA\Katana\update"]=]
                  [=[!define KATANA_UPDATE_STATE "$LOCALAPPDATA\Katana"]=])
elseif(MUTATE STREQUAL "update_choices_removed")
    katana_mutate([=[StrCmp $1 "state.json" state_skip]=] "")
elseif(MUTATE STREQUAL "run_once_under_another_name")
    katana_mutate([=["KatanaUpdateRecover"]=] [=["KatanaRecover"]=])
elseif(MUTATE STREQUAL "file_in_use_skipped")
    katana_mutate([=[AllowSkipFiles off]=] [=[AllowSkipFiles on]=])
elseif(MUTATE STREQUAL "block_comment_hides_an_instruction")
    katana_mutate([=[RMDir /r "$INSTDIR\.update"]=] "/*\n  RMDir /r \"$INSTDIR\\.update\"\n  */")
elseif(MUTATE STREQUAL "unknown_condition_hides_an_instruction")
    katana_mutate([=[RMDir /r "$INSTDIR\.update"]=] "!if 0\n  RMDir /r \"$INSTDIR\\.update\"\n  !endif")
elseif(MUTATE STREQUAL "false_condition_hides_an_instruction")
    # The form the template itself uses, false: nothing refuses it, so the call must not be
    # found, and the rule that asks for the call must say so.
    katana_mutate([=[Call un.DeleteManifestFiles]=] "!if \"\" != \"\"\n  Call un.DeleteManifestFiles\n  !endif")
elseif(MUTATE STREQUAL "condition_inside_a_condition")
    katana_mutate([=[Call un.DeleteManifestFiles]=]
                  "!if \"a\" == \"a\"\n  !if \"\" != \"\"\n  !endif\n  Call un.DeleteManifestFiles\n  !endif")
elseif(MUTATE STREQUAL "manifest_deleted_before_it_is_read")
    # Moved, not copied: the delete leaves its place below the call and goes above it.
    katana_mutate("  Delete \"$INSTDIR\\share\\katana\\installed.manifest.json\"\n" "")
    katana_mutate([=[Call un.DeleteManifestFiles]=]
                  "Delete \"$INSTDIR\\share\\katana\\installed.manifest.json\"\n  Call un.DeleteManifestFiles")
elseif(MUTATE STREQUAL "no_folder_at_all_is_installed_into")
    katana_mutate([=[StrCmp $INSTDIR "" folder_not_writable]=] "")
elseif(MUTATE STREQUAL "silent_install_does_not_try_the_folder")
    katana_mutate([=[Call CheckInstallFolder]=] "")
elseif(MUTATE STREQUAL "installed_copy_is_not_offered_again")
    katana_mutate([=[InstallDirRegKey HKCU "${KATANA_PRODUCT_KEY}" ""]=] "")
elseif(MUTATE STREQUAL "uninstaller_runs_without_a_folder")
    katana_mutate([=[Abort "Katana's folder is not known, so nothing was removed."]=] "")
elseif(MUTATE STREQUAL "uninstaller_that_is_gone_stops_the_install")
    katana_mutate([=[IfFileExists "$0" 0 onInitDone]=] "")
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

# 0. What NSIS does not compile is not read as an instruction (the top of this file says what
# that covers and what it does not).
# 0a. Comments.
if(NOT _continued_comment STREQUAL "")
    string(STRIP "${_continued_comment}" _continued_comment)
    katana_problem("a line with a comment character in it ends in a backslash: NSIS continues a comment onto the next line, so the instruction there may be none, and this check would still read it as one. The line:  ${_continued_comment}")
endif()
if(_text MATCHES "/[*]")
    katana_problem("/*: a block comment. NSIS skips everything up to the end of it and this check does not, so an instruction inside one would be read here as an instruction that NSIS never sees. Comments are whole lines")
endif()

# 0b. Conditional compilation: the two kinds the template has, and never one inside another.
string(REGEX MATCHALL "(^|\n)[ \t]*!(if|else|endif)[^\n]*" _conditions "${_text}")
set(_condition_open FALSE)
foreach(_condition IN LISTS _conditions)
    string(STRIP "${_condition}" _condition)
    if(_condition STREQUAL "!endif")
        if(NOT _condition_open)
            katana_problem("!endif: it ends no condition")
        endif()
        set(_condition_open FALSE)
    elseif(_condition_open)
        katana_problem("${_condition}: one condition inside another, or an !else. This check reads a condition up to its !endif and no further, so it would take what follows for text that NSIS compiles")
    elseif(_condition STREQUAL [=[!if /FileExists "${NSISDIR}\Plugins\unicode\nsDialogs.dll"]=]
           OR _condition MATCHES "^!if \"[^\"$]*\" (==|!=) \"[^\"$]*\"$")
        set(_condition_open TRUE)
    else()
        katana_problem("${_condition}: a condition this check cannot evaluate, so it cannot tell whether NSIS compiles what is inside it. The template's own are the test for MSYS2's NSIS and comparisons of two literal strings")
        if(_condition MATCHES "^!if")
            set(_condition_open TRUE)
        endif()
    endif()
endforeach()
if(_condition_open)
    katana_problem("a condition does not end: the script has an !if with no !endif after it")
endif()
# The one that cannot be evaluated here holds the Target line and nothing else.
string(REGEX REPLACE
    "!if /FileExists \"[$][{]NSISDIR[}]\\\\Plugins\\\\unicode\\\\nsDialogs[.]dll\"[ \t]*\n[ \t]*Target amd64-unicode[ \t]*\n[ \t]*!endif"
    "" _without_the_target "${_text}")
if(_without_the_target MATCHES "!if /FileExists")
    katana_problem("!if /FileExists: this check cannot tell whether the file is there, so that condition may hold the line  Target amd64-unicode  and nothing else")
endif()
# A comparison of two literal strings is evaluated as NSIS evaluates it, without regard to
# case, and what is inside a false one goes: NSIS never sees it, so no rule below may.
set(_compiled "")
set(_rest "${_text}")
while(TRUE)
    string(REGEX MATCH "(^|\n)[ \t]*!if \"([^\"$\n]*)\" (==|!=) \"([^\"$\n]*)\"[ \t]*\n" _comparison "${_rest}")
    if(_comparison STREQUAL "")
        break()
    endif()
    string(TOUPPER "${CMAKE_MATCH_2}" _left)
    set(_operator "${CMAKE_MATCH_3}")
    string(TOUPPER "${CMAKE_MATCH_4}" _right)
    string(FIND "${_rest}" "${_comparison}" _at)
    string(LENGTH "${_comparison}" _length)
    math(EXPR _body "${_at} + ${_length}")
    string(SUBSTRING "${_rest}" 0 ${_body} _head)
    string(SUBSTRING "${_rest}" ${_body} -1 _rest)
    string(APPEND _compiled "${_head}")
    string(COMPARE EQUAL "${_left}" "${_right}" _same)
    if((_operator STREQUAL "==" AND NOT _same) OR (_operator STREQUAL "!=" AND _same))
        string(FIND "${_rest}" "!endif" _end)
        if(_end EQUAL -1)
            set(_rest "")
        else()
            string(SUBSTRING "${_rest}" ${_end} -1 _rest)
        endif()
    endif()
endwhile()
set(_text "${_compiled}${_rest}")

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

# 3a. Installed over a copy of Katana that was not uninstalled first, the installer clears what
# the updater left that would be wrong about the new files: its record of the version (the
# engine reads installed.json as the installed version, include/katana/update/install.hpp) and
# the working folder of an update that was cut short (its recovery would put old files back).
# Only in a folder that holds a packaged Katana, which install.json says.
string(FIND "${_text}" [=[Section "-Core installation"]=] _install_start)
if(_install_start EQUAL -1)
    katana_problem("the script has no installer section  Section \"-Core installation\"")
    set(_install "")
else()
    string(SUBSTRING "${_text}" ${_install_start} -1 _install)
    string(FIND "${_install}" "SectionEnd" _install_end)
    if(_install_end EQUAL -1)
        katana_problem("the installer section does not end")
    else()
        string(SUBSTRING "${_install}" 0 ${_install_end} _install)
    endif()
endif()
function(katana_require_in_installer what literal)
    string(FIND "${_install}" "${literal}" _at)
    if(_at EQUAL -1)
        katana_problem("the installer does not ${what}: its section has no  ${literal}")
    endif()
endfunction()
katana_require_in_installer("clear the updater's record of the version of a copy it installs over"
    [=[Delete "$INSTDIR\share\katana\installed.json"]=])
katana_require_in_installer("clear the working folder of an update that was cut short in a copy it installs over"
    [=[RMDir /r "$INSTDIR\.update"]=])
katana_require_in_installer("keep that clearing to a folder that holds a packaged Katana"
    [=[IfFileExists "$INSTDIR\share\katana\install.json" 0 previous_copy_done]=])

# 3b. The folder. A copy that is already installed is offered as the folder again, so that a
# reinstall goes where the first install went. No folder at all, and a folder that cannot be
# written to, are refused before anything is cleared or put down: by the folder page as it is
# left, and by the installer's section as its first instruction, which is where a silent run,
# which has no page, meets the test. Without a folder every path would start at the top of a
# drive.
katana_require("a copy that is already installed is offered as the folder again"
    [=[InstallDirRegKey HKCU "${KATANA_PRODUCT_KEY}" ""]=])
katana_require("no folder at all is refused, like one that cannot be written to"
    [=[StrCmp $INSTDIR "" folder_not_writable]=])
katana_require("the folder is tried for writing"
    [=[FileOpen $0 "$INSTDIR\katana-write-test.tmp" w]=])
katana_require("the folder page refuses a folder that fails that test"
    [=[!define MUI_PAGE_CUSTOMFUNCTION_LEAVE CheckInstallFolder]=])
if(NOT _install STREQUAL ""
   AND NOT _install MATCHES "^Section \"-Core installation\"[ \t\n]*Call CheckInstallFolder[ \t]*\n")
    katana_problem("the installer's section does not begin with  Call CheckInstallFolder: a silent run has no folder page, so it would clear and write a folder that nothing has tried")
endif()

# 3c. A copy that is already installed is uninstalled first, by running the uninstaller that
# its entry in Settings > Apps names. An entry can outlive its uninstaller (the folder deleted
# by hand, which is easy in one's own profile), and running a program that is not there
# fails. The install must not stop on that, so the program is looked for first and, when it is
# gone, the install goes ahead. (Where the installer does not read the entry at all, because
# uninstalling first is switched off, there is nothing to hold.)
string(FIND "${_text}" [=[ReadRegStr $0 HKCU "${KATANA_UNINSTALL_KEY}" "UninstallString"]=] _entry_read)
if(NOT _entry_read EQUAL -1)
    string(SUBSTRING "${_text}" ${_entry_read} -1 _after_the_read)
    string(FIND "${_after_the_read}" [=[IfFileExists "$0" 0 onInitDone]=] _looked_for)
    string(FIND "${_after_the_read}" "ExecWait" _run)
    if(_looked_for EQUAL -1 OR (NOT _run EQUAL -1 AND _run LESS _looked_for))
        katana_problem("the installer runs the uninstaller that the entry in Settings > Apps names without looking for it first (IfFileExists \"$0\" 0 onInitDone, before ExecWait): an entry whose uninstaller is gone, because the folder was deleted by hand, would stop every install at \"Uninstall failed.\"")
    endif()
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

# 4a. A file that cannot be written (a program of Katana's that is running) stops the install.
# NSIS's default lets it be skipped, which a silent run would do without a word, leaving the
# old program among the new libraries.
katana_require("a file in use stops the install and is not skipped" "AllowSkipFiles off")

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
# It stops, before it removes anything, when its folder is not known: every path below starts
# with the folder, and without one would start at the top of a drive.
if(NOT _uninstall STREQUAL ""
   AND NOT _uninstall MATCHES "^Section \"Uninstall\"[ \t\n]*StrCmp [$]INSTDIR \"\" 0 uninstall_folder_known[ \t]*\n[ \t]*Abort [^\n]*\n[ \t]*uninstall_folder_known:")
    katana_problem("the uninstaller's section does not begin by stopping when its folder is not known (StrCmp $INSTDIR \"\" 0 uninstall_folder_known, an Abort, uninstall_folder_known:): without a folder its paths would start at the top of a drive")
endif()
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
# Outside the install folder: the updater's state folder (include/katana/update/state.hpp) and
# no other is cleared - the define is the root of the one recursive removal there, so it is
# held to the letter - and state.json, the person's choices about updating, stays in it.
katana_require("the updater's state folder, which is the only one outside the install folder that the uninstaller clears"
    [=[!define KATANA_UPDATE_STATE "$LOCALAPPDATA\Katana\update"]=])
katana_require_in_uninstaller("clear what the updater staged outside the install folder" [=[Call un.RemoveUpdateState]=])
katana_require("the person's choices about updating, which the uninstaller keeps" [=[StrCmp $1 "state.json" state_skip]=])
# The command the helper registers to run at the next logon (registry.hpp): removed under the
# name the helper gives it, read from that header when it is given, so that the two cannot
# come to differ unnoticed.
set(_run_once_name "")
if(REGISTRY_HEADER)
    file(READ "${REGISTRY_HEADER}" _registry_header)
    if(_registry_header MATCHES "kRunOnceName = \"([A-Za-z0-9_]+)\"")
        set(_run_once_name "${CMAKE_MATCH_1}\"")
    else()
        katana_problem("${REGISTRY_HEADER} has no  kRunOnceName = \"<name>\": the name of the helper's RunOnce value cannot be read from it")
    endif()
endif()
katana_require_in_uninstaller("remove the helper's RunOnce value under the name the helper gives it"
    "DeleteRegValue HKCU \"Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce\" \"${_run_once_name}")
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
# ...and before the list itself is deleted: the call reads the file, and with the file gone it
# finds nothing to read, deletes nothing and says nothing, so every file an update added would
# be left behind.
string(FIND "${_uninstall}" [=[Delete "$INSTDIR\share\katana\installed.manifest.json"]=] _manifest_delete)
if(NOT _manifest_delete EQUAL -1 AND NOT _manifest_call EQUAL -1 AND _manifest_delete LESS _manifest_call)
    katana_problem("Delete \"$INSTDIR\\share\\katana\\installed.manifest.json\" comes before Call un.DeleteManifestFiles: the list would be gone before it is read, and every file an update added would be left behind")
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
    "files the installed manifest lists, and no folder recursively but the updater's; the installer clears "
    "the updater's record and working folder of a copy it installs over.")
