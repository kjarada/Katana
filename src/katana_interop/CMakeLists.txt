# Import and export between external formats and the domain model
# (PLAN.MD Phase 20). This is the ONLY layer that sees both the external readers
# (katana_gis / katana_pointcloud, implemented by katana_io) and the entity
# model, which is what keeps GDAL and PDAL out of katana_cad.
add_library(katana_interop STATIC
    reference_data.cpp
    import.cpp
    export.cpp
    archive12d.cpp
    dataset_info.cpp
    terrain_io.cpp
)
# GIS > Online Data and the ONLINE verbs (docs/gis_online.md): the provider
# catalogue, the request builders, capabilities discovery and the fetcher,
# globbed so that the files can grow without editing this list.
file(GLOB _interop_online_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/online_*.cpp")
target_sources(katana_interop PRIVATE ${_interop_online_sources})
# The built-in catalogue is compiled in with #embed (online_catalogue.cpp)
# from the committed resources/online/, as the plot frame is, so the list of
# services is part of the program and a missing file is a build error.
set_source_files_properties("${CMAKE_CURRENT_SOURCE_DIR}/online_catalogue.cpp" PROPERTIES
    COMPILE_OPTIONS "--embed-dir=${PROJECT_SOURCE_DIR}/resources/online")
katana_target_defaults(katana_interop)
# katana_terrain is PUBLIC and named although katana_archive12d brings it too:
# terrain_io.hpp includes tin_surface.hpp, and a dependency a public header
# has should not rest on another library happening to have it.
target_link_libraries(katana_interop
    PUBLIC katana_core katana_geometry katana_terrain katana_entity katana_archive12d katana_io
    PRIVATE katana_commands
)
find_package(nlohmann_json CONFIG REQUIRED)
target_link_libraries(katana_interop PRIVATE nlohmann_json::nlohmann_json)
