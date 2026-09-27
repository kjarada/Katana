# Installing, bundling and packaging.
#
#   cmake --build build/release --target bundle     -> build/release/dist/Katana/
#   cmake --build build/release --target package    -> Katana-<version>-win64.zip
#                                     and .exe, -win-arm64.zip and .exe,
#                                     -linux-x86_64.tar.gz, -linux-aarch64.tar.gz,
#                                     -macos-arm64.dmg, -macos-x86_64.dmg
#   cmake --install build/release --prefix <dir>    -> the same tree, anywhere
#
# The installed tree is SELF-CONTAINED: it runs on a machine with no MSYS2, no
# Qt and no GDAL installed. That is the whole point of it, and it is what the
# build tree cannot do - build/<config>/bin/katana.exe finds its DLLs only
# because the toolchain's bin directory is on PATH.
#
#   Katana/
#     bin/katana.exe, katana_cli.exe,    the programs
#         katana_mcp.exe
#     bin/*.dll                          Qt, GDAL, PDAL, PROJ, the C++ runtime
#     bin/platforms, styles, ...         Qt plugins, where Qt looks for them
#     share/proj, share/gdal             proj.db and the GDAL support files
#     etc/ssl/certs/ca-bundle.crt        what libcurl checks https:// against
#     share/katana/samples               a project to open
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

install(DIRECTORY "${PROJECT_SOURCE_DIR}/samples/"
    DESTINATION "${KATANA_INSTALL_DATADIR}/katana/samples"
    # A project directory accumulates backups and caches as it is used; a
    # shipped sample should be the drawing and nothing else.
    PATTERN "backups" EXCLUDE
    PATTERN "cache" EXCLUDE)
# The built-in customisation is COMPILED INTO the binary (see
# src/katana_archive12d/CMakeLists.txt and tools/embed_customisation.py), so
# there is nothing to install beside it: its linestyles, symbols and survey
# codes travel inside the executable and need no files at run time.
#
# Its SOURCES are a different matter and must never be installed. The folders
# it is compiled from (resources/customisation, and the reference files under
# docs/) are third-party material kept untracked on the owner's machine: they
# are inputs to the build, not part of the product, and a bundle is something
# that gets handed to other people. No install rule here takes anything from
# resources/ or docs/; the test packaging_installs_only_present_first_party_files
# (tools/check_install_rules.cmake) fails if one ever does.

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
foreach(_target katana katana_cli)
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
# (cmake/KatanaSign.cmake, docs/release.md "Signing").
set(CPACK_PRE_BUILD_SCRIPTS "${CMAKE_CURRENT_LIST_DIR}/KatanaSign.cmake")
set(CPACK_POST_BUILD_SCRIPTS "${CMAKE_CURRENT_LIST_DIR}/KatanaSign.cmake")
set(CPACK_KATANA_PUBLISHER "${KATANA_PUBLISHER}")
set(CPACK_KATANA_OBJDUMP "${CMAKE_OBJDUMP}")
set(CPACK_KATANA_APP_BUNDLE "${KATANA_APP_BUNDLE}")
if(WIN32)
    # win64 is x86-64, as it has been since the first package; ARM64 says so.
    if(CMAKE_SYSTEM_PROCESSOR MATCHES "^(ARM64|arm64|aarch64)$")
        set(CPACK_PACKAGE_FILE_NAME "Katana-${PROJECT_VERSION}-win-arm64")
    else()
        set(CPACK_PACKAGE_FILE_NAME "Katana-${PROJECT_VERSION}-win64")
    endif()
    # A ZIP always; an installer when NSIS is there to build one. NSIS is not
    # a dependency of the project, so its absence is not an error - install it
    # with `pacman -S mingw-w64-ucrt-x86_64-nsis` and reconfigure. MSYS2 has
    # no NSIS for ARM64; NSIS's own x86 build, which Windows on ARM runs
    # emulated, makes the same installer - pass -DKATANA_MAKENSIS=<its
    # makensis.exe>. The installer's own code is x86 either way; what it
    # installs is the programs built here.
    set(CPACK_GENERATOR "ZIP")
    find_program(KATANA_MAKENSIS NAMES makensis HINTS "${KATANA_RUNTIME_BIN}")
    if(KATANA_MAKENSIS)
        list(APPEND CPACK_GENERATOR "NSIS")
        # The makensis found here, not one CPack finds for itself elsewhere.
        set(CPACK_NSIS_EXECUTABLE "${KATANA_MAKENSIS}")
        set(CPACK_NSIS_DISPLAY_NAME "Katana ${PROJECT_VERSION}")
        set(CPACK_NSIS_PACKAGE_NAME "Katana")
        # C:\Program Files, not NSIS's default of Program Files (x86): these
        # are 64-bit programs.
        set(CPACK_NSIS_INSTALL_ROOT "$PROGRAMFILES64")
        set(CPACK_NSIS_INSTALLED_ICON_NAME "bin/katana.exe")
        set(CPACK_NSIS_ENABLE_UNINSTALL_BEFORE_INSTALL ON)
        if(EXISTS "${PROJECT_SOURCE_DIR}/resources/katana.ico")
            set(CPACK_NSIS_MUI_ICON "${PROJECT_SOURCE_DIR}/resources/katana.ico")
            set(CPACK_NSIS_MUI_UNIICON "${PROJECT_SOURCE_DIR}/resources/katana.ico")
        endif()
    endif()
elseif(APPLE)
    set(CPACK_PACKAGE_FILE_NAME "Katana-${PROJECT_VERSION}-macos-${CMAKE_SYSTEM_PROCESSOR}")
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
    set(CPACK_PACKAGE_FILE_NAME "Katana-${PROJECT_VERSION}-linux-${CMAKE_SYSTEM_PROCESSOR}")
    set(CPACK_GENERATOR "TGZ")
endif()
include(CPack)
