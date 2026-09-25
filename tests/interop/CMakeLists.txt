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
        test_online_verbs.cpp
)
# The 12d archive fixtures live with the format's own tests; the container and
# the file handling tested here read the same ones.
target_compile_definitions(katana_interop_tests PRIVATE
    KATANA_ARCHIVE12D_TEST_DATA="${PROJECT_SOURCE_DIR}/tests/archive12d/data")
# Capabilities documents, service answers and a real STAC item, in the
# shapes the services return them (tests/interop/data/online).
target_compile_definitions(katana_interop_tests PRIVATE
    KATANA_ONLINE_TEST_DATA="${CMAKE_CURRENT_SOURCE_DIR}/data/online")

# The ONLINE verbs and GIS > Online Data run in the application, headlessly,
# with no network: a user catalogue (KATANA_ONLINE_CATALOGUE) whose one layer
# is a local GeoJSON reached as a file:// URL, so the verb and the dialog take
# the whole fetch, reprojection and one-command import path.
if(TARGET katana)
    set(KATANA_ONLINE_TEST_DATA_URL "${CMAKE_CURRENT_SOURCE_DIR}/data/online")
    if(NOT KATANA_ONLINE_TEST_DATA_URL MATCHES "^/")
        # file:///C:/... on Windows
        set(KATANA_ONLINE_TEST_DATA_URL "/${KATANA_ONLINE_TEST_DATA_URL}")
    endif()
    configure_file("${CMAKE_CURRENT_SOURCE_DIR}/data/online/headless_catalogue.json.in"
                   "${CMAKE_CURRENT_BINARY_DIR}/online_headless_catalogue.json" @ONLY)
    # Each test its own cache: they import the same area, and one cache shared
    # by two tests running side by side made their results depend on which ran
    # first (and, on Windows, on which held the clipped file open).
    set(_online_env
        "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_ONLINE_CATALOGUE=set:${CMAKE_CURRENT_BINARY_DIR}/online_headless_catalogue.json")
    set(_online_verbs_env
        "${_online_env};KATANA_ONLINE_CACHE=set:${CMAKE_CURRENT_BINARY_DIR}/online_verbs_headless_cache")
    set(_online_dialog_env
        "${_online_env};KATANA_ONLINE_CACHE=set:${CMAKE_CURRENT_BINARY_DIR}/online_dialog_headless_cache")
    add_test(NAME qt_online_verbs_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/online_verbs_headless.png"
            "-DDRIVE=>ONLINE PROVIDERS local|>ONLINE INFO copernicus dem|>ONLINE IMPORT headless-local lots area=lonlat:151.2,-33.875,151.215,-33.865 crs=EPSG:7856|?FrameStatsLabel|>UNDO|>ONLINE IMPORT headless-local lots area=view crs=EPSG:28356"
            # The plan view is moved to what arrived: the four lots in MGA 56
            # and nothing of the sample, which is in local coordinates
            # thousands of kilometres away - left where it was, the view
            # showed the sample and the import looked like it had done nothing.
            "-DEXPECT=provider id=headless-local .*field name=service value=cog.*imported provider=headless-local layer=lots kind=vector entities=4 target=online/headless-local/lots.*attribution=\"test attribution\".*FrameStatsLabel: Plan +4 drawn.*1 undone.*error verb=IMPORT code=InvalidCRS"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_online_verbs_headless PROPERTIES
        ENVIRONMENT_MODIFICATION "${_online_verbs_env}" TIMEOUT 180)
    # The same import through the dialog's own controls: what a person clicks
    # reaches the same executor as the verb.
    add_test(NAME qt_online_dialog_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/online_dialog_headless.png"
            "-DDRIVE=@onlineData|onlineSearch=headless lots|onlineArea=Typed box|onlineBox=151.2,-33.875,151.215,-33.865|onlineBoxCrs=WGS 84 longitude, latitude|onlineCrs=EPSG:7856|!onlineImport|?onlineStatus"
            "-DEXPECT=imported provider=headless-local layer=lots kind=vector entities=4.*onlineStatus: Imported"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_online_dialog_headless PROPERTIES
        ENVIRONMENT_MODIFICATION "${_online_dialog_env}" TIMEOUT 180)
    # The details say the service's system and that the Advanced group's
    # deadline is there; an import given one (timeout=90, well past what a
    # local file takes) still arrives whole.
    add_test(NAME qt_the_online_dialog_shows_the_services_system_and_imports_within_a_deadline_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/online_dialog_deadline_headless.png"
            "-DDRIVE=@onlineData|onlineSearch=headless lots|?onlineDetails|onlineArea=Typed box|onlineBox=151.2,-33.875,151.215,-33.865|onlineBoxCrs=WGS 84 longitude, latitude|onlineCrs=EPSG:7856|onlineTimeoutOn=on|onlineTimeout=90|!onlineImport|?onlineStatus"
            "-DEXPECT=onlineDetails: [^\r\n]*Service CRS.*imported provider=headless-local layer=lots kind=vector entities=4.*onlineStatus: Imported"
            "-DFORBID=no answer within"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_online_dialog_shows_the_services_system_and_imports_within_a_deadline_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "${_online_env};KATANA_ONLINE_CACHE=set:${CMAKE_CURRENT_BINARY_DIR}/online_dialog_deadline_headless_cache"
        TIMEOUT 180)
endif()
