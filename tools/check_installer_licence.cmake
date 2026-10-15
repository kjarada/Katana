# Checks that the Windows installer shows the licence, without building the installer.
#
#   cmake -DKATANA_ROOT=<source dir> -DKATANA_BINARY=<build dir> -P tools/check_installer_licence.cmake
#
# The installer (NSIS, cmake/NSIS.template.in) once showed no licence page: CPack
# builds one only when CPACK_RESOURCE_FILE_LICENSE names a file, and nothing did,
# so the template's `@CPACK_NSIS_LICENSE_PAGE@` was empty and an installer went
# from Welcome to the folder page with the GNU General Public License nowhere in
# it. What holds it now, each failure named:
#
# 1. CPackConfig.cmake, the file CPack reads, sets CPACK_RESOURCE_FILE_LICENSE to
#    a file that exists.
# 2. That file is what a person must be able to read: the licence note, the whole
#    of LICENSE (the GNU General Public License) and the whole of
#    ADDITIONAL_TERMS.md, as the repository has them - so a licence edited
#    without the installer's page being made again is caught - with Windows line
#    ends on every line (a page's text box breaks lines on CRLF, and LICENSE is LF
#    in the repository) and in ASCII only (a Unicode installer reads a file with
#    no byte order mark as it is, so a character outside ASCII would be shown as
#    others).
# 3. The template puts the licence page before the folder page, after Welcome, and
#    says the licence need not be accepted - the GPL asks for no acceptance, and
#    a button that read "I Agree" would claim it did.
# 4. The install rules (the top-level cmake_install.cmake, which cmake --install,
#    the bundle and CPack all run) put the three notices that are not LICENSE
#    under share/katana/licences, where the page tells the reader to look for
#    the libraries' licences.
#
# A CPackConfig.cmake with no NSIS generator is not an error here: the test that
# runs this is registered only where NSIS is found, and a build without it has no
# installer to show a licence in.

if(NOT KATANA_ROOT OR NOT KATANA_BINARY)
    message(FATAL_ERROR "Pass -DKATANA_ROOT=<source dir> -DKATANA_BINARY=<build dir>")
endif()

set(_problems "")
function(_problem text)
    set(_problems "${_problems}\n  ${text}" PARENT_SCOPE)
endfunction()

# The text of a file with line ends made LF, for comparing what the repository has
# with what the page holds.
function(_lf_text file out)
    file(READ "${file}" _text)
    string(REPLACE "\r\n" "\n" _text "${_text}")
    set(${out} "${_text}" PARENT_SCOPE)
endfunction()

# ---- the template (checked wherever the sources are) -------------------------------------
set(_template "${KATANA_ROOT}/cmake/NSIS.template.in")
if(NOT EXISTS "${_template}")
    _problem("cmake/NSIS.template.in is missing")
else()
    file(READ "${_template}" _nsis)
    string(REPLACE "\r\n" "\n" _nsis "${_nsis}")
    string(FIND "${_nsis}" "!insertmacro MUI_PAGE_WELCOME" _welcome)
    string(FIND "${_nsis}" "@CPACK_NSIS_LICENSE_PAGE@" _licence)
    string(FIND "${_nsis}" "!insertmacro MUI_PAGE_DIRECTORY" _directory)
    if(_welcome EQUAL -1 OR _licence EQUAL -1 OR _directory EQUAL -1
       OR NOT _welcome LESS _licence OR NOT _licence LESS _directory)
        _problem("NSIS.template.in must show the licence page (@CPACK_NSIS_LICENSE_PAGE@) after Welcome and before the folder page")
    endif()
    string(FIND "${_nsis}" "MUI_LICENSEPAGE_BUTTON" _button)
    string(FIND "${_nsis}" "MUI_LICENSEPAGE_TEXT_BOTTOM" _bottom)
    if(_button EQUAL -1 OR _bottom EQUAL -1 OR NOT _button LESS _licence OR NOT _bottom LESS _licence)
        _problem("NSIS.template.in must set the licence page's button and bottom text before the page: the GPL asks for no acceptance, so it is not 'I Agree'")
    endif()
    if(_nsis MATCHES "MUI_LICENSEPAGE_BUTTON \"I Agree")
        _problem("the licence page's button says I Agree; the GPL asks for no acceptance")
    endif()
endif()

# What follows needs the installer to be configured: CPackConfig.cmake lists NSIS as a
# generator. A build without NSIS (MSYS2: `pacman -S mingw-w64-ucrt-x86_64-nsis`) has no
# installer to show a licence in; the template above is checked on every machine.
set(_cpack "${KATANA_BINARY}/CPackConfig.cmake")
if(NOT EXISTS "${_cpack}")
    message(FATAL_ERROR "CPackConfig.cmake was not found in ${KATANA_BINARY}: the configure has not run here")
