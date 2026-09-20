# Eigen is an implementation detail of the adjustment code: it is linked PRIVATE
# and never appears in a public header (tools/check_layering.cmake enforces it).
find_package(Eigen3 REQUIRED NO_MODULE)

add_library(katana_survey STATIC
    angles.cpp
    data_model.cpp
    network.cpp
    cogo.cpp
    traverse.cpp
    leveling.cpp
    statistics.cpp
    error_propagation.cpp
    least_squares.cpp
    network_adjustment.cpp
)
katana_target_defaults(katana_survey)
target_link_libraries(katana_survey PUBLIC katana_math PRIVATE Eigen3::Eigen)
