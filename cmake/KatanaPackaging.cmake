# Installing, bundling and packaging.
#
#   cmake --build build/release --target bundle     -> build/release/dist/Katana/
#   cmake --build build/release --target package    -> Katana-<version>-win64.zip
#   cmake --install build/release --prefix <dir>    -> the same tree, anywhere
#
# The installed tree is SELF-CONTAINED: it runs on a machine with no MSYS2, no
# Qt and no GDAL installed. That is the whole point of it, and it is what the
# build tree cannot do - build/<config>/bin/katana.exe finds its DLLs only
# because the toolchain's bin directory is on PATH.
#
#   Katana/
#     bin/katana.exe, katana_cli.exe     the programs
#     bin/*.dll                          Qt, GDAL, PDAL, PROJ, the C++ runtime
#     bin/platforms, styles, ...         Qt plugins, where Qt looks for them
#     share/proj, share/gdal             proj.db and the GDAL support files
#     share/katana/samples               a project to open
#
# bin/ beside share/ is not arbitrary: PROJ looks for its database at
# <directory of the PROJ DLL>/../share/proj, so this layout is found without
# any configuration, and interop::useBundledData points GDAL at share/gdal.

if(TARGET katana)
    install(TARGETS katana
        RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
        BUNDLE DESTINATION .)
endif()
if(TARGET katana_cli)
    install(TARGETS katana_cli RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
endif()

install(DIRECTORY "${PROJECT_SOURCE_DIR}/samples/"
    DESTINATION "${CMAKE_INSTALL_DATADIR}/katana/samples"
    # A project directory accumulates backups and caches as it is used; a
    # shipped sample should be the drawing and nothing else.
    PATTERN "backups" EXCLUDE
    PATTERN "cache" EXCLUDE)
# The 12d customisation is COMPILED INTO the binary (see
# src/katana_archive12d/CMakeLists.txt and tools/embed_customisation.py), so
# there is nothing to install beside it: its linestyles, symbols and survey
# codes travel inside the executable and need no files at run time.

install(FILES "${PROJECT_SOURCE_DIR}/README.md" DESTINATION .)
if(EXISTS "${PROJECT_SOURCE_DIR}/LICENSE")
    install(FILES "${PROJECT_SOURCE_DIR}/LICENSE" DESTINATION .)
endif()

# --- runtime dependencies ------------------------------------------------------
#
# Windows only, and only for a toolchain whose DLLs live beside its compiler
# (MSYS2 / MinGW). On Linux the distribution's package manager owns the
# dependencies, and bundling them would fight it.
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
endif()

# --- CPack -----------------------------------------------------------------------
set(CPACK_PACKAGE_NAME "Katana")
set(CPACK_PACKAGE_VENDOR "Katana")
set(CPACK_PACKAGE_VERSION "${PROJECT_VERSION}")
set(CPACK_PACKAGE_DESCRIPTION_SUMMARY "High-performance survey and CAD platform")
set(CPACK_PACKAGE_INSTALL_DIRECTORY "Katana")
set(CPACK_PACKAGE_EXECUTABLES "katana" "Katana")
if(WIN32)
    set(CPACK_PACKAGE_FILE_NAME "Katana-${PROJECT_VERSION}-win64")
    # A ZIP always; an installer when NSIS is there to build one. NSIS is not
    # a dependency of the project, so its absence is not an error - install it
    # with `pacman -S mingw-w64-ucrt-x86_64-nsis` and reconfigure.
    set(CPACK_GENERATOR "ZIP")
    find_program(KATANA_MAKENSIS NAMES makensis HINTS "${KATANA_RUNTIME_BIN}")
    if(KATANA_MAKENSIS)
        list(APPEND CPACK_GENERATOR "NSIS")
        set(CPACK_NSIS_DISPLAY_NAME "Katana ${PROJECT_VERSION}")
        set(CPACK_NSIS_PACKAGE_NAME "Katana")
        set(CPACK_NSIS_INSTALLED_ICON_NAME "bin/katana.exe")
        set(CPACK_NSIS_ENABLE_UNINSTALL_BEFORE_INSTALL ON)
        if(EXISTS "${PROJECT_SOURCE_DIR}/resources/katana.ico")
            set(CPACK_NSIS_MUI_ICON "${PROJECT_SOURCE_DIR}/resources/katana.ico")
            set(CPACK_NSIS_MUI_UNIICON "${PROJECT_SOURCE_DIR}/resources/katana.ico")
        endif()
    endif()
else()
    set(CPACK_GENERATOR "TGZ")
endif()
include(CPack)
