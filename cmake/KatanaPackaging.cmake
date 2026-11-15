# Installing, bundling and packaging the programs cargo built.
#
# Included by the packaging project, packaging/CMakeLists.txt, which compiles
# nothing (LANGUAGES NONE) and is configured with the folder cargo put the
# release programs in:
#
#   cd packaging && cmake --preset package -DKATANA_PROGRAMS=<cargo target>/release
#   cmake --build build/package --target bundle    -> build/package/dist/Katana/
#   cmake --build build/package --target package   -> Katana-<version>-win64.zip
#                                     and .exe, -win-arm64.zip and .exe,
#                                     -linux-x86_64.tar.gz, -linux-aarch64.tar.gz,
#                                     -macos-arm64.dmg
#   cmake --install build/package --prefix <dir>   -> the same tree, anywhere
#
# (docs/release.md, "Packaging"). Any project that compiles nothing may include
# it the same way - the test runner too, whose packaging tests then read its
# own build directory - given PROJECT_VERSION, Katana's version, and
# KATANA_PROGRAMS.
#
# Until 2026-11-15 this was part of the C++ build: install(TARGETS) of the
# programs it linked, Qt deployed beside them by windeployqt, and the build
# tree's own bin/ made self-contained at every build. The programs are cargo's
# now, so they are installed as files (install(PROGRAMS)) from KATANA_PROGRAMS,
# there is no Qt to deploy, and cargo's target folder is not made
# self-contained: a developer runs the programs where the toolchain is, and
# the bundle is the tree that runs on its own.
#
# The installed tree is SELF-CONTAINED: it runs on a machine with no MSYS2 and
# no GDAL installed.
#
#   Katana/
#     bin/katana.exe, katana_cli.exe,    the programs
#         katana_mcp.exe, katana_updater.exe   (the updater: the helper that swaps
#                                           the files; not on macOS, see below)
#     bin/*.dll                          GDAL, PDAL, PROJ, SQLite, libcurl, the C and
#                                        C++ runtimes the native shims link
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
# any configuration, and katana_core's library_data points GDAL at share/gdal
# and libcurl at the certificates, beside the library that reads them.

cmake_minimum_required(VERSION 3.24)

if(NOT PROJECT_VERSION)
    message(FATAL_ERROR "cmake/KatanaPackaging.cmake needs PROJECT_VERSION, Katana's version: include it from a project that declares it (packaging/CMakeLists.txt)")
endif()

