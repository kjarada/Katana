# Installing, bundling and packaging.
#
#   cmake --build build/release --target bundle     -> build/release/dist/Katana/
#   cmake --build build/release --target package    -> Katana-<version>-win64.zip
#                                     and .exe, -win-arm64.zip and .exe,
#                                     -linux-x86_64.tar.gz, -linux-aarch64.tar.gz,
#                                     -macos-arm64.dmg
#   cmake --install build/release --prefix <dir>    -> the same tree, anywhere
#
# The installed tree is SELF-CONTAINED: it runs on a machine with no MSYS2, no
# Qt and no GDAL installed. That is the whole point of it, and it is what the
# build tree cannot do - build/<config>/bin/katana.exe finds its DLLs only
# because the toolchain's bin directory is on PATH.
#
#   Katana/
#     bin/katana.exe, katana_cli.exe,    the programs
#         katana_mcp.exe, katana_updater.exe   (the updater: the helper that swaps
#                                           the files; not on macOS, see below)
#     bin/*.dll                          Qt, GDAL, PDAL, PROJ, the C++ runtime
#     bin/platforms, styles, ...         Qt plugins, where Qt looks for them
#     share/proj, share/gdal             proj.db and the GDAL support files
#     etc/ssl/certs/ca-bundle.crt        what libcurl checks https:// against
#     LICENSE                            the GNU General Public License
#     share/katana/licences              the additional terms, the plain-words guide
#                                        and the third-party notices
#     share/katana/samples               a project to open
#     share/katana/install.json          CPack's package only: says this is a release and
#                                        names its target and its version (see
#                                        "install.json" below)
#
# bin/ beside share/ is not arbitrary: PROJ looks for its database at
# <directory of the PROJ DLL>/../share/proj, so this layout is found without
# any configuration, and interop::useBundledData points GDAL at share/gdal.

# On macOS the same tree is an application bundle, Katana.app, laid out as
# code signing and Finder expect: programs in Contents/MacOS, libraries and
# Qt's plugins in Contents/Frameworks, and everything that is not code - GDAL,
# PROJ and certificate data, the samples - in Contents/Resources. The tree
# inside Contents keeps the relative layout the programs rely on
# (KatanaDeployUnix.cmake.in), with Frameworks for lib/ and Resources for the
# prefix of share/ and ssl/ (core/library_data.hpp).
if(APPLE AND DEFINED KATANA_TOOLCHAIN)
    set(KATANA_APP_BUNDLE "Katana.app")
    # The name macOS files the application's settings and permissions under;
    # reverse-DNS by convention, and never changed once released, since a new
    # one is a different application to macOS.
    set(KATANA_BUNDLE_IDENTIFIER "com.jarada.katana" CACHE STRING
        "CFBundleIdentifier of Katana.app")
    set(KATANA_INSTALL_BINDIR "${KATANA_APP_BUNDLE}/Contents/MacOS")
    set(KATANA_INSTALL_DATADIR "${KATANA_APP_BUNDLE}/Contents/Resources/share")
else()
    set(KATANA_APP_BUNDLE "")
    set(KATANA_INSTALL_BINDIR "${CMAKE_INSTALL_BINDIR}")
    set(KATANA_INSTALL_DATADIR "${CMAKE_INSTALL_DATADIR}")
endif()

if(TARGET katana)
    install(TARGETS katana RUNTIME DESTINATION ${KATANA_INSTALL_BINDIR})
endif()
foreach(_katana_program katana_cli katana_mcp)
    if(TARGET ${_katana_program})
        install(TARGETS ${_katana_program} RUNTIME DESTINATION ${KATANA_INSTALL_BINDIR})
    endif()
endforeach()
# The fourth program: the helper that swaps the install's files while Katana is
# not running and puts them back if anything goes wrong (docs/update.md). Static,
# linked to the update library and katana_core only, so it imports nothing but the
# operating system's DLLs, and never run from bin/: the engine copies it out first.
# Not on macOS, where updating is check-and-notify only (a file edited inside
# Katana.app breaks its seal): a helper that could not be used would only be one
# more thing signed and notarised.
#
# Linux: cmake/KatanaDeployUnix.cmake.in scans this program for libraries with the other
# three (tools/check_install_rules.cmake fails if an installed program is missing from that
# list, and nothing else would notice: a Windows build never runs the script), and the install gives it
# CMAKE_INSTALL_RPATH, $ORIGIN/../lib, as it gives the others. Not yet settled, and task
# T18's to settle: a Linux helper cannot be static as the Windows one is (glibc), and it runs
# from a copy in a state folder, where $ORIGIN/../lib is not the install's lib/, so it needs
# a way to find that lib/ from there. Nothing here has been built or run on Linux.
if(TARGET katana_updater AND NOT APPLE)
    install(TARGETS katana_updater RUNTIME DESTINATION ${KATANA_INSTALL_BINDIR})
endif()

install(DIRECTORY "${PROJECT_SOURCE_DIR}/samples/"
    DESTINATION "${KATANA_INSTALL_DATADIR}/katana/samples"
    # A project directory accumulates backups and caches as it is used; a
    # shipped sample should be the drawing and nothing else.
    PATTERN "backups" EXCLUDE
    PATTERN "cache" EXCLUDE)
