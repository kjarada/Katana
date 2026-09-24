katana_provide_benchmark()

# Sources are globbed per module so that a module can add benchmarks without
# touching this file. CONFIGURE_DEPENDS re-globs when files appear.
file(GLOB _katana_bench_sources CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/bench_*.cpp")

add_executable(katana_benchmarks ${_katana_bench_sources})
target_link_libraries(katana_benchmarks PRIVATE benchmark::benchmark_main)
foreach(_lib katana_math katana_geometry katana_entity katana_commands katana_survey katana_terrain katana_cad katana_storage)
    if(TARGET ${_lib})
        target_link_libraries(katana_benchmarks PRIVATE ${_lib})
    endif()
endforeach()
katana_target_defaults(katana_benchmarks)
set_target_properties(katana_benchmarks PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${KATANA_BENCHMARK_OUTPUT_DIRECTORY}")

# `cmake --build <dir> --target run-benchmarks`
add_custom_target(run-benchmarks
    COMMAND ${CMAKE_COMMAND} -E env "PATH=${KATANA_RUNTIME_BIN};$ENV{PATH}"
            $<TARGET_FILE:katana_benchmarks> --benchmark_min_time=0.2s
    DEPENDS katana_benchmarks
    USES_TERMINAL)

# The GPU renderer's frame time (benchmarks/gpu): an executable of its own,
# because it needs a QGuiApplication and Qt's private QRhi headers, which the
# plain benchmarks above must not depend on.
if(TARGET katana_gpu)
    add_subdirectory(gpu)
endif()
