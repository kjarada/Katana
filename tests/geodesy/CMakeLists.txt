katana_add_test_suite(geodesy
    LIBS katana_geodesy
    SOURCES
        test_units.cpp
        test_coordinate_reference_system.cpp
        test_coordinate_transformer.cpp
)
