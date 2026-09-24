# GPU frame time against the software rasteriser on the same scenes
# (bench_gpu_render.cpp, docs/gpu.md "Measurements").
add_executable(katana_gpu_benchmarks bench_gpu_render.cpp)
target_link_libraries(katana_gpu_benchmarks PRIVATE
    katana_gpu katana_render katana_cad katana_interop benchmark::benchmark)
katana_target_defaults(katana_gpu_benchmarks)
set_target_properties(katana_gpu_benchmarks PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${KATANA_BENCHMARK_OUTPUT_DIRECTORY}")
