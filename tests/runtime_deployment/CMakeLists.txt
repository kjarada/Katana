cmake_minimum_required(VERSION 3.24)
project(KatanaRuntimeFixture LANGUAGES CXX)

option(KATANA_DEPLOY_RUNTIME "Deploy the fixture's runtime" ON)
include("${DEPLOY_CMAKE_DIR}/KatanaTargetDefaults.cmake")
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY_RELEASE "${CMAKE_BINARY_DIR}/bin")
set(KATANA_RUNTIME_BIN "${CMAKE_BINARY_DIR}/toolchain/bin")

file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/leaf.cpp"
    "extern \"C\" __declspec(dllexport) int leaf() { return 41; }\n")
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/middle.cpp"
    "extern \"C\" __declspec(dllimport) int leaf();\n"
    "extern \"C\" __declspec(dllexport) int middle() { return leaf() + 1; }\n")
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/extra.cpp"
    "extern \"C\" __declspec(dllexport) int extra() { return 7; }\n")
file(COPY_FILE "${CMAKE_CURRENT_BINARY_DIR}/extra.cpp" "${CMAKE_CURRENT_BINARY_DIR}/later.cpp")
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/cli.cpp"
    "extern \"C\" __declspec(dllimport) int middle();\n"
    "int main() { return middle() == 42 ? 0 : 1; }\n")
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/mcp.cpp"
    "extern \"C\" __declspec(dllimport) int extra();\n"
    "int main() { return extra() == 7 ? 0 : 1; }\n")

foreach(_library leaf middle extra later)
    add_library(runtime_${_library} SHARED "${CMAKE_CURRENT_BINARY_DIR}/${_library}.cpp")
    set_target_properties(runtime_${_library} PROPERTIES
        PREFIX "" OUTPUT_NAME "katana_runtime_${_library}"
        RUNTIME_OUTPUT_DIRECTORY "${KATANA_RUNTIME_BIN}"
        RUNTIME_OUTPUT_DIRECTORY_RELEASE "${KATANA_RUNTIME_BIN}")
    target_link_options(runtime_${_library} PRIVATE -static-libgcc -static-libstdc++)
endforeach()
target_link_libraries(runtime_middle PRIVATE runtime_leaf)

add_executable(katana_cli "${CMAKE_CURRENT_BINARY_DIR}/cli.cpp")
target_link_libraries(katana_cli PRIVATE runtime_middle runtime_extra)
add_executable(katana_mcp "${CMAKE_CURRENT_BINARY_DIR}/mcp.cpp")
target_link_libraries(katana_mcp PRIVATE runtime_later)
foreach(_program katana_cli katana_mcp)
    target_link_options(${_program} PRIVATE -static-libgcc -static-libstdc++)
    # An absent hook must fail on the missing DLLs, not while configuring the fixture.
    if(COMMAND katana_deploy_runtime)
        katana_deploy_runtime(${_program})
    endif()
endforeach()

file(MAKE_DIRECTORY "${CMAKE_BINARY_DIR}/toolchain/share/proj"
                    "${CMAKE_BINARY_DIR}/toolchain/share/gdal"
                    "${CMAKE_BINARY_DIR}/toolchain/etc/ssl/certs")
file(WRITE "${CMAKE_BINARY_DIR}/toolchain/share/proj/proj.db" "fixture\n")
file(WRITE "${CMAKE_BINARY_DIR}/toolchain/share/gdal/header.dxf" "fixture\n")
file(WRITE "${CMAKE_BINARY_DIR}/toolchain/etc/ssl/certs/ca-bundle.crt" "fixture\n")
set(KATANA_QT_PLUGIN_DIR "")
set(KATANA_WINDEPLOYQT "")
set(KATANA_DEPLOY_KEY "runtime-fixture")
set(_katana_bundle_targets katana_cli katana_mcp)
configure_file("${DEPLOY_CMAKE_DIR}/KatanaDeploy.cmake.in"
               "${CMAKE_BINARY_DIR}/KatanaDeploy.cmake" @ONLY)
