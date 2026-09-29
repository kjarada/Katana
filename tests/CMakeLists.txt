# One test executable per module keeps link dependencies honest: the math tests
# cannot accidentally rely on SQLite, the geometry tests cannot rely on Qt.
#
#   katana_add_test_suite(<module> LIBS <targets...> SOURCES <files...>)

function(katana_add_test_suite module)
    cmake_parse_arguments(ARG "" "" "LIBS;SOURCES" ${ARGN})
    set(_target katana_${module}_tests)
    add_executable(${_target} ${ARG_SOURCES})
    target_link_libraries(${_target} PRIVATE ${ARG_LIBS} GTest::gtest_main)
    target_include_directories(${_target} PRIVATE "${CMAKE_CURRENT_FUNCTION_LIST_DIR}")
    katana_target_defaults(${_target})
    # Beside the application, not among it: bin/ holds what ships.
    set_target_properties(${_target} PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${KATANA_TEST_OUTPUT_DIRECTORY}")
    katana_register_tests(${_target})
endfunction()

add_test(NAME layering
    COMMAND ${CMAKE_COMMAND} -DKATANA_ROOT=${PROJECT_SOURCE_DIR}
            -P ${PROJECT_SOURCE_DIR}/tools/check_layering.cmake)
# The check itself, on a tree that breaks the rule: a check that cannot see a
# violation passes as quietly as one that finds none. A module name with
# digits in it (archive12d) was exactly that until the include pattern allowed
# them.
add_test(NAME layering_catches_a_cad_file_including_archive12d
    COMMAND ${CMAKE_COMMAND}
            -DKATANA_ROOT=${CMAKE_CURRENT_SOURCE_DIR}/layering_fixtures/cad_includes_archive
            -P ${PROJECT_SOURCE_DIR}/tools/check_layering.cmake)
set_tests_properties(layering_catches_a_cad_file_including_archive12d PROPERTIES
    PASS_REGULAR_EXPRESSION "reads_an_archive[.]cpp: layer 'cad' must not depend on 'archive12d'")

# What `cmake --install`, the bundle and the package will copy from the
# sources: every file present, and nothing from the build's third-party inputs
# (tools/check_install_rules.cmake says why each matters).
add_test(NAME packaging_installs_only_present_first_party_files
    COMMAND ${CMAKE_COMMAND} -DKATANA_ROOT=${PROJECT_SOURCE_DIR} -DKATANA_BINARY=${CMAKE_BINARY_DIR}
            -P ${PROJECT_SOURCE_DIR}/tools/check_install_rules.cmake)

# What the documents cite exists - repository paths, Class::member names,
# test names - no document cites the removed root plan or instructions, and
# docs/index.md lists every document (docs/testing.md, "The docs test").
# Python is a build-time convenience, never a requirement (Rule 1), so
# without it there is no such test rather than a failing one.
if(Python3_Interpreter_FOUND)
    add_test(NAME docs
        COMMAND "${KATANA_PYTHON}" "${PROJECT_SOURCE_DIR}/tools/check_docs.py"
                "${PROJECT_SOURCE_DIR}")
endif()

# tests/<name>/ holds the suite of module katana_<name>; it is configured only
# when that module is.
file(GLOB _suites LIST_DIRECTORIES true "${CMAKE_CURRENT_SOURCE_DIR}/*")
foreach(_suite IN LISTS _suites)
    get_filename_component(_name "${_suite}" NAME)
    if(EXISTS "${_suite}/CMakeLists.txt" AND TARGET katana_${_name})
        add_subdirectory("${_suite}")
    endif()
endforeach()

# The geoprocessing suite (tests/geo/, docs/geoprocessing.md) belongs to no
# single module: the bridge, the bindings and the executor are three, so it is
# configured whenever the session that runs them is, with GDAL.
if(TARGET katana_app AND TARGET katana_interop)
    add_subdirectory(geo)
endif()

# The SIMD kernel guard (cmake/KatanaSimd.cmake, tools/check_simd_kernels.cmake).
# The sources check reads files, like `layering`; the objects check reads what
# the compiler produced for each *_avx2.cpp (or, on 64-bit ARM, *_neon.cpp) in
# THIS build's configuration, which is where the inline-copy hazard would
# actually show.
add_test(NAME simd_kernel_sources
    COMMAND ${CMAKE_COMMAND} -DKATANA_ROOT=${PROJECT_SOURCE_DIR}
            -P ${PROJECT_SOURCE_DIR}/tools/check_simd_kernels.cmake)
get_property(_simd_object_libraries GLOBAL PROPERTY KATANA_SIMD_OBJECT_LIBRARIES)
# The target's own nm and objdump, which CMake finds beside the compiler - a
# cross build's aarch64 ones. Apple's toolchain may have no objdump, so there
# LLVM's is looked for; it reads Mach-O as the check needs.
set(_simd_nm "${CMAKE_NM}")
set(_simd_objdump "${CMAKE_OBJDUMP}")
if(APPLE AND NOT _simd_objdump)
    find_program(KATANA_LLVM_OBJDUMP NAMES llvm-objdump DOC "objdump for the SIMD kernel object check")
    find_program(KATANA_LLVM_NM NAMES llvm-nm DOC "nm for the SIMD kernel object check")
    if(KATANA_LLVM_OBJDUMP AND KATANA_LLVM_NM)
        set(_simd_objdump "${KATANA_LLVM_OBJDUMP}")
        set(_simd_nm "${KATANA_LLVM_NM}")
    endif()
endif()
if(_simd_object_libraries AND NOT MSVC)
    set(_simd_objects "")
    foreach(_library IN LISTS _simd_object_libraries)
        list(APPEND _simd_objects "$<JOIN:$<TARGET_OBJECTS:${_library}>,|>")
    endforeach()
    list(JOIN _simd_objects "|" _simd_objects)
    add_test(NAME simd_kernel_objects
        COMMAND ${CMAKE_COMMAND} "-DOBJECTS=${_simd_objects}" "-DNM=${_simd_nm}"
                "-DOBJDUMP=${_simd_objdump}"
                -P ${PROJECT_SOURCE_DIR}/tools/check_simd_kernels.cmake)

    # The same kernels compiled as the linux-sanitize preset compiles them, with
    # AddressSanitizer, in EVERY build: that preset is not run on the owner's
    # Windows machine, and without katana_simd_kernel_options keeping ASan's
    # module constructor out, its simd_kernel_objects failed every time. Only
    # compiled, never linked, so a toolchain without the ASan runtime (MinGW)
    # still checks it.
    get_property(_simd_kernel_sources GLOBAL PROPERTY KATANA_SIMD_KERNEL_SOURCES)
    add_library(katana_simd_kernels_under_asan OBJECT ${_simd_kernel_sources})
    katana_target_defaults(katana_simd_kernels_under_asan)
    target_compile_options(katana_simd_kernels_under_asan PRIVATE -fsanitize=address)
    katana_simd_kernel_options(katana_simd_kernels_under_asan)
    add_test(NAME simd_kernel_objects_under_asan
        COMMAND ${CMAKE_COMMAND}
                "-DOBJECTS=$<JOIN:$<TARGET_OBJECTS:katana_simd_kernels_under_asan>,|>"
                "-DNM=${_simd_nm}" "-DOBJDUMP=${_simd_objdump}"
                -P ${PROJECT_SOURCE_DIR}/tools/check_simd_kernels.cmake)

    # The check itself, on a kernel of this build's set that fuses a multiply
    # and an add: a check that cannot see the instruction - an AArch64 FMLA
    # its pattern does not name, say - passes as quietly as one that finds
    # none. Compiled with the kernels' own options, never linked.
    add_library(katana_simd_fused_fixture OBJECT
        simd_kernel_fixtures/fused_${KATANA_SIMD_KERNEL_SET}.cpp)
    katana_target_defaults(katana_simd_fused_fixture)
    katana_simd_kernel_options(katana_simd_fused_fixture)
    add_test(NAME simd_kernel_objects_catches_a_fused_multiply_add
        COMMAND ${CMAKE_COMMAND}
                "-DOBJECTS=$<JOIN:$<TARGET_OBJECTS:katana_simd_fused_fixture>,|>"
                "-DNM=${_simd_nm}" "-DOBJDUMP=${_simd_objdump}"
                -P ${PROJECT_SOURCE_DIR}/tools/check_simd_kernels.cmake)
    set_tests_properties(simd_kernel_objects_catches_a_fused_multiply_add PROPERTIES
        PASS_REGULAR_EXPRESSION "fused_${KATANA_SIMD_KERNEL_SET}[.]cpp[.]o[a-z]*: contains [a-z0-9]+ - a fused multiply-add")
endif()

# The suites with kernels in them, run again with the level forced through the
# KATANA_SIMD override. `scalar` is what a processor without the kernels runs,
# proved here on one that has them; `avx2` (x86-64) or `neon` (64-bit ARM) -
# the set this build compiled, KATANA_SIMD_KERNEL_SET - runs every public
# caller of a kernel on the kernel path. (Where an x86-64 processor has no
# AVX2, the avx2 run is refused by the override, says so, and runs scalar.)
set(_simd_levels scalar ${KATANA_SIMD_KERNEL_SET})
foreach(_suite core geometry render)
    if(TARGET katana_${_suite}_tests)
        foreach(_level IN LISTS _simd_levels)
            add_test(NAME simd_${_level}.${_suite} COMMAND katana_${_suite}_tests)
            set_tests_properties(simd_${_level}.${_suite} PROPERTIES
                ENVIRONMENT_MODIFICATION
                    "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};KATANA_SIMD=set:${_level}")
        endforeach()
    endif()
endforeach()
# The cad suite is large and its kernels are the scene build's alone, so only
# the scene tests are run again at each level.
if(TARGET katana_cad_tests)
    foreach(_level IN LISTS _simd_levels)
        add_test(NAME simd_${_level}.cad
                 COMMAND katana_cad_tests "--gtest_filter=CadScene*:Scene*")
        set_tests_properties(simd_${_level}.cad PROPERTIES
            ENVIRONMENT_MODIFICATION
                "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};KATANA_SIMD=set:${_level}")
    endforeach()
endif()

# The view widgets on their own, headlessly (tests/qt_widgets/). Not found by
# the loop above: the application has no katana_qt library, so the suite
# compiles the widgets' sources itself and is configured whenever the
# application is.
if(TARGET katana)
    add_subdirectory(qt_widgets)
endif()

# Every headless check in this file runs the application offscreen, with the
# runtime it was built against first on PATH and a TIMEOUT, whatever shell
# ctest is started from. In a Git Bash whose PATH puts another MinGW's bin
# first, the application loads the wrong C++ runtime and dies before main
# (0xc0000139, entry point not found); without the platform it opens real
# windows, or fails where there is no display; without the TIMEOUT a modal
# box nobody closes holds the run for ctest's default 1,500 seconds. A check
# added without its set_tests_properties passes only where the caller's own
# environment happens to be right, so this test names it. The scan is
# deferred to the end of the file, after the last set_tests_properties.
function(katana_name_headless_checks_without_their_environment)
    get_property(_tests DIRECTORY PROPERTY TESTS)
    set(_bare "")
    foreach(_test IN LISTS _tests)
        if(NOT _test MATCHES "_headless$")
            continue()
        endif()
        get_test_property(${_test} ENVIRONMENT_MODIFICATION _environment)
        get_test_property(${_test} TIMEOUT _timeout)
        if(NOT _environment MATCHES "(^|;)PATH=path_list_prepend:"
           OR NOT _environment MATCHES "(^|;)QT_QPA_PLATFORM=set:offscreen(;|$)"
           OR NOT _timeout)
            list(APPEND _bare ${_test})
        endif()
    endforeach()
    if(_bare)
        string(JOIN ", " _named ${_bare})
    else()
        set(_named "none")
    endif()
    add_test(NAME qt_every_headless_check_runs_offscreen_with_the_runtime_first_on_its_path
        COMMAND ${CMAKE_COMMAND} -E echo
            "Headless checks without the offscreen platform, the runtime on PATH or a timeout: ${_named}.")
    set_tests_properties(qt_every_headless_check_runs_offscreen_with_the_runtime_first_on_its_path
        PROPERTIES PASS_REGULAR_EXPRESSION "or a timeout: none[.]")
endfunction()
if(TARGET katana)
    cmake_language(DEFER CALL katana_name_headless_checks_without_their_environment)
endif()

# The geoprocessing verbs in the real window, one file per package of the
# geoprocessing plan (tests/geo/headless/*.cmake), so the lanes add checks
# without editing this file. Included here, in this directory's scope, so the
# check above sees their environment.
if(TARGET katana)
    file(GLOB _geo_headless_checks CONFIGURE_DEPENDS
         "${CMAKE_CURRENT_SOURCE_DIR}/geo/headless/*.cmake")
    foreach(_geo_headless_check IN LISTS _geo_headless_checks)
        include("${_geo_headless_check}")
    endforeach()
endif()

# A plot produced headlessly, so the PDF writing inside the Qt widget is
# exercised by ctest and not only by hand (PLAN.MD Phase 22). The script
# copies the sample project first: a test must never touch repository data.
if(TARGET katana)
    add_test(NAME qt_plot_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/plot_headless.pdf"
            -P "${PROJECT_SOURCE_DIR}/tools/check_plot.cmake")
    set_tests_properties(qt_plot_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The sheets driven as an agent drives them: --command lines with the
    # sheet verbs and PLOTSHEETS, then --sheets-json and --plot-sheets.
    add_test(NAME qt_sheets_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/sheets_headless.json"
            -P "${PROJECT_SOURCE_DIR}/tools/check_sheets_headless.cmake")
    set_tests_properties(qt_sheets_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 400)

    # Every sheet feature driven as an agent drives it: a legend, a grid, a
    # key plan, the drawing register, the page setup, ARRANGE, SHEETS CHECK
    # and PLOTSHEETS to PNGs, then --plot-sheets, in one headless run.
    add_test(NAME qt_sheets_agent_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/sheets_agent.pdf"
            -P "${PROJECT_SOURCE_DIR}/tools/check_sheets_agent_headless.cmake")
    set_tests_properties(qt_sheets_agent_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 400)

    # The Sheets editor's Generate Sheets filled as a person fills it: the
    # line it shows is the GENERATE line the window's one executor runs
    # (MainWindow::runVerbLine), and UNDO takes the sheets away as one step.
    # The dialog goes when it closes, so the grab is of the editor's toolbar.
    add_test(NAME qt_generate_sheets_runs_the_line_it_shows_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>ALIGN NEW ROAD 0,0 1000,0|*fileSheets|@sheetGenerate|sheetGenerateLayout=Plan strips along an alignment|sheetGenerateScale=1:500|sheetGenerateFrom=100|sheetGenerateTo=900|sheetGeneratePaper=A1|?sheetGenerateLine|!sheetGenerateOk|%sheetToolBar|>SHEETS|>UNDO|>SHEETS"
            "-DEXPECT=sheetGenerateLine: GENERATE strips paper=A1 landscape frame=on alignment=\"ROAD\" scale=500 overlap=10 keyplan=on from=100 to=900[\r\n].*generated [0-9]+ sheets: 1 to [0-9]+.*sheet 1 id=s1 name=[^\r\n]* paper=A1 orientation=landscape.*[\r\n]0 sheets"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/generate_sheets_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_generate_sheets_runs_the_line_it_shows_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The Sheets editor's Sheet Set menu in the window: Copy Sheet Set as
    # JSON runs SHEETS JSON through the window's one executor; Page Setup is
    # filled and saved by its object names, one step and no plot; Save
    # Sheet Set As, whose file dialog no headless session can answer, names
    # its verb; and UNDO takes the page setup back.
    add_test(NAME qt_sheet_set_menu_runs_its_lines_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>SHEET NEW PLAN|*fileSheets|*sheetCopySetJson|@sheetPageSetup|plotColourMode=Greyscale|plotDpi=150|pageSetupFilePerSheet=on|?plotPatternPreview|!pageSetupSave|%sheetToolBar|>SHEETS PAGESETUP|*sheetSaveSet|>UNDO|>SHEETS PAGESETUP"
            "-DEXPECT=\"format\":\"katana-sheets\".*plotPatternPreview: First file: 01 PLAN\\.pdf.*pagesetup style=greyscale lineweight=1 dpi=150 pattern=\"[^\"]*\" filepersheet=on.*type SHEETS SAVE \"path\" instead.*pagesetup style=colour lineweight=1 dpi=300 pattern=\"[^\"]*\" filepersheet=off"
            "-DFORBID=Plotted|file="
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/sheet_set_menu_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_sheet_set_menu_runs_its_lines_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The whole main window, built and painted headlessly: theme, toolbars, every
    # icon, the viewport with the sample in it. See tools/check_screenshot.cmake.
    add_test(NAME qt_screenshot_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/screenshot_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_screenshot_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# Terrain > Alignment Manager (src/katana_qt/alignment_manager.hpp) through the
# real window: an alignment made from the dialog's fields is the ALIGN NEW line
# echoed in the command log and run by the window's executor; the dialog's list
# follows a PI typed on the command line and its undo; the setting-out note and
# ALIGN STATIONS agree. By hand, for (0,0) (100,0) (100,100) with no curve:
# every 25 m is 0 to 175, eight stations, and the key stations are the start
# (on the interval), the PI at 100 (on it) and the end at 200 (not): nine rows,
# three of them key; every 150 m is 0 and 150, so the table is the start, the
# PI - where the northward tangent begins, so azimuth 0 - 150 and the end.
if(TARGET katana)
    add_test(NAME qt_the_alignment_manager_runs_its_align_lines_through_the_window_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@terrainAlignmentManager|alignmentName=main road|alignmentPoints=0,0 100,0 100,100|!alignmentNew|?alignmentStatus|alignmentInterval=25|?alignmentStationsNote|?alignmentList|>ALIGN PI \"main road\" 200,100|?alignmentList|>UNDO|?alignmentList|>ALIGN STATIONS \"main road\" 150|?commandLog"
            "-DEXPECT=--dialog terrainAlignmentManager opened alignmentManagerDialog \"Alignment Manager\".*alignmentStatus: alignment main road created with 3 PIs.*alignmentStationsNote: 9 stations, 3 of them key stations[.].*alignmentList: main road [|] 3 [|] 0[.]000 [|] 200[.]000 [|] 200[.]000 [|] none.*alignmentList: main road [|] 4 [|] 0[.]000 [|] 300[.]000 [|] 300[.]000 [|] none.*alignmentList: main road [|] 3 [|] 0[.]000 [|] 200[.]000.*station=0[.]000 x=0[.]000 y=0[.]000 direction=0[.]000 azimuth=90[^0-9]+00'00\" radius=straight key=start[\r\n]+station=100[.]000 x=100[.]000 y=0[.]000 direction=90[.]000 azimuth=0[^0-9]+00'00\" radius=straight key=PI[\r\n]+station=150[.]000 x=100[.]000 y=50[.]000 direction=90[.]000 azimuth=0[^0-9]+00'00\" radius=straight[\r\n]+station=200[.]000 x=100[.]000 y=100[.]000 [^\r\n]* key=end.*commandLog: .*> ALIGN NEW \"main road\" 0,0 100,0 100,100 alignment main road created with 3 PIs"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/alignment_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The Terrain menu's section and corridor items by the object names
    # --trigger reaches them by, in a drawing with no alignment: each says
    # where one is defined - the Alignment Manager, which a headless run does
    # not open - and the menu starts with it.
    add_test(NAME qt_terrain_commands_point_to_the_alignment_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=*terrainSectionAlongAlignment|*terrainCorridorQuantities|*terrainCorridorSurface|*terrainSectionAlongSelection|?terrainMenu"
            "-DEXPECT=Define an alignment first: Terrain > Alignment Manager, or ALIGN NEW.*No alignment has a design profile[.] Define one in Terrain > Alignment Manager [(]Vertical[)].*No alignment has a design profile.*Select exactly one line or polyline to cut along.*terrainMenu: Terrain: Alignment Manager[.][.][.] [[]Define and edit alignments"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/terrain_commands_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Survey > Parcel Report through the real window, on the 100 by 50
    # rectangle drawn counter-clockwise from the origin: by hand, 5000 m2 is
    # 0.5 ha, the perimeter 300 and the centroid (50,25); the first course
    # runs due east (N 90 E) 100, the second due north (N 0 E) 50. Label
    # Courses is PARCEL 1 LABEL 2 run by the window's executor - four course
    # labels and the area - and a headless Save opens no file dialog.
    add_test(NAME qt_the_parcel_report_computes_labels_and_describes_a_lot_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>PLINE 0,0 100,0 100,50 0,50 CLOSE|>SELECT ALL|@surveyParcelReport|!useSelection|!compute|?parcelSummary|?parcelCourses|labelHeight=2|!label|?message|parcelName=Lot 7|?parcelLegalText|!parcelCsv|?message|?commandLog"
            "-DEXPECT=--dialog surveyParcelReport opened surveyParcelReportDialog \"Parcel Report\".*parcelSummary: area 5000[.]000 m2 [(]0[.]500 ha[)], perimeter 300[.]000 m, centroid 50[.]000,25[.]000, drawn counter-clockwise.*parcelCourses: 1 [|] 0[.]000 [|] 0[.]000 [|] N 90[^|]+ E [|] 100[.]000 ; 2 [|] 100[.]000 [|] 0[.]000 [|] N 0[^|]+ E [|] 50[.]000.*message: 5 labels created on layer 0, the current layer.*parcelLegalText: Lot 7: Beginning at E 0[.]000 N 0[.]000; thence N 90[^;]+, 100[.]000 m;.*Containing 5000[.]000 m[^ ]+ [(]0[.]5000 ha[)][.].*message: a headless session opens no file dialog.*commandLog: .*> PARCEL 1 LABEL 2 5 labels created on layer 0"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/parcel_report_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Format > Styles and Linetypes > Hatch Patterns through the real window:
    # a style made from the tab (STYLE NEW ... HATCH, one line), a delete the
    # layer's use refuses, a purge that takes only the pattern nothing uses,
    # and the table following a HATCH SET typed on the command line. Layer 0
    # names "none" outright, so even "none" is used by a layer.
    add_test(NAME qt_the_hatch_patterns_tab_runs_its_lines_and_follows_a_typed_edit_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>HATCH NEW brick 45 0.25|>LAYER NEW site|>LAYER HATCH site brick|@formatStyles|managerTabs=Hatch Patterns|hatchPatternTable=brick|?hatchPatternTable|?hatchPatternFamilies|hatchPatternName=brick paving|!hatchPatternNewStyle|?hatchPatternStatus|!hatchPatternDelete|?hatchPatternStatus|hatchPatternName=fill|!hatchPatternNewSolid|!hatchPatternPurge|?hatchPatternStatus|>HATCH SET brick 45 0.5 135 0.5|?hatchPatternTable|?commandLog"
            "-DEXPECT=hatchPatternTable: brick [|] 1 family [|] used by 1 layer[^;]* ; none [|] draws nothing [|] used by 1 layer, e[.]g[.] +layer=0.*hatchPatternFamilies: 45 [|] 0[.]25.*hatchPatternStatus: style brick paving created.*hatchPatternStatus: [^\r\n]*used by 1 layer and 1 style.*hatchPatternStatus: purged 0 styles, 0 linetypes and 1 hatch pattern.*hatchPatternTable: brick [|] 2 families [|] used by 1 layer and 1 style.*commandLog: .*> STYLE NEW \"brick paving\" HATCH brick style brick paving created"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/hatch_patterns_tab_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_alignment_manager_runs_its_align_lines_through_the_window_headless
        qt_terrain_commands_point_to_the_alignment_manager_headless
        qt_the_parcel_report_computes_labels_and_describes_a_lot_headless
        qt_the_hatch_patterns_tab_runs_its_lines_and_follows_a_typed_edit_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# The whole main window with a customisation loaded into it: the two
# libraries and the survey code file read, resolved against each other, and
# reported. This is the path the Format > Load Customisation menu item takes, so what
# ctest exercises is what a person gets (PLAN.MD 20.3). It runs without the
# files when they are not in the checkout - they carry their author's licence
# notice - and then proves only that the switch does no harm.
if(TARGET katana)
    add_test(NAME qt_customisation_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_headless.png"
            "-DCUSTOMISE_DIR=${KATANA_CUSTOMISATION_DIR}"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_customisation_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# A layer switched off through its box in the layer panel, headlessly. This
# crashed the application when the panel was rebuilt inside its own itemChanged
# signal; the check is in MainWindow::toggleLayerThroughPanel. BUILDING is a
# layer of the sample with entities on it.
if(TARGET katana)
    add_test(NAME qt_toggle_layer_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/toggle_layer_headless.png"
            "-DTOGGLE_LAYER=BUILDING"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_toggle_layer_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The same click after a 12d archive import that brought a surface, a
    # point cloud and a split layout: the plan viewport is replaced, and its
    # document listener must go with it. This crashed 5 runs in 6 before
    # Document::ListenerHandle; a run that survives by luck still passes,
    # which is why the unit tests in test_cad.cpp carry the proof.
    add_test(NAME qt_import_12da_then_toggle_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/all_geometries.12daz"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_toggle_headless.png"
            "-DTOGGLE_LAYER=Cadastre"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_import_12da_then_toggle_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # An archive of TINs alone (four tin blocks, SOUTH WEST to NORTH EAST, and
    # no strings) and one of point clouds alone (two las_cloud_data strings,
    # SITE SCAN and ROAD SCAN). Neither brings a layer, style, entity or
    # alignment, so the import's drawing transaction is empty; the stack
    # refuses an empty one, and the import used to stop there with "Import
    # failed" before adding the surfaces or the clouds. EXPECT names the
    # imported record, and for the TINs a surface record (written as each
    # surface is added, after the transaction), so the run must get past the
    # transaction to pass. Each tin is 9 points and 8 triangles (the file).
    add_test(NAME qt_import_tin_only_archive_adds_its_surfaces_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/multiple_tins.12da"
            "-DEXPECT=imported file=\"?[^ ]*/multiple_tins[.]12da\"? kind=archive entities=0 alignments=0 surfaces=4 .*surface name=\"TIN SOUTH WEST\" triangles=8 points=9 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_tin_only_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_import_tin_only_archive_adds_its_surfaces_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
    add_test(NAME qt_import_cloud_only_archive_adds_its_clouds_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/las_point_cloud.12da"
            "-DEXPECT=imported file=\"?[^ ]*/las_point_cloud[.]12da\"? kind=archive entities=0 [^\r\n]*clouds=2 .*reference id=1 kind=pointcloud name=\"SITE SCAN\".*reference id=2 kind=pointcloud name=\"ROAD SCAN\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_cloud_only_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_import_cloud_only_archive_adds_its_clouds_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # Point symbols painted from their styles (PLAN.MD 20.2 slice 2), into a
    # directory that is NOT a project: "Open Failed" was a modal box, which in
    # a headless run is a hang the TIMEOUT below catches.
    add_test(NAME qt_import_symbols_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/symbols.12da"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_symbols_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_import_symbols_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The styles and linetypes manager (PLAN.MD 20.2 slice 3), opened over an
    # archive import so that its tables hold real linestyles and symbols. What it
    # protects is that the dialog builds, lays out and PAINTS - its tables,
    # its form, its two previews - which is otherwise only ever checked by a
    # person opening the menu.
    add_test(NAME qt_style_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/symbols.12da"
            "-DSTYLE_MANAGER=ON"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/style_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_style_manager_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # The attribute manager (PLAN.MD 20.2 slice 5) over a 12d import, on the
    # first entity - a permanent mark whose attributes came from 12d as two
    # groups. It builds the tree from the flattened keys and paints it.
    # No project, so entity 1 IS the first imported string rather than the
    # first entity of the sample, which has no 12d attributes at all.
    add_test(NAME qt_attribute_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/all_geometries.12da"
            "-DATTRIBUTES=1"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/attribute_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_attribute_manager_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # ---- the GIS menu: GDAL and PDAL through the QActions a person clicks ----
    #
    # Surface From Raster on the sample DEM. The expected range is the
    # minimum and maximum of the values in samples/gis/terrain.asc, read
    # straight from the text grid (10 800 values, no no-data), not from
    # Katana: 24.892 to 38.819. Every pixel is a TIN vertex at this size (under
    # the 400 000-point cap), and linear interpolation cannot leave the range
    # of its vertices, so the surface's range is exactly the grid's. The old
    # command rebuilt heights from 8-bit display greys between two numbers it
    # asked for - a question that hangs a headless run (audit QT-23). The item
    # opens the Surface From dialog on the raster, and its Run runs the
    # SURFACE FROM line (docs/terrain.md, "Surfaces on every front end"),
    # whose surface record says the range as zmin and zmax.
    set(_gis "${PROJECT_SOURCE_DIR}/samples/gis")
    set(_qt_env "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen")
    add_test(NAME qt_surface_from_raster_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${_gis}/terrain.asc"
            "-DDRIVE=@surfaceFromRaster|!surfaceFromRun"
            "-DEXPECT=zmin=24\\.892 zmax=38\\.819"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/surface_from_raster_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Surface From Point Cloud on the sample scan, which is classified: `pdal
    # info --stats --enumerate Classification` lists classes 2, 3, 5 and 6,
    # and filters.range on Classification[2:2] keeps 29 512 of its 40 000
    # points. The surface must be built from those ground returns and say so
    # (audit QT-10): the input record's points, ground=yes and excluded.
    add_test(NAME qt_surface_from_cloud_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${_gis}/survey_scan.las"
            "-DDRIVE=@surfaceFromPointCloud|!surfaceFromRun"
            "-DEXPECT=input arg=input source=cloud name=survey_scan id=[0-9]+ points=29512 ground=yes excluded=10488 classes=2"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/surface_from_cloud_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # File > New takes the imported raster with the drawing (audit QT-17):
    # afterwards there is no raster for Surface From Raster to use. Before
    # the fix the orthophoto of the discarded drawing stayed behind the new
    # one, and this built a surface from it. SURFACE LIST lists the rasters a
    # surface can be made from.
    add_test(NAME qt_new_drops_reference_data_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DIMPORT=${_gis}/terrain.asc"
            "-DDRIVE=*fileNew|<SURFACE LIST"
            "-DEXPECT=listed surfaces=0 rasters=0"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/new_drops_reference_data_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # GIS > Dataset Information and the import dialog, described by GDAL and
    # PDAL from the file and painted. The dialog runs its INFO lines through
    # the window's one executor, so what they said is in the log.
    add_test(NAME qt_dataset_info_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDATASET_INFO=${_gis}/terrain.asc"
            "-DEXPECT=dataset file=[^ ]*/terrain\\.asc kind=raster driver=AAIGrid "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/dataset_info_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_import_options_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT_OPTIONS=${_gis}/survey_scan.las"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_options_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The vector import dialog, driven by its fields' names: where= goes to
    # GDAL and Preview counts without importing. tests/geo/data/lots.geojson
    # by hand: A and B are kind=lot, C kind=corridor - two of three match.
    add_test(NAME qt_vector_import_dialog_previews_its_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT_OPTIONS=${PROJECT_SOURCE_DIR}/tests/geo/data/lots.geojson"
            "-DDRIVE=vectorImportWhere=kind = 'lot'|?vectorImportCommand|!vectorImportPreview|?vectorImportMatchCount"
            "-DEXPECT=vectorImportCommand: IMPORT \"[^\"]*lots\\.geojson\" where=\"kind = 'lot'\".*vectorImportMatchCount: 2 of 3 features match; 2 entities would be imported"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/vector_import_dialog_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # And its Import (vectorImportRun) imports what the filter takes, as the
    # menu's dialog does once accepted: the two lots. Its buttons had no
    # object names, so a headless run could preview but never import.
    add_test(NAME qt_vector_import_dialog_imports_what_its_filter_takes_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT_OPTIONS=${PROJECT_SOURCE_DIR}/tests/geo/data/lots.geojson"
            "-DDRIVE=vectorImportWhere=kind = 'lot'|!vectorImportRun|>LIST"
            "-DEXPECT=2 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/vector_import_dialog_run_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # File > Export Vector's dialog, opened for a file as the menu opens it
    # once its file dialog has answered (^FILE), over a drawing the command
    # line made: layer "other" holds lots A and B alone, so the filter takes
    # two, Preview writes nothing and Run writes those two under the name
    # given.
    add_test(NAME qt_vector_export_dialog_writes_what_its_filter_takes_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>IMPORT \"${PROJECT_SOURCE_DIR}/tests/geo/data/lots.geojson\" crs=adopt|>LAYER NEW other|>IMPORT \"${PROJECT_SOURCE_DIR}/tests/geo/data/lots.geojson\" target=other where=\"kind = 'lot'\"|^${CMAKE_CURRENT_BINARY_DIR}/vector_export_dialog_headless.gpkg|vectorExportFilterLayer=other|vectorExportLayerName=lots_only|?vectorExportCommand|!vectorExportPreview|?vectorExportStatus|!vectorExportRun|?vectorExportStatus|>INFO \"${CMAKE_CURRENT_BINARY_DIR}/vector_export_dialog_headless.gpkg\""
            "-DEXPECT=vectorExportCommand: EXPORT \"[^\"]*vector_export_dialog_headless\\.gpkg\" DRAWING WHERE LAYER=other layername=lots_only.*vectorExportStatus: Preview: 2 entities would be written; nothing was\\..*vectorExportStatus: Wrote 2 features to .*layer name=lots_only features=2 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/vector_export_dialog_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Removed first, so a file an earlier run left cannot stand in for the
    # one this run's Run writes and INFO reads back.
    set_tests_properties(qt_vector_export_dialog_writes_what_its_filter_takes_headless PROPERTIES
        FIXTURES_REQUIRED vector_export_dialog_clean)
    add_test(NAME qt_vector_export_dialog_clean
        COMMAND ${CMAKE_COMMAND} -E rm -f "${CMAKE_CURRENT_BINARY_DIR}/vector_export_dialog_headless.gpkg")
    set_tests_properties(qt_vector_export_dialog_clean PROPERTIES FIXTURES_SETUP vector_export_dialog_clean)
    set_tests_properties(qt_surface_from_raster_headless qt_surface_from_cloud_headless
                         qt_new_drops_reference_data_headless
                         qt_dataset_info_headless qt_import_options_headless
                         qt_vector_import_dialog_previews_its_line_headless
                         qt_vector_import_dialog_imports_what_its_filter_takes_headless
                         qt_vector_export_dialog_writes_what_its_filter_takes_headless
        PROPERTIES ENVIRONMENT_MODIFICATION "${_qt_env}" TIMEOUT 180)

    # The layer manager (PLAN.MD 20.2 slice 6) over the sample project, whose
    # layers have colours, linetypes and entities to count.
    add_test(NAME qt_layer_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DLAYER_MANAGER=ON"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/layer_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_layer_manager_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # ---- the Survey menu (PLAN.MD 45 slice 9), through the actions a person clicks --
    #
    # Each dialog is opened by its action, filled and pressed by the object
    # names of survey/survey_dialogs.hpp, and grabbed; what it reported is read
    # from the log. The numbers are worked by hand, not taken from a run:
    #
    # Inverse (1000,5000) -> (1003,5004): a 3-4-5 triangle, 5.000 at azimuth
    # atan2(3, 4) = 36.8698976 degrees = 36 deg 52' 11.63" (0.8698976 * 60 =
    # 52.19386', 0.19386 * 60 = 11.63").
    add_test(NAME qt_survey_inverse_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyInverse"
            "-DFILL=from=1000,5000|to=1003,5004"
            "-DPRESS=compute"
            "-DEXPECT=Horizontal distance 5\\.000.*Azimuth 36[^0-9]+52'11\\.63"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_inverse_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Area of Selection over samples/gis/parcels.geojson, everything selected.
    # Its outlines by the shoelace formula, from the file's coordinates:
    #   42A (185,0) (300,0) (310,70) (200,95):
    #     0 + 21000 + (29450 - 14000) + (0 - 17575) = 18875 -> 9437.5 m^2
    #   42B (200,95) (310,70) (320,150) (215,165):
    #     (14000 - 29450) + (46500 - 22400) + (52800 - 32250) + (20425 - 33000)
    #     = 16625 -> 8312.5 m^2
    #   the spoil heaps, 30 x 30 and 30 x 25 rectangles: 900 and 750 m^2
    # 19 400 m^2 in all = 1.9400 ha (1 ha = 10 000 m^2). The two roads and
    # the three control points have no area and are skipped, and said to be.
    add_test(NAME qt_survey_area_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/samples/gis/parcels.geojson"
            "-DSELECT_ALL=ON"
            "-DACTIONS=surveyArea"
            "-DEXPECT=Total  area 19400\\.000 m[^ ]* \\(1\\.9400 ha\\).*Skipped 3: a point has no area"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_area_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Forward along the same 3-4-5 direction, 5 from (1000,5000,20) with a
    # height difference of -1: the far corner (1003,5004) at 20 - 1 = 19,
    # added as a point named P7 on the current layer.
    add_test(NAME qt_survey_forward_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyForward"
            "-DFILL=from=1000,5000,20|direction=N 36d52m11.63s E|distance=5|heightDifference=-1|name=P7"
            "-DPRESS=addPoint"
            "-DEXPECT=New point P7  E 1003\\.000 N 5004\\.000 Z 19\\.000.*Created on layer"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_forward_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A right angle is 100 gon: 400 gon to the circle, by definition.
    add_test(NAME qt_survey_angle_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyAngleCalculator"
            "-DFILL=angle=90|unit=Degrees"
            "-DPRESS=convert"
            "-DEXPECT=100\\.00000000 gon"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_angle_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A 200 x 100 rectangle walked clockwise from A(1000,5000): north 100,
    # east 200, south 100, west 200, turning 270 degrees (clockwise, back to
    # forward) at every corner - with BC booked 200.100. The angles close;
    # the figure overshoots 0.100 east: precision 600.1 / 0.1 = 1 : 6001.
    # Bowditch moves C by -0.100 * (100 + 200.1) / 600.1 = -0.050 in E:
    # 1200.100 - 0.050 = 1200.050. Added: B, C and D (A is the control it
    # hangs on) and the traverse line - 4 entities.
    add_test(NAME qt_survey_traverse_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyTraverse"
            "-DFILL=name=Block|start=1000,5000|startMark=1000,5100|legs=A 270 100\\nB 270 200.1\\nC 270 100\\nD 270 200"
            "-DPRESS=addToDrawing"
            "-DEXPECT=precision 1 : 6001.*C +1200\\.100 +5100\\.000 +1200\\.050 +5100\\.000.*Added to layer 0: 4 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_traverse_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A level book reduced by hand (height of collimation): BM1 100.000 +
    # BS 1.500 = 101.500; CP1 101.500 - 0.500 = 101.000, + 2.250 = 103.250;
    # BM2 103.250 - 3.260 = 99.990 against its known 100.000: -0.010. The
    # check: sum BS 3.750 - sum FS 3.760 = -0.010. Allowable 12 mm x
    # sqrt(0.250 km) = 6 mm, which the 10 mm misclosure exceeds.
    add_test(NAME qt_survey_level_book_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyLevelBook"
            "-DFILL=startLevel=100|closingLevel=100|book=BM1 1.500 - -\\nA - 2.000 -\\nCP1 2.250 - 0.500 100\\nB - 1.250 -\\nBM2 - - 3.260 150|adjustment=By distance levelled"
            "-DPRESS=reduce"
            "-DEXPECT=sum BS 3\\.750 - sum FS 3\\.760 = -0\\.010.*Misclosure -0\\.0100 \\(computed - known\\); allowable 0\\.0060"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_level_book_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # UTM by definition (NGA.SIG.0012): zone 30's central meridian is 3 deg
    # W, the false easting 500 000 m, the false northing 0 in the north, the
    # scale on the central meridian 0.9996 - so (0, -3) is E 500 000, N 0.
    add_test(NAME qt_survey_converter_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSURVEY_DIALOG=surveyCoordinateConverter"
            "-DFILL=source=4326|target=32630|points=origin 0 -3"
            "-DPRESS=convert"
            "-DEXPECT=origin +0\\.000000000 -3\\.000000000 +-> +500000\\.000 0\\.000 +target k 0\\.99960000"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_converter_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The import wizard, the Point Manager, the export and the Point Report
    # (PLAN.MD 45 slices 10, 11, 13), driven page by page (DRIVE, see
    # tools/check_screenshot.cmake) on tests/surveyio/data. The fixture
    # survey_points_pnezd.csv has a header naming its columns and 12 points,
    # counted by hand: 1 to 12, point 4 without an elevation, point 9's
    # description "Hydrant, fire" quoted. Its detection is never Identified
    # (the delimited probe is weak by design), so every run chooses the format.
    set(_survey_data "${PROJECT_SOURCE_DIR}/tests/surveyio/data")
    set(_survey_format "format=Delimited text points (CSV, TXT)")
    # Import all 12, then the Point Manager lists them - 12 rows - and the
    # filter "CP" keeps the two points whose code is CP (1 and 11; no id or
    # description holds "cp").
    add_test(NAME qt_survey_import_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|!next|unit=metres|!next|!next|!import|#surveyPointManager|filter=CP"
            "-DEXPECT=Records read: 12.*Points to create: 12.*Imported 12 point\\(s\\) from survey_points_pnezd\\.csv.*SurveyPointsDock: 12 points, 2 shown"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_import_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A headerless file does not say which column is the northing: Next on the
    # layout page is refused until the order is confirmed, and then its 3
    # points import.
    add_test(NAME qt_survey_import_uncertain_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_headerless.txt|!next|${_survey_format}|!next|columns=P,N,E,Z|delimiter=whitespace|headerLines=0|!next|confirmOrder=on|!next|unit=metres|!next|!next|!import"
            "-DEXPECT=could not tell the order of the columns.*Imported 3 point\\(s\\) from survey_points_headerless\\.txt"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_import_uncertain_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The order corrected through the role boxes, one box at a time, as a
    # person does it. survey_points_headerless_penz.txt is
    # survey_points_headerless.txt with its 2nd and 3rd columns swapped by
    # hand, so it is P,E,N,Z; with no header the proposal cannot tell and puts
    # P,N,E,Z first. Box 2 goes to "ignored" (no northing column) and then to
    # easting (two easting columns) - both layouts surveyio refuses, which once
    # removed every role box, so the fill of role3 found no box - and box 3 to
    # northing. The points exported as P,N,E,Z with no header are then the
    # original file, text for text: the eastings are the 330 000s and the
    # northings the 6 250 000s.
    add_test(NAME qt_survey_import_role_boxes_swap_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_headerless_penz.txt|!next|${_survey_format}|!next|role2=ignored|role2=easting|role3=northing|confirmOrder=on|!next|unit=metres|!next|!next|!import|@surveyExport|file=${CMAKE_CURRENT_BINARY_DIR}/survey_role_swap.txt|columns=P,N,E,Z|delimiter=whitespace|header=off|decimals=3|!export"
            "-DCOMPARE=${_survey_data}/survey_points_headerless.txt|${CMAKE_CURRENT_BINARY_DIR}/survey_role_swap.txt"
            "-DEXPECT=Layout: P,E,N,Z.delimiter=whitespace.header=0.*Imported 3 point\\(s\\) from survey_points_headerless_penz\\.txt.*Exported 3 point\\(s\\) to survey_role_swap\\.txt"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_import_role_boxes_swap_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Round trip through files: export(import(fixture)) with the fixture's own
    # layout and 3 decimals is the fixture, text for text; that export read
    # back in (replacing the points of the same ids) and written again is the
    # fixture again.
    add_test(NAME qt_survey_export_round_trip_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|!next|unit=metres|!next|!next|!import|@surveyExport|file=${CMAKE_CURRENT_BINARY_DIR}/survey_round_trip_1.csv|columns=P,N,E,Z,C,D|decimals=3|!export|@surveyImport|file=${CMAKE_CURRENT_BINARY_DIR}/survey_round_trip_1.csv|!next|${_survey_format}|!next|!next|unit=metres|!next|existing=replace the drawing's point|!next|!import|@surveyExport|file=${CMAKE_CURRENT_BINARY_DIR}/survey_round_trip_2.csv|!export"
            "-DCOMPARE=${_survey_data}/survey_points_pnezd.csv|${CMAKE_CURRENT_BINARY_DIR}/survey_round_trip_1.csv|${CMAKE_CURRENT_BINARY_DIR}/survey_round_trip_2.csv"
            "-DEXPECT=Exported 12 point\\(s\\) to survey_round_trip_1\\.csv.*12 point\\(s\\) in the drawing are replaced.*Exported 12 point\\(s\\) to survey_round_trip_2\\.csv"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_export_round_trip_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The stated transformation: UTM zone 30N (EPSG:32630) to 30S
    # (EPSG:32730) is the same projection with a false northing of 10 000 000 m
    # (NGA.SIG.0012), so the export is the fixture with 10 000 000 added to
    # every northing by hand (survey_points_pnezd_utm30s.csv) and nothing
    # else changed - not the eastings, not the heights.
    add_test(NAME qt_survey_import_transform_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|!next|unit=metres|declared=32630|target=32730|!next|!next|!import|@surveyExport|file=${CMAKE_CURRENT_BINARY_DIR}/survey_transform.csv|columns=P,N,E,Z,C,D|decimals=3|!export"
            "-DCOMPARE=${_survey_data}/survey_points_pnezd_utm30s.csv|${CMAKE_CURRENT_BINARY_DIR}/survey_transform.csv"
            "-DEXPECT=Transformation: from EPSG:32630 to EPSG:32730.*Imported 12 point"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_import_transform_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The Point Report after the import lists all 12 (nothing is selected).
    # A layout template is saved, chosen and deleted in the same run, so the
    # settings are left as they were found.
    add_test(NAME qt_survey_point_report_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|templateName=ctest survey layout|!saveTemplate|template=ctest survey layout|!deleteTemplate|!next|unit=metres|!next|!next|!import|@surveyPointReport"
            "-DEXPECT=Saved the survey layout template \"ctest survey layout\": P,N,E,Z,C,D.delimiter=comma.header=1.quote=double.*Deleted the survey layout template.*Point Report: 12 point\\(s\\)"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_point_report_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A template saved or deleted in the import wizard reaches an Export
    # dialog that is already open. The Export dialog is opened FIRST, so its
    # list is filled before the save (a list filled only when the dialog was
    # built never offered it). The template is then chosen in it; after the
    # wizard deletes it, choosing it again must be refused - the run ends on
    # that refusal (REFUSED, see tools/check_screenshot.cmake), with the
    # settings as it found them. The Delete before the Save clears a
    # template an earlier, killed run may have left; when there is none it
    # only logs that there is none.
    add_test(NAME qt_survey_templates_reach_export_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyExport|@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|templateName=ctest shared layout|!deleteTemplate|!saveTemplate|@surveyExport|template=ctest shared layout|@surveyImport|!deleteTemplate|@surveyExport|template=ctest shared layout"
            "-DREFUSED=Saved the survey layout template \"ctest shared layout\".*Deleted the survey layout template \"ctest shared layout\".*--fill: template has no choice 'ctest shared layout'"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_templates_reach_export_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # ---- Survey > Subsurface Utilities (AS 5488), survey/utility_workbench.hpp --
    #
    # The dialog writes the UTILITY line and runs nothing itself. Opened on
    # Clearance by its own menu item, it shows the line it would run - the
    # schedule, the design and the one option given, in the verb's order - and
    # on Verify the same schedule with nothing else.
    set(_utility_samples "${PROJECT_SOURCE_DIR}/samples/utilities")
    add_test(NAME qt_utility_dialog_writes_the_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@utilityClearance|utilitySchedule=${_utility_samples}/schedule.csv|utilityDesign=${_utility_samples}/design.csv|utilityWidth=0.375|?utilityCommand|utilityTabs=Verify|?utilityCommand"
            "-DEXPECT=--dialog utilityClearance opened utilityDialog \"Subsurface Utilities \\(AS 5488\\)\".*utilityCommand: UTILITY CLEARANCE [^\r\n]*/samples/utilities/schedule\\.csv\"? \"?[^\r\n]*/samples/utilities/design\\.csv\"? WIDTH 0\\.375[\r\n]+utilityCommand: UTILITY VERIFY [^\r\n]*/samples/utilities/schedule\\.csv\"?[\r\n]"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/utility_dialog_line_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Run on Draw: the line goes through the window's command line to the
    # interpreter's UTILITY DRAW, and the plan view is framed on what it drew.
    # By hand from samples/utilities/schedule.csv, graded as UTILITY REPORT
    # grades it (cli.utility_report_grades_the_sample_and_names_the_over_claim,
    # detected spacing 10 m): 4 lines, 14 vertices, 10 segments. The runs of one
    # level are W1 QL-B B B | A | C (the 12.5 m gap to W1-5 is longer than the
    # spacing), E1 B | C C (E1-3's radar pick is QL-C), T1 C and G1 D: 7
    # polylines, and a point per vertex, 21 entities. W1 is 1.420 m QL-A,
    # 8.001 + 8.001 + 0.100 = 16.102 m QL-B and 12.502 m QL-C; G1 is 40 m of
    # records. The bounds are the schedule's extremes, (334000, 6250000) to
    # (334040, 6250007.2) - MGA zone 56, thousands of kilometres from where an
    # empty drawing's view looks, so "Plan 21 drawn" is the framing's doing.
    # The reply is read twice: in the log, and in the dialog's output. It needs
    # the interpreter's UTILITY DRAW; a build without it refuses the line as an
    # unknown command, and this test fails there.
    add_test(NAME qt_utility_dialog_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@utilityDraw|utilitySchedule=${_utility_samples}/schedule.csv|!utilityRun|?utilityStatus|?utilityOutput|?FrameStatsLabel"
            "-DEXPECT=--dialog utilityDraw opened utilityDialog.*utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=[0-9]+ bounds=334000(\\.0*)?,6250000(\\.0*)?,334040(\\.0*)?,6250007\\.20*[\r\n].*line id=W1 type=water length=30\\.02[0-9]* ql_a=1\\.42[0-9]* ql_b=16\\.10[0-9]* ql_c=12\\.50[0-9]* ql_d=0(\\.0*)?[\r\n].*utilityStatus: Drawn as one undo step.*utilityOutput: utilities drawn lines=4 vertices=14 segments=10 entities=21 [^\r\n]*line id=G1 type=gas length=40(\\.0*)? ql_a=0(\\.0*)? ql_b=0(\\.0*)? ql_c=0(\\.0*)? ql_d=40(\\.0*)?.*FrameStatsLabel: Plan +21 drawn"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/utility_dialog_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The same draw TYPED on the command line, as an agent types it, with the
    # MINCOVER the draw also takes: framed just the same, because the window
    # frames the reply, not the dialog. layers=11 by hand from the runs above:
    # water QL-A, QL-B, QL-C and points; electricity QL-B, QL-C and points;
    # telecommunications QL-C and points; gas QL-D and points. The grab is the
    # window, so the PNG shows the four services where they were drawn. The
    # path is quoted as a person types one, so a checkout under a folder with
    # a blank in its name is still one word.
    add_test(NAME qt_utility_draw_typed_frames_the_views_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>UTILITY DRAW \"${_utility_samples}/schedule.csv\" MINCOVER 0.6|?FrameStatsLabel"
            "-DEXPECT=utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 bounds=334000\\.000,6250000\\.000,334040\\.000,6250007\\.200[\r\n]+line id=W1 type=water .*FrameStatsLabel: Plan +21 drawn"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/utility_draw_typed_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A tool waiting for a text's string takes the whole typed line, as a
    # label "Utility pit" on a services plan must be - the UTILITY verb once
    # took it first and refused it. The dialog's Run, pressed while the Text
    # tool waits, still draws: its line is never offered to the tool, which
    # would have made it the text's first line and "Utility pit" its second.
    add_test(NAME qt_utility_line_leaves_a_text_to_the_tool_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>TEXT|>0,0|>|>|@utilityDraw|utilitySchedule=${_utility_samples}/schedule.csv|!utilityRun|?utilityStatus|>Utility pit|>"
            "-DEXPECT=utilityStatus: Drawn as one undo step.*1 line of text"
            "-DFORBID=usage: UTILITY|[0-9]+ lines of text"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/utility_line_text_tool_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The same tools on what is drawn (docs/cad.md, "Scope and filter"), through the dialog's
    # Source choice and the scope and filter controls Global Modify shares
    # with it. The sample is drawn first by a typed line, which frames every
    # plan view on it. The report of the drawing is the report of the
    # schedule (cli.utility_report_of_the_drawing_is_the_report_of_the_schedule_drawn
    # pins that text), led by what the scope took: the 7 runs and 14 points,
    # 4 lines, each whole. Then VIEW, on screen: the plan view the window
    # framed on the sample shows all 21, so the same 4 lines - and the record
    # says which view and where it looked.
    add_test(NAME qt_utility_dialog_reports_what_is_drawn_and_what_the_view_shows_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>UTILITY DRAW \"${_utility_samples}/schedule.csv\"|@utilityReport|!utilitySourceDrawing|?utilityCommand|!utilityRun|?utilityOutput|!utilityScopeView|utilityView=Plan 1 (VIEW 1)|utilityOnScreen=on|?utilityCommand|!utilityRun|?utilityStatus|?utilityOutput"
            "-DEXPECT=utilityCommand: UTILITY REPORT DRAWING[\r\n]+.*utilityOutput: scope=drawing matched=21 lines=4 completed=0 ignored=0 AS 5488 subsurface utility investigation: 4 lines, 14 vertices.*utilityCommand: UTILITY REPORT VIEW 1[\r\n]+.*utilityStatus: Done\\. The report is below.*utilityOutput: scope=view view=[0-9]+ area=[-0-9.e]+,[-0-9.e]+,[-0-9.e]+,[-0-9.e]+ matched=21 lines=4 completed=0 ignored=0 AS 5488 subsurface utility investigation: 4 lines, 14 vertices"
            "-DFORBID=there is none here"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/utility_dialog_drawing_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Regrade with nothing moved: the drawing is as the draw left it, so no
    # line changes and no undo step is added - said in the reply and the
    # status - and the views are framed on what was graded, as after a draw.
    add_test(NAME qt_utility_dialog_regrades_what_is_drawn_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>UTILITY DRAW \"${_utility_samples}/schedule.csv\"|@utilityRegrade|?utilityCommand|!utilityRun|?utilityStatus|?utilityOutput|?FrameStatsLabel"
            "-DEXPECT=utilityCommand: UTILITY REGRADE DRAWING[\r\n]+.*utilityStatus: Nothing needed regrading.*utilityOutput: utilities regraded changed=0 lines=4 vertices=14 segments=10 entities=21 layers=[0-9]+ bounds=334000\\.000,6250000\\.000,334040\\.000,6250007\\.200 scope=drawing matched=21 lines=4 completed=0 ignored=0 line id=W1 type=water length=30\\.024 ql_a=1\\.420 ql_b=16\\.102 ql_c=12\\.502.*FrameStatsLabel: Plan +21 drawn"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/utility_dialog_regrade_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Draw from what an import left in the drawing (docs/subsurface_utilities.md,
    # "Services from surveyed and imported geometry"): the sample
    # investigation as a GIS delivers it, imported by a typed IMPORT, then the
    # Draw tab on what is drawn, its Fields reading the GIS's own attribute
    # names as the schedule's columns. It draws what schedule.csv draws - the
    # hand-worked figures of qt_utility_dialog_headless above - and the record
    # of what the scope took follows the draw's: the 4 strings and 14 points
    # the import made (cli.utility_draw_takes_the_services_an_import_left_in_the_drawing).
    # The plan view is framed on the 18 imported and the 21 drawn.
    if(TARGET katana_interop)
        add_test(NAME qt_utility_dialog_draws_what_an_import_left_headless
            COMMAND ${CMAKE_COMMAND}
                "-DAPP=$<TARGET_FILE:katana>"
                "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
                "-DDRIVE=>IMPORT \"${_utility_samples}/located_services.geojson\"|@utilityDraw|!utilitySourceDrawing|utilityFields=line=ASSET_ID,point=PT_ID,type=ASSET_TYPE,owner=OWNER,material=MATERIAL,diameter_mm=DIA_MM,status=STATUS,config=CONFIG,description=DESC,method=METHOD,h_unc=H_UNC,v_unc=V_UNC,ql=QL,level=LEVEL,level_ref=LEVEL_REF,path=PATH,verifies=VERIFIES|?utilityCommand|!utilityRun|?utilityStatus|?utilityOutput|?FrameStatsLabel"
                "-DEXPECT=utilityCommand: UTILITY DRAW DRAWING FIELDS line=ASSET_ID,point=PT_ID,[^\r\n]*verifies=VERIFIES[\r\n]+.*utilityStatus: Drawn as one undo step.*utilityOutput: utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 bounds=334000\\.000,6250000\\.000,334040\\.000,6250007\\.200 scope=drawing matched=18 lines=4 points=14 loose=0 drawn=0 ignored=0 line id=W1 type=water length=30\\.024 ql_a=1\\.420 ql_b=16\\.102 ql_c=12\\.502 ql_d=0\\.000.*FrameStatsLabel: Plan +39 drawn"
                "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/utility_dialog_import_headless.png"
                -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
        set_tests_properties(qt_utility_dialog_draws_what_an_import_left_headless
            PROPERTIES ENVIRONMENT_MODIFICATION
                "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
            TIMEOUT 180)
    endif()
    # The drawing written back as a schedule from the Schedule tab, and that
    # schedule read by a typed REPORT as the sample is: 4 lines, 14 vertices,
    # and the over-claim the sample carries.
    set(_dialog_scheduled "${CMAKE_CURRENT_BINARY_DIR}/utility_dialog_scheduled.csv")
    add_test(NAME qt_utility_dialog_writes_what_is_drawn_as_a_schedule_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>UTILITY DRAW \"${_utility_samples}/schedule.csv\"|@utilityWriteSchedule|utilityScheduleOut=${_dialog_scheduled}|!utilityRun|?utilityStatus|?utilityOutput|>UTILITY REPORT \"${_dialog_scheduled}\" MINCOVER 0.6"
            "-DEXPECT=utilityStatus: Written\\..*utilityOutput: utilities scheduled path=[^ ]*utility_dialog_scheduled\\.csv\"? lines=4 vertices=14 scope=drawing matched=21 lines=4 completed=0 ignored=0.*AS 5488 subsurface utility investigation: 4 lines, 14 vertices.*E1 / E1-3: claimed QL-B, the ground penetrating radar evidence supports QL-C"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/utility_dialog_schedule_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # VIEW is the window's own word: typed in the window, MODIFY takes what
    # the active plan view shows - here the sample's 14 points, framed on
    # screen - where katana_cli refuses it
    # (cli.utility_view_is_refused_headless_naming_area). EXTENTS drops the
    # area and takes the same 14, the view hiding no layer of its own.
    add_test(NAME qt_modify_view_takes_what_the_plan_view_shows_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>UTILITY DRAW \"${_utility_samples}/schedule.csv\"|>MODIFY VIEW WHERE TYPE=point SET COLOUR=#FF0000|>MODIFY VIEW EXTENTS WHERE TYPE=point SET COLOUR=#00FF00 PREVIEW"
            "-DEXPECT=scope=view view=1 area=[-0-9.e]+,[-0-9.e]+,[-0-9.e]+,[-0-9.e]+ where=.TYPE=point. matched=14[\r\n]+14 entities matched; changing 14 entities\\. One UNDO restores it\\..*scope=view view=1 extents=yes where=.TYPE=point. matched=14[\r\n]+preview: 14 entities matched; changing 14 entities\\."
            "-DFORBID=there is none here|no plan view is open"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/modify_view_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_utility_dialog_writes_the_line_headless qt_utility_dialog_headless
        qt_utility_draw_typed_frames_the_views_headless
        qt_utility_line_leaves_a_text_to_the_tool_headless
        qt_utility_dialog_reports_what_is_drawn_and_what_the_view_shows_headless
        qt_utility_dialog_regrades_what_is_drawn_headless
        qt_utility_dialog_writes_what_is_drawn_as_a_schedule_headless
        qt_modify_view_takes_what_the_plan_view_shows_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
    # ---- the window's wiring of the managers' foundations ----------------------
    #
    # Format > Load Customisation merges (audit QT-21), through the window's
    # CUSTOMISE line, which runs the menu item's applyCustomisation: the
    # hand-written fixture survey code file REPLACES the map, then the symbol fixture
    # alone is MERGED in - and the map must still be there. By hand, from the
    # fixtures' head comments: test_survey.mapfile has 11 rules, and
    # test_symbols.4d 4 definitions, all named TEST ..., which no other
    # library defines, so all 4 are added.
    set(_fixture "${PROJECT_SOURCE_DIR}/tests/archive12d/data/customisation")
    add_test(NAME qt_customisation_merges_and_keeps_the_map_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE REPLACE ${_fixture}/test_survey.mapfile|>CUSTOMISE ${_fixture}/test_symbols.4d"
            "-DEXPECT=test_symbols\\.4d: style library, 4 added, 0 replaced[^\r\n]*[\r\n]+Customisation now: [0-9,]+ definitions \\([0-9,]+ symbols\\) and 11 survey code rules"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_merge_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # A file named twice in one CUSTOMISE is read once, as katana_cli reads it
    # (cad::distinctCustomisationFiles): the second naming is the same file
    # by another spelling. By hand: the mapfile REPLACES the map, read once,
    # so the map holds its 11 rules (the fixture's head comment); read twice
    # it held 22.
    add_test(NAME qt_a_file_named_twice_in_one_load_is_read_once_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE REPLACE ${_fixture}/test_survey.mapfile ${_fixture}/../customisation/test_survey.mapfile"
            "-DEXPECT=test_survey\\.mapfile is named twice in this load; it is read once\\..*Customisation now: [0-9,]+ definitions \\([0-9,]+ symbols\\) and 11 survey code rules"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_named_twice_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # A typed SAVE that cannot save - an untitled drawing, no directory given
    # - leaves a drawing nobody touched unmodified, so a NEW after it asks
    # nothing and goes ahead. Writing the record before the failed save
    # marked it modified.
    add_test(NAME qt_a_save_that_cannot_go_ahead_leaves_the_drawing_unmodified_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>SAVE|>NEW"
            "-DEXPECT=the drawing has not been saved to a project yet.*new drawing"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/failed_save_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A headless run has nobody to answer "save the changes first?": a NEW
    # after an edit is refused and said, and the edit stays - LIST still
    # counts the one line - rather than waiting on a box no one can see.
    add_test(NAME qt_a_headless_new_after_an_edit_is_refused_rather_than_asked_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LINE 0,0 10,0|>NEW|>LIST"
            "-DEXPECT=Unsaved Changes: the drawing has unsaved changes.*1 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/headless_new_refused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # The project records its customisation (ProjectMetadata::customisation):
    # the site plan saved with the symbol fixture loaded, then opened by a
    # second run that never loads it, which must say which file is missing -
    # a warning in the log, the project opened all the same.
    set(_recorded "${CMAKE_CURRENT_BINARY_DIR}/customisation_recorded.katana")
    set(_resaved "${CMAKE_CURRENT_BINARY_DIR}/customisation_resaved.katana")
    add_test(NAME qt_customisation_recorded_cleanup
        COMMAND ${CMAKE_COMMAND} -E rm -rf "${_recorded}" "${_resaved}")
    add_test(NAME qt_a_project_records_its_customisation_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE ${_fixture}/test_symbols.4d|>LINE 0,0 10,0|>SAVE ${_recorded}"
            "-DEXPECT=saved to "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_record_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_opening_names_the_customisation_not_loaded_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>OPEN ${_recorded}"
            "-DEXPECT=opened .*Warning: this project was drawn with customisation files that are not loaded: \"test_symbols\\.4d\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_missing_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Saved where the file is missing, the project still records it: the
    # open warns, the save (to a copy, so the test above reads its own
    # project) writes the record, and opening the copy warns again. By hand:
    # this run never loads test_symbols.4d, so nothing it does can judge it.
    add_test(NAME qt_saving_where_a_file_is_missing_keeps_it_recorded_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>OPEN ${_recorded}|>SAVE ${_resaved}|>NEW|>OPEN ${_resaved}"
            "-DEXPECT=not loaded: \"test_symbols\\.4d\".*saved to .*new drawing.*opened .*not loaded: \"test_symbols\\.4d\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_resaved_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_customisation_recorded_cleanup PROPERTIES
        FIXTURES_SETUP qt_recorded_clean)
    set_tests_properties(qt_a_project_records_its_customisation_headless PROPERTIES
        FIXTURES_REQUIRED qt_recorded_clean FIXTURES_SETUP qt_recorded)
    set_tests_properties(qt_opening_names_the_customisation_not_loaded_headless
                         qt_saving_where_a_file_is_missing_keeps_it_recorded_headless
        PROPERTIES FIXTURES_REQUIRED qt_recorded)

    # The current style (decision D9) and the Properties panel's Style row,
    # driven through their widgets. By hand: Kerb made current, a line is
    # drawn in it (entity 1); ByLayer made current, two lines are drawn
    # ByLayer (2 and 3); everything selected, the row given Kerb and applied:
    # only 2 and 3 change, as ONE command - Kerb holds 3 entities, one UNDO
    # takes it back to 1, and REDO to 3 again.
    add_test(NAME qt_current_style_and_the_style_row_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>STYLE NEW Kerb|%PropertiesToolBar|CurrentStyleCombo=Kerb|>LINE 0,9 10,9|>STYLE USAGE Kerb|CurrentStyleCombo=ByLayer|>LINE 0,0 10,0|>LINE 0,5 10,5|>SELECT ALL|%PropertiesDock|PropertyStyle=Kerb|!PropertyStyleApply|>STYLE USAGE Kerb|>UNDO|>STYLE USAGE Kerb|>REDO|>STYLE USAGE Kerb"
            "-DEXPECT=New work is drawn in Kerb.*style Kerb: used by 1 entity, e.g. id=1.*New work is drawn in ByLayer.*Style Kerb given to 2 entities.*style Kerb: used by 3 entities.*style Kerb: used by 1 entity.*style Kerb: used by 3 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/style_row_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_customisation_merges_and_keeps_the_map_headless
                         qt_customisation_recorded_cleanup
                         qt_a_project_records_its_customisation_headless
                         qt_opening_names_the_customisation_not_loaded_headless
                         qt_current_style_and_the_style_row_headless
                         qt_a_file_named_twice_in_one_load_is_read_once_headless
                         qt_a_save_that_cannot_go_ahead_leaves_the_drawing_unmodified_headless
                         qt_a_headless_new_after_an_edit_is_refused_rather_than_asked_headless
                         qt_saving_where_a_file_is_missing_keeps_it_recorded_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # ---- the menus: Format's managers, Purge Unused, the tools by alias -------
    #
    # Each manager opened THROUGH ITS FORMAT MENU ACTION (@ is --dialog), over
    # the hand-written fixture customisation merged onto whatever is built
    # in; the driver says what opened, and a manager opened modally would say
    # "(modal)" after its title, where each EXPECT wants the line to end. What each
    # manager then shows is read back with ? (--report).
    #
    # Styles: two styles made on the command line, so the table lists them.
    add_test(NAME qt_format_menu_opens_the_style_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=>STYLE NEW Kerb|>STYLE NEW Water|@formatStyles|?styleTable"
            "-DEXPECT=--dialog formatStyles opened styleManagerDialog \"Styles and Linetypes\"[\r\n]+styleTable: Kerb \\|[^;]*; Water \\|"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_style_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The same manager's Linetypes page (managerTabs, filled as a click on
    # its tab does) narrowed to the fixture's group Test/Lines. By hand from
    # test_linestyles.4d's head comment: that group holds TEST Dashed Kerb, a
    # paper linestyle, and TEST Gate, a two-point one, and nothing else. The
    # Kerb chosen, the details pane explains its kind in the general words.
    add_test(NAME qt_the_style_manager_lists_a_library_group_s_linestyles_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=@formatStyles|managerTabs=Linetypes|linetypeGroup=Test/Lines|linetypeTable=TEST Dashed Kerb|?linetypeTable|?libraryDetails"
            "-DEXPECT=linetypeTable: TEST Dashed Kerb \\| Library \\| Test/Lines \\| paper \\|[^;\r\n]*; TEST Gate \\| Library \\| Test/Lines \\| two-point \\|[^;\r\n]*[\r\n]+libraryDetails: TEST Dashed Kerb Group: Test/Lines File: test_linestyles\\.4d Kind: Paper linestyle: millimetres on the plot, the same at every scale[^\r\n]*Library linestyles belong to the session's customisation and are read-only here\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_style_manager_linetypes_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Symbols, searched for TEST. By hand from the fixture's head comments:
    # its four definitions are all symbols by decision D3 - TEST Tree too,
    # though it is not `mode vertex`, because rule TR* draws it as one - and
    # PX* asks for TEST Missing Symbol, which nothing defines and the library
    # lists as missing. Sorted by name, case folded, the five come together.
    add_test(NAME qt_format_menu_opens_the_symbol_library_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=@formatSymbols|filterText=TEST|?symbolGrid"
            "-DEXPECT=--dialog formatSymbols opened symbolLibraryDialog \"Symbol Library\"[\r\n]+symbolGrid: [^\r\n]*TEST Missing Symbol ; TEST Survey Mark ; TEST Tree ; TEST U Turn ; TEST Valve"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_symbol_library_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The code manager explaining a fixture code. By hand from
    # test_survey.mapfile: WM01 is met by the prefix key WM* (rule #0 of the
    # fixture, map_data), which puts it on the layer TEST SERVICES (the
    # rule's model field).
    add_test(NAME qt_format_menu_opens_the_code_manager_and_explains_a_code_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=@formatSurveyCodes|testCode=WM01|?explainHeading|?explainFields"
            "-DEXPECT=--dialog formatSurveyCodes opened surveyCodeManagerDialog \"Survey Code Manager\"[\r\n]+explainHeading: WM01: matched - met by the prefix key WM\\*[^\r\n]*[\r\n]+explainFields: model \\| TEST SERVICES \\| #[0-9]+ WM\\* \\(map_data\\)"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_code_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # What Format says in the real window, in general words: every item of
    # the menu with its status tip (?formatMenu) - the window's own Load and
    # Replace Loaded Customisation among the managers' - the log of a
    # customisation loaded and of each manager opened, and none of it names
    # another program's formats (FORBID, in either case). The widget tests
    # walk the managers' own texts (qt_widgets.GeneralTexts.*).
    add_test(NAME qt_the_format_menu_and_the_customisation_log_name_no_other_program_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=?formatMenu|@formatStyles|@formatSymbols|@formatSurveyCodes"
            "-DEXPECT=test_survey\\.mapfile: survey code file, [0-9,]+ added[^\r\n]*[\r\n].*formatMenu: Format: Layers\\.\\.\\. \\[[^\r\n]*Styles and Linetypes\\.\\.\\. \\[[^\r\n]*library linestyles[^\r\n]*Load Customisation\\.\\.\\. \\[Load linestyle and symbol libraries \\(\\.4d\\) and survey code files \\(\\.mapfile\\)[^\r\n]*Replace Loaded Customisation\\.\\.\\. \\[[^\r\n]*[\r\n].*opened surveyCodeManagerDialog"
            "-DFORBID=12[dD]"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_texts_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # FORBID holds the application to what it SAYS, not to where the run
    # lives. Here the output - and so the empty project the log says it
    # could not open - is in a folder named as a worktree for the archive
    # work might be: the log prints that path (EXPECT), and the check passes.
    add_test(NAME qt_a_forbidden_word_in_the_run_s_own_paths_is_not_held_against_it_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=%CommandLineDock"
            "-DEXPECT=Open Failed[^\r\n]*worktree_12da_export"
            "-DFORBID=12[dD]"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/worktree_12da_export/forbid_paths_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Only the path is set aside: the rest of the line that holds it is still
    # read. Forbidding the words that line opens with fails the run, and the
    # failure names them (PASS_REGULAR_EXPRESSION, set below).
    add_test(NAME qt_a_forbidden_word_beside_a_path_in_the_log_still_fails_the_run_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=%CommandLineDock"
            "-DFORBID=Open Failed"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/worktree_12da_export/forbid_beside_a_path_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_a_forbidden_word_beside_a_path_in_the_log_still_fails_the_run_headless
        PROPERTIES PASS_REGULAR_EXPRESSION
            "the run reported /Open Failed/ \\(\"Open Failed\"\\), its own paths aside")
    # The symbol library putting a fixture symbol on imported points: the 12
    # points of survey_points_pnezd.csv (counted by hand, see the survey
    # import test) imported, all selected, TEST Survey Mark chosen in the
    # grid and Assign pressed - one new style of that name holds all 12.
    add_test(NAME qt_the_symbol_library_puts_a_fixture_symbol_on_imported_points_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|!next|unit=metres|!next|!next|!import|>SELECT ALL|@formatSymbols|symbolGrid=TEST Survey Mark|?detailName|!assignToPoints|>STYLE USAGE TEST Survey Mark"
            "-DEXPECT=Imported 12 point\\(s\\).*detailName: TEST Survey Mark[\r\n]+12 points now in style \"TEST Survey Mark\" \\(new\\), drawing \"TEST Survey Mark\"\\..*style TEST Survey Mark: used by 12 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/symbol_assign_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Format > Purge Unused (* is --trigger, the menu item in its turn). By
    # hand: Spare is worn by nothing; Kerb is current and worn by the line;
    # a new drawing's continuous and none are protected. So one style goes,
    # and one UNDO brings it back. The grab is the command line, showing it.
    add_test(NAME qt_purge_unused_deletes_what_nothing_uses_as_one_undo_step_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${CMAKE_SOURCE_DIR}/tests/archive12d/data/customisation"
            "-DDRIVE=>STYLE NEW Spare|>STYLE NEW Kerb|%PropertiesToolBar|CurrentStyleCombo=Kerb|>LINE 0,0 10,0|*formatPurge|>STYLE USAGE Spare|>UNDO|>STYLE USAGE Spare|%CommandLineDock"
            "-DEXPECT=Purge Unused: purged 1 style, 0 linetypes and 0 hatch patterns \\(one Undo restores them\\)\\. Styles: Spare\\.[\r\n]+! [^\r\n]*Spare[^\r\n]*[\r\n]+1 undone[\r\n]+style Spare: unused"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/purge_unused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Format > Layers opens the Layers dialog BESIDE the drawing, not modal
    # (the opening line would end "(modal)"), and it follows the drawing: a
    # layer made on the command line while it is open appears in its table
    # after the one reload the DocumentWatcher makes. By hand: a new drawing
    # has the one layer "0"; LAYER NEW survey adds "survey", after it by name.
    add_test(NAME qt_format_layers_opens_beside_the_drawing_and_follows_it_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatLayers|?layerTable|>LAYER NEW survey|?layerTable"
            "-DEXPECT=--dialog formatLayers opened formatLayersDialog \"Layers\"[\r\n]+layerTable: 0 \\|[^;\r\n]*[\r\n].*layerTable: 0 \\|[^;\r\n]* ; survey \\|"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_layers_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A tool started by its alias on the command line and answered there: by
    # hand, LINE, 0,0 and 10,0, then Enter - one segment, so ONE line, which
    # the tool reports ("1 line") before starting again for the next chain;
    # Enter at its first prompt ends it, so LIST reaches the interpreter and
    # finds the one entity. ">" alone is Enter on an empty command line. The
    # menus and toolbars show it: Line's action checked while it runs, and
    # Select again once it has ended.
    add_test(NAME qt_a_tool_started_by_its_alias_draws_from_typed_points_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LINE|?draw.line|?toolSelect|>0,0|>10,0|>|>|?draw.line|?toolSelect|>LIST"
            "-DEXPECT=draw.line: Line, checked[\r\n]+toolSelect: Select, unchecked[\r\n]+1 line[\r\n]+draw.line: Line, unchecked[\r\n]+toolSelect: Select, checked[\r\n]+1 entities[\r\n]+1  Line  layer=0  0,0 -> 10,0  length=10"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/tool_by_alias_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A tool that cannot start - the only view shows a section, so there is
    # no plan view to draw in - is left unchecked, with Select checked: the
    # click checks a tool's action before the start is refused, and a
    # refusal changes no tool, so nothing else would put the marks right.
    add_test(NAME qt_a_tool_refused_for_want_of_a_plan_view_is_left_unchecked_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=*viewShowsSection|*draw.line|?draw.line|?toolSelect"
            "-DEXPECT=there is no plan view to draw in[^\r\n]*[\r\n]+draw.line: Line, unchecked[\r\n]+toolSelect: Select, checked"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/refused_tool_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Quitting while the Survey Code Manager holds unapplied rule edits: they
    # live only in its buffer, so a headless QUIT, with nobody to ask, is
    # refused and said - as a quit with unsaved drawing changes is - rather
    # than the edits going with the window. By hand: the built-in map's 1,624
    # rules and a duplicate of rule 0 make 1625 in the buffer.
    add_test(NAME qt_quitting_with_unapplied_code_manager_edits_is_refused_and_said_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatSurveyCodes|!ruleDuplicate|?dirtyIndicator|>QUIT"
            "-DEXPECT=dirtyIndicator: Unapplied edits:[^\r\n]* 1625 rules[.][\r\n]+! Unapplied Edits: the Survey Code Manager has rule edits that are not on the drawing"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/quit_with_code_edits_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The tools a CAD user reaches for daily, in the menus the catalogue
    # makes (src/katana_qt/tools/tool_menus.cpp): a Tools menu made beside
    # the window's tool menus for the Inquiry and Select groups, Divide and
    # Measure in Draw, Lengthen, Reverse and Match Properties in Modify. Each
    # menu is opened for its screenshot and read back item by item.
    add_test(NAME qt_the_tools_menu_holds_the_inquiry_and_select_tools_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=%toolsMenu|?toolsMenu"
            "-DEXPECT=toolsMenu: Tools: Distance .*; Area .*; ID Point .*; Angle .*; List .*; Select Similar .*; Quick Select "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/tools_menu_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_the_draw_menu_offers_divide_and_measure_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=%drawMenu|?drawMenu"
            "-DEXPECT=drawMenu: Draw: Point .*; Arc \\[Arc: [^]]*\\] ; .*Divide .*; Measure "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/draw_menu_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_the_modify_menu_offers_lengthen_reverse_and_match_properties_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=%modifyMenu|?modifyMenu"
            "-DEXPECT=modifyMenu: Modify: Move .*; Explode .*; Lengthen .*; Reverse .*; Match Properties "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/modify_menu_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # An inquiry tool driven from the command line: DIST, then two typed
    # points, and the inverse of the 3-4-5 leg in the log - 5.000 by hand.
    # ZOOM typed between them is taken by the tool host as the view's
    # command (ToolHost::onTransparent), never handed to the tool as a
    # malformed point, and the tool carries on at the same step. No view
    # runs it yet, so the host refuses it aloud; the EXPECT holds that
    # refusal before the inverse, because a ZOOM wrongly sent to the tool
    # is refused by the tool instead ("'ZOOM' is not an input this tool
    # takes now") and the inverse still follows, which the result alone
    # cannot tell apart. When the view sets onTransparent the refusal
    # becomes the prompt shown again, and this EXPECT with it.
    add_test(NAME qt_dist_typed_at_the_command_line_reports_the_inverse_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>DIST|>0,0|>ZOOM|>3,4"
            "-DEXPECT=! ZOOM cannot run inside Distance; press Esc to end the tool first[.].*Horizontal distance 5[.]000"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/dist_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Quick Select is a form asked at the command line, one question at a
    # time with a default for Enter, so a headless run fills it as a person
    # does and no dialog waits on anyone: kind Line, Enter for any layer and
    # for no condition. One line drawn, one selected.
    add_test(NAME qt_quick_select_is_answered_at_the_command_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LINE 0,0 10,0|>CIRCLE 5,5 2|>SELECT NONE|>QSELECT|>Line|>|>"
            "-DEXPECT=1 entity selected: Lines on any layer, in the drawing"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/quick_select_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Every key sequence the window answers to reaches one thing: Qt
    # disables an ambiguous shortcut for BOTH actions, so a clash is two
    # keys that silently do nothing; and within each menu every underlined
    # letter is one item's (--check-shortcuts).
    add_test(NAME qt_every_shortcut_and_menu_letter_reaches_one_thing_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCHECK_SHORTCUTS=ON"
            "-DEXPECT=shortcuts: [0-9]+ key sequences, each reaching one thing"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/shortcuts_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # View's Window section: the text size, the toolbar icons and names, and
    # Reset Window Layout, each a menu item a headless run reaches by name,
    # each saying what it did (docs/desktop.md, "How the window starts").
    # A headless run saves none of it (restoreSession is never called).
    add_test(NAME qt_the_window_section_sizes_the_text_and_toolbars_and_resets_the_layout_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=*viewTextSizeLarge|?viewTextSizeLarge|?viewTextSizeStandard|*viewToolBarIconsLarge|?viewToolBarIconsLarge|*viewToolBarNames|?viewToolBarNames|*viewPanelLayers|*viewResetLayout|?viewPanelLayers|?viewMenu"
            "-DEXPECT=Text size: large\\..*viewTextSizeLarge: Large, checked.*viewTextSizeStandard: Standard, unchecked.*viewToolBarIconsLarge: Large Icons, checked.*viewToolBarNames: Show Toolbar Names, unchecked.*The panels and toolbars are back where a new window has them\\..*viewPanelLayers: Layers, checked.*viewMenu: View: .*Toolbars \\[.*Text Size \\[.*Reset Window Layout \\["
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/window_section_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Every menu item, in every submenu, has an icon to find it by and a
    # status tip saying what it does (--check-menus, MainWindow::menuGaps):
    # until 2026-09-26 sixty of them had neither or one.
    add_test(NAME qt_every_menu_item_has_an_icon_and_a_status_tip_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCHECK_MENUS=ON"
            "-DEXPECT=menus: [0-9]+ items, each with an icon and a status tip"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/menus_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_format_menu_opens_the_style_manager_headless
                         qt_the_style_manager_lists_a_library_group_s_linestyles_headless
                         qt_format_menu_opens_the_symbol_library_headless
                         qt_format_menu_opens_the_code_manager_and_explains_a_code_headless
                         qt_the_format_menu_and_the_customisation_log_name_no_other_program_headless
                         qt_a_forbidden_word_in_the_run_s_own_paths_is_not_held_against_it_headless
                         qt_a_forbidden_word_beside_a_path_in_the_log_still_fails_the_run_headless
                         qt_the_symbol_library_puts_a_fixture_symbol_on_imported_points_headless
                         qt_purge_unused_deletes_what_nothing_uses_as_one_undo_step_headless
                         qt_format_layers_opens_beside_the_drawing_and_follows_it_headless
                         qt_a_tool_started_by_its_alias_draws_from_typed_points_headless
                         qt_a_tool_refused_for_want_of_a_plan_view_is_left_unchecked_headless
                         qt_quitting_with_unapplied_code_manager_edits_is_refused_and_said_headless
                         qt_the_tools_menu_holds_the_inquiry_and_select_tools_headless
                         qt_the_draw_menu_offers_divide_and_measure_headless
                         qt_the_modify_menu_offers_lengthen_reverse_and_match_properties_headless
                         qt_dist_typed_at_the_command_line_reports_the_inverse_headless
                         qt_quick_select_is_answered_at_the_command_line_headless
                         qt_every_shortcut_and_menu_letter_reaches_one_thing_headless
                         qt_every_menu_item_has_an_icon_and_a_status_tip_headless
                         qt_the_window_section_sizes_the_text_and_toolbars_and_resets_the_layout_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
    # The quitting check counts the built-in map's rules, which come from the
    # third-party customisation (src/katana_archive12d/CMakeLists.txt). A
    # checkout without it has no rule to duplicate, so the check is skipped
    # there rather than failed, as the suites that read it skip
    # (tests/archive12d/CMakeLists.txt): the suite stays green without it.
    if(NOT EXISTS "${KATANA_CUSTOMISATION_DIR}/survey_codes.mapfile")
        set_tests_properties(qt_quitting_with_unapplied_code_manager_edits_is_refused_and_said_headless
            PROPERTIES DISABLED TRUE)
    endif()

    set_tests_properties(qt_survey_inverse_headless qt_survey_area_headless
                         qt_survey_forward_headless qt_survey_angle_headless
                         qt_survey_traverse_headless qt_survey_level_book_headless
                         qt_survey_converter_headless qt_survey_import_headless
                         qt_survey_import_uncertain_headless
                         qt_survey_import_role_boxes_swap_headless
                         qt_survey_export_round_trip_headless
                         qt_survey_import_transform_headless
                         qt_survey_point_report_headless
                         qt_survey_templates_reach_export_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# A .dxf through the window both ways goes to the native module, not GDAL:
# the hand-written R2000 fixture imported (9 entities - see
# tests/dxf/test_reader.cpp) and the drawing exported from the command line,
# each reported in the records every build writes (docs/interop.md).
if(TARGET katana)
    add_test(NAME qt_dxf_import_and_export_go_through_the_native_module_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_dxf"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/dxf/data/r2000_site.dxf"
            "-DDRIVE=>EXPORT ${CMAKE_CURRENT_BINARY_DIR}/qt_dxf_export.dxf"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_dxf_headless.png"
            "-DEXPECT=imported file=\"?[^ ]*/r2000_site\\.dxf\"? kind=dxf format=\"DXF R2000\" entities=9 .*exported file=\"?[^ ]*/qt_dxf_export\\.dxf\"? kind=dxf driver=DXF format=\"DXF R2000\" entities=9 skipped=0 layers=3 "
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_dxf_import_and_export_go_through_the_native_module_headless PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # IFC both ways in the window (docs/ifc.md, "In the window"), natively.
    # The drawing is samples/ifc/scenario.txt typed line by line - MC01, a
    # kerb, a fence, a pit, a point and a text - and the counts are the ones
    # tests/ifc/test_export.cpp works out for the sample schedule: 4 services,
    # 10 graded segments, 6 of them in 3D, 14 located points; read back, the
    # 29 objects, and MC01 again as "MC01 (2)" beside the drawing's own.
    file(STRINGS "${PROJECT_SOURCE_DIR}/samples/ifc/scenario.txt" _ifc_scenario_lines)
    set(_ifc_scenario "")
    foreach(_line IN LISTS _ifc_scenario_lines)
        if(NOT _line STREQUAL "" AND NOT _line MATCHES "^#")
            string(APPEND _ifc_scenario ">${_line}|")
        endif()
    endforeach()
    set(_ifc_schedule "${PROJECT_SOURCE_DIR}/samples/utilities/schedule.csv")
    set(_ifc_typed "${CMAKE_CURRENT_BINARY_DIR}/qt ifc typed.ifc")
    add_test(NAME qt_ifc_typed_export_and_import_are_the_command_lines_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DDRIVE=${_ifc_scenario}>EXPORT ${_ifc_typed} UTILITIES ${_ifc_schedule}|>INFO ${_ifc_typed}|>IMPORT ${_ifc_typed}|>ALIGN LIST"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_typed_headless.png"
            "-DEXPECT=ifc exported file=\"qt ifc typed\\.ifc\" schema=IFC4X3_ADD2 instances=[0-9]+ bytes=[0-9]+[\r\n]+counts alignments=1 services=4 segments=10 segments_3d=6 located_points=14 entities_written=5 entities_skipped=0 surfaces=0.*ifc described file=\"qt ifc typed\\.ifc\" schema=IFC4X3_ADD2 crs=\"EPSG:7856\".*ifc imported file=\"qt ifc typed\\.ifc\" schema=IFC4X3_ADD2 crs=\"EPSG:7856\" entities=29 alignments=1 surfaces=0.*the import's is \\\\\"MC01 \\(2\\)\\\\\".*MC01 \\(2\\)  4 PIs  1000\\.000 to 1339\\.016"
            "-DFORBID=IfcBuildingElementProxy"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # File > Export IFC, driven as a person would: the schedule chosen, the
    # preview's table - the writer's account, layer by layer and service by
    # service - then the file written. The dialog is non-modal: --dialog
    # must not report it modal.
    set(_ifc_dialog "${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_dialog.ifc")
    add_test(NAME qt_ifc_export_dialog_previews_each_class_and_writes_the_file_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DDRIVE=${_ifc_scenario}@fileExportIfc|ifcExportSchedule=${_ifc_schedule}|!ifcExportPreview|?ifcExportClasses|ifcExportFile=${_ifc_dialog}|!ifcExportExport|?ifcExportCheck"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_export_dialog_headless.png"
            "-DEXPECT=--dialog fileExportIfc opened fileExportIfcDialog \"Export IFC\"[\r\n].*> EXPORT \"preview\\.ifc\" UTILITIES \"[^\"]*schedule\\.csv\" PREVIEW[\r\n]+ifc previewed file=\"preview\\.ifc\".*ifcExportClasses: [^\r\n]*layer Survey/Kerb \\| 1 \\| IfcKerb NOTDEFINED \\| \\| rule kerb[^\r\n]*service E1 \\| 3 \\| IfcCableCarrierSegment CONDUITSEGMENT \\| ELECTRICAL.*> EXPORT \"[^\"]*qt_ifc_dialog\\.ifc\" UTILITIES \"[^\"]*schedule\\.csv\"[\r\n]+ifc exported file=\"qt_ifc_dialog\\.ifc\" schema=IFC4X3_ADD2 instances=[0-9]+ bytes=[0-9]+[\r\n]+counts alignments=1 services=4 segments=10 segments_3d=6 located_points=14 entities_written=5.*ifcExportCheck: Wrote [^\r\n]*qt_ifc_dialog\\.ifc: [0-9]+ instances"
            "-DFORBID=IfcBuildingElementProxy"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # File > Import IFC on that file, into a drawing with no coordinate
    # system: described, imported, and the project takes the file's EPSG:7856
    # (the dialog's default), as its own undoable step.
    add_test(NAME qt_ifc_import_dialog_describes_imports_and_takes_the_files_system_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DDRIVE=@fileImportIfc|ifcImportFile=${_ifc_dialog}|!ifcImportDescribe|?ifcImportSummary|!ifcImportImport|>ALIGN LIST"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_import_dialog_headless.png"
            "-DEXPECT=--dialog fileImportIfc opened fileImportIfcDialog \"Import IFC\"[\r\n].*> INFO \"[^\"]*qt_ifc_dialog\\.ifc\".*ifcImportSummary: ifc described file=\"qt_ifc_dialog\\.ifc\" schema=IFC4X3_ADD2 crs=\"EPSG:7856\".*alignment name=\"MC01\" pis=4 pvis=4.*> IMPORT \"[^\"]*qt_ifc_dialog\\.ifc\" TAKECRS[\r\n]+ifc imported file=\"qt_ifc_dialog\\.ifc\" schema=IFC4X3_ADD2 crs=\"EPSG:7856\" entities=29 alignments=1 surfaces=0.*note text=\"the project is now in EPSG:7856.*MC01  4 PIs  1000\\.000 to 1339\\.016"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The session's surfaces go out as IFC terrain and come back as surfaces
    # of the session: the four TINs of a .12da archive, there and back. Each
    # is added to the session (its "Surface" line) before the import's reply
    # is logged.
    add_test(NAME qt_ifc_surfaces_of_the_session_go_out_and_come_back_as_surfaces_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/multiple_tins.12da"
            "-DDRIVE=>EXPORT ${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_surfaces.ifc|>IMPORT ${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_surfaces.ifc"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_surfaces_headless.png"
            "-DEXPECT=ifc exported file=\"qt_ifc_surfaces\\.ifc\".*counts alignments=0 services=0 segments=0 segments_3d=0 located_points=0 entities_written=0 entities_skipped=0 surfaces=4.*class name=IfcGeographicElement count=4.*Surface '[^']+': [0-9]+ vertices, [0-9]+ triangles.*ifc imported file=\"qt_ifc_surfaces\\.ifc\" schema=IFC4X3_ADD2 crs=\"[^\"]*\" entities=0 alignments=0 surfaces=4"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The File menu offers both, and a typed line with a word the verb does
    # not take is refused in the command line's own words.
    add_test(NAME qt_ifc_menu_items_and_the_verbs_refusals_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DDRIVE=%fileMenu|?fileMenu|>EXPORT ${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_refused.ifc UTILITIS x.csv|>IMPORT ${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_refused.ifc LOCALLY|>EXPORT \"${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_unclosed.ifc"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_menu_headless.png"
            "-DEXPECT=fileMenu: [^\r\n]*Import IFC\\.\\.\\. \\[Import an IFC file[^\r\n]*Export IFC\\.\\.\\. \\[Export IFC 4\\.3.*! InvalidArgument: \"UTILITIS\" is not an option of EXPORT <file\\.ifc>: UTILITIES <schedule\\.csv>, SCHEMA <schema\\.csv>, RULES <rules\\.csv>, SPACING <m>, NODRAWING, NOENTITIES, SELECTED, NOALIGNMENTS, NOSURFACES, PREVIEW.*! InvalidArgument: \"LOCALLY\" is not an option of IMPORT <file\\.ifc>: LOCAL, NOALIGNMENTS.*! InvalidArgument: a quoted path is never closed"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # --import-options on a .ifc builds File > Import IFC's dialog for it.
    add_test(NAME qt_ifc_import_options_are_the_import_dialog_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DIMPORT_OPTIONS=${_ifc_dialog}"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_import_options_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A path given to the window is imported as File > Import would, and a
    # headless session is told how to take the file's system, not asked.
    add_test(NAME qt_ifc_a_path_given_to_the_window_is_imported_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DIMPORT=${_ifc_dialog}"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_path_headless.png"
            "-DEXPECT=> IMPORT \"[^\"]*qt_ifc_dialog\\.ifc\"[\r\n]+ifc imported file=\"qt_ifc_dialog\\.ifc\" schema=IFC4X3_ADD2 crs=\"EPSG:7856\" entities=29 alignments=1 surfaces=0.*note text=\"the file is in EPSG:7856 and the project has no coordinate system: File > Project Coordinate System or CRS SET EPSG:7856 sets it\""
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A drawing of an alignment alone is as far from a file as one of
    # lines: the far-apart question weighs the drawing's alignments too, and
    # headless keeps the survey coordinates and says so.
    add_test(NAME qt_ifc_a_file_far_from_the_drawings_alignment_is_questioned_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DDRIVE=>ALIGN NEW A1 0,0 100,50 200,0|>IMPORT ${_ifc_dialog}"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_far_headless.png"
            "-DEXPECT=ifc imported file=\"qt_ifc_dialog\\.ifc\" schema=IFC4X3_ADD2 crs=\"EPSG:7856\" entities=29 alignments=1 surfaces=0.*note text=\"[^\"]*\\(kept: no one to ask\\)\""
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # GIS > Dataset Information on a .ifc describes it; on one that cannot be
    # read, it and --import-options fail the run saying why, as they do for
    # the GIS formats.
    add_test(NAME qt_ifc_dataset_information_describes_the_file_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DDATASET_INFO=${_ifc_dialog}"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_dataset_info_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_ifc_a_missing_file_has_no_description_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DDATASET_INFO=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_no_such_file.ifc"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_missing_info_headless.png"
            "-DREFUSED=NotFound: the file cannot be read"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_ifc_a_missing_file_has_no_import_options_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DIMPORT_OPTIONS=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_no_such_file.ifc"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_missing_options_headless.png"
            "-DREFUSED=NotFound: the file cannot be read"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_ifc_export_dialog_previews_each_class_and_writes_the_file_headless
        PROPERTIES FIXTURES_SETUP qt_ifc_dialog_file)
    set_tests_properties(qt_ifc_import_dialog_describes_imports_and_takes_the_files_system_headless
                         qt_ifc_import_options_are_the_import_dialog_headless
                         qt_ifc_a_path_given_to_the_window_is_imported_headless
                         qt_ifc_a_file_far_from_the_drawings_alignment_is_questioned_headless
                         qt_ifc_dataset_information_describes_the_file_headless
        PROPERTIES FIXTURES_REQUIRED qt_ifc_dialog_file)
    set_tests_properties(qt_ifc_typed_export_and_import_are_the_command_lines_headless
                         qt_ifc_export_dialog_previews_each_class_and_writes_the_file_headless
                         qt_ifc_import_dialog_describes_imports_and_takes_the_files_system_headless
                         qt_ifc_surfaces_of_the_session_go_out_and_come_back_as_surfaces_headless
                         qt_ifc_menu_items_and_the_verbs_refusals_headless
                         qt_ifc_import_options_are_the_import_dialog_headless
                         qt_ifc_a_path_given_to_the_window_is_imported_headless
                         qt_ifc_a_file_far_from_the_drawings_alignment_is_questioned_headless
                         qt_ifc_dataset_information_describes_the_file_headless
                         qt_ifc_a_missing_file_has_no_description_headless
                         qt_ifc_a_missing_file_has_no_import_options_headless
        PROPERTIES
            ENVIRONMENT_MODIFICATION
                "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
            TIMEOUT 180)
    # The window's file judged by IfcOpenShell (tools/check_ifc.py), where the
    # configuring Python has it - as the command line's files are
    # (src/katana_app/CMakeLists.txt).
    if(Python3_Interpreter_FOUND)
        execute_process(COMMAND "${KATANA_PYTHON}" -c "import ifcopenshell"
            RESULT_VARIABLE _ifcopenshell_missing OUTPUT_QUIET ERROR_QUIET)
        if(_ifcopenshell_missing EQUAL 0)
            add_test(NAME qt_ifc_the_windows_export_is_valid_to_ifcopenshell
                COMMAND "${KATANA_PYTHON}" "${PROJECT_SOURCE_DIR}/tools/check_ifc.py" "${_ifc_dialog}")
            set_tests_properties(qt_ifc_the_windows_export_is_valid_to_ifcopenshell
                PROPERTIES FIXTURES_REQUIRED qt_ifc_dialog_file)
        endif()
    endif()
endif()

# The 3D view at the owner's display scale, 125%: the framebuffer must be in
# device pixels (500 x 375 for a 400 x 300 view), not the logical size scaled
# up, which beaded every 1 px line (docs/render.md, HiDPI). The widget suite
# runs this test at ratio 1, where it cannot tell the two apart.
if(TARGET katana_qt_widget_tests)
    add_test(NAME qt_render_view_at_125_percent
        COMMAND katana_qt_widget_tests
            --gtest_filter=RenderView.TheFramebufferIsTheViewsSizeInDevicePixels)
    set_tests_properties(qt_render_view_at_125_percent PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;QT_SCALE_FACTOR=set:1.25"
        TIMEOUT 120)
endif()

# The session verbs on the window's command line and the window's one
# executor (docs/desktop.md, "One executor: the command runner"). Typed with
# --command as an agent types them: INFO with an entity's id, CODE CENSUS,
# MAPFILE CHECK, CUSTOMISE alone, COPC and IMPORT ... LOCAL, each of which the
# window once refused or misread. Run with --run-line as a dialog runs its
# line: never a running tool's answer, with what it logged coming back.
if(TARGET katana)
    set(_gis "${PROJECT_SOURCE_DIR}/samples/gis")
    set(_fixture "${PROJECT_SOURCE_DIR}/tests/archive12d/data/customisation")
    # A 10 x 5 rectangle: perimeter 30, area 50, described by INFO 1 and
    # again by INFO #1 (a typed line's echo is not printed). The fixture survey code file
    # holds 11 rules over 8 distinct codes (counted by hand in its head
    # comment), none of which cannot be applied as written; the rectangle
    # carries no code.
    add_test(NAME qt_the_session_verbs_run_on_the_windows_command_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>RECT 0,0 10,5|>INFO 1|>INFO #1|>CUSTOMISE REPLACE \"${_fixture}/test_survey.mapfile\"|>CODE CENSUS|>MAPFILE CHECK|>CUSTOMISE"
            "-DEXPECT=1  Polyline  layer=0  vertices=4  closed  length=30  area=50[\r\n]+1  Polyline  layer=0  vertices=4  closed  length=30  area=50.*0 entities carry a code in \"[^\"]*\", 0 distinct codes.*11 rules checked: 0 errors, [0-9]+ warnings?.*11 survey code rules over 8 distinct codes[\r\n]+Loaded files, in load order:.*\"test_survey\\.mapfile\", a survey code file"
            "-DFORBID=unknown command|usage: CUSTOMISE"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/session_verbs_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # INFO #1 - what Entity Information sends - is the entity in a working
    # directory holding files called 1 and #1, where it was once read as a
    # file: "no importer reads files named ''".
    set(_info_dir "${CMAKE_CURRENT_BINARY_DIR}/qt_info_beside_files")
    file(WRITE "${_info_dir}/1" "")
    file(WRITE "${_info_dir}/#1" "")
    add_test(NAME qt_info_hash_id_is_the_entity_beside_a_file_of_that_name_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>RECT 0,0 10,5|>INFO #1"
            "-DEXPECT=1  Polyline  layer=0  vertices=4  closed  length=30  area=50"
            "-DFORBID=no importer"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/info_beside_files_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake"
        WORKING_DIRECTORY "${_info_dir}")
    # COPC typed, into a folder name with blanks, then described: every point
    # of the sample's 40 000 (`pdal info`, as cli.copc_then_info) kept, and
    # the converted record naming the file. One path is a usage error; the
    # menu item, whose two file dialogs no headless session can answer,
    # points at the verb instead of waiting on them.
    set(_window_copc "${CMAKE_CURRENT_BINARY_DIR}/qt copc/window scan.copc.laz")
    add_test(NAME qt_copc_cleanup
        COMMAND ${CMAKE_COMMAND} -E rm -rf "${CMAKE_CURRENT_BINARY_DIR}/qt copc")
    add_test(NAME qt_copc_folder
        COMMAND ${CMAKE_COMMAND} -E make_directory "${CMAKE_CURRENT_BINARY_DIR}/qt copc")
    add_test(NAME qt_copc_typed_on_the_windows_command_line_converts_the_cloud_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>COPC \"${_gis}/survey_scan.las\" \"${_window_copc}\"|>INFO \"${_window_copc}\"|>COPC \"${_gis}/survey_scan.las\"|*convertCopc"
            "-DEXPECT=converted source=\"?[^ ]*/survey_scan\\.las\"? file=\"[^\"]*/qt copc/window scan\\.copc\\.laz\" format=copc.*pointcloud points=40000 [^\r\n]*copc=yes.*usage: COPC <source> <destination\\.copc\\.laz>.*A headless session opens no file dialog: type COPC <source> <destination\\.copc\\.laz> instead"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/copc_typed_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_copc_cleanup PROPERTIES FIXTURES_SETUP qt_copc_clean)
    set_tests_properties(qt_copc_folder PROPERTIES
        FIXTURES_REQUIRED qt_copc_clean FIXTURES_SETUP qt_copc)
    set_tests_properties(qt_copc_typed_on_the_windows_command_line_converts_the_cloud_headless
        PROPERTIES FIXTURES_REQUIRED qt_copc)
    # IMPORT ... LOCAL typed, the path quoted: parcels.geojson spans (180, 0)
    # to (365, 165) (its coordinates, by hand), so it is moved by -180,0 and
    # spans (0, 0) to (185, 165); nobody is asked where to put it. A raster's
    # LOCAL is refused by name, as katana_cli refuses it.
    add_test(NAME qt_import_local_typed_on_the_windows_command_line_moves_the_data_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>IMPORT \"${_gis}/parcels.geojson\" LOCAL|>IMPORT ${_gis}/terrain.asc local"
            "-DEXPECT=imported file=\"?[^ ]*/parcels\\.geojson\"? kind=vector entities=9 [^\r\n]*bounds=0,0,185,165[^\r\n]*[\r\n]+placed placement=local east=-180 north=0 text=\"LOCAL: moved as one piece by -180\\.000,0\\.000, so its lower-left corner sits at 0,0\\.\".*LOCAL is not supported for rasters and point clouds"
            "-DFORBID=No importer|Far from|no one to ask"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_local_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # ALONGSIDE and OFFSET typed as an agent types them, each moving the
    # parcels (180,0 to 365,165) as one piece and saying so; a DXF placed
    # the same way; a raster refusing a placement by name, and a mistyped
    # offset refused before anything is read.
    add_test(NAME qt_import_placement_typed_on_the_windows_command_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>RECT 1000,2000 1010,2005|>IMPORT \"${_gis}/parcels.geojson\" ALONGSIDE|>UNDO|>IMPORT ${_gis}/parcels.geojson OFFSET=10,-20.5|>IMPORT \"${PROJECT_SOURCE_DIR}/tests/dxf/data/r2000_site.dxf\" local|>IMPORT ${_gis}/terrain.asc ALONGSIDE|>IMPORT ${_gis}/parcels.geojson OFFSET=ten,0"
            "-DEXPECT=bounds=1000,2000,1185,2165[^\r\n]*[\r\n]+placed placement=alongside east=820 north=2000 text=\"ALONGSIDE: moved as one piece by 820\\.000,2000\\.000, so its lower-left corner sits on the drawing's, at 1000\\.000,2000\\.000\\.\".*bounds=190,-20\\.5,375,144\\.5[^\r\n]*[\r\n]+placed placement=offset east=10 north=-20\\.5 text=\"OFFSET: moved as one piece by 10\\.000,-20\\.500\\.\".*imported file=\"?[^ ]*/r2000_site\\.dxf\"? kind=dxf format=\"DXF R2000\" entities=9 [^\r\n]*[\r\n]+placed placement=local [^\r\n]*text=\"LOCAL: moved as one piece by [^\r\n]*, so its lower-left corner sits at 0,0\\.\".*ALONGSIDE is not supported for rasters and point clouds.*OFFSET= takes the east and north"
            "-DFORBID=No importer|Far from"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/import_placement_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A dialog's line while a tool waits for a point: the CIRCLE is drawn,
    # not taken for the Line's first point, its reply comes back, and the
    # typed points then draw the line as ever - entity 2, from 0,0 to 10,0;
    # the first Enter ends the chain, the second the tool. A refused line
    # comes back not ok, with why.
    add_test(NAME qt_a_dialogs_line_is_run_never_given_to_a_running_tool_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LINE|<CIRCLE 5,5 2|>0,0|>10,0|>|>|<CIRCLE 0,0 -1|>LIST"
            "-DEXPECT=--run-line CIRCLE 5,5 2: ok=yes[\r\n]+  reply: circle created.*--run-line CIRCLE 0,0 -1: ok=no[\r\n]+  error: InvalidGeometry: [^\r\n]*radius.*2 entities[\r\n]+1  Circle  layer=0  centre 5,5  radius=2 [^\r\n]*[\r\n]+2  Line  layer=0  0,0 -> 10,0  length=10"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/run_line_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_session_verbs_run_on_the_windows_command_line_headless
        qt_info_hash_id_is_the_entity_beside_a_file_of_that_name_headless
        qt_copc_typed_on_the_windows_command_line_converts_the_cloud_headless
        qt_import_local_typed_on_the_windows_command_line_moves_the_data_headless
        qt_import_placement_typed_on_the_windows_command_line_headless
        qt_a_dialogs_line_is_run_never_given_to_a_running_tool_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# File > Run Script, the SCRIPT verb and --script (docs/desktop.md, "Run
# Script"): a katana_cli script run in the window, each line through the one
# executor. good.kcs holds three commands among comments and blank lines, on
# its lines 4, 6 and 8; bad.kcs's second command, on line 5, is a circle of
# radius -1, which the interpreter refuses. A headless run prints what each
# line logged (not its echo), and the run ends with its record.
if(TARGET katana)
    set(_scripts "${CMAKE_CURRENT_SOURCE_DIR}/qt_widgets/data/scripts")
    # --script as a step, then LIST: every command ran, in order.
    add_test(NAME qt_script_switch_runs_every_line_of_a_script_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSCRIPT=${_scripts}/good.kcs"
            "-DDRIVE=>LIST"
            "-DEXPECT=rectangle created[\r\n]+circle created[\r\n]+line created[\r\n]+script=\"[^\"]*/good\\.kcs\" lines=3 ran=3 failed=0[\r\n]+3 entities"
            "-DFORBID=unknown command|a line that was refused"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/script_switch_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A refused line stops the run there and fails it: the third command is
    # never run (ran=2).
    add_test(NAME qt_script_switch_stops_at_the_first_refused_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSCRIPT=${_scripts}/bad.kcs"
            "-DREFUSED=rectangle created[\r\n]+[^!]*! InvalidGeometry: circle radius must be positive \\[-1\\][\r\n]+script=\"[^\"]*/bad\\.kcs\" lines=3 ran=2 failed=1 stopped_at=5[\r\n]+! bad\\.kcs stopped at line 5, which was refused: CIRCLE 0,0 -1[\r\n]+--script [^\r\n]*bad\\.kcs stopped at a line that was refused"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/script_switch_refused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Typed, with CONTINUE: every line runs and the refused one is counted,
    # leaving the rectangle and the line. A typed '#' line is a note that
    # runs nothing (the interpreter would have called it an unknown command);
    # SCRIPT alone is its usage, and a missing file is NotFound by its path.
    add_test(NAME qt_typed_script_continue_runs_every_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>SCRIPT \"${_scripts}/bad.kcs\" continue|># a note, not a command|>LIST|>SCRIPT|>SCRIPT \"${_scripts}/no_such_script.kcs\""
            "-DEXPECT=rectangle created[\r\n]+[^!]*! InvalidGeometry[^\r\n]*[\r\n]+line created[\r\n]+script=\"[^\"]*/bad\\.kcs\" lines=3 ran=3 failed=1[\r\n]+! bad\\.kcs: 1 of the 3 lines run were refused\\.[\r\n]+2 entities.*! InvalidArgument: usage: SCRIPT <file\\.kcs> \\[CONTINUE\\][\r\n]+! NotFound: no such script \\[[^\r\n]*no_such_script\\.kcs\\]"
            "-DFORBID=unknown command"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/typed_script_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A script whose second line runs the script itself: refused as already
    # running, so the run stops there rather than recursing for ever, and
    # its third line never runs.
    set(_runs_itself "${CMAKE_CURRENT_BINARY_DIR}/qt_scripts/runs_itself.kcs")
    file(WRITE "${_runs_itself}" "RECT 0,0 1,1\nSCRIPT \"${_runs_itself}\"\nLINE 0,0 1,0\n")
    add_test(NAME qt_a_script_may_not_run_itself_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>SCRIPT \"${_runs_itself}\"|>LIST"
            "-DEXPECT=rectangle created[\r\n]+! SCRIPT: runs_itself\\.kcs is already running; a script may not run itself\\.[\r\n]+script=\"[^\"]*/runs_itself\\.kcs\" lines=3 ran=2 failed=1 stopped_at=2[\r\n]+! runs_itself\\.kcs stopped at line 2, which was refused: SCRIPT [^\r\n]*[\r\n]+1 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/script_runs_itself_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # File > Run Script, driven as a person drives it: the dialog shows the
    # file's commands with their line numbers and the SCRIPT line it will
    # run; Browse, headless, says what to do instead; Run runs the line
    # through the one executor and says how it ended.
    add_test(NAME qt_run_script_dialog_runs_the_line_it_shows_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@fileRunScript|scriptPath=${_scripts}/good.kcs|scriptContinueOnError=on|?scriptPreview|?scriptCommand|!scriptBrowse|?scriptStatus|!scriptRun|?scriptStatus|>LIST"
            "-DEXPECT=opened fileRunScriptDialog \"Run Script\"[\r\n]+scriptPreview: 4 RECT 0,0 10,5 6 CIRCLE 5,5 2 8 LINE 0,0 10,0[\r\n]+scriptCommand: SCRIPT \"[^\"]*/good\\.kcs\" CONTINUE[\r\n]+scriptStatus: A headless session opens no file dialog: fill scriptPath with the path instead\\.[\r\n]+rectangle created[\r\n]+circle created[\r\n]+line created[\r\n]+script=\"[^\"]*/good\\.kcs\" lines=3 ran=3 failed=0[\r\n]+scriptStatus: Done: script=\"[^\"]*/good\\.kcs\" lines=3 ran=3 failed=0[\r\n]+3 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/run_script_dialog_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A dialog that runs a SCRIPT line gets back what every line of the script
    # logged, not only the record: a line run inside another line's run adds
    # to both captures. A script that stops is not ok, with its refusal.
    add_test(NAME qt_a_dialogs_script_line_gets_back_what_every_line_said_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=<SCRIPT \"${_scripts}/good.kcs\"|<SCRIPT \"${_scripts}/bad.kcs\""
            "-DEXPECT=--run-line SCRIPT \"[^\"]*/good\\.kcs\": ok=yes[\r\n]+  reply: rectangle created[\r\n]+  reply: circle created[\r\n]+  reply: line created[\r\n]+  reply: script=\"[^\"]*/good\\.kcs\" lines=3 ran=3 failed=0[\r\n]+.*--run-line SCRIPT \"[^\"]*/bad\\.kcs\": ok=no[\r\n]+  reply: rectangle created[\r\n]+  reply: script=[^\r\n]*stopped_at=5[\r\n]+  error: InvalidGeometry: circle radius must be positive \\[-1\\][\r\n]+  error: bad\\.kcs stopped at line 5, which was refused: CIRCLE 0,0 -1"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/script_run_line_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Several lines at once on the command line - what a paste puts there -
    # run as a script that stops at the first refused: the circle of radius
    # -1 stops it, and the LINE after it is never run.
    add_test(NAME qt_several_lines_on_the_command_line_run_as_a_script_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>RECT 0,0 10,5\nCIRCLE 0,0 -1\nLINE 0,0 10,0|>LIST"
            "-DEXPECT=rectangle created[\r\n]+[^!]*! InvalidGeometry[^\r\n]*[\r\n]+script=pasted lines=3 ran=2 failed=1 stopped_at=2[\r\n]+! The pasted lines stopped at line 2, which was refused: CIRCLE 0,0 -1[\r\n]+1 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/pasted_lines_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The same lines pasted while Text waits for its words are those words: a
    # line at a time, the blank one between them left out, so the text has
    # two lines and "RECT 0,0 5,5" draws nothing. As a script, the rectangle
    # was drawn and the second line refused as an unknown command.
    add_test(NAME qt_lines_pasted_at_a_text_prompt_are_the_texts_lines_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>DT|>0,0|>2.5|>0|>RECT 0,0 5,5\n\nsecond line|>"
            "-DEXPECT=2 lines of text"
            "-DFORBID=rectangle created|unknown command|pasted lines|stopped at"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/pasted_text_lines_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A script's bare tool word is the interpreter's, as katana_cli runs it:
    # ERASE erases the selection, and MOVE with nothing selected is refused.
    # The window once started the Erase and Move tools for them, erased
    # nothing, and counted every line run.
    set(_bare_words "${CMAKE_CURRENT_BINARY_DIR}/qt_scripts/bare_tool_words.kcs")
    file(WRITE "${_bare_words}"
        "RECT 0,0 10,10\nSELECT ALL\nERASE\nLIST\nCIRCLE 0,0 5\nMOVE\n")
    add_test(NAME qt_a_scripts_bare_tool_word_runs_as_katana_cli_runs_it_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>SCRIPT \"${_bare_words}\" CONTINUE|>LIST"
            "-DEXPECT=1 erased[\r\n]+0 entities[\r\n]+circle created[\r\n]+! InvalidState: nothing is selected[^\r\n]*[\r\n]+script=\"[^\"]*/bare_tool_words\\.kcs\" lines=6 ran=6 failed=1[\r\n]+.*1 entities[\r\n]+2  Circle"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/script_bare_tool_words_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Without --screenshot, --script is a batch run as katana_cli's is: never
    # shown, over when its steps are, 0 when every line ran and 1 at the first
    # refused.
    add_test(NAME qt_script_batch_run_exits_when_the_script_is_done_headless
        COMMAND katana --script "${_scripts}/good.kcs" --command LIST)
    set_tests_properties(qt_script_batch_run_exits_when_the_script_is_done_headless PROPERTIES
        PASS_REGULAR_EXPRESSION "script=\"[^\"]*/good\\.kcs\" lines=3 ran=3 failed=0[\r\n]+3 entities")
    add_test(NAME qt_script_batch_run_fails_at_a_refused_line_headless
        COMMAND katana --script "${_scripts}/bad.kcs")
    set_tests_properties(qt_script_batch_run_fails_at_a_refused_line_headless PROPERTIES
        WILL_FAIL TRUE)
    set_tests_properties(qt_script_switch_runs_every_line_of_a_script_headless
        qt_script_switch_stops_at_the_first_refused_line_headless
        qt_typed_script_continue_runs_every_line_headless
        qt_a_script_may_not_run_itself_headless
        qt_run_script_dialog_runs_the_line_it_shows_headless
        qt_a_dialogs_script_line_gets_back_what_every_line_said_headless
        qt_several_lines_on_the_command_line_run_as_a_script_headless
        qt_lines_pasted_at_a_text_prompt_are_the_texts_lines_headless
        qt_a_scripts_bare_tool_word_runs_as_katana_cli_runs_it_headless
        qt_script_batch_run_exits_when_the_script_is_done_headless
        qt_script_batch_run_fails_at_a_refused_line_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# Help > Command Reference, Sheets and Plotting Commands and Keyboard Shortcuts
# (docs/desktop.md, "The Command Reference and the keyboard shortcuts"), and
# the typed HELP, which adds the window's own verbs to the interpreter's
# list. A search for COPC finds the window's verb; the Sheets item opens the
# reference at the sheet verbs' introduction; the shortcuts list Ctrl+L as
# Format > Layers and no key reaching two commands. The table shows a key as
# the platform writes it, so on macOS Ctrl+L is "⌘L"; the search for Ctrl+L
# finds it either way.
if(TARGET katana)
    add_test(NAME qt_the_help_menu_finds_a_verb_and_lists_every_key_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>HELP|@helpCommandReference|commandReferenceSearch=COPC|?commandReferenceTree|?commandReferenceCount|@helpSheetCommands|?commandReferenceDetail|@helpKeyboardShortcuts|?keyboardShortcutsTable|keyboardShortcutsSearch=Ctrl+L|?keyboardShortcutsCount|?keyboardShortcutsClashes"
            "-DEXPECT=History   UNDO \\[n\\].*PLOTSHEETS path\\.pdf [^\r\n]*the desktop window's alone.*Script    SCRIPT <file\\.kcs> \\[CONTINUE\\].*opened commandReferenceDialog \"Command Reference\"[\r\n]+commandReferenceTree: Commands \\| 0 of [0-9]+ ; .*Window \\| [1-9][0-9]* of [0-9]+ ; .*Draw tools \\| 0 of [0-9]+[^\r\n]*[\r\n]+commandReferenceCount: [1-9][0-9]* of [0-9]+ entries[\r\n]+[^\r\n]*opened commandReferenceDialog[^\r\n]*[\r\n]+commandReferenceDetail: Sheets on the command line.*opened keyboardShortcutsDialog \"Keyboard Shortcuts\"[\r\n]+keyboardShortcutsTable: [^\r\n]*(Ctrl\\+|⌘)L \\| Layers \\| Format \\|[^\r\n]*[\r\n]+keyboardShortcutsCount: 1 of [0-9]+ keys[\r\n]+keyboardShortcutsClashes: Every key reaches one command\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/help_menu_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_help_menu_finds_a_verb_and_lists_every_key_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# STATUS [JSON] typed in the window, the status bar's selection count, and File
# > Drawing Summary (docs/desktop.md, "File > Drawing Summary"). The style
# "kerb" is drawn with the fixture's TEST Dashed Kerb, and the library that
# defines it is then replaced by the symbol fixture, so the name is
# unresolved: the summary lists it, and Show in Styles and Linetypes leaves the
# style manager showing only the styles that name it - kerb, not "other".
# Copy as JSON runs STATUS JSON through the one executor. The fixture's survey
# code file is loaded with the first library, so the survey code count is its
# 11 rules (counted by hand in its header) and not the build's: a REPLACE of
# libraries alone keeps the survey codes, and the built-in ones are the
# gitignored reference customisation's, present in one checkout and not
# another.
if(TARGET katana)
    set(_fixture "${PROJECT_SOURCE_DIR}/tests/archive12d/data/customisation")
    add_test(NAME qt_drawing_summary_and_status_json_describe_the_drawing_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE REPLACE \"${_fixture}/test_linestyles.4d\" \"${_fixture}/test_survey.mapfile\"|>STYLE NEW kerb|>STYLE NEW other|>STYLE SET kerb linetype \"TEST Dashed Kerb\"|>CUSTOMISE REPLACE \"${_fixture}/test_symbols.4d\"|>RECT 0,0 10,5|>SELECT ALL|>STATUS|>STATUS JSON|?statusSelectionCount|@fileDrawingSummary|?drawingSummaryDrawing|?drawingSummaryCurrent|?drawingSummarySelection|?drawingSummaryHistory|?drawingSummaryUnresolved|drawingSummaryUnresolved=TEST Dashed Kerb|!drawingSummaryShowMissing|?drawingSummaryStatus|!drawingSummaryCopyJson|?drawingSummaryStatus|@formatStyles|?styleTable"
            "-DEXPECT=Entities: 1  Layers: 1  Alignments: 0[\r\n]+Current layer: 0[\r\n]+Selected: 1  Undo steps: 4  Redo steps: 0[\r\n]+Customisation: 4 linestyles and symbols, 11 survey code rules[\r\n]+{[\r\n]+  \"alignments\": 0,[\r\n]+  \"currentLayer\": \"0\",[\r\n]+  \"currentStyle\": \"\",[\r\n]+  \"entities\": 1,.*  \"undoSteps\": 4[\r\n]+}[\r\n]+statusSelectionCount: 1 selected / 1 entities[\r\n]+[^\r\n]*opened drawingSummaryDialog \"Drawing Summary\"[\r\n]+drawingSummaryDrawing: 1 entity, 1 layer, 0 alignments, 0 sheets[\r\n]+drawingSummaryCurrent: layer 0, style ByLayer, annotation scale 1:[0-9]+, no coordinate system[\r\n]+drawingSummarySelection: 1 entity selected[\r\n]+drawingSummaryHistory: 4 steps to undo \\(next: [^)]+\\), 0 steps to redo[\r\n]+drawingSummaryUnresolved: TEST Dashed Kerb[\r\n]+drawingSummaryStatus: Styles and Linetypes shows the styles that name TEST Dashed Kerb\\.[\r\n]+{.*}[\r\n]+drawingSummaryStatus: Copied the drawing's state as JSON \\(STATUS JSON\\)\\.[\r\n]+[^\r\n]*opened styleManagerDialog[^\r\n]*[\r\n]+styleTable: kerb \\| [^;\r\n]*TEST Dashed Kerb[^;\r\n]*[\r\n]"
            "-DFORBID=unknown command"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/drawing_summary_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_drawing_summary_and_status_json_describe_the_drawing_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# File > Plot to PDF, File > Export View as Image and Edit > Copy View as
# Image, driven as a person drives them, and the PLOT and SNAPSHOT verbs they
# run (docs/desktop.md, "Plot to PDF and view images"). The plot dialog's
# colours and line weights reach the line, and the line's record says the
# plot was made with them; the image dialog's custom size is the PNG's size,
# read from its header; the 3D view is grabbed once the active view shows 3D.
# The inks each plot style prints are checked by qt_plot_headless
# (tools/check_plot.cmake).
if(TARGET katana)
    set(_out "${CMAKE_CURRENT_BINARY_DIR}/qt_output")
    file(MAKE_DIRECTORY "${_out}")
    add_test(NAME qt_plot_and_view_image_dialogs_run_their_verbs_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=@filePlot|plotDrawingPath=${_out}/dialog_plot.pdf|plotDrawingPaper=A4|plotDrawingOrientation=Portrait|plotDrawingFit=off|plotDrawingScale=250|plotDrawingDpi=150|plotDrawingColourMode=Greyscale|plotDrawingLineWeightScale=0.7|plotDrawingMargin=5|?plotDrawingCommand|!plotDrawingPlot|?plotDrawingStatus|@fileExportViewImage|viewImagePath=${_out}/dialog_view.png|viewImageSize=Custom size|viewImageWidth=300|viewImageHeight=150|viewImageBackground=White|?viewImageCommand|!viewImageExport|?viewImageStatus|*editCopyViewImage|>SNAPSHOT \"${_out}/typed_view.png\" width=320 height=200 bg=none|>SNAPSHOT \"${_out}/view.bmp\"|>PLOT|*viewShows3D|>SNAPSHOT \"${_out}/view_3d.png\" view=3d width=200 height=100"
            "-DEXPECT=opened plotDrawingDialog \"Plot to PDF\"[\r\n]+plotDrawingCommand: PLOT \"[^\"]*/dialog_plot\\.pdf\" paper=A4 portrait scale=250 dpi=150 style=greyscale lineweight=0\\.7 margin=5[\r\n]+Plotted to [^\r\n]*dialog_plot\\.pdf: A4 portrait at 1 : 250, 150 dpi[^\r\n]*[\r\n]+file=\"[^\"]*dialog_plot\\.pdf\" paper=A4 orientation=portrait scale=250 dpi=150 style=greyscale lineweight=0\\.7[\r\n]+plotDrawingStatus: Plotted: file=[^\r\n]*dialog_plot\\.pdf[^\r\n]*[\r\n]+[^\r\n]*opened viewImageDialog \"Export View as Image\"[\r\n]+viewImageCommand: SNAPSHOT \"[^\"]*/dialog_view\\.png\" width=300 height=150 bg=white[\r\n]+file=\"[^\"]*dialog_view\\.png\" view=plan width=300 height=150[\r\n]+viewImageStatus: Exported: file=[^\r\n]*dialog_view\\.png[^\r\n]*[\r\n]+clipboard=yes view=plan width=[1-9][0-9]* height=[1-9][0-9]*[\r\n]+file=\"[^\"]*typed_view\\.png\" view=plan width=320 height=200[\r\n]+! InvalidArgument: the file's extension names its format[^\r\n]*[\r\n]+! InvalidArgument: usage: PLOT <file\\.pdf>.*file=\"[^\"]*view_3d\\.png\" view=3d width=200 height=100"
            "-DIMAGE_SIZE=${_out}/dialog_view.png|300|150"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/plot_and_view_image_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The dialogs are kept, and their files follow the drawing: open another
    # project with Plot to PDF left open and it suggests that project's PDF,
    # as Export View as Image its PNG. It once plotted into the first
    # project's folder under its name. The two projects are saved into the
    # folder check_screenshot.cmake makes afresh for each run.
    set(_follow "${CMAKE_CURRENT_BINARY_DIR}/plot_file_follows_headless_project")
    add_test(NAME qt_the_plot_dialogs_file_follows_the_drawing_opened_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>RECT 0,0 10,10|>SAVE \"${_follow}/projB\"|>NEW|>RECT 0,0 5,5|>SAVE \"${_follow}/projA\"|@filePlot|?plotDrawingPath|>OPEN \"${_follow}/projB\"|?plotDrawingPath|@fileExportViewImage|?viewImagePath"
            "-DEXPECT=plotDrawingPath: [^\r\n]*.projA.projA\\.pdf[\r\n]+opened [^\r\n]*projB[^\r\n]*[\r\n]+plotDrawingPath: [^\r\n]*.projB.projB\\.pdf[\r\n]+.*viewImagePath: [^\r\n]*.projB.projB\\.png"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/plot_file_follows_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_plot_and_view_image_dialogs_run_their_verbs_headless
        qt_the_plot_dialogs_file_follows_the_drawing_opened_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# The Edit toolbar's Undo and Redo lists (docs/desktop.md, "Undo and Redo
# lists"): four points, so the Undo list holds four CREATE_POINT steps, newest
# first; its third entry runs UNDO 3 and leaves one point; the Redo list then
# holds three, and its second entry runs REDO 2, leaving three points and one
# step still to redo.
if(TARGET katana)
    add_test(NAME qt_the_undo_and_redo_lists_step_through_the_history_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>POINT 1,1|>POINT 2,2|>POINT 3,3|>RECT 0,0 4,4|?editUndoMenu|*undoStep3|>LIST|?editRedoMenu|*redoStep2|>LIST|?editRedoMenu"
            "-DEXPECT=editUndoMenu: : 1 CREATE_POLYLINE \\[UNDO 1: CREATE_POLYLINE\\] ; 2 CREATE_POINT \\[UNDO 2: the last 2 steps, down to CREATE_POINT\\] ; 3 CREATE_POINT \\[UNDO 3: the last 3 steps, down to CREATE_POINT\\] ; 4 CREATE_POINT \\[UNDO 4: the last 4 steps, down to CREATE_POINT\\][\r\n]+3 undone[\r\n]+1 entities[\r\n]+1  Point [^\r\n]*[\r\n]+editRedoMenu: : 1 CREATE_POINT \\[REDO 1: CREATE_POINT\\] ; 2 CREATE_POINT \\[REDO 2: the next 2 steps, down to CREATE_POINT\\] ; 3 CREATE_POLYLINE \\[REDO 3: the next 3 steps, down to CREATE_POLYLINE\\][\r\n]+2 redone[\r\n]+3 entities[\r\n]+.*editRedoMenu: : 1 CREATE_POLYLINE \\[REDO 1: CREATE_POLYLINE\\][\r\n]"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/undo_lists_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_undo_and_redo_lists_step_through_the_history_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# The File items that ask for a file, reached by the object names they now
# carry: in a headless session each opens no file dialog - nobody could close
# it - and names the line that does the same without one, as GIS > Convert
# Point Cloud to COPC does; Import names every placement IMPORT takes
# (docs/interop.md, "Placing an import"). Save of a drawing with no project is
# Save As. The
# empty drawing's Export says so in the log, not in a box.
if(TARGET katana)
    add_test(NAME qt_the_file_items_name_their_verbs_in_a_headless_run_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=*fileOpen|*fileSaveAs|*fileImport|*fileExportVector|>RECT 0,0 1,1|*fileExportVector|*loadCustomisation|*replaceCustomisation|*fileSave"
            "-DEXPECT=type OPEN <directory> instead\\.[\r\n]+[^\r\n]*type SAVE <directory> instead\\.[\r\n]+[^\r\n]*type IMPORT <file> \\[LOCAL \\| ALONGSIDE \\| OFFSET=dE,dN\\] instead\\.[\r\n]+! Export: the drawing is empty\\.[\r\n]+rectangle created[\r\n]+[^\r\n]*type EXPORT <file> instead\\.[\r\n]+[^\r\n]*type CUSTOMISE <file> \\[<file>\\.\\.\\.\\] instead\\.[\r\n]+[^\r\n]*type CUSTOMISE REPLACE <file> \\[<file>\\.\\.\\.\\] instead\\.[\r\n]+[^\r\n]*type SAVE <directory> instead\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/file_items_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_file_items_name_their_verbs_in_a_headless_run_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# Edit > Select by ID and the Edit and View actions' object names
# (docs/desktop.md, "The plan view's shortcut menu"). Three points; Select All
# by its action; the dialog, non-modal, selects #2 and 3 through the executor
# and says what SELECT answered; Grid is reached by its name and turned off;
# Erase by its name erases the two, and Undo by its name brings them back.
if(TARGET katana)
    add_test(NAME qt_select_by_id_selects_and_frames_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>POINT 1,1|>POINT 20,20|>POINT 40,40|*editSelectAll|@editSelectById|selectByIdIds=#2, 3|!selectByIdSelect|?selectByIdStatus|*viewGrid|?viewGrid|*editErase|>LIST|*editUndo|>LIST"
            "-DEXPECT=3 selected.*--dialog editSelectById opened selectByIdDialog \"Select by ID\"[\r\n]+2 selected[\r\n]+selectByIdStatus: 2 selected.*viewGrid: Grid, unchecked[\r\n]+2 erased[\r\n]+1 entities[\r\n]+1 +Point +layer=0 +at 1,1.*3 entities"
            "-DFORBID=\\(modal\\)|no such menu item|not an entity id"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/select_by_id_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The rest of the View menu by name, and the values a click cannot carry
    # typed. A snap mode toggled by its name, then set by SNAP <mode>; SNAP
    # ENDPOINT OFF leaves object snap on (it once turned it off); an unknown
    # word is refused; the quad layout, a standard view and perspective by
    # name; EXAGGERATION sets and reports the factor; Vertical Exaggeration,
    # headless, says to type the line rather than open a box nobody can
    # answer; a Panels toggle by name.
    add_test(NAME qt_every_view_menu_item_is_named_and_its_values_are_typed_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=?viewSnapModeEndpoint|*viewSnapModeEndpoint|?viewSnapModeEndpoint|>SNAP ENDPOINT ON|?viewSnapModeEndpoint|>SNAP CENTRE OFF|>SNAP ENDPOINT OFF|?viewSnap|>SNAP BOGUS|>GRID MAYBE|*viewLayoutQuad|*viewStandardIsoSouthWest|*viewTogglePerspective|>EXAGGERATION 2.5|>EXAGGERATION|>EXAGGERATION 0|*viewVerticalExaggeration|*viewPanelLayers|?viewPanelLayers"
            "-DEXPECT=viewSnapModeEndpoint: Endpoint, checked[\r\n]+viewSnapModeEndpoint: Endpoint, unchecked[\r\n]+snap_mode=endpoint state=on[\r\n]+viewSnapModeEndpoint: Endpoint, checked[\r\n]+snap_mode=center state=off[\r\n]+snap_mode=endpoint state=off[\r\n]+viewSnap: Object Snap, checked[\r\n]+! usage: SNAP [^\r\n]*[\r\n]+! usage: GRID [^\r\n]*[\r\n]+(Orthographic|Perspective) projection\\.[\r\n]+Vertical exaggeration x2\\.50\\.[\r\n]+vertical_exaggeration=2\\.5[\r\n]+! usage: EXAGGERATION [^\r\n]*[\r\n]+! A headless session opens no dialog: type EXAGGERATION <factor> instead\\.[\r\n]+viewPanelLayers: Layers, unchecked"
            "-DFORBID=no such menu item|No 3D viewport"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/view_menu_named_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The object snap has one owner, the document's drafting settings
    # (docs/drawing.md): the drawing system's SNAP with options reaches the
    # interpreter through the window's command line, a mode the drafting
    # toolbar turned on survives a click in View > Snap Modes (the views once
    # kept their own copy and wrote it back), and the menu follows a SNAP
    # line. Loose between the reports (.*), as the offscreen platform prints
    # its own notes among them.
    add_test(NAME qt_the_object_snap_has_one_owner_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>SNAP add=quadrant|?actionSnapQuadrant|*viewSnapModeEndpoint|?actionSnapQuadrant|?viewSnapModeEndpoint|>SNAP add=endpoint|?viewSnapModeEndpoint|>DRAFTING"
            "-DEXPECT=modes=[^\r\n]*quadrant.*actionSnapQuadrant: Quadrant, checked.*actionSnapQuadrant: Quadrant, checked.*viewSnapModeEndpoint: Endpoint, unchecked.*viewSnapModeEndpoint: Endpoint, checked.*modes=endpoint[^\r\n]*quadrant"
            "-DFORBID=usage: SNAP|no such menu item"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/object_snap_owner_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_select_by_id_selects_and_frames_headless
        qt_every_view_menu_item_is_named_and_its_values_are_typed_headless
        qt_the_object_snap_has_one_owner_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# The text and dimension styles in the real window (docs/annotation.md, "In
# the window"): each dialog builds the verb line and runs it through the
# window's one executor, so its reply is logged as a typed line's is and one
# typed UNDO takes its edit back.
if(TARGET katana)
    # Format > Dimension Styles...: a style named and made, three fields set
    # by one Apply and read back by INFO; one UNDO restores all three (the
    # defaults: 2.5, closed filled, model units - tables.hpp).
    add_test(NAME qt_the_dimension_styles_manager_runs_its_lines_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatDimensionStyles|dimStyleNewName=site|!dimStyleNew|dimStyleText=3.5|dimStyleHead=Tick|dimStylePaper=on|!dimStyleApply|?dimStylePreview|?dimStyleList|>DIMSTYLE INFO site|>UNDO|>DIMSTYLE INFO site"
            "-DEXPECT=dimension style site created.*dimension style site updated.*dimStylePreview: a dimension of 10 reads as 10\\.000.*dimStyleList: Standard \\| no layer \\(the default\\) ; site \\| no layer.*name=site text=3\\.5 [^\r\n]*head=Tick [^\r\n]*paper=on.*name=site text=2\\.5 [^\r\n]*head=ClosedFilled [^\r\n]*paper=off"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/dimension_styles_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # MT alone starts Multiline Text: at 1:500 the Standard style's 2.5 mm is
    # 1.25 m, and the two lines are one text (the break written \n by LIST).
    # Annotate > Edit Text... on a plain text selected: one TEXTEDIT of what
    # the form changed.
    add_test(NAME qt_multiline_text_and_edit_text_run_in_the_window_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>ANNOSCALE 500|>MT|>5,5|>|>PIT 12|>IL 10.50|>|>|>TEXT 0,0 2.5 old|>SELECT 2|@annotateEditText|?textEditStatus|textEditText=new words|textEditJustify=MC|!textEditApply|>LIST"
            "-DEXPECT=textEditStatus: Text 2.*updated text id=2.*1  Text  layer=0  \"PIT 12.nIL 10\\.50\"  height=1\\.25  style=Standard[\r\n]+2  Text  layer=0  \"new words\"  height=2\\.5  justify=MC"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/text_tools_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Format > Label Styles and Rules...: a style named and given its kind,
    # a rule added with its type, previewed and run as the rule chosen in the
    # table - one label for the rectangle's area (auto_label.hpp) - and read
    # back by the verbs.
    add_test(NAME qt_the_label_style_manager_runs_chosen_rules_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatLabelStyles|!labelStyleDefaults|labelStyleNewName=Lot areas|labelStyleNewKind=area|!labelStyleNew|labelStyleTabs=Auto-Label Rules|labelRuleName=lots|labelRuleStyle=Lot Area|labelRuleType=Polyline|!labelRuleAdd|>RECT 0,0 20,20|labelRuleTable=lots|!labelRulePreview|?labelRuleReport|?labelRuleTable|!labelRuleRun|>AUTOLABEL RULE LIST"
            "-DEXPECT=created label style name=\"Lot areas\" kind=area.*labelRuleReport: preview created=1 kept=0 removed=0 skipped=0.*labelRuleTable: lots \\| Lot Area \\| \\| \\| Polyline \\| \\| yes \\| 1.*autolabel created=1 kept=0 removed=0 skipped=0.*name=lots style=\"Lot Area\" layer=\"\" code=\"\" type=Polyline labellayer=\"\" enabled=on"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/label_rules_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_dimension_styles_manager_runs_its_lines_headless
        qt_multiline_text_and_edit_text_run_in_the_window_headless
        qt_the_label_style_manager_runs_chosen_rules_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# Labels made, edited and reported in the window (docs/annotation.md, "In the
# window"). By hand: a 30 x 40 lot (1), 1200 m2, is labelled by the bare LBL
# tool on the selection, its Style option naming Lot Area, and the label (2)
# pinned at 12,20; the Properties panel shows its words and where it is;
# Edit Label unpins it and gives it its own words through the one executor,
# naming only those two; the Label Layout Report finds the one label placed
# and none without room; the Annotate menu offers the tool and both dialogs.
if(TARGET katana)
    add_test(NAME qt_labels_are_made_edited_and_reported_in_the_window_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>PLINE 0,0 30,0 30,40 0,40 CLOSE|>LABELSTYLE DEFAULTS|>SELECT 1|>LBL|>S|>Lot Area|>12,20|>|?annotate.label|>SELECT 2|?propertyTable|@annotateEditLabel|labelEditPinned=off|labelEditOverride=on|labelEditText=LOT 7|?labelEditCommand|!labelEditApply|?labelEditStatus|@annotateLabelLayout|?labelLayoutPlaced|?labelLayoutSuppressed|>LABEL LIST|?annotateMenu"
            "-DEXPECT=1 label in style Lot Area.*annotate\\.label: Label Objects, unchecked.*propertyTable: .*Text \\| 1200\\.0 m[^;]* ; Position \\| pinned at 12\\.0000, 20\\.0000.*labelEditCommand: LABEL SET 2 text=\"LOT 7\" at=none[\r\n]+.*labelEditStatus: updated label id=2.*labelLayoutPlaced: 1[\r\n]+labelLayoutSuppressed: 0.*id=2 style=\"Lot Area\" target=1 part=-1 layer=0 rule=\"\" text=\"LOT 7\".*annotateMenu: Annotate: .*Label Objects .*Edit Label\\.\\.\\. .*Label Layout Report\\.\\.\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/labels_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_labels_are_made_edited_and_reported_in_the_window_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
    # The chain and balloon tools started by their aliases and answered on
    # the command line (docs/tools.md). By hand: DBA goes on from the newest
    # dimension (1: 0,0 to 10,0, its line 5 above), and one origin at 20,0
    # makes a baseline dimension measuring 20; Enter makes it, Enter again
    # ends the tool. BALLOON from 30,0 to 35,5 is balloon 1, the drawing's
    # first. LEADER's Callout option boxes the next leader's note.
    add_test(NAME qt_the_chain_balloon_and_leader_options_answer_on_the_command_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>DIM LINEAR 0,0 10,0 at=5,5|>DBA|?annotate.dimbaseline|>20,0|>|>|>BALLOON|>30,0|>35,5|>|>|>LE|>C|>B|>40,0|>45,5|>|>NOTE|>|>|>LIST"
            "-DEXPECT=annotate\\.dimbaseline: Baseline Dimension, checked[\r\n]+1 baseline dimension.*balloon 1.*leader with 1 line of text.*4 entities[\r\n]+1  Dimension  layer=0  linear  measures 10[\r\n]+2  Dimension  layer=0  linear  measures 20[\r\n]+3  Leader  layer=0  vertices=2  tip 30,0  \"1\"  callout=circle[\r\n]+4  Leader  layer=0  vertices=2  tip 40,0  \"NOTE\"  callout=box"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/chain_balloon_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_chain_balloon_and_leader_options_answer_on_the_command_line_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# File > Project Coordinate System through the real window (docs/cad.md, "The
# project's coordinate system"): non-modal, so --dialog fills it by its object
# names instead of hanging on exec() as it once did. Perth, 115.86 E, is in
# UTM zone 50 (114 to 120 E), which GDA2020 names EPSG:7850, "GDA2020 / MGA
# zone 50" in the EPSG registry; Set is one step, which one UNDO takes back.
if(TARGET katana)
    add_test(NAME qt_the_project_crs_dialog_is_driven_and_sets_one_step_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@fileProjectCrs|?projectCrsCurrent|projectCrsPlace=115.86,-31.95|!projectCrsSuggest|?projectCrsList|projectCrsText=EPSG:7850|?projectCrsCheck|!projectCrsApply|>CRS|>UNDO|>CRS"
            "-DEXPECT=--dialog fileProjectCrs opened projectCrsDialog \"Project Coordinate System\"[\r\n]+projectCrsCurrent: Now: no coordinate system[\r\n]+projectCrsList: Suggested for this place [^\r\n]*[\r\n]+projectCrsCheck: EPSG:7850 GDA2020 / MGA zone 50 - projected, metre[^\r\n]*[\r\n]+crs id=EPSG:7850 name=\"GDA2020 / MGA zone 50\"[^\r\n]*[\r\n]+1 undone[\r\n]+crs id=none"
            "-DFORBID=\\(modal\\)"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/project_crs_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_project_crs_dialog_is_driven_and_sets_one_step_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# A step that closes a dialog which deletes itself on close - the Sheets
# editor's Page Setup, opened with open() - leaves the window the target
# (docs/headless.md, "The steps"): the run grabs the window and says so, and a
# step that still wants the dialog is refused naming it. The driver once kept
# a raw pointer to the dialog, and such a run crashed at the grab (exit
# 0xC0000005) after every step had passed.
if(TARGET katana)
    add_test(NAME qt_a_dialog_that_deletes_itself_on_close_leaves_the_window_the_target_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>SHEET NEW PLAN|*fileSheets|@sheetPageSetup|plotDpi=150|!pageSetupSave|>SHEETS PAGESETUP"
            "-DEXPECT=pagesetup style=colour lineweight=1 dpi=150 pattern=\"[^\"]*\" filepersheet=off.*sheetPageSetup has closed: the window is grabbed"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/closed_target_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_a_step_for_a_dialog_that_has_closed_is_refused_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>SHEET NEW PLAN|*fileSheets|@sheetPageSetup|!pageSetupSave|plotDpi=300"
            "-DREFUSED=pagesetup style=colour[^\r\n]*[\r\n]+.*--fill plotDpi=300: sheetPageSetup has closed"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/closed_target_refused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_a_dialog_that_deletes_itself_on_close_leaves_the_window_the_target_headless
        qt_a_step_for_a_dialog_that_has_closed_is_refused_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()