# The checkout: the samples, the licence and the notices are taken from it, whichever
# project includes this.
get_filename_component(KATANA_ROOT "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)

set(KATANA_PROGRAMS "" CACHE PATH
    "The folder cargo put the release programs in (<cargo target>/release): what is installed and packaged")
if(KATANA_PROGRAMS STREQUAL "")
    message(FATAL_ERROR "Give -DKATANA_PROGRAMS=<the folder of the release programs cargo built>, "
                        "e.g. target/release, or ~/katana-build/<worktree>/cargo/release in WSL")
endif()
file(TO_CMAKE_PATH "${KATANA_PROGRAMS}" KATANA_PROGRAMS)
if(NOT DEFINED KATANA_PUBLISHER)
    set(KATANA_PUBLISHER "Jarada" CACHE STRING "The publisher named in the installer and signatures")
endif()

# The prefix tools/setup_linux_toolchain.py installs, on Linux and macOS: where
# the libraries the programs link come from, and so what the deploy copies.
if(NOT DEFINED KATANA_TOOLCHAIN AND NOT WIN32 AND NOT "$ENV{KATANA_TOOLCHAIN}" STREQUAL "")
    set(KATANA_TOOLCHAIN "$ENV{KATANA_TOOLCHAIN}" CACHE PATH "The toolchain prefix the programs were built against")
endif()

set(KATANA_EXE "")
if(WIN32)
    set(KATANA_EXE ".exe")
endif()

# On macOS the same tree is an application bundle, Katana.app, laid out as
# code signing and Finder expect: programs in Contents/MacOS, libraries in
# Contents/Frameworks, and everything that is not code - GDAL, PROJ and
# certificate data, the samples - in Contents/Resources. The tree inside
# Contents keeps the relative layout the programs rely on
# (KatanaDeployUnix.cmake.in), with Frameworks for lib/ and Resources for the
# prefix of share/ and ssl/ (katana_core's library_data).
if(APPLE AND DEFINED KATANA_TOOLCHAIN)
    set(KATANA_APP_BUNDLE "Katana.app")
    # The name macOS files the application's settings and permissions under;
    # reverse-DNS by convention, and never changed once released, since a new
    # one is a different application to macOS.
    set(KATANA_BUNDLE_IDENTIFIER "com.jarada.katana" CACHE STRING
        "CFBundleIdentifier of Katana.app")
    # The oldest macOS the bundle says it starts on. 13 was the C++ release's
    # floor, the oldest its Qt supported; the Rust programs have kept it until
    # their own is measured (docs/release.md, "Not done").
    set(KATANA_MACOS_MINIMUM "13.0" CACHE STRING "LSMinimumSystemVersion of Katana.app")
    set(KATANA_INSTALL_BINDIR "${KATANA_APP_BUNDLE}/Contents/MacOS")
    set(KATANA_INSTALL_DATADIR "${KATANA_APP_BUNDLE}/Contents/Resources/share")
else()
    set(KATANA_APP_BUNDLE "")
    set(KATANA_INSTALL_BINDIR "bin")
    set(KATANA_INSTALL_DATADIR "share")
endif()

# The programs installed are these four, by name, and no others. A fifth
# program cargo builds - the icon renderers of the window crate, the update
# tests' stand-ins and their end-to-end driver, which carry a TEST KEY - is
# never installed: a test key in a shipped program would move the trust root
# of every update. tools/check_install_rules.cmake fails on a fifth.
#
# The fourth is the helper that swaps the install's files while Katana is not
# running and puts them back if anything goes wrong (docs/update.md); never run
# from bin/: the engine copies it out first. Not on macOS, where updating is
# check-and-notify only (a file edited inside Katana.app breaks its seal): a
# helper that could not be used would only be one more thing signed and
# notarised.
set(KATANA_PACKAGED_PROGRAMS katana katana_cli katana_mcp)
if(NOT APPLE)
    list(APPEND KATANA_PACKAGED_PROGRAMS katana_updater)
endif()
set(_katana_missing "")
foreach(_katana_program IN LISTS KATANA_PACKAGED_PROGRAMS)
    set(_katana_file "${KATANA_PROGRAMS}/${_katana_program}${KATANA_EXE}")
    if(NOT EXISTS "${_katana_file}")
        list(APPEND _katana_missing "${_katana_file}")
    endif()
    install(PROGRAMS "${_katana_file}" DESTINATION "${KATANA_INSTALL_BINDIR}")
endforeach()
if(_katana_missing)
    list(JOIN _katana_missing "\n  " _katana_missing)
    message(FATAL_ERROR "The programs to package are not built:\n  ${_katana_missing}\n"
                        "Build them first: cargo build --locked --workspace --release")
endif()

# --- the programs are the build this packages ----------------------------------------
#
# What a release must be built with reaches the programs through cargo's
# environment, read by crates/katana_update/build.rs: the update keys
# (cmake/KatanaUpdateKeys.cmake, refused while they are placeholders when
# KATANA_REQUIRE_UPDATE_KEYS is set), the repository an installed copy asks for
# the next version (KATANA_UPDATE_REPO) and the version (project(Katana
# VERSION ...)). A variable cargo was not given is not an error to cargo: the
# program is built without it, and no program says which repository it asks.
# So a configure for a release reads back what the build script wrote - the
# file `output` of its run, under <programs>/build/katana_update-<hash>/, one
# `cargo:rustc-env=NAME=value` a line - and refuses programs that were not
# built with real keys, this version and the repository named here. Every run
# of the script is read, for a target folder may hold one per build of other
# settings: on a developer's machine `cargo clean --release -p katana_update`
# clears the old ones.
option(KATANA_REQUIRE_UPDATE_KEYS
    "Refuse to configure while the update keys are placeholders, or for programs not built with real ones" OFF)
include("${CMAKE_CURRENT_LIST_DIR}/KatanaUpdateKeys.cmake")
set(KATANA_UPDATE_REPO "" CACHE STRING
    "owner/name of the repository the programs were built to ask for the next version (KATANA_UPDATE_REPO in cargo's environment); empty: not checked")
if(KATANA_REQUIRE_UPDATE_KEYS)
    file(GLOB _katana_build_outputs "${KATANA_PROGRAMS}/build/katana_update-*/output")
    if(NOT _katana_build_outputs)
        message(FATAL_ERROR
            "No run of katana_update's build script under ${KATANA_PROGRAMS}/build: the programs there "
            "were not built by cargo in this target folder, and what they were built with cannot be read")
    endif()
    set(_katana_expected
        "cargo:rustc-env=KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS=0"
        "cargo:rustc-env=KATANA_VERSION=${PROJECT_VERSION}")
    if(NOT KATANA_UPDATE_REPO STREQUAL "")
        list(APPEND _katana_expected "cargo:rustc-env=KATANA_UPDATE_REPO=${KATANA_UPDATE_REPO}")
    endif()
    foreach(_katana_output IN LISTS _katana_build_outputs)
        file(STRINGS "${_katana_output}" _katana_lines)
        foreach(_katana_line IN LISTS _katana_expected)
            if(NOT _katana_line IN_LIST _katana_lines)
                message(FATAL_ERROR
                    "${_katana_output} does not say ${_katana_line}: the programs in ${KATANA_PROGRAMS} were "
                    "not built for this release (real update keys, version ${PROJECT_VERSION}, repository "
                    "'${KATANA_UPDATE_REPO}'). Build them with KATANA_REQUIRE_UPDATE_KEYS=ON and "
                    "KATANA_UPDATE_REPO in cargo's environment (docs/release.md, \"Packaging\").")
            endif()
        endforeach()
    endforeach()
    list(LENGTH _katana_build_outputs _katana_count)
    message(STATUS "Katana packaging: the programs were built with real update keys, version "
                   "${PROJECT_VERSION}, repository '${KATANA_UPDATE_REPO}' (${_katana_count} build-script run(s) read)")
endif()

install(DIRECTORY "${KATANA_ROOT}/samples/"
    DESTINATION "${KATANA_INSTALL_DATADIR}/katana/samples"
    # A project directory accumulates backups and caches as it is used; a
    # shipped sample should be the drawing and nothing else.
    PATTERN "backups" EXCLUDE
    PATTERN "cache" EXCLUDE)
# The built-in customisations are COMPILED INTO the programs
# (crates/katana_cad/build.rs embeds them with include_bytes!), so there is
# nothing to install beside them: their linestyles, symbols and survey codes
# travel inside the executable and need no files at run time.
#
# Their SOURCES are a different matter and must never be installed. The files
# they are compiled from (resources/customisation) are tracked, on the owner's
# decision of 2026-10-08, and the reference files they were converted from
# (under docs/) are third-party material kept untracked on the owner's
# machine: both are inputs to the build, not part of the product, and a bundle
# is something that gets handed to other people. No install rule here takes
# anything from resources/ or docs/; tools/check_install_rules.cmake fails if
# one ever does.

# Only files that exist. An install rule naming a missing file is not skipped:
# `cmake --install` stops at it, after the programs are copied and before the
# runtime DLLs are, and leaves a tree that does not start. A top-level file
# this used to install unconditionally was deleted from the repository, and
# every bundle and package failed that way until the rule was made
# conditional. tools/check_install_rules.cmake fails on a rule that names a
# missing file.
foreach(_katana_top_level_file LICENSE)
    if(EXISTS "${KATANA_ROOT}/${_katana_top_level_file}")
        install(FILES "${KATANA_ROOT}/${_katana_top_level_file}" DESTINATION .)
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
    if(EXISTS "${KATANA_ROOT}/${_katana_notice}")
        install(FILES "${KATANA_ROOT}/${_katana_notice}"
            DESTINATION "${KATANA_INSTALL_DATADIR}/katana/licences")
    endif()
endforeach()

# --- runtime dependencies ------------------------------------------------------
#
# Windows: the DLLs the programs import, from the toolchain whose DLLs live
# beside its tools (MSYS2: UCRT64, or CLANGARM64 on ARM64), found by a scan of
# the programs' import tables. KATANA_RUNTIME_BIN is that folder; in an MSYS2
# shell it is $MINGW_PREFIX/bin.
if(WIN32)
    if(NOT DEFINED KATANA_RUNTIME_BIN AND NOT "$ENV{MINGW_PREFIX}" STREQUAL "")
        file(TO_CMAKE_PATH "$ENV{MINGW_PREFIX}/bin" _katana_mingw_bin)
        set(KATANA_RUNTIME_BIN "${_katana_mingw_bin}" CACHE PATH
            "The toolchain folder the programs' DLLs come from (MSYS2: <prefix>/bin)")
    endif()
    find_program(KATANA_OBJDUMP NAMES objdump llvm-objdump HINTS "${KATANA_RUNTIME_BIN}")
    if(NOT KATANA_OBJDUMP)
        message(FATAL_ERROR "The Windows runtime dependency scan needs objdump; set KATANA_OBJDUMP to the toolchain's objdump.")
    endif()
    if(NOT DEFINED KATANA_RUNTIME_BIN)
        get_filename_component(_katana_objdump_bin "${KATANA_OBJDUMP}" DIRECTORY)
        set(KATANA_RUNTIME_BIN "${_katana_objdump_bin}" CACHE PATH
            "The toolchain folder the programs' DLLs come from (MSYS2: <prefix>/bin)")
    endif()
    if(NOT IS_DIRECTORY "${KATANA_RUNTIME_BIN}")
        message(FATAL_ERROR "KATANA_RUNTIME_BIN (${KATANA_RUNTIME_BIN}) is not a folder: name the toolchain's bin/, which holds GDAL's DLL")
    endif()
    list(JOIN KATANA_PACKAGED_PROGRAMS " " KATANA_DEPLOY_PROGRAMS)
    configure_file("${CMAKE_CURRENT_LIST_DIR}/KatanaDeploy.cmake.in"
                   "${CMAKE_BINARY_DIR}/KatanaDeploy.cmake" @ONLY)
    install(SCRIPT "${CMAKE_BINARY_DIR}/KatanaDeploy.cmake")
endif()

# Linux and macOS, when the programs were built against a toolchain prefix of
# their own (tools/setup_linux_toolchain.py): the libraries come from that
# prefix and nowhere else, so an installed tree that did not carry them would
# run only on the machine that built it. KatanaDeployUnix.cmake.in copies them
# into lib/, beside the GDAL, PROJ and certificate data, and points the
# programs' run-time path at that lib/. Programs built against a
# distribution's own libraries leave the dependencies to its package manager.
if(NOT WIN32 AND DEFINED KATANA_TOOLCHAIN)
    set(KATANA_TOOLCHAIN_PREFIX "${KATANA_TOOLCHAIN}")
    if(KATANA_APP_BUNDLE)
        configure_file("${CMAKE_CURRENT_LIST_DIR}/KatanaInfo.plist.in"
                       "${CMAKE_BINARY_DIR}/Info.plist" @ONLY)
    endif()
    configure_file("${CMAKE_CURRENT_LIST_DIR}/KatanaDeployUnix.cmake.in"
                   "${CMAKE_BINARY_DIR}/KatanaDeployUnix.cmake" @ONLY)
    install(SCRIPT "${CMAKE_BINARY_DIR}/KatanaDeployUnix.cmake")
endif()

# Why DLLs beside the program and not static linking: MSYS2 ships PDAL and
# PROJ as DLLs only, and libpdalcpp links the GDAL DLL itself, so a Katana
# with GDAL linked in statically would load a second GDAL through PDAL - two
# driver registries and two error states in one process. A static build means
# building GDAL, PROJ, PDAL and their dependencies from source (docs/building.md,
# "Why DLLs beside the program, and not static linking").
#
# `bundle` is `cmake --install` into the build directory, under a name a person
# can find, cleared first so that a file removed from the install rules does
# not linger in an old bundle.
add_custom_target(bundle
    COMMAND ${CMAKE_COMMAND} -E rm -rf "${CMAKE_BINARY_DIR}/dist/Katana"
    COMMAND ${CMAKE_COMMAND} --install "${CMAKE_BINARY_DIR}"
            --prefix "${CMAKE_BINARY_DIR}/dist/Katana"
    COMMENT "Bundling a self-contained Katana into ${CMAKE_BINARY_DIR}/dist/Katana"
    USES_TERMINAL)

# --- CPack -----------------------------------------------------------------------
set(CPACK_PACKAGE_NAME "Katana")
# The installer shows it as the Publisher in Settings > Apps.
set(CPACK_PACKAGE_VENDOR "${KATANA_PUBLISHER}")
set(CPACK_PACKAGE_VERSION "${PROJECT_VERSION}")
set(CPACK_PACKAGE_DESCRIPTION_SUMMARY "High-performance survey and CAD platform")
set(CPACK_PACKAGE_INSTALL_DIRECTORY "Katana")
set(CPACK_PACKAGE_EXECUTABLES "katana" "Katana")
# Where the packages are written: this build directory, whichever folder cpack
# is started from.
set(CPACK_PACKAGE_DIRECTORY "${CMAKE_BINARY_DIR}")
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
set(CPACK_KATANA_APP_BUNDLE "${KATANA_APP_BUNDLE}")
# The target token: the last part of a package's name (Katana-<version>-<token>.zip), the
# `target` of share/katana/install.json, and the name of the update feed's manifest
# (Katana-update-<token>.json). One place, so a package and its update cannot disagree
# about what they are. The release workflow names it (-DKATANA_PACKAGE_TARGET) as its
# matrix names the package; otherwise it is the machine's.
if(WIN32)
    # win64 is x86-64, as it has been since the first package; ARM64 says so.
    if(CMAKE_SYSTEM_PROCESSOR MATCHES "^(ARM64|arm64|aarch64)$")
        set(_katana_default_target "win-arm64")
    else()
        set(_katana_default_target "win64")
    endif()
elseif(APPLE)
    set(_katana_default_target "macos-${CMAKE_SYSTEM_PROCESSOR}")
else()
    set(_katana_default_target "linux-${CMAKE_SYSTEM_PROCESSOR}")
endif()
set(KATANA_PACKAGE_TARGET "${_katana_default_target}" CACHE STRING
    "The target token of the package and its update (win64, win-arm64, linux-x86_64, linux-aarch64, macos-arm64)")
if(NOT KATANA_PACKAGE_TARGET MATCHES "^(win64|win-arm64|linux-x86_64|linux-aarch64|macos-arm64)$")
    message(FATAL_ERROR "KATANA_PACKAGE_TARGET '${KATANA_PACKAGE_TARGET}' is not a target a release has: "
                        "win64, win-arm64, linux-x86_64, linux-aarch64 or macos-arm64")
endif()
set(KATANA_UPDATE_TARGET "${KATANA_PACKAGE_TARGET}")
set(CPACK_PACKAGE_FILE_NAME "Katana-${PROJECT_VERSION}-${KATANA_UPDATE_TARGET}")
if(WIN32)
    # A ZIP always; an installer when NSIS is there to build one. NSIS is not
    # a dependency of the project, so its absence is not an error - install it
    # with `pacman -S mingw-w64-ucrt-x86_64-nsis` and reconfigure. MSYS2 has
    # no NSIS for ARM64; NSIS's own x86 build, which Windows on ARM runs
    # emulated, makes the same installer - pass -DKATANA_MAKENSIS=<its
    # makensis.exe>. The installer's own code is that NSIS's: x86 for its own
    # build, x86-64 for MSYS2's (cmake/NSIS.template.in says which to make);
    # what it installs is the programs packaged here.
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
        list(APPEND CPACK_MODULE_PATH "${CMAKE_CURRENT_LIST_DIR}")
        # Four backslashes: CPack writes this value into CPackConfig.cmake as it is and CMake reads
        # that file again, so the one backslash NSIS wants must survive a second unescaping (with
        # two, cpack stops at "Invalid character escape '\P'").
        set(CPACK_NSIS_INSTALL_ROOT "$LOCALAPPDATA\\\\Programs")
        # The key the installer writes under HKCU Software\<vendor> and ...\Uninstall, and the
        # updater rewrites DisplayVersion in (crates/katana_update/src/registry.rs): said
        # here rather than left to CPack's default, which is the install directory's name, so
        # that renaming one cannot move the other.
        set(CPACK_PACKAGE_INSTALL_REGISTRY_KEY "Katana")
        # The icon Settings > Apps shows, said with a backslash like the rest of the path (the
        # same four as above).
        set(CPACK_NSIS_INSTALLED_ICON_NAME "bin\\\\katana.exe")
        set(CPACK_NSIS_ENABLE_UNINSTALL_BEFORE_INSTALL ON)
        # The Desktop shortcut, beside the Start menu one CPACK_PACKAGE_EXECUTABLES makes: the same
        # program (bin\katana.exe), so the same icon - the shortcut names none, and Windows takes
        # the program's own - and the install folder as the folder it starts in (a shortcut starts
        # in the installer's current output folder, hence the SetOutPath). Per user, as the rest:
        # $DESKTOP is the Desktop of the account that runs the installer, because the template
        # sets the current user's shell context. The template puts these two values inside the
        # Start menu page's own block, so a person who ticks "Do not create shortcuts" gets no
        # Desktop one either, and no wizard page of its own is added; the person who does not
        # want it deletes the file, and nothing puts it back but an install (an update replaces
        # files in the install folder and never the shortcut, which is not one of them:
        # UPDATE_EXCLUDE and the manifest are unchanged). docs/release.md, "The Windows
        # installer", says it too. Two things about how the text is written. Single quotes, which
        # NSIS takes as it takes double ones, because CPack writes the value into
        # CPackConfig.cmake inside double quotes and a double quote in it would end the string
        # there. And doubled backslashes, four as at CPACK_NSIS_INSTALL_ROOT above: CMake reads
        # that file again, and the single backslash NSIS wants must survive the second
        # unescaping. The line break is \n and not a line of this file, so that it does not
        # depend on this file's line ends.
        set(CPACK_NSIS_CREATE_ICONS_EXTRA
            "  SetOutPath '$INSTDIR'\n  CreateShortCut '$DESKTOP\\\\Katana.lnk' '$INSTDIR\\\\bin\\\\katana.exe'")
        set(CPACK_NSIS_DELETE_ICONS_EXTRA "  Delete '$DESKTOP\\\\Katana.lnk'")
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
        # result.
        if(EXISTS "${KATANA_ROOT}/LICENSE" AND EXISTS "${KATANA_ROOT}/ADDITIONAL_TERMS.md")
            set(_katana_licence_page "${CMAKE_BINARY_DIR}/KatanaInstallerLicence.txt")
            file(READ "${KATANA_ROOT}/LICENSE" _katana_gpl)
            file(READ "${KATANA_ROOT}/ADDITIONAL_TERMS.md" _katana_terms)
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
                "${KATANA_ROOT}/LICENSE" "${KATANA_ROOT}/ADDITIONAL_TERMS.md")
        endif()
        if(EXISTS "${KATANA_ROOT}/resources/katana.ico")
            set(CPACK_NSIS_MUI_ICON "${KATANA_ROOT}/resources/katana.ico")
            set(CPACK_NSIS_MUI_UNIICON "${KATANA_ROOT}/resources/katana.ico")
        endif()
    endif()