endif()
file(STRINGS "${_cpack}" _generators REGEX "^set\\(CPACK_GENERATOR ")
if(NOT _generators MATCHES "NSIS")
    if(_problems)
        message(FATAL_ERROR "the installer's licence page is not as it must be:${_problems}")
    endif()
    message(STATUS "installer licence: no NSIS generator is configured here, so there is no installer; "
                   "the template shows the licence page")
    return()
endif()

# ---- 1. CPack names a licence file ------------------------------------------------------
file(STRINGS "${_cpack}" _licence_lines REGEX "^set\\(CPACK_RESOURCE_FILE_LICENSE ")
if(NOT _licence_lines)
    _problem("CPackConfig.cmake does not set CPACK_RESOURCE_FILE_LICENSE: the installer has no licence page")
else()
    # CPack writes the value in quotes.
    string(REGEX REPLACE "^set\\(CPACK_RESOURCE_FILE_LICENSE \"(.*)\"\\)$" "\\1" _page "${_licence_lines}")
    if(NOT EXISTS "${_page}")
        _problem("CPACK_RESOURCE_FILE_LICENSE names ${_page}, which does not exist")
    else()
        # ---- 2. what the page holds ---------------------------------------------------
        # Line ends are judged from the file's size, not from its text: file(READ) is a text-mode
        # read, which on Windows turns each CRLF into LF before the check could see it. A file
        # whose every line ends in CR LF is as many bytes as it has characters (read, LF only)
        # plus one for each LF; a lone LF makes it fewer and a CR CR LF more.
        file(SIZE "${_page}" _bytes)
        file(READ "${_page}" _raw)
        if(CMAKE_HOST_WIN32)
            string(LENGTH "${_raw}" _characters)
            string(REGEX MATCHALL "\n" _line_ends "${_raw}")
            list(LENGTH _line_ends _line_end_count)
            math(EXPR _expected_bytes "${_characters} + ${_line_end_count}")
            if(NOT _bytes EQUAL _expected_bytes)
                _problem("the licence page does not end every line in CRLF alone (${_bytes} bytes, ${_expected_bytes} if it did): its text box would break lines in the wrong places")
            endif()
        elseif(_raw MATCHES "[^\r]\n" OR _raw MATCHES "^\n")
            _problem("the licence page has a line that does not end in CRLF: its text box would not break it")
        endif()
        string(REPLACE "\r\n" "\n" _text "${_raw}")
        if(_text MATCHES "[^\n\t -~]")
            _problem("the licence page has a character outside ASCII, which a Unicode installer would show as others")
        endif()
        string(FIND "${_text}" "Katana, copyright (c) 2026 Jarada, https://github.com/kjarada/Katana" _at)
        if(NOT _at EQUAL 0)
            _problem("the licence page does not begin with the line the additional terms ask for: 'Katana, copyright (c) 2026 Jarada, https://github.com/kjarada/Katana'")
        endif()
        foreach(_source LICENSE ADDITIONAL_TERMS.md)
            if(NOT EXISTS "${KATANA_ROOT}/${_source}")
                _problem("${_source} is missing from the repository")
                continue()
            endif()
            _lf_text("${KATANA_ROOT}/${_source}" _whole)
            string(FIND "${_text}" "${_whole}" _found)
            if(_found EQUAL -1)
                _problem("the licence page does not hold all of ${_source} as the repository has it")
            endif()
        endforeach()
        foreach(_phrase "GNU GENERAL PUBLIC LICENSE" "Version 3, 29 June 2007" "END OF TERMS AND CONDITIONS"
                        "Additional terms" "do not have to accept")
            string(FIND "${_text}" "${_phrase}" _found)
            if(_found EQUAL -1)
                _problem("the licence page does not say '${_phrase}'")
            endif()
        endforeach()
    endif()
endif()

# ---- 4. the notices are installed ----------------------------------------------------------
set(_install "${KATANA_BINARY}/cmake_install.cmake")
if(NOT EXISTS "${_install}")
    _problem("${_install} was not found")
else()
    file(READ "${_install}" _rules)
    foreach(_notice ADDITIONAL_TERMS.md LICENSING.md THIRD_PARTY_NOTICES.md)
        if(EXISTS "${KATANA_ROOT}/${_notice}" AND NOT _rules MATCHES "${_notice}")
            _problem("no install rule puts ${_notice} in the package: the licence page points to share/katana/licences")
        endif()
    endforeach()
    if(NOT _rules MATCHES "katana/licences")
        _problem("no install rule names share/katana/licences")
    endif()
endif()

if(_problems)
    message(FATAL_ERROR "the installer's licence page is not as it must be:${_problems}")
endif()
message(STATUS "installer licence: the page holds the licence and the additional terms, CRLF, ASCII; "
               "the template shows it before the folder page; the notices are installed")
