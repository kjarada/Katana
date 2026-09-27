# Signs a package under KATANA_PUBLISHER's name (docs/release.md, "Signing").
#
# Run by CPack twice (KatanaPackaging.cmake): as a PRE-build script, on the
# staged tree before it is archived, so the programs inside the ZIP, the
# tarball and the installer are signed; and as a POST-build script, on what
# was produced, so the Windows installer is signed too.
#
# Nothing is signed unless a certificate is named in the environment, so an
# ordinary `cmake --build <build> --target package` is unchanged:
#
#   Windows  KATANA_SIGN_PFX       a code-signing certificate (.pfx / PKCS#12)
#            KATANA_SIGN_PASSWORD  its password
#            -> Authenticode, with osslsigncode (MSYS2: <prefix>-osslsigncode)
#               or signtool, time-stamped
#   macOS    KATANA_SIGN_IDENTITY  a keychain identity, "Developer ID
#                                  Application: <name> (<team>)"
#            -> codesign with the hardened runtime, time-stamped; every
#               library and plugin too, since the hardened runtime loads
#               only code signed by the same team; then Katana.app, and
#               afterwards the disk image
#   both     KATANA_SIGN_TIMESTAMP the time-stamp server (default DigiCert's)
#
# Linux has no signature inside a package; the release's files are signed
# with GPG when they are published (.github/workflows/release.yml).
#
# A signature says who published the file; it is worth something only when
# the certificate is one Windows or macOS trusts - a code-signing certificate
# from a certificate authority, or an Apple Developer ID. A self-signed one
# changes nothing a user sees, which is why this script never makes one.

set(_timestamp "$ENV{KATANA_SIGN_TIMESTAMP}")
if(_timestamp STREQUAL "")
    set(_timestamp "http://timestamp.digicert.com")
endif()
set(_publisher "${CPACK_KATANA_PUBLISHER}")

if(DEFINED CPACK_PACKAGE_FILES)
    set(_stage post)
else()
    set(_stage pre)
endif()

# --- Windows: Authenticode --------------------------------------------------------
function(_katana_sign_windows file)
    find_program(_osslsigncode osslsigncode)
    find_program(_signtool signtool)
    if(_osslsigncode)
        # Signing twice (the ZIP and the installer stage the same tree) would
        # nest a second signature; a file already signed is left as it is.
        execute_process(COMMAND "${_osslsigncode}" verify -in "${file}"
            RESULT_VARIABLE _verified OUTPUT_QUIET ERROR_QUIET)
        if(_verified EQUAL 0)
            return()
        endif()
        execute_process(
            COMMAND "${_osslsigncode}" sign -pkcs12 "$ENV{KATANA_SIGN_PFX}"
                    -pass "$ENV{KATANA_SIGN_PASSWORD}" -n "Katana" -h sha256
                    -ts "${_timestamp}" -in "${file}" -out "${file}.signed"
            RESULT_VARIABLE _rc ERROR_VARIABLE _err OUTPUT_QUIET)
        if(NOT _rc EQUAL 0)
            message(FATAL_ERROR "Signing ${file} failed: ${_err}")
        endif()
        file(RENAME "${file}.signed" "${file}")
    elseif(_signtool)
        execute_process(
            COMMAND "${_signtool}" sign /f "$ENV{KATANA_SIGN_PFX}" /p "$ENV{KATANA_SIGN_PASSWORD}"
                    /fd sha256 /tr "${_timestamp}" /td sha256 /d "Katana" "${file}"
            RESULT_VARIABLE _rc ERROR_VARIABLE _err OUTPUT_QUIET)
        if(NOT _rc EQUAL 0)
            message(FATAL_ERROR "Signing ${file} failed: ${_err}")
        endif()
    else()
        message(FATAL_ERROR
            "KATANA_SIGN_PFX is set but neither osslsigncode nor signtool was found to sign with")
    endif()
    message(STATUS "Katana sign: ${file} signed as ${_publisher}")
endfunction()

