find_package(Qt6 REQUIRED COMPONENTS Widgets)

# The desktop application's Import/Export and its Reference Data panel are not
# optional features, so the module they need is a hard requirement. Failing here
# with a clear message beats a wall of undefined references.
if(NOT TARGET katana_interop)
    message(FATAL_ERROR
        "katana_qt_app requires katana_interop; configure with -DKATANA_BUILD_IO=ON "
        "or turn the Qt app off with -DKATANA_BUILD_QT_APP=OFF")
endif()

# No Q_OBJECT classes: signals are connected to lambdas, so moc is not needed.
add_executable(katana_qt_app WIN32 MACOSX_BUNDLE
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
)
katana_target_defaults(katana_qt_app)
target_link_libraries(katana_qt_app PRIVATE katana_cad katana_interop katana_render Qt6::Widgets)
