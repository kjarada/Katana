find_package(Qt6 REQUIRED COMPONENTS Widgets)

# The desktop application's Import/Export and its Reference Data panel are not
# optional features, so the module they need is a hard requirement. Failing here
# with a clear message beats a wall of undefined references.
if(NOT TARGET katana_interop)
    message(FATAL_ERROR
        "The Katana desktop application requires katana_interop; configure with -DKATANA_BUILD_IO=ON "
        "or turn the Qt app off with -DKATANA_BUILD_QT_APP=OFF")
endif()

# The target is `katana` and the file is katana.exe: this IS the product, and
# "katana_qt_app" was a name for the people building it. The directory keeps
# the name of its LAYER (`qt`, see tools/check_layering.cmake), which is a
# statement about dependencies and not about what the user double-clicks.
#
# WIN32 means no console window opens behind the application. The cost is
# that stderr goes nowhere when it is started from cmd.exe, so the --plot
# switch reports failure through its exit code as well as its message.
#
# No Q_OBJECT classes: signals are connected to lambdas, so moc is not needed.
add_executable(katana WIN32 MACOSX_BUNDLE
    main.cpp
    main_window.cpp
    main_window.hpp
    main_window_dxf.cpp
    viewport_widget.cpp
    plan_painter.cpp
    plan_painter.hpp
    viewport_widget.hpp
    view_workspace.cpp
    view_workspace.hpp
    dock_chrome.cpp
    dock_chrome.hpp
    view_layers_popup.cpp
    view_layers_popup.hpp
    render_view_widget.cpp
    render_view_widget.hpp
    section_view_widget.cpp
    section_view_widget.hpp
    style_manager.cpp
    style_manager.hpp
    attribute_manager.cpp
    attribute_manager.hpp
    layer_manager.cpp
    layer_manager.hpp
    gis_dialogs.cpp
    gis_dialogs.hpp
    format.hpp
    icons.cpp
    icons.hpp
    theme.cpp
    theme.hpp
    jobs.cpp
    jobs.hpp
)
katana_target_defaults(katana)
# The Survey workbench (PLAN.MD 45 slice 9) is globbed, for the reason
# src/katana_surveyio is: its dialogs, wizard and panels are written by several
# authors at once, and an explicit list here would make each of them edit this
# one shared file. The directory is on the include path so that a file there
# includes the window's headers ("icons.hpp") the way the files beside them do.
file(GLOB _katana_survey_ui CONFIGURE_DEPENDS
     "${CMAKE_CURRENT_SOURCE_DIR}/survey/*.cpp" "${CMAKE_CURRENT_SOURCE_DIR}/survey/*.hpp")
target_sources(katana PRIVATE ${_katana_survey_ui})
target_include_directories(katana PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(katana PRIVATE katana_survey katana_surveyio katana_geodesy)
# The interactive tools' icons (tools/icons_<family>.cpp) and, later, the menus
# built from the tool catalogue: globbed for the same reason, one file per tool
# family, each written by that family's author.
file(GLOB _katana_tool_ui CONFIGURE_DEPENDS
     "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.cpp" "${CMAKE_CURRENT_SOURCE_DIR}/tools/*.hpp")
target_sources(katana PRIVATE ${_katana_tool_ui})
# The customisation managers (styles and linestyles, the symbol library, the
# survey code table) and what they share (the definition painter, pickers,
# thumbnails): globbed, one directory, several authors at once.
file(GLOB _katana_customisation_ui CONFIGURE_DEPENDS
     "${CMAKE_CURRENT_SOURCE_DIR}/customisation/*.cpp" "${CMAKE_CURRENT_SOURCE_DIR}/customisation/*.hpp")
target_sources(katana PRIVATE ${_katana_customisation_ui})
# One place a version is written: project(Katana VERSION ...). The About box
# reads it from here and the Windows resource below from the same variable.
target_compile_definitions(katana PRIVATE KATANA_VERSION="${PROJECT_VERSION}")

# The executable's own icon and version block (see katana.rc.in). The .ico is
# committed, not generated - make_icons.cpp says why.
if(WIN32)
    enable_language(RC)
    set(KATANA_ICON_FILE "${PROJECT_SOURCE_DIR}/resources/katana.ico")
    if(NOT EXISTS "${KATANA_ICON_FILE}")
        message(FATAL_ERROR
            "resources/katana.ico is missing. Regenerate it with:
"
            "  cmake --build <build> --target katana_make_icons
"
            "  <build>/bin/katana_make_icons resources")
    endif()
    configure_file("${CMAKE_CURRENT_SOURCE_DIR}/katana.rc.in"
                   "${CMAKE_CURRENT_BINARY_DIR}/katana.rc" @ONLY)
    target_sources(katana PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/katana.rc")
endif()
target_link_libraries(katana PRIVATE katana_cad katana_interop katana_dxf katana_render Qt6::Widgets)

# The developer tool that renders the icons to files - see make_icons.cpp for
# why its output is committed rather than generated during the build. Not part
# of `all`: nothing in the product depends on it.
add_executable(katana_make_icons EXCLUDE_FROM_ALL make_icons.cpp icons.cpp theme.cpp)
katana_target_defaults(katana_make_icons)
target_link_libraries(katana_make_icons PRIVATE Qt6::Widgets)

# A developer tool, like katana_make_icons: every catalogue tool's icon at two
# sizes with its name and id, so a tool's author can see the icon without
# launching the application. Not part of `all`.
add_executable(katana_tool_icon_sheet EXCLUDE_FROM_ALL tool_icon_sheet.cpp theme.cpp
               ${_katana_tool_ui})
katana_target_defaults(katana_tool_icon_sheet)
target_include_directories(katana_tool_icon_sheet PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(katana_tool_icon_sheet PRIVATE katana_cad Qt6::Widgets)

# The GPU renderer for the 3D view: a library of its own (gpu/CMakeLists.txt,
# docs/gpu.md), switched by KATANA_GPU. Not linked into `katana` yet - a later
# change hosts it inside the 3D view with the software rasteriser as the
# fallback - so today its tests (tests/gpu) and benchmark (benchmarks/gpu)
# are what build it.
add_subdirectory(gpu)
