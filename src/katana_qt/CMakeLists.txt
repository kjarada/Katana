# PrintSupport: the sheets are printed as well as plotted to files
# (plotting/plot_output.hpp, printSheets).
find_package(Qt6 REQUIRED COMPONENTS Widgets PrintSupport)

# The desktop application's Import/Export and its Reference Data panel are not
# optional features, so the module they need is a hard requirement. Failing here
# with a clear message beats a wall of undefined references.
if(NOT TARGET katana_interop)
    message(FATAL_ERROR
        "The Katana desktop application requires katana_interop; configure with -DKATANA_BUILD_IO=ON "
        "or turn the Qt app off with -DKATANA_BUILD_QT_APP=OFF")
endif()
# File > Import IFC and Export IFC are the application's too (docs/ifc.md).
if(NOT TARGET katana_ifc)
    message(FATAL_ERROR
        "The Katana desktop application requires katana_ifc; leave it in KATANA_MODULE_FILTER "
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
# Not MACOSX_BUNDLE: on macOS the program is bin/katana, as on Linux, so the
# headless checks, the tests and the bundle (KatanaDeployUnix.cmake.in) find
# it where they find it everywhere else. A signed Katana.app is a packaging
# project of its own (docs/release.md, "Not done").
#
# No Q_OBJECT classes: signals are connected to lambdas, so moc is not needed.
add_executable(katana WIN32
    main.cpp
    command_reference_dialog.cpp
    command_reference_dialog.hpp
    command_runner.hpp
    main_window.cpp
    main_window.hpp
    main_window_dxf.cpp
    main_window_ifc.cpp
    ifc_dialogs.cpp
    ifc_dialogs.hpp
    viewport_widget.cpp
    plan_painter.cpp
    plan_painter.hpp
    sheet_painter.cpp
    sheet_painter.hpp
    sheet_editor.cpp
    sheet_editor.hpp
    viewport_widget.hpp
    view_workspace.cpp
    view_workspace.hpp
    dock_chrome.cpp
    dock_chrome.hpp
    view_layers_popup.cpp
    view_layers_popup.hpp
    plan_context_menu.cpp
    plan_context_menu.hpp
    select_by_id_dialog.cpp
    select_by_id_dialog.hpp
    render_view_widget.cpp
    render_view_widget.hpp
    section_view_widget.cpp
    section_view_widget.hpp
    alignment_manager.cpp
    alignment_manager.hpp
    command_word.hpp
    style_manager.cpp
    style_manager.hpp
    attribute_manager.cpp
    attribute_manager.hpp
    layer_manager.cpp
    layer_manager.hpp
    project_crs_dialog.cpp
    project_crs_dialog.hpp
    property_panel.cpp
    property_panel.hpp
    script_runner.cpp
    script_runner.hpp
    gis_dialogs.cpp
    gis_dialogs.hpp
    import_placement.cpp
    import_placement.hpp
    gis_online.cpp
    gis_online.hpp
    gis_online_dialog.cpp
    gis_online_dialog.hpp
    format.hpp
    icons.cpp
    icons.hpp
    theme.cpp
    theme.hpp
    jobs.cpp
    jobs.hpp
    keyboard_shortcuts_dialog.cpp
    keyboard_shortcuts_dialog.hpp
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
# The sheets' dialogs, panels and output (docs/plotting.md), beside the sheet
# painter and editor: globbed, one directory, several authors at once.
file(GLOB _katana_plotting_ui CONFIGURE_DEPENDS
     "${CMAKE_CURRENT_SOURCE_DIR}/plotting/*.cpp" "${CMAKE_CURRENT_SOURCE_DIR}/plotting/*.hpp")
target_sources(katana PRIVATE ${_katana_plotting_ui})
# The annotation front end (docs/annotation.md): the painter of a laid-out
# annotation and the text and label style managers, thin over katana_cad's
# annotation code. Globbed, one directory, like the sheets.
file(GLOB _katana_annotation_ui CONFIGURE_DEPENDS
     "${CMAKE_CURRENT_SOURCE_DIR}/annotation/*.cpp" "${CMAKE_CURRENT_SOURCE_DIR}/annotation/*.hpp")
target_sources(katana PRIVATE ${_katana_annotation_ui})
# The drawing system's front end (docs/drawing.md): the grips' gesture, the
# Vertices panel and the drafting settings bar, thin over katana_cad's
# drawing code. Globbed, one directory, like the sheets and annotation.
file(GLOB _katana_drawing_ui CONFIGURE_DEPENDS
     "${CMAKE_CURRENT_SOURCE_DIR}/drawing/*.cpp" "${CMAKE_CURRENT_SOURCE_DIR}/drawing/*.hpp")
target_sources(katana PRIVATE ${_katana_drawing_ui})
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
target_link_libraries(katana PRIVATE katana_cad katana_interop katana_dxf katana_ifc katana_render
                                     Qt6::Widgets Qt6::PrintSupport)

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
# docs/gpu.md), switched by KATANA_GPU. RenderViewWidget hosts it, with the
# software rasteriser as the fallback, when it is built (KATANA_HAS_GPU).
add_subdirectory(gpu)
if(TARGET katana_gpu)
    target_link_libraries(katana PRIVATE katana_gpu)
endif()