elseif(APPLE AND KATANA_APP_BUNDLE)
    # A disk image holding Katana.app beside a link to /Applications, the
    # drag-to-install a Mac user expects (CPack's DragNDrop adds the link).
    set(CPACK_GENERATOR "DragNDrop")
    set(CPACK_DMG_VOLUME_NAME "Katana ${PROJECT_VERSION}")
    set(CPACK_DMG_FORMAT "UDZO")
else()
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
# `cmake --install`, the `bundle` target and cargo's target folder are development
# trees and stay without it: an update there would replace a developer's build with a
# release. tools/check_install_rules.cmake asserts both halves (no install rule names
# the file; CPackConfig.cmake runs this script before the signing script).
#
# The target is the token above and not the machine's: Windows on ARM running the x64 zip
# is win64 and must be updated with the x64 files. The script is written here, at
# configure time, because the token and the data directory are known only here.
#
# The version is PROJECT_VERSION, always: the version these programs were built as. The
# engine takes it for the copy's own build whichever program asks (katana_update's
# install.rs, package_build): the helper that reads an install may be a later release's, and
# its own version is then not the install's. It is also the floor a stale installed.json
# cannot lower, where a newer zip was unpacked by hand over a copy that had updated itself.
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
# (katana_update's manifest.rs) - so that a package cannot name a sequence no manifest could
# carry, above every release of its own version for good. Digits alone, because the text
# goes into the record as a JSON number as it is: a sign, a fraction, an exponent or a
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
    # 2 to 4 components of digits, as the engine reads a version (katana_update's version.rs):
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
    # texts are the two shapes drawn under "install.json" above, written out by hand.
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
    # is when the rule is applied, and reported by an echo test.
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