# The built-in customisation is COMPILED INTO the binary (#embed, see
# src/katana_cad/CMakeLists.txt), so there is nothing to install beside it: its
# linestyles, symbols and survey codes travel inside the executable and need no
# files at run time.
#
# Its SOURCES are a different matter and must never be installed. The file it
# is compiled from (resources/customisation) is tracked, on the owner's
# decision of 2026-10-08, and the reference files it was converted from (under
# docs/) are third-party material kept untracked on the owner's machine: both
# are inputs to the build, not part of the product, and a bundle is something
# that gets handed to other people. The file is already in the executable; a
# second copy beside it would add nothing but a file made from another author's
# libraries. No install rule here takes anything from resources/ or docs/; the test
# packaging_installs_only_present_first_party_files
# (tools/check_install_rules.cmake) fails if one ever does.
#
# The programs installed are the four above, by name, and no others: the
# converter of the older customisation formats is built beside them and
# carries the readers of those formats, which the product does not, and the
# two executables the update tests build with a test key (katana_updater_test,
# katana_update_e2e) must never ship. The same check fails on a fifth program,
# and a test of the link closure (tests/CMakeLists.txt) fails if one of the four
# links the readers.

# Only files that exist. An install rule naming a missing file is not skipped:
# `cmake --install` stops at it, after the programs are copied and before the
# runtime DLLs are, and leaves a tree that does not start. A top-level file
# this used to install unconditionally was deleted from the repository, and
# every bundle and package failed that way until the rule was made
# conditional. The same test fails on a rule that names a missing file.
foreach(_katana_top_level_file LICENSE)
    if(EXISTS "${PROJECT_SOURCE_DIR}/${_katana_top_level_file}")
        install(FILES "${PROJECT_SOURCE_DIR}/${_katana_top_level_file}" DESTINATION .)
    endif()
endforeach()

# The rest of what Katana is licensed under, and the licences of what it ships with, beside
# the programs the licence is for: the additional terms (ADDITIONAL_TERMS.md), the plain-words
# guide (LICENSING.md) and the third-party audit (THIRD_PARTY_NOTICES.md). Under share/katana
# and not at the top beside LICENSE, which the update manifest leaves out (UPDATE_EXCLUDE in
# the release workflow: the producer refuses any other file outside bin, share, etc and lib),
# so an update brings the notices that match the programs it brings. Only files that exist,
# for the reason above.
foreach(_katana_notice ADDITIONAL_TERMS.md LICENSING.md THIRD_PARTY_NOTICES.md)
    if(EXISTS "${PROJECT_SOURCE_DIR}/${_katana_notice}")
        install(FILES "${PROJECT_SOURCE_DIR}/${_katana_notice}"
            DESTINATION "${KATANA_INSTALL_DATADIR}/katana/licences")
    endif()
endforeach()

# --- runtime dependencies ------------------------------------------------------
#
# Windows only, and only for a toolchain whose DLLs live beside its compiler
# (MSYS2 / MinGW). Linux and macOS are below.
if(WIN32)
    find_program(KATANA_WINDEPLOYQT NAMES windeployqt6 windeployqt
        HINTS "${KATANA_RUNTIME_BIN}")
    if(TARGET katana AND NOT KATANA_WINDEPLOYQT)
        message(WARNING
            "windeployqt was not found: `cmake --install` will copy katana.exe but not Qt, "
            "and the installed application will not start on a machine without Qt.")
    endif()
    # Where Qt keeps its plugins, for the one windeployqt leaves behind (see
    # the offscreen note in KatanaDeploy.cmake.in). Asked of Qt itself rather
    # than guessed from the toolchain layout.
    set(KATANA_QT_PLUGIN_DIR "")
    find_program(KATANA_QTPATHS NAMES qtpaths6 qtpaths HINTS "${KATANA_RUNTIME_BIN}")
    if(KATANA_QTPATHS)
        execute_process(COMMAND "${KATANA_QTPATHS}" --query QT_INSTALL_PLUGINS
            OUTPUT_VARIABLE KATANA_QT_PLUGIN_DIR OUTPUT_STRIP_TRAILING_WHITESPACE
            ERROR_QUIET)
    endif()
    # What decides WHICH DLLs a deploy needs, hashed: every module's link
    # libraries, the toolchain and Qt locations, and the deploy script itself.
    # The build-tree deploy is skipped while this and the toolchain's files are
    # unchanged - see step 0 of KatanaDeploy.cmake.in for why this is the key
    # and not the programs' import tables.
    set(_katana_deploy_inputs "${KATANA_RUNTIME_BIN}|${KATANA_QT_PLUGIN_DIR}|${KATANA_WINDEPLOYQT}")
    foreach(_target IN LISTS KATANA_MODULES ITEMS katana katana_cli)
        if(TARGET ${_target})
            foreach(_property LINK_LIBRARIES INTERFACE_LINK_LIBRARIES)
                get_target_property(_links ${_target} ${_property})
                if(_links)
                    string(APPEND _katana_deploy_inputs "|${_target}.${_property}=${_links}")
                endif()
            endforeach()
        endif()
    endforeach()
    file(MD5 "${CMAKE_CURRENT_LIST_DIR}/KatanaDeploy.cmake.in" _katana_deploy_script_hash)
    string(APPEND _katana_deploy_inputs "|${_katana_deploy_script_hash}")
    string(MD5 KATANA_DEPLOY_KEY "${_katana_deploy_inputs}")

    configure_file("${CMAKE_CURRENT_LIST_DIR}/KatanaDeploy.cmake.in"
                   "${CMAKE_BINARY_DIR}/KatanaDeploy.cmake" @ONLY)
    install(SCRIPT "${CMAKE_BINARY_DIR}/KatanaDeploy.cmake")
