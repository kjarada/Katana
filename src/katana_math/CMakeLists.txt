# Header-only: the math types are small, constexpr and must inline.
add_library(katana_math INTERFACE)
katana_target_defaults(katana_math)
target_link_libraries(katana_math INTERFACE katana_core)
