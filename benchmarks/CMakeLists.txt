katana_provide_benchmark()

# Sources are globbed per module so that a module can add benchmarks without
# touching this file. CONFIGURE_DEPENDS re-globs when files appear.
file(GLOB _katana_bench_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/bench_*.cpp")
# The export benchmarks need GDAL, which a -DKATANA_BUILD_IO=OFF build has not.
if(NOT TARGET katana_interop)
    list(FILTER _katana_bench_sources EXCLUDE REGEX "bench_interop[.]cpp$")
endif()

add_executable(katana_benchmarks ${_katana_bench_sources})
target_link_libraries(katana_benchmarks PRIVATE benchmark::benchmark_main)
foreach(_lib katana_math katana_geometry katana_entity katana_commands katana_survey katana_terrain katana_cad katana_storage katana_interop katana_archive12d katana_dxf)
    if(TARGET ${_lib})
        target_link_libraries(katana_benchmarks PRIVATE ${_lib})
    endif()
endforeach()
# bench_dxf.cpp measures GDAL's DXF driver beside the native one, where GDAL
# is built.
if(TARGET katana_interop)
    target_link_libraries(katana_benchmarks PRIVATE katana_interop)
    target_compile_definitions(katana_benchmarks PRIVATE KATANA_BENCH_WITH_INTEROP=1)
endif()
# bench_customisation.cpp measures a generated customisation and, where the
# build has one to compile in, that very file: the one KATANA_BUILTIN_CUSTOMISATION
# names, or resources/customisation/nsw.customisation.json when nothing names
# one. The file is tracked, so a default build measures both, and one given no
# file measures the generated one alone. Globbed with
# CONFIGURE_DEPENDS so that a file converted after the configure is found by
# the next build.
set(_katana_builtin_customisation "${KATANA_BUILTIN_CUSTOMISATION}")
if(NOT _katana_builtin_customisation)
    set(_katana_builtin_customisation
        "${PROJECT_SOURCE_DIR}/resources/customisation/nsw.customisation.json")
endif()
file(GLOB _katana_builtin_customisation_found CONFIGURE_DEPENDS "${_katana_builtin_customisation}")
if(_katana_builtin_customisation_found)
    target_compile_definitions(katana_benchmarks PRIVATE
        KATANA_BENCH_BUILTIN_CUSTOMISATION="${_katana_builtin_customisation}")
endif()
katana_target_defaults(katana_benchmarks)
set_target_properties(katana_benchmarks PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${KATANA_BENCHMARK_OUTPUT_DIRECTORY}")

# `cmake --build <dir> --target run-benchmarks`
add_custom_target(run-benchmarks
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_benchmarks> --benchmark_min_time=0.2s
    DEPENDS katana_benchmarks
    USES_TERMINAL)

# The plan view's paint and plot (benchmarks/qt/), which need Qt and the
# application's sources: only where the application is built.
if(KATANA_BUILD_QT_APP AND TARGET katana)
    add_subdirectory(qt)
endif()

# The GPU renderer's frame time (benchmarks/gpu): an executable of its own,
# because it needs a QGuiApplication and Qt's private QRhi headers, which the
# plain benchmarks above must not depend on.
if(TARGET katana_gpu)
    add_subdirectory(gpu)
endif()
