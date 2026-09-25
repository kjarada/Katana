katana_add_test_suite(interop
    LIBS katana_interop katana_commands
    SOURCES
        test_interop.cpp
        test_archive12d.cpp
        test_round_trip.cpp
        test_dataset_info.cpp
        test_terrain_io.cpp
        # GIS > Online Data (docs/gis_online.md): the catalogue, the request
        # builders, discovery and the fetcher, all without the network; the
        # Live* tests reach it only with KATANA_ONLINE_TESTS=1.
        test_online_catalogue.cpp
        test_online_requests.cpp
        test_online_fetch.cpp
)
# The 12d archive fixtures live with the format's own tests; the container and
# the file handling tested here read the same ones.
target_compile_definitions(katana_interop_tests PRIVATE
    KATANA_ARCHIVE12D_TEST_DATA="${PROJECT_SOURCE_DIR}/tests/archive12d/data")
# Capabilities documents, service answers and a real STAC item, in the
# shapes the services return them (tests/interop/data/online).
target_compile_definitions(katana_interop_tests PRIVATE
    KATANA_ONLINE_TEST_DATA="${CMAKE_CURRENT_SOURCE_DIR}/data/online")
