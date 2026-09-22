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
    viewport_widget.cpp
    viewport_widget.hpp
    viewport_container.cpp
    viewport_container.hpp
    render_view_widget.cpp
    render_view_widget.hpp
    section_view_widget.cpp
    section_view_widget.hpp
    style_manager.cpp
    style_manager.hpp
    attribute_manager.cpp
    attribute_manager.hpp
    icons.cpp
    icons.hpp
    theme.cpp
    theme.hpp
)
katana_target_defaults(katana)
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
target_link_libraries(katana PRIVATE katana_cad katana_interop katana_render Qt6::Widgets)

# The developer tool that renders the icons to files - see make_icons.cpp for
# why its output is committed rather than generated during the build. Not part
# of `all`: nothing in the product depends on it.
add_executable(katana_make_icons EXCLUDE_FROM_ALL make_icons.cpp icons.cpp theme.cpp)
katana_target_defaults(katana_make_icons)
target_link_libraries(katana_make_icons PRIVATE Qt6::Widgets)