# --- macOS: codesign ------------------------------------------------------------------
function(_katana_sign_macos file)
    execute_process(
        COMMAND codesign --force --timestamp --options runtime
                --sign "$ENV{KATANA_SIGN_IDENTITY}" "${file}"
        RESULT_VARIABLE _rc ERROR_VARIABLE _err OUTPUT_QUIET)
    if(NOT _rc EQUAL 0)
        message(FATAL_ERROR "codesign ${file} failed: ${_err}")
    endif()
endfunction()

if(NOT "$ENV{KATANA_SIGN_PFX}" STREQUAL "")
    if(_stage STREQUAL "pre")
        file(GLOB_RECURSE _programs "${CPACK_TEMPORARY_INSTALL_DIRECTORY}/*/katana*.exe"
                                    "${CPACK_TEMPORARY_INSTALL_DIRECTORY}/katana*.exe")
        list(REMOVE_DUPLICATES _programs)
        foreach(_program IN LISTS _programs)
            _katana_sign_windows("${_program}")
        endforeach()
    else()
        foreach(_package IN LISTS CPACK_PACKAGE_FILES)
            if(_package MATCHES "\\.exe$")
                _katana_sign_windows("${_package}")
            endif()
        endforeach()
    endif()
elseif(NOT "$ENV{KATANA_SIGN_IDENTITY}" STREQUAL "" AND _stage STREQUAL "pre")
    # Inside out: the libraries and plugins, then the programs that load
    # them, then Katana.app, whose seal records every nested signature and
    # resource. A signature covers the file it is in, and the hardened
    # runtime loads only libraries signed by the same team.
    file(GLOB_RECURSE _libraries "${CPACK_TEMPORARY_INSTALL_DIRECTORY}/*.dylib"
                                 "${CPACK_TEMPORARY_INSTALL_DIRECTORY}/*.so")
    foreach(_library IN LISTS _libraries)
        if(NOT IS_SYMLINK "${_library}")
            _katana_sign_macos("${_library}")
        endif()
    endforeach()
    file(GLOB_RECURSE _programs "${CPACK_TEMPORARY_INSTALL_DIRECTORY}/*/MacOS/katana*"
                                "${CPACK_TEMPORARY_INSTALL_DIRECTORY}/*/bin/katana*")
    foreach(_program IN LISTS _programs)
        if(NOT _program MATCHES "\\.conf$")
            _katana_sign_macos("${_program}")
        endif()
    endforeach()
    file(GLOB _bundles "${CPACK_TEMPORARY_INSTALL_DIRECTORY}/${CPACK_KATANA_APP_BUNDLE}"
                       "${CPACK_TEMPORARY_INSTALL_DIRECTORY}/*/${CPACK_KATANA_APP_BUNDLE}")
    if(CPACK_KATANA_APP_BUNDLE)
        foreach(_bundle IN LISTS _bundles)
            _katana_sign_macos("${_bundle}")
        endforeach()
    endif()
    list(LENGTH _libraries _count)
    message(STATUS "Katana sign: the programs and ${_count} libraries signed as "
                   "$ENV{KATANA_SIGN_IDENTITY}")
elseif(NOT "$ENV{KATANA_SIGN_IDENTITY}" STREQUAL "")
    # The disk image itself, so Gatekeeper can name its publisher before it is
    # opened; the release workflow then notarises it and staples the ticket.
    foreach(_package IN LISTS CPACK_PACKAGE_FILES)
        if(_package MATCHES "\\.dmg$")
            execute_process(
                COMMAND codesign --force --timestamp --sign "$ENV{KATANA_SIGN_IDENTITY}" "${_package}"
                RESULT_VARIABLE _rc ERROR_VARIABLE _err OUTPUT_QUIET)
            if(NOT _rc EQUAL 0)
                message(FATAL_ERROR "codesign ${_package} failed: ${_err}")
            endif()
            message(STATUS "Katana sign: ${_package} signed as $ENV{KATANA_SIGN_IDENTITY}")
        endif()
    endforeach()
endif()