endif()

# Linux and macOS, when the build uses a toolchain prefix of its own
# (cmake/toolchains/katana-linux.cmake, katana-macos.cmake): the libraries come
# from that prefix and nowhere else, so an installed tree that did not carry
# them would run only on the machine that built it. KatanaDeployUnix.cmake.in
# copies them into lib/, beside Qt's plugins and the GDAL, PROJ and
# certificate data. A build against a distribution's own libraries leaves the
# dependencies to its package manager, as before.
if(NOT WIN32 AND DEFINED KATANA_TOOLCHAIN)
    set(KATANA_TOOLCHAIN_PREFIX "${KATANA_TOOLCHAIN}")
    set(KATANA_QT_PLUGIN_DIR "")
    find_program(KATANA_QMAKE NAMES qmake6 qmake HINTS "${KATANA_TOOLCHAIN}/bin" NO_DEFAULT_PATH)
    if(KATANA_QMAKE)
        execute_process(COMMAND "${KATANA_QMAKE}" -query QT_INSTALL_PLUGINS
            OUTPUT_VARIABLE KATANA_QT_PLUGIN_DIR OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
    endif()
    if(NOT IS_DIRECTORY "${KATANA_QT_PLUGIN_DIR}")
        set(KATANA_QT_PLUGIN_DIR "${KATANA_TOOLCHAIN}/lib/qt6/plugins")
    endif()
    if(KATANA_APP_BUNDLE)
        configure_file("${CMAKE_CURRENT_LIST_DIR}/KatanaInfo.plist.in"
                       "${CMAKE_BINARY_DIR}/Info.plist" @ONLY)
    endif()
    configure_file("${CMAKE_CURRENT_LIST_DIR}/KatanaDeployUnix.cmake.in"
                   "${CMAKE_BINARY_DIR}/KatanaDeployUnix.cmake" @ONLY)
    install(SCRIPT "${CMAKE_BINARY_DIR}/KatanaDeployUnix.cmake")
endif()

# --- the build tree runs on its own ----------------------------------------------
#
# GDAL, PDAL, PROJ, Qt and the C++ runtime are copied beside the programs in
# <build>/bin by the build itself, so build/release/bin/katana.exe starts with
# no MSYS2 on PATH - part of the program, not borrowed from the toolchain at run
# time. The same script as the bundle, run incrementally: a full deploy once
# per configuration or toolchain update, and a check of timestamps (well under
# a second) on every other build.
#
# A target of its own that FOLLOWS katana and katana_cli, rather than a
# POST_BUILD step on each: the two link in parallel under Ninja, and two
# deploys writing one bin/ at once would race.
#
# Why DLLs beside the program and not static linking: MSYS2 ships PDAL and
# PROJ as DLLs only, and libpdalcpp links the GDAL DLL itself, so a Katana
# with GDAL linked in statically would load a second GDAL through PDAL - two
# driver registries and two error states in one process. A static build means
# building GDAL, PROJ, PDAL and their dependencies from source; see
# docs/architecture.md, "Why DLLs beside the program, and not static linking".
option(KATANA_DEPLOY_RUNTIME
       "Copy the runtime DLLs and the GDAL/PROJ data into <build>/bin and <build>/share" ON)

# `bundle` is `cmake --install` into the build tree, under a name a person can
# find, cleared first so that a file removed from the install rules does not
# linger in an old bundle.
set(_katana_bundle_targets "")
foreach(_target katana katana_cli katana_updater)
    if(TARGET ${_target})
        list(APPEND _katana_bundle_targets ${_target})
    endif()
endforeach()
add_custom_target(bundle
    COMMAND ${CMAKE_COMMAND} -E rm -rf "${CMAKE_BINARY_DIR}/dist/Katana"
    COMMAND ${CMAKE_COMMAND} --install "${CMAKE_BINARY_DIR}"
            --prefix "${CMAKE_BINARY_DIR}/dist/Katana"
    DEPENDS ${_katana_bundle_targets}
    COMMENT "Bundling a self-contained Katana into ${CMAKE_BINARY_DIR}/dist/Katana"
    USES_TERMINAL)

if(WIN32 AND KATANA_DEPLOY_RUNTIME AND _katana_bundle_targets)
    add_custom_target(katana_runtime ALL
        COMMAND ${CMAKE_COMMAND} "-DKATANA_DEPLOY_PREFIX=${CMAKE_BINARY_DIR}"
                -P "${CMAKE_BINARY_DIR}/KatanaDeploy.cmake"
        COMMENT "Katana runtime: GDAL, PDAL, PROJ and Qt beside the programs in ${CMAKE_BINARY_DIR}/bin"
        VERBATIM)
    add_dependencies(katana_runtime ${_katana_bundle_targets})
    if(KATANA_BUILD_TESTS)
        # The deploy copies libcurl beside the programs, and libcurl reads the
        # certificates it checks https:// against from ../etc/ssl/certs beside
        # its own DLL. Every web request of GIS > Online Data failed while the
        # deploy left them out, and no other test saw it: the tests load
        # libcurl from the toolchain, whose bundle is in place.
        add_test(NAME runtime_has_the_certificates_https_is_checked_against
            COMMAND ${CMAKE_COMMAND} -E cat "${CMAKE_BINARY_DIR}/etc/ssl/certs/ca-bundle.crt")
        set_tests_properties(runtime_has_the_certificates_https_is_checked_against PROPERTIES
            PASS_REGULAR_EXPRESSION "-----BEGIN CERTIFICATE-----")
    endif()
endif()

# --- CPack -----------------------------------------------------------------------
set(CPACK_PACKAGE_NAME "Katana")
# The installer shows it as the Publisher in Settings > Apps.
set(CPACK_PACKAGE_VENDOR "${KATANA_PUBLISHER}")
set(CPACK_PACKAGE_VERSION "${PROJECT_VERSION}")
set(CPACK_PACKAGE_DESCRIPTION_SUMMARY "High-performance survey and CAD platform")
set(CPACK_PACKAGE_INSTALL_DIRECTORY "Katana")
set(CPACK_PACKAGE_EXECUTABLES "katana" "Katana")
# Signing, between staging the tree and archiving it, so what is inside the
# archive and the installer is signed; and the installer itself afterwards.
# Each does nothing unless a certificate is named in the environment
# (cmake/KatanaSign.cmake, docs/release.md "Signing") - but for one thing: a
# macOS bundle is sealed again, ad hoc, when none is named.
# The install record first, so what is signed already holds it (see "install.json" below).
set(CPACK_PRE_BUILD_SCRIPTS
    "${CMAKE_BINARY_DIR}/KatanaInstallRecord.cmake;${CMAKE_CURRENT_LIST_DIR}/KatanaSign.cmake")
set(CPACK_POST_BUILD_SCRIPTS "${CMAKE_CURRENT_LIST_DIR}/KatanaSign.cmake")
set(CPACK_KATANA_PUBLISHER "${KATANA_PUBLISHER}")
set(CPACK_KATANA_OBJDUMP "${CMAKE_OBJDUMP}")
set(CPACK_KATANA_APP_BUNDLE "${KATANA_APP_BUNDLE}")
# The target token: the last part of a package's name (Katana-<version>-<token>.zip), the
# `target` of share/katana/install.json, and the name of the update feed's manifest
# (Katana-update-<token>.json). One place, so a package and its update cannot disagree
# about what they are.
if(WIN32)
    # win64 is x86-64, as it has been since the first package; ARM64 says so.
    if(CMAKE_SYSTEM_PROCESSOR MATCHES "^(ARM64|arm64|aarch64)$")
        set(KATANA_UPDATE_TARGET "win-arm64")
    else()
        set(KATANA_UPDATE_TARGET "win64")
    endif()
    set(CPACK_PACKAGE_FILE_NAME "Katana-${PROJECT_VERSION}-${KATANA_UPDATE_TARGET}")
    # A ZIP always; an installer when NSIS is there to build one. NSIS is not
    # a dependency of the project, so its absence is not an error - install it
    # with `pacman -S mingw-w64-ucrt-x86_64-nsis` and reconfigure. MSYS2 has
    # no NSIS for ARM64; NSIS's own x86 build, which Windows on ARM runs
    # emulated, makes the same installer - pass -DKATANA_MAKENSIS=<its
    # makensis.exe>. The installer's own code is that NSIS's: x86 for its own
    # build, x86-64 for MSYS2's (cmake/NSIS.template.in says which to make);
    # what it installs is the programs built here.
    set(CPACK_GENERATOR "ZIP")
    find_program(KATANA_MAKENSIS NAMES makensis HINTS "${KATANA_RUNTIME_BIN}")
    if(KATANA_MAKENSIS)
        list(APPEND CPACK_GENERATOR "NSIS")
        # The makensis found here, not one CPack finds for itself elsewhere.
        set(CPACK_NSIS_EXECUTABLE "${KATANA_MAKENSIS}")
        set(CPACK_NSIS_DISPLAY_NAME "Katana ${PROJECT_VERSION}")
        set(CPACK_NSIS_PACKAGE_NAME "Katana")
        # The installer is PER USER (docs/release.md, "The Windows installer"): it runs without
        # administrator rights, installs into the user's own %LOCALAPPDATA%\Programs\Katana unless
        # they choose another folder they can write to, and writes only under HKCU. That is what
        # lets the updater (docs/update.md) replace files in the install as the same user, with no
        # UAC prompt on every update. cmake/NSIS.template.in is what makes it so: CPack's own
        # template is elevated and machine-wide, and CPack takes this project's in preference
        # because cmake/ is on CPACK_MODULE_PATH. A template that went missing would not be an
        # error to CPack, only a different installer, so it is one here.
        if(NOT EXISTS "${CMAKE_CURRENT_LIST_DIR}/NSIS.template.in")
            message(FATAL_ERROR
                "cmake/NSIS.template.in is missing: without it CPack builds its own installer, which "
                "asks for administrator rights and writes machine-wide, and the updater cannot update it")
        endif()
        # Four backslashes: CPack writes this value into CPackConfig.cmake as it is and CMake reads
        # that file again, so the one backslash NSIS wants must survive a second unescaping (with
        # two, cpack stops at "Invalid character escape '\P'").
        set(CPACK_NSIS_INSTALL_ROOT "$LOCALAPPDATA\\\\Programs")
        # The key the installer writes under HKCU Software\<vendor> and ...\Uninstall, and the
        # updater rewrites DisplayVersion in (include/katana/update/registry.hpp): said here rather
        # than left to CPack's default, which is the install directory's name, so that renaming one
        # cannot move the other.
        set(CPACK_PACKAGE_INSTALL_REGISTRY_KEY "Katana")
        # The icon Settings > Apps shows, said with a backslash like the rest of the path (the
        # same four as above).
        set(CPACK_NSIS_INSTALLED_ICON_NAME "bin\\\\katana.exe")
        set(CPACK_NSIS_ENABLE_UNINSTALL_BEFORE_INSTALL ON)
        # The licence page: the installer shows the licence before it asks for a folder. What it
        # shows is made here from the two texts that are binding - LICENSE, the GNU General Public
        # License, and ADDITIONAL_TERMS.md, the terms added under its section 7 - behind a short
        # note that nobody has to accept them to run Katana (the template's page says so too, and
        # its button reads Next, not I Agree: the GPL asks for no acceptance, and a button that
        # asked would say it did). One file, because the installer has one page for it. Written
        # with Windows line ends, which the page's text box needs to break lines: the text is
        # made LF only here (LICENSE is LF in the repository and CRLF in a checkout that
        # converts) and file(WRITE) is a text-mode write, which on Windows - the only host that
        # builds this installer - ends each line CRLF; converting here as well gave CR CR LF
        # (a blank line after every line). And in ASCII, which a Unicode
        # installer reads as it is: a character outside it would need a byte order mark that
        # CMake cannot write, so it is refused here, in the configure, by name, rather than shown
        # as other characters on a user's screen. tools/check_installer_licence.cmake holds the
        # result (the test packaging_installer_shows_the_licence).
        if(EXISTS "${PROJECT_SOURCE_DIR}/LICENSE" AND EXISTS "${PROJECT_SOURCE_DIR}/ADDITIONAL_TERMS.md")
            set(_katana_licence_page "${CMAKE_BINARY_DIR}/KatanaInstallerLicence.txt")
            file(READ "${PROJECT_SOURCE_DIR}/LICENSE" _katana_gpl)
            file(READ "${PROJECT_SOURCE_DIR}/ADDITIONAL_TERMS.md" _katana_terms)
            set(_katana_licence_text
"Katana, copyright (c) 2026 Jarada, https://github.com/kjarada/Katana

Katana is free software. You may use it for any purpose, study and change it, and
share it, changed or not, under the terms below: the GNU General Public License,
version 3 or, at your option, any later version, and the additional terms that
follow it. You do not have to accept them to install or run Katana; they are
here so that you can read what you are free to do and what you are asked to keep.

The licences of the libraries Katana is built with and ships with are in
THIRD_PARTY_NOTICES.md, in share/katana/licences in the folder Katana is
installed to.

================================================================================
THE GNU GENERAL PUBLIC LICENSE
================================================================================

${_katana_gpl}

================================================================================
ADDITIONAL TERMS (ADDITIONAL_TERMS.md)
================================================================================

${_katana_terms}")
            string(REPLACE "\r\n" "\n" _katana_licence_text "${_katana_licence_text}")
            if(_katana_licence_text MATCHES "[^\n\t -~]")
                message(FATAL_ERROR
                    "The installer's licence page is made of LICENSE and ADDITIONAL_TERMS.md, and one "
                    "of them has a character outside ASCII, which the page would show as other "
                    "characters. Write it in ASCII (cmake/KatanaPackaging.cmake says why).")
            endif()
            file(WRITE "${_katana_licence_page}" "${_katana_licence_text}")
            set(CPACK_RESOURCE_FILE_LICENSE "${_katana_licence_page}")
            # The page is made again when either text changes, not only at the next fresh configure.
            set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS
                "${PROJECT_SOURCE_DIR}/LICENSE" "${PROJECT_SOURCE_DIR}/ADDITIONAL_TERMS.md")
        endif()
        if(EXISTS "${PROJECT_SOURCE_DIR}/resources/katana.ico")
            set(CPACK_NSIS_MUI_ICON "${PROJECT_SOURCE_DIR}/resources/katana.ico")
            set(CPACK_NSIS_MUI_UNIICON "${PROJECT_SOURCE_DIR}/resources/katana.ico")
        endif()
    endif()
elseif(APPLE)
    set(KATANA_UPDATE_TARGET "macos-${CMAKE_SYSTEM_PROCESSOR}")
    set(CPACK_PACKAGE_FILE_NAME "Katana-${PROJECT_VERSION}-${KATANA_UPDATE_TARGET}")
    if(KATANA_APP_BUNDLE)
        # A disk image holding Katana.app beside a link to /Applications, the
        # drag-to-install a Mac user expects (CPack's DragNDrop adds the link).
        set(CPACK_GENERATOR "DragNDrop")
        set(CPACK_DMG_VOLUME_NAME "Katana ${PROJECT_VERSION}")
        set(CPACK_DMG_FORMAT "UDZO")
    else()
        set(CPACK_GENERATOR "TGZ")
    endif()
else()
    set(KATANA_UPDATE_TARGET "linux-${CMAKE_SYSTEM_PROCESSOR}")
    set(CPACK_PACKAGE_FILE_NAME "Katana-${PROJECT_VERSION}-${KATANA_UPDATE_TARGET}")
    set(CPACK_GENERATOR "TGZ")
endif()

# --- install.json ---------------------------------------------------------------------
#
# share/katana/install.json is what makes an install a RELEASE: without it a tree is a
# development tree, the update engine refuses to apply anything to it and no automatic
# check runs. It says what the package is:
#
#   {"schema":1,"target":"<token>","version":"<version>"}                 built anywhere
#   {"schema":1,"target":"<token>","version":"<version>","sequence":<n>}  built for a release
#
# So it is written ONLY into the tree CPack stages for a package - by the script below,
# which CPack runs before it signs (a file added after the signature would break a macOS
# bundle's seal) - and NEVER by an install rule. The install itself has sealed a macOS
# bundle by then, ad hoc
# (KatanaDeployUnix.cmake.in, step 5), and the record is a file that seal does not list:
# that is why KatanaSign.cmake seals a bundle again even when no certificate is named.
# `cmake --install`, the `bundle` target and build/<preset>/bin are development
# trees and stay without it: an update there would replace a developer's build with a
# release. tools/check_install_rules.cmake asserts both halves (no install rule names
# the file; CPackConfig.cmake runs this script before the signing script) and the
# tests of that check prove it on fixtures.
#
# The target is the token above and not the machine's: Windows on ARM running the x64 zip
# is win64 and must be updated with the x64 files. The script is written here, at
# configure time, because the token and the data directory are known only here.
#
# The version is PROJECT_VERSION, always: the version these programs were built as. The
# engine takes it for the copy's own build whichever program asks (src/katana_update/state.cpp,
# packageBuild): the helper that reads an install may be a later release's, and its own
# version is then not the install's. It is also the floor a stale installed.json cannot
# lower, where a newer zip was unpacked by hand over a copy that had updated itself.
#
# The sequence is a RELEASE's, and a package built by hand belongs to none: the release
# workflow settles one number for a run before anything is built, and every update manifest
# of that release carries it (tools/update_make_manifest.py --sequence). A configure that
# packages for a release is given the same number here; with none the record names the
# version alone. Never a sequence without a version - the engine does not read such a record
# (ParseFailure), and a copy whose record does not read cannot be updated at all - which is
# why the version is no option, and why the script below writes it first and the sequence
# only after it.
#
# A cache entry, so it stays until a configure gives another: a build directory that packaged
# one release must be given the next one's (or -UKATANA_UPDATE_SEQUENCE) before it packages
# again. The record script prints the record it writes, so a package's log shows which.
set(KATANA_UPDATE_SEQUENCE "" CACHE STRING
    "Sequence of the release this build is packaged for, the number its update manifests carry (UTC epoch seconds); empty: the package names its version alone")

# What is wrong with `sequence` as the sequence of a release, in `out`; nothing ("") for a
# sequence that is one and for the empty text, which names none. The rule is the one a
# manifest is held to - UTC epoch seconds from 1 to 4102444800, the first second of 2100, by
# the producer (tools/update_make_manifest.py, MAX_SEQUENCE) and by the engine that reads it
# (src/katana_update/manifest.cpp) - so that a package cannot name a sequence no manifest
# could carry, above every release of its own version for good. Digits alone, because the
# text goes into the record as a JSON number as it is: a sign, a fraction, an exponent or a
# leading zero would be a record that does not read, or a number other than the manifests'.
function(katana_update_sequence_problem sequence out)
    set(_problem "")
    if(NOT "${sequence}" STREQUAL "")
        string(LENGTH "${sequence}" _digits)
        if(NOT "${sequence}" MATCHES "^[1-9][0-9]*$")
            set(_problem "is not a whole number of 1 or more, written in digits alone")
        elseif(_digits GREATER 10 OR "${sequence}" GREATER 4102444800)
            set(_problem "is above 4102444800, the last sequence an update manifest may carry (UTC epoch seconds, the year 2100)")
        endif()
    endif()
    set(${out} "${_problem}" PARENT_SCOPE)
endfunction()

# Writes the record script `output`: run by CPack, it writes <staged tree>/<datadir>/katana/
# install.json for a package of `target`, built as `version`, of the release `sequence` ("" for
# none). A function, so that the tests below read back records made with and without a
# sequence whatever this configure was given.
function(katana_write_install_record_script output target version sequence datadir)
    # 2 to 4 components of digits, as update::parseVersion reads a version (version.hpp):
    # project(VERSION) cannot give a fifth, and could give one alone.
    if(NOT "${version}" MATCHES "^[0-9]+[.][0-9]+([.][0-9]+)?([.][0-9]+)?$")
        message(FATAL_ERROR
            "The install record cannot name the version '${version}': the update engine reads "
            "MAJOR.MINOR, MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH.BUILD, and a package whose "
            "record does not read can never be updated")
    endif()
    katana_update_sequence_problem("${sequence}" _problem)
    if(_problem)
        message(FATAL_ERROR
            "KATANA_UPDATE_SEQUENCE '${sequence}' ${_problem}. Give the sequence of the release "
            "this build is packaged for, or leave it empty for a package of no release")
    endif()
    set(KATANA_RECORD_TARGET "${target}")
    set(KATANA_RECORD_VERSION "${version}")
    set(KATANA_RECORD_SEQUENCE "${sequence}")
    set(KATANA_RECORD_DATADIR "${datadir}")
    get_filename_component(_output_directory "${output}" DIRECTORY)
    file(MAKE_DIRECTORY "${_output_directory}")
    file(CONFIGURE OUTPUT "${output}" @ONLY CONTENT [==[
# Written by cmake/KatanaPackaging.cmake; run by CPack on the staged tree, before the
# signing script. Not part of any install rule: see "install.json" there.
if(NOT CPACK_TEMPORARY_INSTALL_DIRECTORY)
    message(FATAL_ERROR "KatanaInstallRecord.cmake is a CPack script: it runs on the tree CPack stages")
endif()
set(_directory "${CPACK_TEMPORARY_INSTALL_DIRECTORY}/@KATANA_RECORD_DATADIR@/katana")
file(MAKE_DIRECTORY "${_directory}")
# The version always and first; the sequence only where the configure named one, and so
# never without a version (the engine does not read such a record).
set(_record "{\"schema\":1,\"target\":\"@KATANA_RECORD_TARGET@\",\"version\":\"@KATANA_RECORD_VERSION@\"")
set(_sequence "@KATANA_RECORD_SEQUENCE@")
if(NOT "${_sequence}" STREQUAL "")
    string(APPEND _record ",\"sequence\":${_sequence}")
endif()
string(APPEND _record "}")
# No trailing newline: file(WRITE) writes a line break as CRLF on Windows and LF elsewhere,
# and nothing is gained by the file differing between the packages.
file(WRITE "${_directory}/install.json" "${_record}")
message(STATUS "Katana install record: ${_record}")
]==])
endfunction()
katana_write_install_record_script("${CMAKE_BINARY_DIR}/KatanaInstallRecord.cmake"
    "${KATANA_UPDATE_TARGET}" "${PROJECT_VERSION}" "${KATANA_UPDATE_SEQUENCE}"
    "${KATANA_INSTALL_DATADIR}")

if(KATANA_BUILD_TESTS)
    # The record, read back from a tree the script wrote it into, as CPack runs the script:
    # the real one of this build, and two made here with a fixed target and version - one of
    # a release, one of none - so that both shapes are read in every build. The expected
    # texts are the two shapes drawn under "install.json" above, written out by hand, and
    # the example of src/katana_update/state.cpp (packageBuild) for the one of a release.
    #
    # The reader is written to the build directory, as the record script is: the staged
    # folder is emptied first, so a record left by an earlier run cannot stand for one the
    # script no longer writes, and the text is printed between < and >, so that a line
    # break or anything else after the closing brace fails the test.
    file(WRITE "${CMAKE_BINARY_DIR}/KatanaReadInstallRecord.cmake" [==[
# Written by cmake/KatanaPackaging.cmake for its tests of the install record.
#   cmake -DRECORD_SCRIPT=<record script> -DSTAGE=<folder to stage in> -DRECORD=<install.json, under it> -P <this>
foreach(_argument IN ITEMS RECORD_SCRIPT STAGE RECORD)
    if(NOT ${_argument})
        message(FATAL_ERROR "Pass -DRECORD_SCRIPT=<script> -DSTAGE=<folder> -DRECORD=<path of install.json under it>")
    endif()
endforeach()
file(REMOVE_RECURSE "${STAGE}")
set(CPACK_TEMPORARY_INSTALL_DIRECTORY "${STAGE}")
include("${RECORD_SCRIPT}")
if(NOT EXISTS "${STAGE}/${RECORD}")
    message(FATAL_ERROR "${RECORD_SCRIPT} wrote no ${RECORD} into the staged tree")
endif()
file(READ "${STAGE}/${RECORD}" _text)
message(STATUS "The record as written: <${_text}>")
]==])
    # The test packaging_install_record_<name>: `script` run on a staged folder of its own, and
    # the record it leaves under `datadir` held to `expected`, a regular expression of the
    # whole text (the reader's < and > included).
    function(katana_add_install_record_test name script datadir expected)
        add_test(NAME packaging_install_record_${name}
            COMMAND ${CMAKE_COMMAND} "-DRECORD_SCRIPT=${script}"
                    "-DSTAGE=${CMAKE_BINARY_DIR}/install_record_staged/${name}"
                    "-DRECORD=${datadir}/katana/install.json"
                    -P "${CMAKE_BINARY_DIR}/KatanaReadInstallRecord.cmake")
        set_tests_properties(packaging_install_record_${name} PROPERTIES
            PASS_REGULAR_EXPRESSION "${expected}"
            FAIL_REGULAR_EXPRESSION "CMake Error")
    endfunction()

    # This build's: its own target and version, and its sequence where the configure was
    # given one. A dot of the version is a dot, not any character.
    string(REPLACE "." "[.]" _katana_record_version "${PROJECT_VERSION}")
    set(_katana_record_of_this_build
        "<[{]\"schema\":1,\"target\":\"${KATANA_UPDATE_TARGET}\",\"version\":\"${_katana_record_version}\"")
    if(NOT "${KATANA_UPDATE_SEQUENCE}" STREQUAL "")
        string(APPEND _katana_record_of_this_build ",\"sequence\":${KATANA_UPDATE_SEQUENCE}")
    endif()
    string(APPEND _katana_record_of_this_build "[}]>")
    katana_add_install_record_test(names_this_builds_target_and_version
        "${CMAKE_BINARY_DIR}/KatanaInstallRecord.cmake" "${KATANA_INSTALL_DATADIR}"
        "${_katana_record_of_this_build}")

    # The two shapes, on fixed values: 0.5.0 for win64, of the release 1792312800 and of none.
    set(_katana_record_fixtures "${CMAKE_BINARY_DIR}/install_record_fixtures")
    katana_write_install_record_script("${_katana_record_fixtures}/of_a_release/KatanaInstallRecord.cmake"
        "win64" "0.5.0" "1792312800" "share")
    katana_write_install_record_script("${_katana_record_fixtures}/of_no_release/KatanaInstallRecord.cmake"
        "win64" "0.5.0" "" "share")
    katana_add_install_record_test(of_a_release_names_its_version_and_then_its_sequence
        "${_katana_record_fixtures}/of_a_release/KatanaInstallRecord.cmake" "share"
        "<[{]\"schema\":1,\"target\":\"win64\",\"version\":\"0[.]5[.]0\",\"sequence\":1792312800[}]>")
    katana_add_install_record_test(of_no_release_names_its_version_and_no_sequence
        "${_katana_record_fixtures}/of_no_release/KatanaInstallRecord.cmake" "share"
        "<[{]\"schema\":1,\"target\":\"win64\",\"version\":\"0[.]5[.]0\"[}]>")

    # The rule a sequence is held to, on what a configure may be given: the least and the
    # greatest a manifest may carry (1 and 4102444800) and one between; and what is refused -
    # nought, a leading zero, a sign, a fraction, an exponent, a word, two numbers, the first
    # number above the greatest, and one beyond 64 bits. Worked out at configure time, which
    # is when the rule is applied, and reported by an echo test as the link closure is.
    set(_katana_sequences_taken "")
    set(_katana_sequences_refused "")
    foreach(_sequence IN ITEMS 1 1792312800 4102444800
            0 01 -1 +1 1.0 1e9 abc "1 2" 4102444801 18446744073709551616)
        katana_update_sequence_problem("${_sequence}" _problem)
        if(_problem)
            list(APPEND _katana_sequences_refused "${_sequence}")
        else()
            list(APPEND _katana_sequences_taken "${_sequence}")
        endif()
    endforeach()
    katana_update_sequence_problem("" _problem)
    if(_problem)
        set(_katana_no_sequence "refused")
    else()
        set(_katana_no_sequence "no sequence")
    endif()
    list(JOIN _katana_sequences_taken ", " _katana_sequences_taken)
    list(JOIN _katana_sequences_refused ", " _katana_sequences_refused)
    add_test(NAME packaging_takes_a_release_sequence_and_refuses_what_no_manifest_could_carry
        COMMAND ${CMAKE_COMMAND} -E echo
            "Sequences taken: ${_katana_sequences_taken}. Refused: ${_katana_sequences_refused}. The empty text: ${_katana_no_sequence}.")
    set_tests_properties(packaging_takes_a_release_sequence_and_refuses_what_no_manifest_could_carry PROPERTIES
        PASS_REGULAR_EXPRESSION
            "Sequences taken: 1, 1792312800, 4102444800[.] Refused: 0, 01, -1, [+]1, 1[.]0, 1e9, abc, 1 2, 4102444801, 18446744073709551616[.] The empty text: no sequence[.]")
endif()

include(CPack)
