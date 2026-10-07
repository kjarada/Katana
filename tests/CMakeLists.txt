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

# The programs do not hold the older customisation formats. Their readers and
# writers (src/katana_archive12d/legacy) are the converter's, and the program
# reads one format, the Katana customisation; a reader that found its way back
# into katana, katana_cli or katana_mcp would be a second way to load a file
# that nobody had decided to keep. Two checks, each proved on a fixture below.
#
#   the link closure   every library a program links, however far down and
#                      however it is linked (PUBLIC, PRIVATE, through an
#                      INTERFACE library), holds no katana_legacy_customisation.
#                      Walked at configure time, which is when CMake knows it,
#                      and reported by an echo test as the headless scan is.
#   the install rules  tools/check_install_rules.cmake: the only programs
#                      installed are katana, katana_cli and katana_mcp, so the
#                      converter is never handed on.
#
# What a target links is its LINK_LIBRARIES (everything it was given, private
# included) and its INTERFACE_LINK_LIBRARIES (what it hands on, and where a
# static library's private links appear, as $<LINK_ONLY:...>): both are read for
# every target, and an entry is searched for names of targets rather than parsed,
# since a generator expression wraps them. Over-approximate on purpose - a name
# that is a target is followed, wherever it sits - because the check is only
# useful if it errs towards finding one.
function(katana_link_closure target out)
    set(_todo "${target}")
    set(_seen "")
    while(_todo)
        list(POP_FRONT _todo _current)
        if(_current IN_LIST _seen)
            continue()
        endif()
        list(APPEND _seen "${_current}")
        foreach(_property IN ITEMS LINK_LIBRARIES INTERFACE_LINK_LIBRARIES)
            get_target_property(_value ${_current} ${_property})
            if(NOT _value)
                continue()
            endif()
            foreach(_entry IN LISTS _value)
                string(REGEX MATCHALL "[A-Za-z_][A-Za-z0-9_:.+-]*" _words "${_entry}")
                foreach(_word IN LISTS _words)
                    if(TARGET ${_word})
                        list(APPEND _todo ${_word})
                    endif()
                endforeach()
            endforeach()
        endforeach()
    endwhile()
    set(${out} "${_seen}" PARENT_SCOPE)
endfunction()

# The programs of `programs` whose closure holds `forbidden`, as
# "<program> (<library>)" entries, and the ones whose walk did not reach
# katana_core - a walk that found nothing because it saw nothing.
function(katana_programs_holding programs forbidden holding blind)
    set(_holding "")
    set(_blind "")
    foreach(_program IN LISTS programs)
        if(NOT TARGET ${_program})
            continue()
        endif()
        katana_link_closure(${_program} _closure)
        foreach(_library IN LISTS forbidden)
            if(_library IN_LIST _closure)
                list(APPEND _holding "${_program} (${_library})")
            endif()
        endforeach()
        if(NOT "katana_core" IN_LIST _closure)
            list(APPEND _blind "${_program}")
        endif()
    endforeach()
    set(${holding} "${_holding}" PARENT_SCOPE)
    set(${blind} "${_blind}" PARENT_SCOPE)
endfunction()

function(katana_register_link_closure_checks)
    if(NOT TARGET katana_legacy_customisation)
        return()
    endif()
    set(_forbidden katana_legacy_customisation)
    katana_programs_holding("katana;katana_cli;katana_mcp" "${_forbidden}" _holding _blind)
    foreach(_list IN ITEMS _holding _blind)
        if(${_list})
            string(JOIN ", " ${_list}_text ${${_list}})
        else()
            set(${_list}_text "none")
        endif()
    endforeach()
    add_test(NAME the_programs_link_none_of_the_legacy_customisation_readers
        COMMAND ${CMAKE_COMMAND} -E echo
            "Programs whose link closure holds the legacy customisation readers: ${_holding_text}. Programs whose walk did not reach katana_core: ${_blind_text}.")
    set_tests_properties(the_programs_link_none_of_the_legacy_customisation_readers PROPERTIES
        PASS_REGULAR_EXPRESSION
            "readers: none[.] Programs whose walk did not reach katana_core: none[.]")

    # The check on a closure that holds one, and on one that does not. The
    # library under test is linked two targets down and PRIVATELY, which is
    # how katana_app links katana_archive12d: an INTERFACE library hands on
    # what a static library keeps to itself only as $<LINK_ONLY:...>, and a
    # walk that read INTERFACE_LINK_LIBRARIES of the outer target alone, or
    # looked for the name only at the first level, would not see it. The
    # fixtures are never built (EXCLUDE_FROM_ALL).
    add_library(katana_link_closure_fixture_middle STATIC EXCLUDE_FROM_ALL
        "${CMAKE_CURRENT_SOURCE_DIR}/link_closure_fixtures/empty.cpp")
    target_link_libraries(katana_link_closure_fixture_middle PRIVATE katana_legacy_customisation)
    add_library(katana_link_closure_fixture_outer INTERFACE)
    target_link_libraries(katana_link_closure_fixture_outer INTERFACE katana_link_closure_fixture_middle)
    add_library(katana_link_closure_fixture_clean INTERFACE)
    target_link_libraries(katana_link_closure_fixture_clean INTERFACE katana_archive12d)
    katana_programs_holding("katana_link_closure_fixture_outer;katana_link_closure_fixture_clean"
        "${_forbidden}" _fixture_holding _fixture_blind)
    string(JOIN ", " _fixture_text ${_fixture_holding})
    add_test(NAME the_link_closure_check_finds_a_legacy_library_two_targets_down_and_only_there
        COMMAND ${CMAKE_COMMAND} -E echo
            "Fixture programs whose link closure holds the legacy customisation readers: ${_fixture_text}.")
    set_tests_properties(the_link_closure_check_finds_a_legacy_library_two_targets_down_and_only_there
        PROPERTIES PASS_REGULAR_EXPRESSION
            "readers: katana_link_closure_fixture_outer [(]katana_legacy_customisation[)][.]")
endfunction()
cmake_language(DEFER CALL katana_register_link_closure_checks)

# ... and the install rules, on three install scripts written here, each as
# CMake writes one: a build's programs and nothing else; those and the converter
# installed from a directory below (the check follows the scripts the top one
# includes); and no program at all, which would make the check pass with
# nothing to check. The files named are never read, only named.
set(_install_fixture_root "${CMAKE_CURRENT_BINARY_DIR}/install_fixtures")
function(katana_write_install_fixture name programs below)
    set(_dir "${_install_fixture_root}/${name}")
    file(MAKE_DIRECTORY "${_dir}/below")
    set(_top "# A top-level install script, as CMake writes it.\n")
    string(APPEND _top "file(INSTALL DESTINATION \"\${CMAKE_INSTALL_PREFIX}/share\" TYPE FILE FILES \"${PROJECT_SOURCE_DIR}/README.md\")\n")
    foreach(_program IN LISTS programs)
        string(APPEND _top "file(INSTALL DESTINATION \"\${CMAKE_INSTALL_PREFIX}/bin\" TYPE EXECUTABLE FILES \"${_dir}/bin/${_program}.exe\")\n")
    endforeach()
    if(below)
        string(APPEND _top "include(\"${_dir}/below/cmake_install.cmake\")\n")
        set(_sub "")
        foreach(_program IN LISTS below)
            string(APPEND _sub "file(INSTALL DESTINATION \"\${CMAKE_INSTALL_PREFIX}/bin\" TYPE EXECUTABLE FILES \"${_dir}/bin/${_program}\")\n")
        endforeach()
        file(WRITE "${_dir}/below/cmake_install.cmake" "${_sub}")
    endif()
    file(WRITE "${_dir}/cmake_install.cmake" "${_top}")
endfunction()
katana_write_install_fixture(the_programs "katana;katana_cli;katana_mcp" "")
katana_write_install_fixture(a_converter_from_a_directory_below "katana;katana_cli;katana_mcp"
    "katana_customisation_convert")
katana_write_install_fixture(no_program "" "")
foreach(_fixture IN ITEMS the_programs a_converter_from_a_directory_below no_program)
    add_test(NAME packaging_check_on_fixture_${_fixture}
        COMMAND ${CMAKE_COMMAND} -DKATANA_ROOT=${PROJECT_SOURCE_DIR}
                -DKATANA_BINARY=${_install_fixture_root}/${_fixture}
                -P ${PROJECT_SOURCE_DIR}/tools/check_install_rules.cmake)
endforeach()
set_tests_properties(packaging_check_on_fixture_the_programs PROPERTIES
    PASS_REGULAR_EXPRESSION "Programs installed: katana, katana_cli, katana_mcp[.]")
set_tests_properties(packaging_check_on_fixture_a_converter_from_a_directory_below PROPERTIES
    PASS_REGULAR_EXPRESSION
        "installs the program katana_customisation_convert, which is not one of katana, katana_cli, katana_mcp")
set_tests_properties(packaging_check_on_fixture_no_program PROPERTIES
    PASS_REGULAR_EXPRESSION "No install rule for a program was found")

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
# the loop above: the suite links the application's objects (katana_qt_ui,
# src/katana_qt/CMakeLists.txt) rather than a module library, and is
# configured whenever the application is.
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

# Every test that runs one of the programs - a `qt_..._headless` check of the
# window, a `cli.` line through katana_cli - says what customisation the
# program starts with, so that it is the same test on a machine whose build
# has a customisation compiled in and on one whose build has none (the
# owner's has the third-party reference one; a clean checkout and every
# release build have nothing). A program reads two environment variables
# where it starts (cad/customisation_host.hpp):
#
#   KATANA_BUILTIN_CUSTOMISATION   the seam: `none` for no built-in, or a
#                                  file that IS the built-in for this run
#   KATANA_CUSTOMISATION           the file a customisation is kept in
#                                  between runs; unset, there is none
#
# A test that sets neither starts with NOTHING: the scan below gives it
# KATANA_BUILTIN_CUSTOMISATION=none, and unsets KATANA_CUSTOMISATION, which a
# person may have set for their own sessions. A test of the built-in or of the
# kept customisation sets the variable itself and names a committed fixture
# (tests/data/customisation). The cli tests put their environment on a
# `cmake -E env` command line, which the scan cannot read and need not: the
# property is set on the process that command line starts from, so a variable
# named on the command line still wins for katana_cli.
#
# TWO parts, which share nothing:
#
#   the scan    at configure time, GIVES the variables to the tests it knows
#               for a program's by their names (`cli.` and `..._headless`),
#               in every directory, as each was left by its own CMakeLists.
#   the check   at TEST time, reads what ctest will really run and NAMES every
#               test whose command names a program and which does not say
#               the variable (tools/check_program_tests.cmake) - one test a
#               variable, since what "says" is differs: the built-in's must
#               be given a value, and the kept file's is said by unsetting
#               it too.
#
# They were one function once, which set the variable on its list of tests and
# then named those of that same list without it: nothing the scan missed could
# be named, so the test could not fail - a program test registered under
# another name, or in a directory configured after this one (benchmarks is),
# ran with whatever its machine had, and the test said "none". And until
# 2026-10-07 the check asked about the built-in's variable alone: the scan's
# unsetting of KATANA_CUSTOMISATION could be dropped with nothing failing, and
# every program test then started with whatever file the person running the
# suite keeps.
#
# The scan is deferred to the end of the TOP directory, by when every
# directory has been configured - not to the end of this one, which a
# directory added after it is not in. A test of another directory is reached
# with `DIRECTORY`, which CMake has had since 3.28.
#
# AN OLDER CMAKE cannot set a property there at all, and the top
# CMakeLists.txt and the presets still say 3.24 (raising the declared minimum
# also turns on every policy up to it - C++ module scanning of each source
# among them - and is a decision for the build, not for this scan). This
# directory once REFUSED to configure with one. It degrades instead: the scan
# is then deferred to the end of THIS directory and gives the variables to
# the program tests it can reach from here - those of tests/ itself, which are
# most of the window's checks - and neither check is registered, since each
# would name the `cli.` tests (src/katana_app) and the window checks of the
# directories below that do not say for themselves, at every run, for a
# reason nobody running it can mend.
# Those tests then start with whatever the build compiled in and the caller's
# environment holds; one STATUS line says so as the tree is configured.
set(_katana_scan_reaches_every_directory TRUE)
if(CMAKE_VERSION VERSION_LESS 3.28)
    set(_katana_scan_reaches_every_directory FALSE)
    message(STATUS
        "CMake ${CMAKE_VERSION} is older than 3.28 and cannot set a property on a test of "
        "another directory: only the program tests of tests/ are given their starting "
        "customisation (KATANA_BUILTIN_CUSTOMISATION=none, KATANA_CUSTOMISATION unset). The "
        "`cli.` tests and the window checks of other directories start with what this build "
        "compiled in and the caller's environment holds, and the two tests that name a "
        "program test left so (every_program_test_says_which_built_in_customisation_it_starts_with "
        "and ..._which_kept_customisation_it_starts_with) are not registered.")
endif()

function(katana_directories_below directory out)
    set(_found "${directory}")
    get_property(_children DIRECTORY "${directory}" PROPERTY SUBDIRECTORIES)
    foreach(_child IN LISTS _children)
        katana_directories_below("${_child}" _below)
        list(APPEND _found ${_below})
    endforeach()
    set(${out} "${_found}" PARENT_SCOPE)
endfunction()

# `every_directory`: TRUE where this CMake can reach a test of another
# directory (3.28); FALSE to take the tests of the directory it is called at
# the end of, and no other - without the `DIRECTORY` word, which an older
# CMake does not know and is never shown (a command is read when it is run).
function(katana_say_what_every_program_test_starts_with every_directory)
    if(every_directory)
        katana_directories_below("${CMAKE_SOURCE_DIR}" _directories)
    else()
        set(_directories "${CMAKE_CURRENT_SOURCE_DIR}")
    endif()
    foreach(_directory IN LISTS _directories)
        get_property(_tests DIRECTORY "${_directory}" PROPERTY TESTS)
        foreach(_test IN LISTS _tests)
            if(NOT _test MATCHES "^cli[.]" AND NOT _test MATCHES "_headless$")
                continue()
            endif()
            foreach(_setting "KATANA_BUILTIN_CUSTOMISATION=set:none" "KATANA_CUSTOMISATION=unset:")
                string(REGEX REPLACE "=.*$" "" _variable "${_setting}")
                if(every_directory)
                    get_test_property(${_test} ENVIRONMENT_MODIFICATION DIRECTORY "${_directory}"
                        _modification)
                    get_test_property(${_test} ENVIRONMENT DIRECTORY "${_directory}" _environment)
                else()
                    get_test_property(${_test} ENVIRONMENT_MODIFICATION _modification)
                    get_test_property(${_test} ENVIRONMENT _environment)
                endif()
                if("${_modification};${_environment}" MATCHES "(^|;)${_variable}=")
                    continue()
                endif()
                if(every_directory)
                    set_property(TEST ${_test} DIRECTORY "${_directory}"
                        APPEND PROPERTY ENVIRONMENT_MODIFICATION "${_setting}")
                else()
                    set_property(TEST ${_test}
                        APPEND PROPERTY ENVIRONMENT_MODIFICATION "${_setting}")
                endif()
            endforeach()
        endforeach()
    endforeach()
endfunction()
if(_katana_scan_reaches_every_directory)
    cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}"
        CALL katana_say_what_every_program_test_starts_with TRUE)
else()
    cmake_language(DEFER CALL katana_say_what_every_program_test_starts_with FALSE)
endif()

# The check, once for each variable. The programs are handed over in a file -
# written when the build system is, so it holds each one's real path - and not
# on the command line, where they would make this test one that names a
# program. Not registered where the scan could not reach every directory (an
# older CMake, above).
#
# The kept file's variable is said by being UNSET too (-DUNSET_SAYS): no kept
# file is a state a program is the same in on every machine, where an unset
# built-in variable leaves the choice to the build. What that check holds is
# the scan's second setting above, and the `--unset=KATANA_CUSTOMISATION` of
# the cli tests that write it themselves.
set(_program_files "")
foreach(_program katana katana_cli katana_mcp)
    if(TARGET ${_program})
        string(APPEND _program_files "$<TARGET_FILE:${_program}>\n")
    endif()
endforeach()
if(_program_files AND _katana_scan_reaches_every_directory)
    file(GENERATE OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/program_files_$<CONFIG>.txt"
        CONTENT "${_program_files}")
    add_test(NAME every_program_test_says_which_built_in_customisation_it_starts_with
        COMMAND ${CMAKE_COMMAND}
            "-DCTEST=${CMAKE_CTEST_COMMAND}"
            "-DBUILD=${CMAKE_BINARY_DIR}"
            "-DCONFIGURATION=$<CONFIG>"
            "-DPROGRAM_LIST=${CMAKE_CURRENT_BINARY_DIR}/program_files_$<CONFIG>.txt"
            "-DVARIABLE=KATANA_BUILTIN_CUSTOMISATION"
            -P "${PROJECT_SOURCE_DIR}/tools/check_program_tests.cmake")
    add_test(NAME every_program_test_says_which_kept_customisation_it_starts_with
        COMMAND ${CMAKE_COMMAND}
            "-DCTEST=${CMAKE_CTEST_COMMAND}"
            "-DBUILD=${CMAKE_BINARY_DIR}"
            "-DCONFIGURATION=$<CONFIG>"
            "-DPROGRAM_LIST=${CMAKE_CURRENT_BINARY_DIR}/program_files_$<CONFIG>.txt"
            "-DVARIABLE=KATANA_CUSTOMISATION"
            "-DUNSET_SAYS=ON"
            "-DWHAT=which kept customisation they start with"
            -P "${PROJECT_SOURCE_DIR}/tools/check_program_tests.cmake")
    set_tests_properties(every_program_test_says_which_built_in_customisation_it_starts_with
                         every_program_test_says_which_kept_customisation_it_starts_with
        PROPERTIES TIMEOUT 300)
endif()
# The check itself, on a listing with tests in it that it must name: a check
# that cannot see one passes as quietly as one that finds none. The listing is
# written by hand in ctest's format (tests/program_test_fixtures): eleven
# tests, of which eight name one of three programs - a window that starts
# with nothing and one that does not say; a cli with a fixture in its
# ENVIRONMENT, one with the variable on its own `cmake -E env` line, one that
# does not say, one whose modification unsets what its environment set and one
# whose own line does (`--unset=`, which is the last word and takes away what
# the property gave); an mcp that unsets only the kept file's variable - and
# three do not: a unit suite, a tool whose name only begins as a program's,
# and a test with no command. Five of the eight do not say, and are named, in
# the listing's order. (By hand from Git Bash, set MSYS_NO_PATHCONV=1: the
# shell rewrites /b/...)
add_test(NAME the_program_test_check_names_a_test_left_to_the_machines_customisation
    COMMAND ${CMAKE_COMMAND}
        "-DLISTING=${CMAKE_CURRENT_SOURCE_DIR}/program_test_fixtures/listing.json"
        "-DPROGRAMS=/b/bin/katana|/b/bin/katana_cli|/b/bin/katana_mcp"
        "-DVARIABLE=KATANA_BUILTIN_CUSTOMISATION"
        -P "${PROJECT_SOURCE_DIR}/tools/check_program_tests.cmake")
set_tests_properties(the_program_test_check_names_a_test_left_to_the_machines_customisation
    PROPERTIES PASS_REGULAR_EXPRESSION
        "Of 8 tests that run a program, those that do not say which built-in customisation they start with: window_left_to_the_machine, cli_left_to_the_machine, cli_whose_modification_takes_it_away_again, mcp_that_only_unsets_the_kept_file, cli_whose_own_line_takes_it_away_again[.]")
# ... and for the kept file's variable, where taking it away SAYS. A listing
# of its own (listing_kept.json): eleven tests, of which nine name a program.
# Five say - a window that unsets it and one with a file of its own; a cli
# that unsets it on its own `cmake -E env` line and one that names a file
# there; an mcp given an empty one, which is none. Four do not, and are named,
# in the listing's order: a window that says only its built-in (what every
# window check would be without the scan's second setting), a cli likewise, a
# cli whose modification unsets it and then gives the caller's back
# (`reset:`), and an mcp that says nothing at all. A unit suite and a test
# with no command are not program tests.
add_test(NAME the_program_test_check_names_a_test_left_to_the_callers_kept_customisation
    COMMAND ${CMAKE_COMMAND}
        "-DLISTING=${CMAKE_CURRENT_SOURCE_DIR}/program_test_fixtures/listing_kept.json"
        "-DPROGRAMS=/b/bin/katana|/b/bin/katana_cli|/b/bin/katana_mcp"
        "-DVARIABLE=KATANA_CUSTOMISATION"
        "-DUNSET_SAYS=ON"
        "-DWHAT=which kept customisation they start with"
        -P "${PROJECT_SOURCE_DIR}/tools/check_program_tests.cmake")
set_tests_properties(the_program_test_check_names_a_test_left_to_the_callers_kept_customisation
    PROPERTIES PASS_REGULAR_EXPRESSION
        "Of 9 tests that run a program, those that do not say which kept customisation they start with: window_left_to_the_callers_shell, cli_left_to_the_callers_shell, cli_whose_modification_gives_the_callers_back, mcp_left_to_the_callers_shell[.]")

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

# The whole main window with a customisation loaded into it by --customise:
# the three hand-written Katana customisation files of tests/data/customisation
# in one CUSTOMISE line (the script globs them in name order: linestyles,
# survey, symbols), merged into a session that starts with nothing, and
# reported - the line a person types, so what ctest exercises is what a person
# gets (PLAN.MD 20.3). By hand from the three files: 3 and 4 definitions, 11
# rules over 8 keys - a key once for each section it has rules in is 7
# feature, 3 symbol and 1 attributes, 11; the session takes its first file's
# name; and with all three loaded the one name the rules ask for that nothing
# defines is TEST Missing Symbol. The files are in every checkout, so this
# loads on every machine: pointed at the older reference folder it loaded
# nothing wherever that folder was absent, and proved only that the switch
# did no harm. "J" below is the suffix, and the quote a record puts round a
# path that holds a blank.
if(TARGET katana)
    set(_twins "${PROJECT_SOURCE_DIR}/tests/data/customisation")
    set(_J "\\.customisation\\.json\"?")
    set(_nothing_else "colours_added=0 colours_replaced=0 linework=no automation=no")
    add_test(NAME qt_customisation_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_headless.png"
            "-DCUSTOMISE_DIR=${_twins}"
            "-DEXPECT=loaded file=[^\r\n]*test_linestyles${_J} name=test_linestyles definitions_added=3 definitions_replaced=0 codes_added=0 codes_replaced=0 ${_nothing_else}[\r\n]+loaded file=[^\r\n]*test_survey${_J} name=test_survey definitions_added=0 definitions_replaced=0 codes_added=11 codes_replaced=0 ${_nothing_else}[\r\n]+loaded file=[^\r\n]*test_symbols${_J} name=test_symbols definitions_added=4 definitions_replaced=0 codes_added=0 codes_replaced=0 ${_nothing_else}[\r\n]+undefined names=1[\r\n]+undefined name=\"TEST Missing Symbol\"[\r\n]+customisation name=test_linestyles origin=loaded kept=no definitions=7 codes=8 rules=11 colours=0"
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
    # The 3D view's wheel in the real window (docs/render.md, "Zooming towards
    # the cursor"). SURFACE FROM FILE opens a 3D view of the sample terrain
    # docked beside the plan, 328 x 497, and ~RenderView2 turns the wheel over
    # its near ground as a person does, ten notches at a time. The wheel once
    # anchored every notch on the plane through the orbit target, which there
    # lies under the ground: the eye went through it between the tenth and the
    # twentieth notch, and the view was blank (painted=0) at the twentieth and
    # the thirtieth. Each ?RenderView2 paints the view afresh and says how many
    # of its pixels are not the background - never none, zooming in or back
    # out - and where its camera is.
    add_test(NAME qt_the_3d_views_wheel_keeps_zooming_into_the_ground_under_the_cursor_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SURFACE FROM FILE \"${_gis}/terrain.asc\"|?RenderView2|~RenderView2 160,330 10|?RenderView2|~RenderView2 160,330 10|?RenderView2|~RenderView2 160,330 10|?RenderView2|~RenderView2 160,330 -30|?RenderView2|%View2"
            "-DEXPECT=RenderView2: painted=[1-9][0-9]* target=[^\r\n]* projection=perspective[\r\n]+--wheel RenderView2: 10 notches at 160,330[\r\n]+RenderView2: painted=[1-9][0-9]* [^\r\n]*[\r\n]+--wheel RenderView2: 10 notches at 160,330[\r\n]+RenderView2: painted=[1-9][0-9]* [^\r\n]*[\r\n]+--wheel RenderView2: 10 notches at 160,330[\r\n]+RenderView2: painted=[1-9][0-9]* [^\r\n]*[\r\n]+--wheel RenderView2: -30 notches at 160,330[\r\n]+RenderView2: painted=[1-9][0-9]* "
            "-DFORBID=painted=0 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/the_3d_views_wheel_headless.png"
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
    # File > Export > Export Drawing's dialog, opened for a file as the menu opens it
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
                         qt_the_3d_views_wheel_keeps_zooming_into_the_ground_under_the_cursor_headless
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
    # The note under the wizard's Options step (finishNote) says what the
    # step's two boxes will do, and for a session with no survey codes what
    # brings some; it is read here in the real window, walked to the step
    # (the note is filled in when the step is reached, not before). It named
    # Format > Load Customisation and "a survey code file" after the menu item
    # had gone and that kind of file had stopped loading; what loads survey
    # codes is a Katana customisation file, loaded in File > Settings > Import
    # or by the CUSTOMISE <file> line. This group runs with no built-in, so the
    # session has none. The label's text is plain (Qt::PlainText), and --report
    # read every label as markup until 2026-10-07: a word in angle brackets was
    # on the screen and not in the report, so "<file>" is what shows that this
    # expression can be met. The report wraps a long label at the width of the
    # window, which differs with the font, so the expression allows a break
    # between any two words of a phrase.
    add_test(NAME qt_the_import_wizards_options_note_names_the_customise_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/survey_points_pnezd.csv|!next|${_survey_format}|!next|!next|unit=metres|!next|!next|?finishNote"
            "-DEXPECT=finishNote: [^\r\n]*Survey codes: the session has none.*Katana customisation file.*load one in File[ \r\n]+>[ \r\n]+Settings[ \r\n]+>[ \r\n]+Import.*type[ \r\n]+CUSTOMISE[ \r\n]+<file>[ \r\n]+on"
            "-DFORBID=Load Customisation|survey code file"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_import_options_note_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # SURVEY IMPORT typed on the window's command line runs the session's
    # verb (src/katana_app/survey_verbs.hpp): the hand-built field file's 4
    # points come in as job-1, as katana_cli's cli.survey_import_field_file
    # has them.
    add_test(NAME qt_survey_verb_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>SURVEY IMPORT \"${_survey_data}/fld/setup.fld\"|>STATUS"
            "-DEXPECT=survey file=setup\\.fld format=opcode-field-file .*imported job=job-1 entities=4 layer=survey/points.*Entities: 4 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_verb_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The wizard in a window that HAS survey codes: the hand-written fixture
    # tests/data/field_codes/test_field_codes, named as the built-in of the
    # run through the seam, and the RTK job gnss.fld - the pair
    # cli.survey_import_codes_and_strings_by_the_built_in_customisation_as_one_undo_step
    # works by hand (src/katana_app/CMakeLists.txt), which is the same import
    # from a line. The customisation is one linestyle and three rules, and
    # says nothing of its switches, so both are on: the Options step shows
    # both boxes ticked. Import draws the 5 points; codes the 4 that carry KB
    # (all matched, by KB*, onto the ONE layer FIELD KERB in the ONE style
    # named after its linestyle); and strings the file's closed string KB 1
    # through R001, R002 and R003 - a right-angled triangle with legs of 10,
    # 10 + 10 + sqrt(200) = 34.1421... round and 50 in area. The log says so
    # in the finish's own sentences, LIST shows 6 entities, and ONE undo
    # takes the points, the line, the layers, the style and the job back.
    # (Every other wizard check here runs with no built-in - the seam is
    # `none` - and so draws its points and nothing else, as it always did.)
    set(_wizard_codes
        "${PROJECT_SOURCE_DIR}/tests/data/field_codes/test_field_codes.customisation.json")
    add_test(NAME qt_survey_import_codes_and_strings_by_the_built_in_customisation_as_one_undo_step_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@surveyImport|file=${_survey_data}/fld/gnss.fld|!next|!next|!next|!next|!next|?applyCodes|?drawLinework|layer=fieldwork|!next|!import|>LIST|>UNDO|>STATUS"
            "-DEXPECT=Customisation: test_field_codes, 1 definition [^\r\n]*and 3 survey code rules, built in\\..*applyCodes: Apply survey codes to the imported points, checked[\r\n]+drawLinework: Draw linework through the imported points, checked[\r\n].*Imported survey job job-1 from gnss\\.fld [^\r\n]*point\\(s\\) on fieldwork[^\r\n]*[\r\n]+Survey codes were applied to the 5 point\\(s\\) drawn: 4 carry a code and 4 of those have a rule; 1 layer\\(s\\) and 1 style\\(s\\) were created\\.[\r\n]+Linework: 1 line\\(s\\) drawn\\.[\r\n].*6 entities[\r\n]+[^\r\n]*Point +layer=fieldwork +at 500000,6200000[\r\n]+[^\r\n]*Point +layer=FIELD KERB +at 500010,6200020[\r\n]+[^\r\n]*Point +layer=FIELD KERB +at 500020,6200020[\r\n]+[^\r\n]*Point +layer=FIELD KERB +at 500020,6200030[\r\n]+[^\r\n]*Point +layer=FIELD KERB +at 500040,6200040[\r\n]+[^\r\n]*Polyline +layer=FIELD KERB +vertices=3 +closed +length=34[.]1421[0-9]* +area=50[\r\n].*Entities: 0 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/survey_import_codes_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(
        qt_survey_import_codes_and_strings_by_the_built_in_customisation_as_one_undo_step_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:${_wizard_codes}"
        TIMEOUT 180)
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
    # A load MERGES (audit QT-21), typed as the CUSTOMISE line - the
    # interpreter's verb, and the one door for a customisation file since
    # Format > Load Customisation went: the hand-written fixture of survey
    # codes REPLACES the codes, then the symbol fixture alone is MERGED in -
    # and the codes must still be there. By hand from the two files:
    # test_survey has 11 rules over 8 keys, and test_symbols 4 definitions,
    # all new to a session that starts with nothing, so 4 are added and none
    # replaced. The session is named by its first load. The rules ask for
    # five names beside the plain "0"; the symbols file defines two of them
    # (TEST Survey Mark, TEST Tree), so three are undefined, said in name
    # order: the two linestyles of the file this test does not load, and
    # TEST Missing Symbol, which no file defines.
    add_test(NAME qt_customisation_merges_and_keeps_the_map_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE REPLACE \"${_twins}/test_survey.customisation.json\"|>CUSTOMISE \"${_twins}/test_symbols.customisation.json\""
            "-DEXPECT=loaded file=[^\r\n]*test_symbols${_J} name=test_symbols definitions_added=4 definitions_replaced=0 codes_added=0 codes_replaced=0 ${_nothing_else}[\r\n]+undefined names=3[\r\n]+undefined name=\"TEST Dashed Kerb\"[\r\n]+undefined name=\"TEST Missing Symbol\"[\r\n]+undefined name=\"TEST Water Main\"[\r\n]+customisation name=test_survey origin=loaded kept=no definitions=4 codes=8 rules=11 colours=0"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_merge_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # A file named twice in one CUSTOMISE is read once
    # (cad::distinctCustomisationFiles): the second naming is the same file
    # by another spelling, said as a `repeated` record before the ONE
    # `loaded` record. By hand: the file REPLACES the codes, read once, so
    # there are its 11 rules; read twice there were 22. Nothing is loaded
    # that defines any of the five names its rules ask for.
    add_test(NAME qt_a_file_named_twice_in_one_load_is_read_once_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE REPLACE \"${_twins}/test_survey.customisation.json\" \"${_twins}/../customisation/test_survey.customisation.json\""
            "-DEXPECT=repeated file=[^\r\n]*test_survey${_J}[\r\n]+loaded file=[^\r\n]*test_survey${_J} name=test_survey definitions_added=0 definitions_replaced=0 codes_added=11 codes_replaced=0 ${_nothing_else}[\r\n]+undefined names=5[\r\n]+.*customisation name=test_survey origin=loaded kept=no definitions=0 codes=8 rules=11 colours=0"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_named_twice_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # What a window starts with, and that the interpreter was handed the host
    # (cad/customisation_host.hpp). The built-in is the symbol fixture, named
    # through the seam: the start-up line says its name, its 4 definitions -
    # all in its "symbols" list - no rules, and that it is built in; CUSTOMISE
    # alone says the same as records, the session kept (it is what the next
    # start gives) and its own one source. A REPLACE of the definitions by the
    # linestyle fixture leaves that file's 3, the 4 symbols gone, under the
    # session's own name; RESET puts the built-in back - which only a session
    # given its host can - and it is `kept` again, there being no kept file
    # to be read in its place. And there is none: a headless window reads and
    # writes no per-user place, so with KATANA_CUSTOMISATION unset KEEP is
    # refused, naming the variable.
    add_test(NAME qt_the_window_starts_with_its_built_in_customisation_and_reset_returns_to_it_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE|>CUSTOMISE REPLACE \"${_twins}/test_linestyles.customisation.json\"|>CUSTOMISE RESET|>CUSTOMISE KEEP"
            "-DEXPECT=Customisation: test_symbols, 4 definitions \\(4 symbols\\) and 0 survey code rules, built in\\..*customisation name=test_symbols origin=builtIn kept=yes definitions=4 codes=0 rules=0 colours=0[\r\n]+source name=test_symbols definitions=yes rules=no[\r\n].*loaded file=[^\r\n]*test_linestyles${_J} name=test_linestyles definitions_added=3 definitions_replaced=0 codes_added=0 codes_replaced=0 ${_nothing_else}[\r\n]+removed definitions=4 codes=0[\r\n].*customisation name=test_symbols origin=loaded kept=no definitions=3 codes=0 rules=0 colours=0[\r\n]+reset name=test_symbols definitions=4 codes=0 rules=0 kept=yes[\r\n]+! InvalidState: CUSTOMISE KEEP needs a kept customisation file, and this session has none; the environment variable KATANA_CUSTOMISATION names one"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_built_in_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_window_starts_with_its_built_in_customisation_and_reset_returns_to_it_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:${_twins}/test_symbols.customisation.json"
        TIMEOUT 180)

    # The kept customisation, which a headless window has only where
    # KATANA_CUSTOMISATION names the file: one run loads the fixture of
    # survey codes and KEEPs it - written, with nothing there before to back
    # up - and the next run starts with it and says so: its name, its 11
    # rules over 8 keys, origin kept. Two processes, as the cli tests save in
    # one and reopen in another.
    set(_kept "${CMAKE_CURRENT_BINARY_DIR}/qt_kept_customisation")
    add_test(NAME qt_kept_customisation_cleanup COMMAND ${CMAKE_COMMAND} -E rm -rf "${_kept}")
    add_test(NAME qt_a_customisation_is_kept_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE \"${_twins}/test_survey.customisation.json\"|>CUSTOMISE KEEP"
            "-DEXPECT=kept file=[^\r\n]*qt_kept_customisation/customisation\\.json\"? written=yes name=test_survey definitions=0 codes=8 rules=11 backup=none"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_kept_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_a_kept_customisation_is_what_the_next_window_starts_with_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE"
            "-DEXPECT=Customisation: test_survey, 0 definitions \\(0 symbols\\) and 11 survey code rules, kept\\..*11 survey code rules over 8 distinct codes[\r\n]+customisation name=test_survey origin=kept kept=yes definitions=0 codes=8 rules=11 colours=0[\r\n]+source name=test_survey definitions=no rules=yes"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_kept_start_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_a_customisation_is_kept_headless
                         qt_a_kept_customisation_is_what_the_next_window_starts_with_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_CUSTOMISATION=set:${_kept}/customisation.json"
        TIMEOUT 180)
    set_tests_properties(qt_kept_customisation_cleanup PROPERTIES FIXTURES_SETUP qt_kept_clean)
    set_tests_properties(qt_a_customisation_is_kept_headless PROPERTIES
        FIXTURES_REQUIRED qt_kept_clean FIXTURES_SETUP qt_kept)
    set_tests_properties(qt_a_kept_customisation_is_what_the_next_window_starts_with_headless
        PROPERTIES FIXTURES_REQUIRED qt_kept)

    # A kept customisation that does not read is SAID, once, as an error line,
    # and the built-in stands in for it rather than the session starting with
    # nothing (cad::startCustomisation). The kept file here is JSON of another
    # format, which the reader refuses as not a Katana customisation file; the
    # built-in is the symbol fixture, through the seam. The run goes on: a
    # problem at start-up is not a refused step.
    set(_not_one "${CMAKE_CURRENT_BINARY_DIR}/qt_not_a_customisation.json")
    file(WRITE "${_not_one}" "{\"format\": \"another-format\", \"version\": 1, \"name\": \"x\"}\n")
    add_test(NAME qt_a_kept_customisation_that_does_not_read_is_said_and_the_built_in_stands_in_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE"
            "-DEXPECT=! Customisation: the kept customisation is not read, so the built-in customisation is used: [^\r\n]*not a Katana customisation file[^\r\n]*[\r\n]+Customisation: test_symbols, 4 definitions \\(4 symbols\\) and 0 survey code rules, built in\\..*customisation name=test_symbols origin=builtIn "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_kept_unreadable_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_a_kept_customisation_that_does_not_read_is_said_and_the_built_in_stands_in_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:${_twins}/test_symbols.customisation.json;KATANA_CUSTOMISATION=set:${_not_one}"
        TIMEOUT 180)

    # Survey > Apply Survey Codes is the CODE line through the window's one
    # executor: on the selection when there is one, else on the drawing. By
    # hand: two points, both given the code WM01, which the fixture's prefix
    # key WM* meets. With point 1 alone selected the scope takes 1 entity and
    # the line is CODE SELECTION; with nothing selected it takes the
    # drawing's 2 and the line is CODE DRAWING. The log (?commandLog, on one
    # line) shows each line echoed and answered where it ran.
    add_test(NAME qt_apply_survey_codes_runs_the_code_line_on_the_selection_or_the_drawing_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE \"${_twins}/test_survey.customisation.json\"|>POINT 1,1|>POINT 2,2|>SELECT ALL|>PROP SET code WM01|>SELECT 1|*applySurveyCodes|>SELECT NONE|*applySurveyCodes|?commandLog"
            "-DEXPECT=scope=selection matched=1[\r\n]+1 entity carries a code in \"code\": 1 matched, 0 fallback-only[^\r\n]*[\r\n].*scope=drawing matched=2[\r\n]+2 entities carry a code in \"code\": 2 matched, 0 fallback-only.*commandLog: [^\r\n]*> CODE SELECTION scope=selection matched=1 [^\r\n]*> CODE DRAWING scope=drawing matched=2 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/apply_survey_codes_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # With no survey codes loaded - a session that starts with nothing, and
    # nothing selected, so the line is CODE DRAWING - the item is refused in
    # the verb's own words, in the log, and the run goes on. The window once
    # had a box for this case ("No survey codes: Load a survey code file
    # first: Format > Load Customisation...") and another for a failed run:
    # FORBID holds this run to neither, here where the old code showed the
    # first. (It stood on the test above, where codes ARE loaded and neither
    # box could have shown whatever the window did.)
    add_test(NAME qt_apply_survey_codes_with_no_codes_loaded_is_refused_in_the_verbs_words_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>POINT 1,1|*applySurveyCodes|?commandLog"
            "-DEXPECT=! InvalidState: no survey codes are loaded; CUSTOMISE <file> loads a Katana customisation file[\r\n]+commandLog: [^\r\n]*> CODE DRAWING "
            "-DFORBID=No survey codes|Survey codes failed|Load Customisation"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/apply_survey_codes_none_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # The Survey Code Manager's Linework tab strings with the CUSTOMISATION'S
    # control codes - it reads the Document's - and follows a change of them
    # while it is open; and its Execute is the LINEWORK line, through the
    # window's one executor. By hand, over the three fixtures, whose rule WM*
    # draws a line: four points coded WM1 are one string, joined in the order
    # made, and the third is coded "WM1 GO". The tab shows the line it will
    # run - the whole drawing, in the order observed - and its summaries
    # begin with what that scope takes, the 4 points. While the start code is
    # the default ST, GO is a token nothing knows: ONE line through the four,
    # and one note. After CUSTOMISE SET linework.start=GO the tab shows GO,
    # and the third point begins a second line: TWO lines of two points, no
    # note. Execute then runs the line: it is echoed in the log (?commandLog,
    # on one line) and answered there in the verb's records, and the tab says
    # what was drawn from that answer.
    # (The window once followed the codes into a copy of its own and pressed
    # the tab's Use These Codes to do it, which logged "Linework control codes
    # set for this session." after the SET line's reply: FORBID holds the run
    # to no such line, the tab reading the Document itself now.)
    add_test(NAME qt_the_linework_tab_strings_with_the_customisations_control_codes_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${_twins}"
            "-DDRIVE=>POINT 0,0|>POINT 10,0|>POINT 20,0|>POINT 30,0|>SELECT ALL|>PROP SET code WM1|>SELECT 3|>PROP SET code \"WM1 GO\"|>SELECT NONE|@formatSurveyCodes|lineworkOrder=In the order observed|?lineworkStart|?lineworkCommand|!lineworkPreview|?lineworkSummary|>CUSTOMISE SET linework.start=GO|?lineworkStart|!lineworkPreview|?lineworkSummary|!lineworkExecute|?lineworkSummary|?commandLog"
            "-DEXPECT=lineworkStart: ST[\r\n]+lineworkCommand: LINEWORK DRAWING ORDER entity[\r\n]+.*lineworkSummary: 4 matched\\. Preview only: 1 lines from 4 points; 0 points not placed; 1 notes\\..*[\r\n]set linework\\.start=GO[\r\n]+lineworkStart: GO[\r\n]+.*lineworkSummary: 4 matched\\. Preview only: 2 lines from 4 points; 0 points not placed; 0 notes\\..*lineworkSummary: Executed as one undoable step: 2 lines drawn\\. linework scope=drawing matched=4 considered=4 lines=2 unplaced=0 notes=0[\r\n]+commandLog: [^\r\n]*> LINEWORK DRAWING ORDER entity linework scope=drawing matched=4 considered=4 lines=2 unplaced=0 notes=0 "
            "-DFORBID=control codes set for this"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/linework_tab_codes_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # A kept customisation made from ANOTHER built-in than this program has is
    # installed all the same - it is the user's - and the start says so, once
    # (cad::CustomisationStart::keptFromAnotherBuiltIn). The kept file says it
    # is based on a customisation called "another"; the built-in, through the
    # seam, is the symbol fixture, test_symbols. By hand from the file written
    # here: ONE definition, in its "symbols" list, and ONE rule - so the
    # start-up line has each noun in the singular, where it once said "1
    # survey code rules" - and the rule's one key is its one code.
    set(_from_another "${CMAKE_CURRENT_BINARY_DIR}/qt_kept_from_another_built_in.json")
    file(WRITE "${_from_another}"
"{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"mine\",
 \"basedOn\": {\"name\": \"another\", \"digest\": \"0123456789abcdef\"},
 \"symbols\": [{\"name\": \"MINE Mark\", \"atVertices\": true, \"strokes\": [[\"move\", 0, 0], [\"circle\", 0.5]]}],
 \"codes\": [{\"key\": \"MK*\", \"sets\": \"symbol\", \"symbol\": {\"name\": \"MINE Mark\"}}]}
")
    add_test(NAME qt_a_kept_customisation_made_from_another_built_in_is_installed_and_said_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE"
            "-DEXPECT=Customisation: mine, 1 definition \\(1 symbol\\) and 1 survey code rule, kept\\.[\r\n]+The kept customisation was made from another built-in customisation than this program has; CUSTOMISE RESET gives this program's\\.[\r\n].*customisation name=mine origin=kept kept=yes definitions=1 codes=1 rules=1 colours=0"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_kept_from_another_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_a_kept_customisation_made_from_another_built_in_is_installed_and_said_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:${_twins}/test_symbols.customisation.json;KATANA_CUSTOMISATION=set:${_from_another}"
        TIMEOUT 180)

    # ---- File > Settings in the real window --------------------------------------
    #
    # The item opens the one Settings dialog, non-modal (no " (modal)" after
    # what --dialog says opened), and its controls run the CUSTOMISE lines
    # through the window's executor. By hand, from the fixture test_survey
    # (11 rules over 8 keys, a key in a section 11 times, no definition; five
    # names its rules ask for that nothing defines) and the reply formats of
    # docs/customisation.md, "The replies":
    #  - Import into a session that starts with nothing: a merge, all 11
    #    added, and the session takes the file's name;
    #  - the page follows the Document: its name, "11 over 8 codes";
    #  - the box unticked as the driver fills it runs SET, and CUSTOMISE JSON
    #    then has the switch off beside the other, on by default;
    #  - Export writes the whole session to a file of this folder that the
    #    set-up test removed first, so it replaces nothing.
    # The second test reads that file in ANOTHER window, which also starts
    # with nothing: the same 11 added over 8 codes, the control codes and the
    # switches said (a whole session always says both), and the switch still
    # off.
    set(_settings_out "${CMAKE_CURRENT_BINARY_DIR}/qt_settings_exported.customisation.json")
    add_test(NAME qt_settings_exported_cleanup
        COMMAND ${CMAKE_COMMAND} -E rm -f "${_settings_out}")
    add_test(NAME qt_settings_imports_sets_and_exports_by_the_customise_lines_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@fileSettings|settingsImportPath=${_twins}/test_survey.customisation.json|!settingsImport|?settingsActiveName|?settingsActiveRules|settingsAutoCodes=off|>CUSTOMISE JSON|settingsExportPath=${_settings_out}|!settingsExport|?settingsStatus"
            "-DEXPECT=--dialog fileSettings opened settingsDialog \"Settings\"[\r\n]+loaded file=[^\r\n]*test_survey${_J} name=test_survey definitions_added=0 definitions_replaced=0 codes_added=11 codes_replaced=0 ${_nothing_else}[\r\n]+undefined names=5[\r\n].*customisation name=test_survey origin=loaded kept=no definitions=0 codes=8 rules=11 colours=0[\r\n]+settingsActiveName: test_survey[\r\n]+settingsActiveRules: 11 over 8 codes[\r\n]+set auto\\.codes=off[\r\n].*\"automation\": {[\r\n]+ +\"codesOnSurveyImport\": false,[\r\n]+ +\"lineworkOnSurveyImport\": true[\r\n].*exported file=[^\r\n]*qt_settings_exported${_J} name=test_survey definitions=0 codes=8 rules=11 replaced=no[\r\n]+settingsStatus: > CUSTOMISE EXPORT \"[^\"]*qt_settings_exported\\.customisation\\.json\" exported file="
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/settings_dialog_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_what_settings_exported_loads_again_with_the_same_counts_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE REPLACE \"${_settings_out}\"|>CUSTOMISE"
            "-DEXPECT=loaded file=[^\r\n]*qt_settings_exported${_J} name=test_survey definitions_added=0 definitions_replaced=0 codes_added=11 codes_replaced=0 colours_added=0 colours_replaced=0 linework=yes automation=yes[\r\n]+undefined names=5[\r\n].*customisation name=test_survey origin=loaded kept=no definitions=0 codes=8 rules=11 colours=0[\r\n].*automation auto\\.codes=off auto\\.linework=on"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/settings_exported_read_back_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_settings_imports_sets_and_exports_by_the_customise_lines_headless
                         qt_what_settings_exported_loads_again_with_the_same_counts_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
    set_tests_properties(qt_settings_exported_cleanup
        PROPERTIES FIXTURES_SETUP qt_settings_export_clean)
    set_tests_properties(qt_settings_imports_sets_and_exports_by_the_customise_lines_headless
        PROPERTIES FIXTURES_REQUIRED qt_settings_export_clean FIXTURES_SETUP qt_settings_exported)
    set_tests_properties(qt_what_settings_exported_loads_again_with_the_same_counts_headless
        PROPERTIES FIXTURES_REQUIRED qt_settings_exported)

    # What is edited in Settings is KEPT, and a reset made there is kept too -
    # which retires the kept file. Four windows, one after another, with the
    # symbol fixture as the built-in (4 definitions, all symbols, no rule)
    # and KATANA_CUSTOMISATION naming a file of this folder that the set-up
    # test removed: a headless window keeps a file ONLY where that variable
    # names one. By hand, from the two fixtures:
    #  1. It starts with the built-in, which is "kept" - the next start would
    #     give it - with no file there yet. The survey fixture imported in
    #     Settings is merged (the session keeps its own name; of the five
    #     names the rules ask for the built-in defines two, so three are
    #     undefined) and leaves the session not kept, so the dialog runs KEEP
    #     behind its line: written, with nothing there before to back up.
    #  2. The next window starts with that file - 4 definitions and 11 rules,
    #     "kept" - and is not told it was made from another built-in, having
    #     been kept from this very one.
    #  3. Reset to Built-in leaves the built-in, not kept (the file is what a
    #     start would give), so the dialog runs KEEP - and a session that IS
    #     the built-in is kept by setting the file aside as .bak, nothing
    #     written. Revert to Kept then has none to read and is refused in the
    #     verb's words (docs/desktop.md, "Settings").
    #  4. The fourth window starts with the built-in again, and the file set
    #     aside still holds what the first kept: loaded in the place of both
    #     kinds, 4 definitions and 11 rules over 8 codes.
    set(_settings_kept "${CMAKE_CURRENT_BINARY_DIR}/qt_settings_kept")
    add_test(NAME qt_settings_kept_cleanup
        COMMAND ${CMAKE_COMMAND} -E rm -rf "${_settings_kept}")
    add_test(NAME qt_an_import_made_in_settings_is_kept_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@fileSettings|?settingsActiveKept|settingsImportPath=${_twins}/test_survey.customisation.json|!settingsImport|?settingsStatus|?settingsActiveKept"
            "-DEXPECT=Customisation: test_symbols, 4 definitions \\(4 symbols\\) and 0 survey code rules, built in\\..*settingsActiveKept: Kept: the next start gives this customisation\\. Kept file: [^\r\n]*qt_settings_kept/customisation\\.json[\r\n]+loaded file=[^\r\n]*test_survey${_J} name=test_survey definitions_added=0 definitions_replaced=0 codes_added=11 codes_replaced=0 ${_nothing_else}[\r\n]+undefined names=3[\r\n].*customisation name=test_symbols origin=loaded kept=no definitions=4 codes=8 rules=11 colours=0[\r\n]+kept file=[^\r\n]*qt_settings_kept/customisation\\.json\"? written=yes name=test_symbols definitions=4 codes=8 rules=11 backup=none[\r\n]+settingsStatus: > CUSTOMISE \"[^\"]*test_survey\\.customisation\\.json\" loaded file=[^\r\n]* > CUSTOMISE KEEP kept file=[^\r\n]* written=yes [^\r\n]*[\r\n]+settingsActiveKept: Kept: the next start gives this customisation\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/settings_kept_1_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_what_settings_kept_is_what_the_next_window_starts_with_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@fileSettings|?settingsActiveOrigin|?settingsActiveRules|?settingsActiveKept"
            "-DEXPECT=Customisation: test_symbols, 4 definitions \\(4 symbols\\) and 11 survey code rules, kept\\..*settingsActiveOrigin: Kept by you[\r\n]+settingsActiveRules: 11 over 8 codes[\r\n]+settingsActiveKept: Kept: the next start gives this customisation\\."
            "-DFORBID=made from another built-in"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/settings_kept_2_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_a_reset_made_in_settings_retires_the_kept_file_and_revert_has_none_to_read_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@fileSettings|!settingsReset|?settingsStatus|?settingsActiveOrigin|!settingsRevert|?settingsStatus"
            "-DEXPECT=and 11 survey code rules, kept\\..*reset name=test_symbols definitions=4 codes=0 rules=0 kept=no[\r\n]+kept file=[^\r\n]*qt_settings_kept/customisation\\.json\"? written=no reason=the-session-is-the-built-in backup=[^\r\n]*qt_settings_kept/customisation\\.json\\.bak\"?[\r\n]+settingsStatus: > CUSTOMISE RESET reset name=test_symbols [^\r\n]* > CUSTOMISE KEEP kept file=[^\r\n]* written=no reason=the-session-is-the-built-in [^\r\n]*[\r\n]+settingsActiveOrigin: Built in[\r\n]+! NotFound: CUSTOMISE REVERT: no customisation is kept[^\r\n]*[\r\n]+settingsStatus: > CUSTOMISE REVERT Refused: NotFound: CUSTOMISE REVERT: no customisation is kept"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/settings_kept_3_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_after_a_reset_made_in_settings_the_next_window_starts_with_the_built_in_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE REPLACE \"${_settings_kept}/customisation.json.bak\""
            "-DEXPECT=Customisation: test_symbols, 4 definitions \\(4 symbols\\) and 0 survey code rules, built in\\..*loaded file=[^\r\n]*qt_settings_kept/customisation\\.json\\.bak\"? name=test_symbols [^\r\n]*[\r\n].*customisation name=test_symbols origin=loaded kept=no definitions=4 codes=8 rules=11 colours=0"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/settings_kept_4_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_an_import_made_in_settings_is_kept_headless
                         qt_what_settings_kept_is_what_the_next_window_starts_with_headless
                         qt_a_reset_made_in_settings_retires_the_kept_file_and_revert_has_none_to_read_headless
                         qt_after_a_reset_made_in_settings_the_next_window_starts_with_the_built_in_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:${_twins}/test_symbols.customisation.json;KATANA_CUSTOMISATION=set:${_settings_kept}/customisation.json"
        TIMEOUT 180)
    # Each window needs what the one before it left, so they run in order.
    set_tests_properties(qt_settings_kept_cleanup PROPERTIES FIXTURES_SETUP qt_settings_kept_0)
    set_tests_properties(qt_an_import_made_in_settings_is_kept_headless
        PROPERTIES FIXTURES_REQUIRED qt_settings_kept_0 FIXTURES_SETUP qt_settings_kept_1)
    set_tests_properties(qt_what_settings_kept_is_what_the_next_window_starts_with_headless
        PROPERTIES FIXTURES_REQUIRED qt_settings_kept_1 FIXTURES_SETUP qt_settings_kept_2)
    set_tests_properties(qt_a_reset_made_in_settings_retires_the_kept_file_and_revert_has_none_to_read_headless
        PROPERTIES FIXTURES_REQUIRED qt_settings_kept_2 FIXTURES_SETUP qt_settings_kept_3)
    set_tests_properties(qt_after_a_reset_made_in_settings_the_next_window_starts_with_the_built_in_headless
        PROPERTIES FIXTURES_REQUIRED qt_settings_kept_3)

    # Survey > Process Linework is the LINEWORK line through the window's one
    # executor, as Apply Survey Codes is the CODE line: on the selection when
    # there is one, else on the drawing, the scope word written so that the
    # echoed line says which. The data and every number are those worked by
    # hand for cli.linework_strings_points_by_their_control_codes_as_one_undo_step
    # (src/katana_app/CMakeLists.txt): no survey codes; points 1 and 2, ten
    # metres apart, coded "KB ST" and "KB END". With both selected the line
    # is LINEWORK SELECTION: 2 taken, both points, one line of two - entity
    # 3 - noted as unstyled. With nothing selected it is LINEWORK DRAWING
    # over the 2 points and that line: the line is already there, so nothing
    # is drawn and its 2 points are left out. The log (?commandLog, on one
    # line) shows each line echoed and answered where it ran.
    add_test(NAME qt_process_linework_runs_the_linework_line_on_the_selection_or_the_drawing_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>POINT 0,0|>POINT 10,0|>SELECT 1|>PROP SET code \"KB ST\" text|>PROP SET point 1 text|>SELECT 2|>PROP SET code \"KB END\" text|>PROP SET point 2 text|>SELECT ALL|*surveyLinework|>SELECT NONE|*surveyLinework|?commandLog"
            "-DEXPECT=linework scope=selection matched=2 considered=2 lines=1 unplaced=0 notes=1[\r\n]+string name=KB key=\"\" number=\"\" points=2 vertices=2 closed=no layer=0 entity=3[\r\n].*linework scope=drawing matched=3 considered=2 lines=0 unplaced=0 notes=1[\r\n]+left_out=2 reason=already-drawn lines=1[\r\n].*commandLog: [^\r\n]*> LINEWORK SELECTION linework scope=selection matched=2 [^\r\n]*> LINEWORK DRAWING linework scope=drawing matched=3 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/process_linework_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_process_linework_runs_the_linework_line_on_the_selection_or_the_drawing_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

    # --customise beside an import file: both are taken. The script once put
    # the file after the switch, which takes every path up to the next switch
    # - so the archive was read as a fourth customisation file, the one
    # CUSTOMISE line refused them all, nothing was imported, and the run
    # still wrote its picture and passed. Each record is pinned on its own
    # elsewhere, from its file: the three fixtures' (qt_customisation_headless)
    # and the archive's four surfaces
    # (qt_import_tin_only_archive_adds_its_surfaces_headless). The
    # customisation comes first: it is loaded before anything is opened.
    add_test(NAME qt_a_customisation_folder_and_an_import_file_are_both_taken_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${_twins}"
            "-DIMPORT=${PROJECT_SOURCE_DIR}/tests/archive12d/data/multiple_tins.12da"
            "-DEXPECT=customisation name=test_linestyles origin=loaded kept=no definitions=7 codes=8 rules=11 colours=0[\r\n].*imported file=\"?[^ ]*/multiple_tins[.]12da\"? kind=archive entities=0 alignments=0 surfaces=4 "
            "-DFORBID=not a Katana customisation file"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_and_import_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")

    # A --customise that is refused does not stop a run that takes a picture:
    # it is said, in the reader's words, and the steps after it run - LIST
    # here, of an empty drawing. (A batch run it fails: the tests beside
    # qt_script_batch_run_fails_at_a_refused_line_headless.) The folder holds
    # one file with the suffix the script looks for and JSON of another
    # format in it.
    set(_refused_folder "${CMAKE_CURRENT_BINARY_DIR}/qt_refused_customisation")
    file(WRITE "${_refused_folder}/not_one.customisation.json"
        "{\"format\": \"another-format\", \"version\": 1, \"name\": \"x\"}\n")
    add_test(NAME qt_a_refused_customise_switch_leaves_a_screenshot_run_going_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${_refused_folder}"
            "-DDRIVE=>LIST"
            "-DEXPECT=! ParseFailure: not a Katana customisation file.*[\r\n]0 entities"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_refused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_apply_survey_codes_runs_the_code_line_on_the_selection_or_the_drawing_headless
        qt_apply_survey_codes_with_no_codes_loaded_is_refused_in_the_verbs_words_headless
        qt_the_linework_tab_strings_with_the_customisations_control_codes_headless
        qt_a_customisation_folder_and_an_import_file_are_both_taken_headless
        qt_a_refused_customise_switch_leaves_a_screenshot_run_going_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)

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
    # a drawing saved with the symbol fixture loaded, then opened by a second
    # run that never loads it, which must say which customisation is missing
    # - a warning in the log, the project opened all the same. By NAME: what
    # the fixture's "name" says, test_symbols, where the record once held the
    # name of the file a library was read from.
    set(_recorded "${CMAKE_CURRENT_BINARY_DIR}/customisation_recorded.katana")
    set(_resaved "${CMAKE_CURRENT_BINARY_DIR}/customisation_resaved.katana")
    add_test(NAME qt_customisation_recorded_cleanup
        COMMAND ${CMAKE_COMMAND} -E rm -rf "${_recorded}" "${_resaved}")
    add_test(NAME qt_a_project_records_its_customisation_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE \"${_twins}/test_symbols.customisation.json\"|>LINE 0,0 10,0|>SAVE ${_recorded}"
            "-DEXPECT=saved to "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_record_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_opening_names_the_customisation_not_loaded_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>OPEN ${_recorded}"
            "-DEXPECT=opened .*Warning: this project was drawn with customisations that are not loaded: \"test_symbols\"\\. CUSTOMISE <file> loads a Katana customisation file\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/customisation_missing_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Saved where the customisation is missing, the project still records
    # it: the open warns, the save (to a copy, so the test above reads its own
    # project) writes the record, and opening the copy warns again. By hand:
    # this run never loads test_symbols, so nothing it does can judge it. The
    # closing quote and the full stop are in the expression, so a longer name
    # cannot pass for it.
    add_test(NAME qt_saving_where_a_file_is_missing_keeps_it_recorded_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>OPEN ${_recorded}|>SAVE ${_resaved}|>NEW|>OPEN ${_resaved}"
            "-DEXPECT=not loaded: \"test_symbols\"\\..*saved to .*new drawing.*opened .*not loaded: \"test_symbols\"\\."
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
    # the three hand-written fixture customisations (-DCUSTOMISE_DIR, one
    # CUSTOMISE line) loaded into a session that starts with nothing - every
    # program test does, unless it names its built-in (the scan at the head of
    # this file) - so what a manager lists is the fixtures' and nothing else;
    # the driver says what opened, and a manager opened modally would say
    # "(modal)" after its title, where each EXPECT wants the line to end. What each
    # manager then shows is read back with ? (--report).
    #
    # Styles: two styles made on the command line, so the table lists them.
    add_test(NAME qt_format_menu_opens_the_style_manager_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${_twins}"
            "-DDRIVE=>STYLE NEW Kerb|>STYLE NEW Water|@formatStyles|?styleTable"
            "-DEXPECT=--dialog formatStyles opened styleManagerDialog \"Styles and Linetypes\"[\r\n]+styleTable: Kerb \\|[^;]*; Water \\|"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_style_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The same manager's Linetypes page (managerTabs, filled as a click on
    # its tab does) narrowed to the fixture's group Test/Lines. By hand from
    # test_linestyles: that group holds TEST Dashed Kerb, a paper linestyle,
    # and TEST Gate, a two-point one, and nothing else. The Kerb chosen, the
    # details pane explains its kind in the general words, and says where the
    # definition came from: the NAME of its customisation, test_linestyles
    # (LineStyle::source), where it was the name of the file a library was
    # read from.
    add_test(NAME qt_the_style_manager_lists_a_library_group_s_linestyles_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${_twins}"
            "-DDRIVE=@formatStyles|managerTabs=Linetypes|linetypeGroup=Test/Lines|linetypeTable=TEST Dashed Kerb|?linetypeTable|?libraryDetails"
            "-DEXPECT=linetypeTable: TEST Dashed Kerb \\| Library \\| Test/Lines \\| paper \\|[^;\r\n]*; TEST Gate \\| Library \\| Test/Lines \\| two-point \\|[^;\r\n]*[\r\n]+libraryDetails: TEST Dashed Kerb Group: Test/Lines Customisation: test_linestyles Kind: Paper linestyle: millimetres on the plot, the same at every scale[^\r\n]*Library linestyles belong to the session's customisation and are read-only here\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_style_manager_linetypes_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Symbols, searched for TEST. By hand from the fixtures: the four
    # definitions of test_symbols are all symbols - its "symbols" list says
    # so, of TEST Tree too, which is not drawn at vertices, and rule TR* draws
    # it as one besides - and PX* asks for TEST Missing Symbol, which nothing
    # defines and the library lists as missing. Sorted by name, case folded,
    # the five come together.
    add_test(NAME qt_format_menu_opens_the_symbol_library_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${_twins}"
            "-DDRIVE=@formatSymbols|filterText=TEST|?symbolGrid"
            "-DEXPECT=--dialog formatSymbols opened symbolLibraryDialog \"Symbol Library\"[\r\n]+symbolGrid: [^\r\n]*TEST Missing Symbol ; TEST Survey Mark ; TEST Tree ; TEST U Turn ; TEST Valve"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_symbol_library_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The definition editor, which no menu item opens: the symbol library's
    # New Symbol asks for it, and it is then a panel of its own (%). A symbol
    # is typed as two strokes and saved, and the library - made the target
    # again - lists it and says what it holds. By hand: a move and a draw are
    # 2 strokes, with no text and no pen change; made in the editor, it came
    # from no customisation. No customisation is loaded: nothing here reads
    # a file.
    #
    # New Symbol, a name, strokes and Save - the path a person takes - and
    # nothing else: At vertices is NOT ticked. The symbol is in the grid
    # because the definition is listed as a symbol (LineStyle::symbol, which
    # New Symbol sets and cad::symbolLibrary reads), and no rule or style
    # names it. The steps once ticked At vertices, in a tree whose catalogue
    # did not read that flag yet, and would then have gone on passing with
    # the flag lost.
    add_test(NAME qt_a_symbol_made_in_the_definition_editor_is_listed_by_the_symbol_library_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatSymbols|!symbolNew|%definitionEditorDialog|definitionName=TEST Post|definitionStrokes=[\"move\", 0, 0],\\n[\"draw\", 0, 2]|!definitionSave|?definitionHeading|%symbolLibraryDialog|symbolGrid=TEST Post|?detailName|?detailSource|?detailContent"
            "-DEXPECT=Symbol \"TEST Post\" added to the library: 2 strokes, 0 texts\\.[\r\n]+definitionHeading: Symbol \"TEST Post\"[\r\n]+detailName: TEST Post[\r\n]+detailSource: Made in this session[\r\n]+detailContent: 2 strokes, 0 texts, 0 pen changes"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/definition_editor_new_symbol_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Delete Anyway deletes, with FORCE, what something uses, and the library
    # has no undo: it is offered only under Delete's refusal, with what uses
    # the definition listed. A press of it out of nowhere - here on a symbol
    # just saved, which nothing has refused to delete - is turned down by the
    # driver as any disabled button's is. The button ITSELF is disabled and
    # hidden, not only the row it sits in: with the row alone hidden this
    # press was accepted, and ran the FORCE line.
    add_test(NAME qt_delete_anyway_cannot_be_pressed_before_delete_has_been_refused_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatSymbols|!symbolNew|%definitionEditorDialog|definitionName=TEST Post|definitionStrokes=[\"move\", 0, 0],\\n[\"draw\", 0, 2]|!definitionSave|!definitionDeleteAnyway"
            "-DREFUSED=Symbol \"TEST Post\" added to the library: 2 strokes, 0 texts\\.[\r\n]+.*--press: definitionEditorDialog's button definitionDeleteAnyway is disabled"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/definition_editor_delete_anyway_refused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Delete, end to end: the editor's line is CUSTOMISE REMOVE, run by the
    # window's one executor and answered by the interpreter's verb. By hand
    # from the fixtures (tests/data/customisation, the three loaded by
    # --customise into a session that starts with nothing): test_survey's
    # eleven rules are, in the order its file lists them, seven feature
    # rules, then three symbol rules, then one of attributes, so the first
    # symbol rule is #7, AC*, and the symbol it draws is TEST Survey Mark,
    # which test_symbols defines - and which no other rule, and in an empty
    # drawing no style or layer, names. So:
    #   - Delete is REFUSED, and the log holds the verb's own words - one of
    #     the one definition is in use, and the rule that names it - as the
    #     window logs any refused line: "! " and the error with its code;
    #   - the editor shows those words as they came, ONCE - they cite the
    #     rule, so the editor's own list of what names the definition does
    #     not follow them, as it did until 2026-10-07 - and then what Delete
    #     Anyway would do (a point is all that draws it, so a stand-in mark
    #     is all that follows);
    #   - Delete Anyway runs the line with FORCE, and the verb's reply counts
    #     the one rule that still names what is gone;
    #   - the form then holds a definition the library does not.
    # The refusal's ";" is passed over with [^\r\n]*: nothing else is.
    add_test(NAME qt_a_definition_a_survey_code_names_is_refused_by_delete_and_removed_by_delete_anyway_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${_twins}"
            "-DDRIVE=@formatSymbols|symbolGrid=TEST Survey Mark|!symbolEdit|%definitionEditorDialog|!definitionDelete|?definitionIssues|!definitionDeleteAnyway|?definitionHeading|?definitionIssues"
            "-DEXPECT=! InvalidState: CUSTOMISE REMOVE: 1 of the 1 definition named is in use, so nothing was removed[^\r\n]* FORCE removes what is used too, and what names it then draws plain[\r\n]+  \"TEST Survey Mark\": rule #7 AC\\* \\(symbol\\) names it[\r\n]+definitionIssues: \"TEST Survey Mark\" was not deleted: InvalidState: CUSTOMISE REMOVE: 1 of the 1 definition named is in use, so nothing was removed[^\r\n]* FORCE removes what is used too, and what names it then draws plain \"TEST Survey Mark\": rule #7 AC\\* \\(symbol\\) names it Delete Anyway runs the line again with FORCE\\. A point naming it is then drawn as a stand-in mark\\.[\r\n]+removed definition=\"TEST Survey Mark\" rules=1 styles=0 layers=0[\r\n]+definitionHeading: Symbol \"TEST Survey Mark\" - not in the library[\r\n]+definitionIssues: \"TEST Survey Mark\" was deleted from the library\\. It is still shown here: Save puts it back\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/definition_editor_delete_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_a_symbol_made_in_the_definition_editor_is_listed_by_the_symbol_library_headless
                         qt_delete_anyway_cannot_be_pressed_before_delete_has_been_refused_headless
                         qt_a_definition_a_survey_code_names_is_refused_by_delete_and_removed_by_delete_anyway_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
    # An editor's own commit keeps a kept session kept - in the WINDOW. Its
    # half of that is two lines: MainWindow::buildFormatActions hands the
    # workbench `hasKeptFile`, and loadDefaultCustomisation sets what that
    # answers from its host's kept-file path. The workbench's own tests supply
    # the predicate themselves and the kept checks above TYPE the KEEP line,
    # so with either of the window's lines deleted every test passed while the
    # real window kept nothing (found in review, 2026-10-07).
    #
    # Both runs start with the symbol fixture as the built-in, through the
    # seam - a KEPT session, being what the next start gives - and save a new
    # symbol in the definition editor as a person does: New Symbol, a name,
    # two strokes, Save. By hand: the fixture's 4 definitions and the new one
    # are 5, it has no rule, and nothing was kept before, so no backup.
    #
    #   a kept file    the Save is followed by the CUSTOMISE KEEP line, through
    #                  the window's one executor. Its `kept` record - written,
    #                  the session being no longer the built-in - comes BEFORE
    #                  the editor's own line: the hook is run after the commit
    #                  and before the editor says what it saved. An edit makes
    #                  a session `edited` and not kept (Document::
    #                  setStyleLibrary), and a KEEP makes it kept again
    #                  (setCustomisationKept): edited AND kept.
    #   none           no line is run: neither a `kept` record nor KEEP's
    #                  refusal, which a window that ran it would log after
    #                  every Save. The session is edited and no longer kept.
    #
    # KATANA_CUSTOMISATION is said by both, set and unset: the second run is
    # about its absence. The file of the first is this test's own, removed
    # before it.
    set(_editor_kept "${CMAKE_CURRENT_BINARY_DIR}/qt_editor_kept_customisation")
    add_test(NAME qt_editor_kept_customisation_cleanup
        COMMAND ${CMAKE_COMMAND} -E rm -rf "${_editor_kept}")
    add_test(NAME qt_a_save_in_the_definition_editor_keeps_a_kept_session_in_its_kept_file_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatSymbols|!symbolNew|%definitionEditorDialog|definitionName=TEST Post|definitionStrokes=[\"move\", 0, 0],\\n[\"draw\", 0, 2]|!definitionSave|>CUSTOMISE"
            "-DEXPECT=Customisation: test_symbols, 4 definitions \\(4 symbols\\) and 0 survey code rules, built in\\..*kept file=[^\r\n]*qt_editor_kept_customisation/customisation\\.json\"? written=yes name=test_symbols definitions=5 codes=0 rules=0 backup=none[\r\n].*Symbol \"TEST Post\" added to the library: 2 strokes, 0 texts\\.[\r\n].*customisation name=test_symbols origin=edited kept=yes definitions=5 codes=0 rules=0 colours=0"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/definition_editor_save_kept_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_a_save_in_the_definition_editor_runs_no_keep_where_the_window_has_no_kept_file_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatSymbols|!symbolNew|%definitionEditorDialog|definitionName=TEST Post|definitionStrokes=[\"move\", 0, 0],\\n[\"draw\", 0, 2]|!definitionSave|>CUSTOMISE"
            "-DEXPECT=Customisation: test_symbols, 4 definitions \\(4 symbols\\) and 0 survey code rules, built in\\..*Symbol \"TEST Post\" added to the library: 2 strokes, 0 texts\\.[\r\n].*customisation name=test_symbols origin=edited kept=no definitions=5 codes=0 rules=0 colours=0"
            "-DFORBID=kept file=|CUSTOMISE KEEP"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/definition_editor_save_not_kept_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_a_save_in_the_definition_editor_keeps_a_kept_session_in_its_kept_file_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:${_twins}/test_symbols.customisation.json;KATANA_CUSTOMISATION=set:${_editor_kept}/customisation.json"
        TIMEOUT 180)
    set_tests_properties(qt_a_save_in_the_definition_editor_runs_no_keep_where_the_window_has_no_kept_file_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;KATANA_BUILTIN_CUSTOMISATION=set:${_twins}/test_symbols.customisation.json;KATANA_CUSTOMISATION=unset:"
        TIMEOUT 180)
    set_tests_properties(qt_editor_kept_customisation_cleanup
        PROPERTIES FIXTURES_SETUP qt_editor_kept_clean)
    set_tests_properties(qt_a_save_in_the_definition_editor_keeps_a_kept_session_in_its_kept_file_headless
        PROPERTIES FIXTURES_REQUIRED qt_editor_kept_clean)
    # The code manager explaining a fixture code. By hand from test_survey:
    # WM01 is met by the prefix key WM* (rule #0 of the fixture, a feature
    # rule - and rule #0 of the session, which starts with nothing), which
    # puts it on the layer TEST SERVICES. The
    # field is "layer" and the section "feature" - the Katana customisation
    # format's words, where the window used to show the survey code file's
    # `model` and `map_data`: the expression was changed from the table "The
    # words a rule is shown by" in docs/customisation.md, not from a run.
    add_test(NAME qt_format_menu_opens_the_code_manager_and_explains_a_code_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${_twins}"
            "-DDRIVE=@formatSurveyCodes|testCode=WM01|?explainHeading|?explainFields"
            "-DEXPECT=--dialog formatSurveyCodes opened surveyCodeManagerDialog \"Survey Code Manager\"[\r\n]+explainHeading: WM01: matched - met by the prefix key WM\\*[^\r\n]*[\r\n]+explainFields: layer \\| TEST SERVICES \\| #0 WM\\* \\(feature\\)"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/format_code_manager_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # What Format says in the real window, in general words: every item of
    # the menu with its status tip (?formatMenu), the log of a customisation
    # loaded and of each manager opened, and none of it names another
    # program or its file suffixes (FORBID). The menu no longer offers Load
    # Customisation and Replace Loaded Customisation - the file dialogs over
    # those suffixes - so Global Modify follows the Survey Code Manager with
    # nothing between ("] ; "), and FORBID holds the run to that too. The
    # widget tests walk the managers' own texts (qt_widgets.GeneralTexts.*).
    add_test(NAME qt_the_format_menu_and_the_customisation_log_name_no_other_program_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DCUSTOMISE_DIR=${_twins}"
            "-DDRIVE=?formatMenu|@formatStyles|@formatSymbols|@formatSurveyCodes"
            "-DEXPECT=loaded file=[^\r\n]*test_survey${_J} name=test_survey [^\r\n]*codes_added=11[^\r\n]*[\r\n].*formatMenu: Format: Layers\\.\\.\\. \\[[^\r\n]*Styles and Linetypes\\.\\.\\. \\[[^\r\n]*library linestyles[^\r\n]*Survey Code Manager\\.\\.\\. \\[[^]]*\\] ; Global Modify\\.\\.\\. \\[[^]]*\\] ; Purge Unused\\.\\.\\. \\[[^\r\n]*[\r\n].*opened surveyCodeManagerDialog"
            "-DFORBID=12[dD]|\\.4d|[.]mapfile|Load Customisation|Replace Loaded Customisation"
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
            "-DCUSTOMISE_DIR=${_twins}"
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
            "-DCUSTOMISE_DIR=${_twins}"
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
    # than the edits going with the window. The codes are the built-in's, so
    # the test names its built-in: the fixture of survey codes, through the
    # seam (set below). By hand: its 11 rules and a duplicate of rule 0 make
    # 12 in the buffer. The start-up line is this run's too - the fixture's
    # name, its counts (no definitions, so no symbols) and that it is built in.
    add_test(NAME qt_quitting_with_unapplied_code_manager_edits_is_refused_and_said_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@formatSurveyCodes|!ruleDuplicate|?dirtyIndicator|>QUIT"
            "-DEXPECT=Customisation: test_survey, 0 definitions \\(0 symbols\\) and 11 survey code rules, built in\\..*dirtyIndicator: Unapplied edits:[^\r\n]* 12 rules[.][\r\n]+! Unapplied Edits: the Survey Code Manager has rule edits that are not on the drawing"
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
    # malformed point, and the tool carries on at the same step. The view
    # runs it (ViewWorkspace::wireTools) as its ZOOM line through the one
    # executor - naming the view the tool runs in, ZOOM view=1, whichever
    # view is active - so its record comes before the inverse, and the line
    # is logged once: the command line leaves the echo to the executor. Before
    # the view ran it the host refused it aloud ("cannot run inside"); a
    # ZOOM wrongly sent to the tool would be refused by the tool instead and
    # the inverse would still follow, which the result alone cannot tell
    # apart - hence the record, and the refusals forbidden.
    add_test(NAME qt_dist_typed_at_the_command_line_reports_the_inverse_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>DIST|>0,0|>ZOOM|>3,4|?commandLog"
            "-DEXPECT=view=1 kind=plan[^\r\n]*.*Horizontal distance 5[.]000.*commandLog: .*> ZOOM view=1 view=1 kind=plan"
            "-DFORBID=cannot run inside|not an input this tool takes|> ZOOM > ZOOM"
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
    # Linked plan views (cad/view_link.hpp, docs/desktop.md "Linked views"):
    # a second plan view opened and linked with the first, the first zoomed
    # by its line, and the second is where the first is - the ZOOM's reply
    # says it followed, and VIEWS lists it there. The Link button of the
    # second view's bar is checked from the link the lines made; the grab is
    # that view's dock, bar and all.
    add_test(NAME qt_linked_plan_views_zoom_together_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>VIEWS OPEN plan|>VIEWS LINK 1,2|>ZOOM CENTRE 50,40 SCALE 8 view=1|>VIEWS|%View2|?ViewLinkButton"
            "-DEXPECT=leader=1 linked=1,2 moved=2.*view=1 kind=plan centre=50,40 scale=8[^\r\n]*[\r\n]+view=2 kind=plan followed=1 centre=50,40 scale=8.*view=2 kind=plan title=\"Plan 2\" active=[a-z]+ linked=yes centre=50,40 scale=8.*ViewLinkButton: Linked, checked"
            "-DFORBID=unknown command"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/linked_plan_views_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # And out again: UNLINK of one of two leaves the other alone, which is no
    # link, so both leave, a ZOOM moves one view only and the buttons show
    # the open chain.
    add_test(NAME qt_unlinking_one_of_two_views_unlinks_both_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>VIEWS OPEN plan|>VIEWS LINK 1,2|>VIEWS UNLINK 2|>ZOOM IN view=1|%View2|?ViewLinkButton"
            "-DEXPECT=unlinked=1,2 linked=none.*view=1 kind=plan centre=.*ViewLinkButton: Link this view, unchecked"
            "-DFORBID=followed="
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/unlinked_plan_views_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The owner's case: a design view hiding the as-built layer beside an
    # as-built view hiding the design one (VIEWS HIDE), linked, framed on the
    # works from the design view, the design line selected. Each record says
    # what its view hides; the PNG shows the design line selected on the left
    # and the as-built line alone on the right, both at the one scale.
    add_test(NAME qt_a_design_view_and_an_as_built_view_hide_each_others_layers_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LAYER NEW design|>LAYER SET design|>LINE 10,10 40,30|>LAYER NEW asbuilt|>LAYER SET asbuilt|>LINE 10,30 40,10|>VIEWS OPEN plan|>VIEWS HIDE 1 asbuilt|>VIEWS HIDE 2 design|>VIEWS LINK 1,2|>ZOOM WINDOW 0,0,50,40 view=1|>SELECT LAYER design|>VIEWS"
            "-DEXPECT=view=1 kind=plan title=\"Plan 1\"[^\r\n]*linked=yes[^\r\n]*hidden=asbuilt[\r\n]+view=2 kind=plan title=\"Plan 2\"[^\r\n]*linked=yes[^\r\n]*hidden=design[\r\n]+views=2 linked=2"
            "-DFORBID=unknown command|no layer '"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/design_and_as_built_views_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The selection in every view (docs/desktop.md): the design line selected
    # is drawn dashed in the design view and as a dotted ghost in the as-built
    # view that hides its layer; each record says the view's switch is on.
    # What each view drew says it too: the design view, its line and the
    # line's three grips; the as-built view, its own line, the ghost and no
    # grip on it. The PNG is what to look at: the ghost in the right-hand
    # view, where the as-built line crosses it.
    add_test(NAME qt_a_selection_shows_as_a_ghost_where_the_view_hides_its_layer_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LAYER NEW design|>LAYER SET design|>LINE 10,10 40,30|>LAYER NEW asbuilt|>LAYER SET asbuilt|>LINE 10,30 40,10|>VIEWS OPEN plan|>VIEWS HIDE 1 asbuilt|>VIEWS HIDE 2 design|>VIEWS LINK 1,2|>ZOOM WINDOW 0,0,50,40 view=1|>SELECT LAYER design|>VIEWS|?PlanView1|?PlanView2"
            "-DEXPECT=view=1 kind=plan[^\r\n]*ghosts=on hidden=asbuilt[\r\n]+view=2 kind=plan[^\r\n]*ghosts=on hidden=design[\r\n]+views=2.*PlanView1: drawn=1 ghosts=0 grips=3[\r\n]+PlanView2: drawn=1 ghosts=1 grips=0"
            "-DFORBID=unknown command|no layer '"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/selection_ghost_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The as-built view's switch off, by the line its Layers popup runs: no
    # trace of the design line there - no ghost, and no grip - while the
    # as-built line is still drawn. The PNG shows the right-hand view with
    # the as-built line alone.
    add_test(NAME qt_a_view_with_its_ghosts_off_shows_no_trace_of_the_selection_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LAYER NEW design|>LAYER SET design|>LINE 10,10 40,30|>LAYER NEW asbuilt|>LAYER SET asbuilt|>LINE 10,30 40,10|>VIEWS OPEN plan|>VIEWS HIDE 1 asbuilt|>VIEWS HIDE 2 design|>VIEWS LINK 1,2|>ZOOM WINDOW 0,0,50,40 view=1|>SELECT LAYER design|>VIEWS SET 2 ghosts=off|?PlanView2"
            "-DEXPECT=view=2 kind=plan[^\r\n]*ghosts=off hidden=design.*PlanView2: drawn=1 ghosts=0 grips=0"
            "-DFORBID=unknown command|no layer '|does not take"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/selection_ghost_off_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The same switch by the mouse: view 2's Layers popup opened from its
    # bar, a panel of its own, and its box unticked - which runs the line,
    # whose reply is view 2's record with its ghosts off.
    add_test(NAME qt_the_layers_popups_box_turns_a_views_ghosts_off_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LAYER NEW design|>LAYER SET design|>LINE 10,10 40,30|>LAYER NEW asbuilt|>LAYER SET asbuilt|>LINE 10,30 40,10|>VIEWS OPEN plan|>VIEWS HIDE 2 design|>SELECT LAYER design|%View2|!ViewLayersButton|%ViewLayersPopup|?ViewLayersShowSelection|!ViewLayersShowSelection|?ViewLayersShowSelection|?PlanView2"
            "-DEXPECT=ViewLayersShowSelection: Show the selection on hidden layers, checked[\r\n]+view=2 kind=plan[^\r\n]*ghosts=off hidden=design[\r\n]+ViewLayersShowSelection: Show the selection on hidden layers, unchecked[\r\n]+PlanView2: drawn=1 ghosts=0 grips=0"
            "-DFORBID=unknown command|no layer '|does not take"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/layers_popup_ghosts_off_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # And by the View menu, for the active view: the same line through the
    # same executor (its reply, view 2's record with its ghosts off), the
    # item's check set from the view, and view 1 untouched.
    add_test(NAME qt_the_view_menu_turns_the_active_views_ghosts_off_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>VIEWS OPEN plan|?viewSelectionGhosts|*viewSelectionGhosts|?viewSelectionGhosts|>VIEWS"
            "-DEXPECT=viewSelectionGhosts: Show the Selection on Hidden Layers, checked[\r\n]+view=2 kind=plan[^\r\n]*ghosts=off[\r\n]+viewSelectionGhosts: Show the Selection on Hidden Layers, unchecked[\r\n]+view=1 kind=plan[^\r\n]*ghosts=on[\r\n]+view=2 kind=plan[^\r\n]*ghosts=off[\r\n]+views=2"
            "-DFORBID=unknown command|does not take"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/view_menu_ghosts_off_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The View menu offers the zoom items the active view's kind takes, as
    # its bar does (cad::zoomTakes): a section frames no scope, so its Zoom
    # to Selection is disabled, and a click on it refused, where it would be
    # offered and ZOOM refuse it.
    add_test(NAME qt_the_view_menus_zoom_to_selection_is_disabled_for_a_section_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>VIEWS OPEN section|*viewZoomSelection"
            "-DREFUSED=view=2 kind=section[^\r\n]*[\r\n]+.*--trigger viewZoomSelection failed: [^\r\n]*that menu item is disabled"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/view_menu_zoom_selection_section_refused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The View menu's zoom items and a view bar's zoom buttons, run: each is
    # its ZOOM line through the one executor - the menu's for the active
    # view, a button's with its view= - logged as the line it is. In and
    # Out about the plan view's centre, the line's middle (25, 20) once it
    # is framed; Zoom to Selection frames the selected line there. On a 3D
    # view, which takes them all but WINDOW and CENTRE, In and Out as its
    # wheel zooms there, and Zoom to Selection puts its target at the line's
    # middle on the datum, (25, 20, 0).
    add_test(NAME qt_the_view_menus_zoom_items_and_the_bars_buttons_run_their_lines_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LINE 10,10 40,30|>ZOOM|>SELECT ALL|*viewZoomIn|*viewZoomOut|*viewZoomSelection|%View1|!ViewZoomInButton|!ViewZoomOutButton|>VIEWS OPEN 3d|*viewZoomIn|*viewZoomSelection|%View2|!ViewZoomOutButton|?commandLog"
            "-DEXPECT=commandLog: .*> ZOOM IN view=1 kind=plan centre=25,20 scale=[^ ]+ area=[^ ]+ > ZOOM OUT view=1 kind=plan centre=25,20 [^>]*> ZOOM SELECTION scope=selection matched=1 view=1 kind=plan centre=25,20 [^>]*> ZOOM IN view=1 view=1 kind=plan centre=25,20 [^>]*> ZOOM OUT view=1 view=1 kind=plan centre=25,20 [^>]*> VIEWS OPEN 3d view=2 kind=3d [^>]*> ZOOM IN view=2 kind=3d target=[^>]*> ZOOM SELECTION scope=selection matched=1 view=2 kind=3d target=25,20,0 [^>]*> ZOOM OUT view=2 view=2 kind=3d target="
            "-DFORBID=unknown command|does not take|--press: .* is hidden"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/view_zoom_items_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # View > Link This View links the ACTIVE view, as it says, by its Link
    # button's line through the one executor, and is checked from the link:
    # the second plan view, alone in a link that waits; the first, activated,
    # joining it; then View > Unlink All Views takes both out.
    add_test(NAME qt_the_view_menus_link_items_link_the_active_view_and_unlink_all_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>VIEWS OPEN plan|*viewLinkActive|?viewLinkActive|>VIEWS ACTIVATE 1|?viewLinkActive|*viewLinkActive|?viewLinkActive|*viewUnlinkAll|?viewLinkActive|?commandLog"
            "-DEXPECT=leader=2 linked=2 moved=none[\r\n]+viewLinkActive: Link This View, checked[\r\n]+view=1 kind=plan[^\r\n]*[\r\n]+viewLinkActive: Link This View, unchecked[\r\n]+leader=2 linked=1,2 moved=1[\r\n]+viewLinkActive: Link This View, checked[\r\n]+unlinked=1,2 linked=none[\r\n]+viewLinkActive: Link This View, unchecked[\r\n]+commandLog: .*> VIEWS LINK 2 TO 2 leader=2 linked=2 moved=none .*> VIEWS LINK 1 TO 2 leader=2 linked=1,2 moved=1 > VIEWS UNLINK ALL unlinked=1,2 linked=none"
            "-DFORBID=unknown command|No plan view"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/view_menu_link_items_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A 3D view cannot be linked, and while one is active Link This View is
    # disabled - it linked the plan view used last, out of sight, and showed
    # itself checked for the 3D view.
    add_test(NAME qt_link_this_view_is_disabled_while_a_3d_view_is_active_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>VIEWS OPEN 3d|*viewLinkActive"
            "-DREFUSED=view=2 kind=3d[^\r\n]*[\r\n]+.*--trigger viewLinkActive failed: [^\r\n]*that menu item is disabled"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/link_this_view_3d_refused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A press of a view bar's tool that the bar hid for want of room is
    # refused, as a disabled button's is: a person cannot see it to click
    # it. A 3D view's bar hides Link, which its kind has none of.
    add_test(NAME qt_a_press_of_a_hidden_bar_tool_is_refused_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>VIEWS OPEN 3d|%View2|!ViewLinkButton"
            "-DREFUSED=view=2 kind=3d[^\r\n]*[\r\n]+.*--press: View2's button ViewLinkButton is hidden"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/hidden_bar_tool_refused_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # ZOOM on the shared scope (cad/view_verbs.hpp): the selection framed in
    # the second view, the first following it. By hand the line's middle is
    # (25, 20); the reply is the scope's record, then the view moved, then
    # the one that followed.
    add_test(NAME qt_zoom_to_selection_moves_the_linked_views_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>VIEWS OPEN plan|>VIEWS LINK 1,2|>LINE 10,10 40,30|>SELECT ALL|>ZOOM SELECTION view=2|>VIEWS"
            "-DEXPECT=scope=selection matched=1[\r\n]+view=2 kind=plan centre=25,20[^\r\n]*[\r\n]+view=1 kind=plan followed=2 centre=25,20"
            "-DFORBID=unknown command|does not take"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/zoom_to_selection_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A view frames what it shows of the selection, in plan as in 3D: the
    # as-built view hides the design layer with its ghosts off, so its bar's
    # Zoom to Selection, with the design line selected, moves nothing and
    # says so. It framed the line it does not show - empty ground there, the
    # design line's middle (25, 20) - and took the design view along. Both
    # stay on the as-built line's middle, (70, 20), where ZOOM WINDOW put
    # them; the PNG is view 2 still showing the as-built line.
    add_test(NAME qt_a_plan_views_zoom_to_selection_moves_nothing_it_does_not_show_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LAYER NEW design|>LAYER SET design|>LINE 10,10 40,30|>LAYER NEW asbuilt|>LAYER SET asbuilt|>LINE 60,30 90,10|>VIEWS OPEN plan|>VIEWS HIDE 1 asbuilt|>VIEWS HIDE 2 design|>VIEWS SET 2 ghosts=off|>VIEWS LINK 1,2|>ZOOM WINDOW 40,0,100,40 view=2|>SELECT LAYER design|%View2|!ViewZoomSelectionButton|>VIEWS"
            "-DEXPECT=scope=selection matched=1[\r\n]+view=2 kind=plan shown=0 moved=no[\r\n]+.*view=1 kind=plan title=\"Plan 1\"[^\r\n]* linked=yes centre=70,20 [^\r\n]*[\r\n]+view=2 kind=plan title=\"Plan 2\"[^\r\n]* linked=yes centre=70,20 "
            "-DFORBID=unknown command|no layer '|centre=25,20"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/zoom_to_selection_shows_none_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # View > Zoom To: the shared scope and filter controls build the ZOOM
    # line and hand it to the one executor; the status says what ZOOM
    # answered. The design line alone is taken: the whole drawing, filtered
    # to the design layer.
    add_test(NAME qt_zoom_to_dialog_runs_its_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LAYER NEW design|>LAYER SET design|>LINE 10,10 40,30|>LAYER NEW other|>LAYER SET other|>CIRCLE 100,100 5|@viewZoomTo|!zoomToScopeDrawing|zoomToFilterLayer=design|?zoomToLine|!zoomToRun|?zoomToStatus"
            "-DEXPECT=zoomToLine: ZOOM DRAWING WHERE LAYER=design[\r\n]+scope=drawing where=\"LAYER=design\" matched=1[\r\n]+view=1 kind=plan centre=25,20 [^\r\n]*[\r\n]+zoomToStatus: 1 matched, framed in view 1[.]"
            "-DFORBID=unknown command|does not take"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/zoom_to_dialog_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Zoom To's view chooser (zoomToView): the scope framed in the view
    # chosen - the second plan view, about the line's middle, and then a 3D
    # view, its target at that middle on the datum - each said in the status
    # as a sentence.
    add_test(NAME qt_zoom_to_frames_the_scope_in_the_view_chosen_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>LAYER NEW design|>LAYER SET design|>LINE 10,10 40,30|>LAYER NEW other|>LAYER SET other|>CIRCLE 100,100 5|>VIEWS OPEN plan|>VIEWS OPEN 3d|@viewZoomTo|!zoomToScopeDrawing|zoomToFilterLayer=design|zoomToView=Plan 2 (view 2)|?zoomToLine|!zoomToRun|?zoomToStatus|zoomToView=3D 1 (view 3)|?zoomToLine|!zoomToRun|?zoomToStatus"
            "-DEXPECT=zoomToLine: ZOOM DRAWING WHERE LAYER=design view=2[\r\n]+scope=drawing where=\"LAYER=design\" matched=1[\r\n]+view=2 kind=plan centre=25,20 [^\r\n]*[\r\n]+zoomToStatus: 1 matched, framed in view 2[.][\r\n]+zoomToLine: ZOOM DRAWING WHERE LAYER=design view=3[\r\n]+scope=drawing where=\"LAYER=design\" matched=1[\r\n]+view=3 kind=3d target=25,20,0 [^\r\n]*[\r\n]+zoomToStatus: 1 matched, framed in view 3[.]"
            "-DFORBID=unknown command|does not take|has no choice"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/zoom_to_view_chosen_headless.png"
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
    # Every icon-only button with a menu on a toolbar or in a dock's title
    # bar draws the sign of its menu clear of its icon (--check-toolbars,
    # tools::menuSignClashes), each measured from its own rendering against
    # itself - with its menu and without, with an opaque icon and a clear
    # one. Until 2026-09-30 the tool families were split buttons whose arrow
    # strip the stylesheet drew over the icon, and a view's kind switcher's
    # arrow sat on its icon's frame where no check looked. Seven buttons: the
    # catalogue's four families, all on the Draw toolbar (Circle, Arc,
    # Ellipse; Vertices, which the window's first size hides behind the
    # overflow arrow), Edit's Undo and Redo, and the sample's one view's
    # kind switcher. By hand, from Qt 6.11's qstylesheetstyle.cpp and
    # qfusionstyle.cpp:
    #  - a family at the standard 20 px icons is 33 px (20, Qt's 3, the
    #    theme's 4 px padding and 1 px border each side), its icon's square
    #    centred at 6..25 and its 5 px mark at 26..30, inside the border and
    #    a pixel of air;
    #  - Undo and Redo are split buttons padded on the right by 13 px instead
    #    of 4: 20 + 3 + 4 + 13 + 2 = 42 px wide, the icon's square at 6..25;
    #    the 12 px strip is laid in the border rectangle (defaultOrigin gives
    #    ::menu-button Origin_Border), 30..41, and Fusion's arrow, 8 x 4 px,
    #    is (12 - 8) / 2 = 2 px into it and (33 - 4) / 2 = 14.5 down
    #    (qt_fusion_draw_arrow): 32..39 across, antialiased over rows 14..18;
    #  - the kind switcher is 34 x 22 with a 1 px border; its arrow's box is
    #    a ::menu-indicator's default 13 x 13 (defaultSize), set right and
    #    centred in the padding box 1..32 x 1..20: 20..32 across, 5..17 down.
    #    Fusion's arrow is 2.5 px in and 4.5 down, 22.5..30.5 x 9.5..13.5,
    #    antialiased over 22..30 x 9..13. Padded on the right by the box's
    #    13 px, the contents are 3..19 x 3..18 and the 14 px icon is centred
    #    at 4..17 both ways.
    # The grab is the Draw toolbar, for a person to look at.
    add_test(NAME qt_every_toolbar_menu_sign_is_drawn_clear_of_its_icon_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DCHECK_TOOLBARS=ON"
            "-DDRIVE=%DrawToolBar"
            "-DEXPECT=toolbar button EditToolBar > editUndoButton: sign x 32[.][.]39 y 14[.][.]18 clear of its icon x 6[.][.]25 y 6[.][.]25[\r\n]+toolbar button EditToolBar > editRedoButton: sign x 32[.][.]39 y 14[.][.]18 clear of its icon x 6[.][.]25 y 6[.][.]25[\r\n].*toolbar button DrawToolBar > toolFamilyButton[.]Draw[.]Circle: sign x 26[.][.]30 y 26[.][.]30 clear of its icon x 6[.][.]25 y 6[.][.]25[\r\n].*toolFamilyButton[.]Draw[.]Arc: sign x 26[.][.]30 y 26[.][.]30 clear of its icon x 6[.][.]25 y 6[.][.]25[\r\n].*toolFamilyButton[.]Draw[.]Ellipse: sign x 26[.][.]30 y 26[.][.]30 clear of its icon x 6[.][.]25 y 6[.][.]25[\r\n].*toolFamilyButton[.]Draw[.]Vertices: sign x 26[.][.]30 y 26[.][.]30 clear of its icon x 6[.][.]25 y 6[.][.]25[\r\n].*title bar button View[0-9]+ > ViewKindButton: sign x 22[.][.]30 y 9[.][.]13 clear of its icon x 4[.][.]17 y 4[.][.]17[\r\n].*toolbars and title bars: 7 buttons with a menu, each drawing its sign clear of its icon"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/toolbar_signs_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The same after View > Toolbars > Large Icons and with Delete Vertex -
    # a tool of the Vertices family other than its first - started at the
    # command line; the check measures once the steps have run. The families
    # and Undo and Redo take the bars' new 28 px as the bars' own buttons do
    # (tools::followToolBarIconSize): until 2026-09-30 a family kept the
    # 33 px it was made at, its icon drawn at 23 px beside the others' 28
    # and its mark 3 px against the icon. By hand at 28 px a family is 41 px,
    # its icon's square at 6..33 and its 5 px mark at 34..38; Undo is
    # 28 + 3 + 4 + 13 + 2 = 50 px wide, its icon at 6..33, its strip 38..49
    # and its arrow 2 px in, 40..47, and (41 - 4) / 2 = 18.5 down, rows
    # 18..22. The kind switcher is sized by its title bar, not the toolbars.
    # A family's frame is its own tool's alone (FlyoutButton::paintEvent), so
    # with Delete Vertex running the Vertices family is unframed, its mark
    # lit, and what its menu adds is the mark alone.
    add_test(NAME qt_every_toolbar_menu_sign_is_drawn_clear_of_its_icon_with_large_icons_and_a_tool_running_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DCHECK_TOOLBARS=ON"
            "-DDRIVE=*viewToolBarIconsLarge|>DELETEVERTEX|?draw.vertex.delete|?draw.vertex.insert|%DrawToolBar"
            "-DEXPECT=draw[.]vertex[.]delete: Delete Vertex, checked[\r\n]+draw[.]vertex[.]insert: Insert Vertex, unchecked[\r\n].*toolbar button EditToolBar > editUndoButton: sign x 40[.][.]47 y 18[.][.]22 clear of its icon x 6[.][.]33 y 6[.][.]33[\r\n]+toolbar button EditToolBar > editRedoButton: sign x 40[.][.]47 y 18[.][.]22 clear of its icon x 6[.][.]33 y 6[.][.]33[\r\n].*toolbar button DrawToolBar > toolFamilyButton[.]Draw[.]Circle: sign x 34[.][.]38 y 34[.][.]38 clear of its icon x 6[.][.]33 y 6[.][.]33[\r\n].*toolFamilyButton[.]Draw[.]Arc: sign x 34[.][.]38 y 34[.][.]38 clear of its icon x 6[.][.]33 y 6[.][.]33[\r\n].*toolFamilyButton[.]Draw[.]Ellipse: sign x 34[.][.]38 y 34[.][.]38 clear of its icon x 6[.][.]33 y 6[.][.]33[\r\n].*toolFamilyButton[.]Draw[.]Vertices: sign x 34[.][.]38 y 34[.][.]38 clear of its icon x 6[.][.]33 y 6[.][.]33[\r\n].*title bar button View[0-9]+ > ViewKindButton: sign x 22[.][.]30 y 9[.][.]13 clear of its icon x 4[.][.]17 y 4[.][.]17[\r\n].*toolbars and title bars: 7 buttons with a menu, each drawing its sign clear of its icon"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/toolbar_signs_large_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The check at the owner's display scale, 125% (as
    # qt_render_view_at_125_percent): judged in device pixels, where a
    # family's mark starts the pixel after its icon's square ends. By hand at
    # a ratio of 1.25:
    #  - a family's icon's square at 6..25 spans device 7.5 to 32.5 and is
    #    drawn from pixel 8 for its 25 pixels, 8..32; the mark, 5 px times
    #    1.25 taken down, 6, ends 2 x 1.25 taken down, 2, short of the
    #    button's last device pixel, 40 of its 41.25: 33..38;
    #  - the kind switcher's 14 px icon at 4 spans 5 to 22.5, drawn over
    #    5..22; its arrow's box, 20..32 x 5..17, is device 25..41.25 x
    #    6.25..22.5, and Fusion's arrow, 22.5..30.5 x 9.5..13.5, is
    #    28.125..38.125 x 11.875..16.875 - columns 28..38, and rows 11..17,
    #    the cached arrow set at the box's quarter pixel spreading into one
    #    more row.
    # Until 2026-09-30 the check took each box back to the widget's pixels,
    # rounded outwards; both reached pixel 26, and the run failed with all
    # four families "drawn over" icons nothing was drawn over.
    add_test(NAME qt_every_toolbar_menu_sign_is_drawn_clear_of_its_icon_at_125_percent_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DCHECK_TOOLBARS=ON"
            "-DDRIVE=%DrawToolBar"
            "-DEXPECT=toolbar button DrawToolBar > toolFamilyButton[.]Draw[.]Circle: sign x 33[.][.]38 y 33[.][.]38 clear of its icon x 8[.][.]32 y 8[.][.]32 [(]device pixels at a ratio of 1[.]25[)][\r\n].*toolFamilyButton[.]Draw[.]Arc: sign x 33[.][.]38 y 33[.][.]38 clear of its icon x 8[.][.]32 y 8[.][.]32 [(]device pixels at a ratio of 1[.]25[)][\r\n].*toolFamilyButton[.]Draw[.]Ellipse: sign x 33[.][.]38 y 33[.][.]38 clear of its icon x 8[.][.]32 y 8[.][.]32 [(]device pixels at a ratio of 1[.]25[)][\r\n].*toolFamilyButton[.]Draw[.]Vertices: sign x 33[.][.]38 y 33[.][.]38 clear of its icon x 8[.][.]32 y 8[.][.]32 [(]device pixels at a ratio of 1[.]25[)][\r\n].*title bar button View[0-9]+ > ViewKindButton: sign x 28[.][.]38 y 11[.][.]17 clear of its icon x 5[.][.]22 y 5[.][.]22 [(]device pixels at a ratio of 1[.]25[)][\r\n].*toolbars and title bars: 7 buttons with a menu, each drawing its sign clear of its icon"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/toolbar_signs_125_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_every_toolbar_menu_sign_is_drawn_clear_of_its_icon_at_125_percent_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;QT_SCALE_FACTOR=set:1.25"
        TIMEOUT 180)
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
                         qt_linked_plan_views_zoom_together_headless
                         qt_unlinking_one_of_two_views_unlinks_both_headless
                         qt_a_design_view_and_an_as_built_view_hide_each_others_layers_headless
                         qt_a_selection_shows_as_a_ghost_where_the_view_hides_its_layer_headless
                         qt_a_view_with_its_ghosts_off_shows_no_trace_of_the_selection_headless
                         qt_the_layers_popups_box_turns_a_views_ghosts_off_headless
                         qt_the_view_menu_turns_the_active_views_ghosts_off_headless
                         qt_the_view_menus_zoom_to_selection_is_disabled_for_a_section_headless
                         qt_the_view_menus_zoom_items_and_the_bars_buttons_run_their_lines_headless
                         qt_the_view_menus_link_items_link_the_active_view_and_unlink_all_headless
                         qt_link_this_view_is_disabled_while_a_3d_view_is_active_headless
                         qt_a_press_of_a_hidden_bar_tool_is_refused_headless
                         qt_zoom_to_selection_moves_the_linked_views_headless
                         qt_a_plan_views_zoom_to_selection_moves_nothing_it_does_not_show_headless
                         qt_zoom_to_dialog_runs_its_line_headless
                         qt_zoom_to_frames_the_scope_in_the_view_chosen_headless
                         qt_every_shortcut_and_menu_letter_reaches_one_thing_headless
                         qt_every_menu_item_has_an_icon_and_a_status_tip_headless
                         qt_every_toolbar_menu_sign_is_drawn_clear_of_its_icon_headless
                         qt_every_toolbar_menu_sign_is_drawn_clear_of_its_icon_with_large_icons_and_a_tool_running_headless
                         qt_the_window_section_sizes_the_text_and_toolbars_and_resets_the_layout_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
    # The quitting check counts the built-in's rules, so it says what the
    # built-in is: the fixture of survey codes, by the seam
    # (cad/customisation_host.hpp). It counted the third-party reference
    # customisation's 1,624 while that was the only built-in there could be,
    # and was DISABLED in every checkout without those files - all but one.
    set_property(TEST qt_quitting_with_unapplied_code_manager_edits_is_refused_and_said_headless
        APPEND PROPERTY ENVIRONMENT_MODIFICATION
            "KATANA_BUILTIN_CUSTOMISATION=set:${_twins}/test_survey.customisation.json")

    set_tests_properties(qt_survey_inverse_headless qt_survey_area_headless
                         qt_survey_forward_headless qt_survey_angle_headless
                         qt_survey_traverse_headless qt_survey_level_book_headless
                         qt_survey_converter_headless qt_survey_import_headless
                         qt_the_import_wizards_options_note_names_the_customise_line_headless
                         qt_survey_verb_headless
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
    # The File menu offers both - each in its submenu, File > Import and File
    # > Export, since every import and export went under those two (a menu's
    # report lists its own items, so the two submenus are asked by their
    # names, where `?fileMenu` listed both items until then) - and a typed
    # line with a word the verb does not take is refused in the command
    # line's own words.
    add_test(NAME qt_ifc_menu_items_and_the_verbs_refusals_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_project_ifc"
            "-DDRIVE=%fileMenu|?fileImportMenu|?fileExportMenu|>EXPORT ${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_refused.ifc UTILITIS x.csv|>IMPORT ${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_refused.ifc LOCALLY|>EXPORT \"${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_unclosed.ifc"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/qt_ifc_menu_headless.png"
            "-DEXPECT=fileImportMenu: Import: [^\r\n]*Import IFC\\.\\.\\. \\[Import an IFC file[^\r\n]*[\r\n]+fileExportMenu: Export: [^\r\n]*Export IFC\\.\\.\\. \\[Export IFC 4\\.3.*! InvalidArgument: \"UTILITIS\" is not an option of EXPORT <file\\.ifc>: UTILITIES <schedule\\.csv>, SCHEMA <schema\\.csv>, RULES <rules\\.csv>, SPACING <m>, NODRAWING, NOENTITIES, SELECTED, NOALIGNMENTS, NOSURFACES, PREVIEW.*! InvalidArgument: \"LOCALLY\" is not an option of IMPORT <file\\.ifc>: LOCAL, NOALIGNMENTS.*! InvalidArgument: a quoted path is never closed"
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
    # The menu signs of the toolbars and title bars at the same 125%
    # (docs/desktop.md, "Tool families on the toolbars"), every case of their
    # five suites, a process a suite: the mark, the arrows and the icons
    # measured in device pixels - where a family's mark meets its icon's
    # square without overlapping it, and where rounded outwards to the
    # widget's pixels the two met - the mark the same whole staircase on
    # every button, the overflow arrow whole pixels. At ratio 1, where the
    # suite runs them, a widget pixel is a device pixel: until 2026-09-30
    # six of these cases held only there, comparing widget rectangles with
    # device grabs.
    foreach(_suite IN ITEMS ToolFamilyButtons SplitButtons TitleBarButtons ToolBarSigns
                            ToolBarOverflow)
        add_test(NAME qt_toolbar_signs_at_125_percent.${_suite}
            COMMAND katana_qt_widget_tests "--gtest_filter=${_suite}.*")
        set_tests_properties(qt_toolbar_signs_at_125_percent.${_suite} PROPERTIES
            ENVIRONMENT_MODIFICATION
                "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;QT_SCALE_FACTOR=set:1.25"
            TIMEOUT 120)
    endforeach()
    # The wheel at 125%: the cursor comes in logical pixels and the camera
    # counts in device ones, so the zoom must pick at the cursor times 1.25 -
    # at ratio 1 a missing scale would pass unseen (docs/render.md,
    # "Zooming towards the cursor").
    add_test(NAME qt_render_view_zoom_at_125_percent
        COMMAND katana_qt_widget_tests
            --gtest_filter=RenderView.EveryWheelNotchMagnifiesTheGroundUnderTheCursorByTheSameFactor)
    set_tests_properties(qt_render_view_zoom_at_125_percent PROPERTIES
        ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen;QT_SCALE_FACTOR=set:1.25"
        TIMEOUT 120)
endif()

# The session verbs on the window's command line and the window's one
# executor (docs/desktop.md, "One executor: the command runner"). Typed with
# --command as an agent types them: INFO with an entity's id, CODE CENSUS,
# CODE CHECK, CUSTOMISE alone, COPC and IMPORT ... LOCAL, each of which the
# window once refused or misread. Run with --run-line as a dialog runs its
# line: never a running tool's answer, with what it logged coming back.
if(TARGET katana)
    set(_gis "${PROJECT_SOURCE_DIR}/samples/gis")
    # A 10 x 5 rectangle: perimeter 30, area 50, described by INFO 1 and
    # again by INFO #1 (a typed line's echo is not printed). The fixture of
    # survey codes holds 11 rules over 8 distinct codes (counted by hand from
    # the file), none of which cannot be applied as written; the rectangle
    # carries no code. CUSTOMISE alone is the interpreter's: after the two
    # count lines, the session as records - named by the one file loaded,
    # which is its one source and brought rules alone, with the settings a
    # session starts with (both automation switches on, the seven linework
    # control codes at their defaults).
    add_test(NAME qt_the_session_verbs_run_on_the_windows_command_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>RECT 0,0 10,5|>INFO 1|>INFO #1|>CUSTOMISE REPLACE \"${_twins}/test_survey.customisation.json\"|>CODE CENSUS|>CODE CHECK|>CUSTOMISE"
            "-DEXPECT=1  Polyline  layer=0  vertices=4  closed  length=30  area=50[\r\n]+1  Polyline  layer=0  vertices=4  closed  length=30  area=50.*0 entities carry a code in \"[^\"]*\", 0 distinct codes.*11 rules checked: 0 errors, [0-9]+ warnings?.*11 survey code rules over 8 distinct codes[\r\n]+customisation name=test_survey origin=loaded kept=no definitions=0 codes=8 rules=11 colours=0[\r\n]+source name=test_survey definitions=no rules=yes[\r\n]+automation auto\\.codes=on auto\\.linework=on[\r\n]+linework linework\\.start=ST linework\\.end=END linework\\.close=CL linework\\.arcstart=BC linework\\.arcend=EC linework\\.join=JPN linework\\.rectangle=RECT"
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
    # A script that opens a plan view, links it and zooms it, with no paint
    # between its lines: the zoom starts from what the view draws - a survey
    # line at MGA coordinates, its middle (300050, 6200025) - where it
    # started from the place a new view holds until its first paint, the
    # origin at 10 px a unit, and took the view linked with it there too.
    # VIEWS OPEN's record says the view has not framed yet (framed=no).
    add_test(NAME qt_a_script_that_opens_a_view_and_zooms_it_zooms_about_what_it_draws_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DSCRIPT=${_scripts}/open_and_zoom.kcs"
            "-DDRIVE=?PlanView2|>VIEWS"
            "-DEXPECT=view=2 kind=plan title=\"Plan 2\" active=yes linked=no centre=0,0 scale=10 area=[^ ]+ framed=no ghosts=on[\r\n]+leader=2 linked=1,2 moved=none[\r\n]+view=2 kind=plan centre=300050,6200025 scale=[^\r\n]*[\r\n]+view=1 kind=plan followed=2 centre=300050,6200025 [^\r\n]*[\r\n]+script=\"[^\"]*/open_and_zoom\\.kcs\" lines=4 ran=4 failed=0.*PlanView2: drawn=1 ghosts=0 grips=0.*view=2 kind=plan title=\"Plan 2\" active=yes linked=yes centre=300050,6200025 "
            "-DFORBID=centre=0,0 scale=20|unknown command|stopped at"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/script_open_and_zoom_headless.png"
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
    # A --customise that is refused fails a batch run, as a refused --command
    # does, and before any step: the file here is JSON of another format
    # (the one a kept customisation that does not read is tested with, above),
    # so the one CUSTOMISE line is refused in the reader's words, the switch
    # says it was, and good.kcs's first line - a rectangle - never runs. Two
    # tests of one run, as the pair above is: a PASS expression sets the exit
    # code aside, so the second holds the run to failing.
    add_test(NAME qt_a_refused_customise_switch_stops_a_batch_run_before_its_script_headless
        COMMAND katana --customise "${_not_one}" --script "${_scripts}/good.kcs")
    set_tests_properties(qt_a_refused_customise_switch_stops_a_batch_run_before_its_script_headless
        PROPERTIES
        PASS_REGULAR_EXPRESSION "! ParseFailure: not a Katana customisation file[^\r\n]*[\r\n]+--customise [^\r\n]*qt_not_a_customisation[.]json was refused"
        FAIL_REGULAR_EXPRESSION "rectangle created")
    add_test(NAME qt_a_refused_customise_switch_fails_a_batch_run_headless
        COMMAND katana --customise "${_not_one}" --script "${_scripts}/good.kcs")
    set_tests_properties(qt_a_refused_customise_switch_fails_a_batch_run_headless PROPERTIES
        WILL_FAIL TRUE)
    set_tests_properties(qt_a_refused_customise_switch_stops_a_batch_run_before_its_script_headless
        qt_a_refused_customise_switch_fails_a_batch_run_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
    set_tests_properties(qt_script_switch_runs_every_line_of_a_script_headless
        qt_script_switch_stops_at_the_first_refused_line_headless
        qt_typed_script_continue_runs_every_line_headless
        qt_a_script_may_not_run_itself_headless
        qt_run_script_dialog_runs_the_line_it_shows_headless
        qt_a_dialogs_script_line_gets_back_what_every_line_said_headless
        qt_several_lines_on_the_command_line_run_as_a_script_headless
        qt_lines_pasted_at_a_text_prompt_are_the_texts_lines_headless
        qt_a_scripts_bare_tool_word_runs_as_katana_cli_runs_it_headless
        qt_a_script_that_opens_a_view_and_zooms_it_zooms_about_what_it_draws_headless
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
# Copy as JSON runs STATUS JSON through the one executor. The fixture of
# survey codes is loaded with the linestyles, so the survey code count is its
# 11 rules (counted by hand from the file): a REPLACE of definitions alone
# keeps the survey codes. Last, the summary's customisation pane, which is
# what a bare CUSTOMISE replies, of the Document's own state (a report prints
# a text box on one line, its line breaks as blanks): the first REPLACE
# brought both kinds into a session with no name, so the session is named
# test_linestyles; the second replaced the definitions, so test_linestyles,
# which brought only those, is no source any more - test_survey (rules) and
# test_symbols (definitions) are, in load order.
if(TARGET katana)
    add_test(NAME qt_drawing_summary_and_status_json_describe_the_drawing_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE REPLACE \"${_twins}/test_linestyles.customisation.json\" \"${_twins}/test_survey.customisation.json\"|>STYLE NEW kerb|>STYLE NEW other|>STYLE SET kerb linetype \"TEST Dashed Kerb\"|>CUSTOMISE REPLACE \"${_twins}/test_symbols.customisation.json\"|>RECT 0,0 10,5|>SELECT ALL|>STATUS|>STATUS JSON|?statusSelectionCount|@fileDrawingSummary|?drawingSummaryDrawing|?drawingSummaryCurrent|?drawingSummarySelection|?drawingSummaryHistory|?drawingSummaryUnresolved|drawingSummaryUnresolved=TEST Dashed Kerb|!drawingSummaryShowMissing|?drawingSummaryStatus|!drawingSummaryCopyJson|?drawingSummaryStatus|@formatStyles|?styleTable|?drawingSummaryCustomisation"
            "-DEXPECT=Entities: 1  Layers: 1  Alignments: 0[\r\n]+Current layer: 0[\r\n]+Selected: 1  Undo steps: 4  Redo steps: 0[\r\n]+Customisation: 4 linestyles and symbols, 11 survey code rules[\r\n]+{[\r\n]+  \"alignments\": 0,[\r\n]+  \"currentLayer\": \"0\",[\r\n]+  \"currentStyle\": \"\",[\r\n]+  \"entities\": 1,.*  \"undoSteps\": 4[\r\n]+}[\r\n]+statusSelectionCount: 1 selected / 1 entities[\r\n]+[^\r\n]*opened drawingSummaryDialog \"Drawing Summary\"[\r\n]+drawingSummaryDrawing: 1 entity, 1 layer, 0 alignments, 0 sheets[\r\n]+drawingSummaryCurrent: layer 0, style ByLayer, annotation scale 1:[0-9]+, no coordinate system[\r\n]+drawingSummarySelection: 1 entity selected[\r\n]+drawingSummaryHistory: 4 steps to undo \\(next: [^)]+\\), 0 steps to redo[\r\n]+drawingSummaryUnresolved: TEST Dashed Kerb[\r\n]+drawingSummaryStatus: Styles and Linetypes shows the styles that name TEST Dashed Kerb\\.[\r\n]+{.*}[\r\n]+drawingSummaryStatus: Copied the drawing's state as JSON \\(STATUS JSON\\)\\.[\r\n]+[^\r\n]*opened styleManagerDialog[^\r\n]*[\r\n]+styleTable: kerb \\| [^;\r\n]*TEST Dashed Kerb[^;\r\n]*[\r\n].*drawingSummaryCustomisation: [^\r\n]*11 survey code rules over 8 distinct codes customisation name=test_linestyles origin=loaded kept=no definitions=4 codes=8 rules=11 colours=0 source name=test_survey definitions=no rules=yes source name=test_symbols definitions=yes rules=no automation auto\\.codes=on auto\\.linework=on "
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
# empty drawing's Export says so in the log, not in a box. (Load Customisation
# and Replace Loaded Customisation were triggered here too, and named their
# CUSTOMISE lines: the two items are gone, which the pair of tests after this
# one holds the window to.)
if(TARGET katana)
    add_test(NAME qt_the_file_items_name_their_verbs_in_a_headless_run_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=*fileOpen|*fileSaveAs|*fileImport|*fileExportVector|>RECT 0,0 1,1|*fileExportVector|*fileSave"
            "-DEXPECT=type OPEN <directory> instead\\.[\r\n]+[^\r\n]*type SAVE <directory> instead\\.[\r\n]+[^\r\n]*type IMPORT <file> \\[LOCAL \\| ALONGSIDE \\| OFFSET=dE,dN\\] instead\\.[\r\n]+! Export: the drawing is empty\\.[\r\n]+rectangle created[\r\n]+[^\r\n]*type EXPORT <file> instead\\.[\r\n]+[^\r\n]*type SAVE <directory> instead\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/file_items_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_file_items_name_their_verbs_in_a_headless_run_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
    # The GIS menu's three imports and Dataset Information ask for a file
    # first too, and were not guarded: reached by name in a headless run each
    # opened a file dialog nobody could close, and the run waited on it until
    # its timeout. Each is refused as File's items above are, naming the line
    # and the words of it that are its own kind of data's (docs/interop.md,
    # "Import options": a vector file's layers, filter and target layer; a
    # raster's band, size and name; a cloud's budget, class and resolution),
    # and INFO for Dataset Information.
    add_test(NAME qt_the_gis_items_that_ask_for_a_file_name_their_verbs_in_a_headless_run_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=*importVectorData|*importRaster|*importPointCloud|*datasetInformation"
            "-DEXPECT=type IMPORT <file> \\[layers=a,b\\] \\[where=\"<filter>\"\\] \\[target=<layer>\\] instead\\.[\r\n]+[^\r\n]*type IMPORT <file> \\[band=N\\] \\[maxpixels=N\\] \\[name=<n>\\] instead\\.[\r\n]+[^\r\n]*type IMPORT <file> \\[budget=N\\] \\[class=N\\] \\[resolution=<m>\\] instead\\.[\r\n]+[^\r\n]*type INFO <file> instead\\."
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/gis_file_items_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The menus as the window says them (docs/desktop.md, "The File menu"):
    # File's fifteen items, with every import under File > Import and every
    # export under File > Export - File's own, then Survey's, then GIS's -
    # Settings above Quit, and Survey > Survey Coding ending with the code
    # manager, Apply Survey Codes and Process Linework before the utilities.
    # Each list is the WHOLE of its menu from its title to the end of the
    # line, every item with a status tip (${_tip}: a bracket holding
    # something), in the tree's order; a menu's report lists its own items,
    # so each submenu is asked by its name. The other nine menus are listed
    # so that FORBID reads every top-level item of the window: none names
    # another program. The log (?commandLog) is there for the error an item
    # missing from a submenu's list would leave in it (fillMenuByName).
    set(_tip "\\[[^]]+\\]")
    add_test(NAME qt_file_keeps_every_import_and_export_in_two_submenus_and_settings_above_quit_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=?commandLog|?fileMenu|?fileImportMenu|?fileExportMenu|?surveyMenu|?editMenu|?viewMenu|?drawMenu|?modifyMenu|?annotateMenu|?formatMenu|?terrainMenu|?gisMenu|?helpMenu"
            "-DEXPECT=fileMenu: File: New ${_tip} ; Open Project\\.\\.\\. ${_tip} ; Save ${_tip} ; Save As\\.\\.\\. ${_tip} ; Import ${_tip} ; Export ${_tip} ; Run Script\\.\\.\\. ${_tip} ; Recent Scripts ${_tip} ; Plot to PDF\\.\\.\\. ${_tip} ; Sheets\\.\\.\\. ${_tip} ; Plot Sheets to PDF\\.\\.\\. ${_tip} ; Project Coordinate System\\.\\.\\. ${_tip} ; Drawing Summary\\.\\.\\. ${_tip} ; Settings\\.\\.\\. ${_tip} ; Quit ${_tip}[\r\n]+fileImportMenu: Import: Import Any File\\.\\.\\. ${_tip} ; Import IFC\\.\\.\\. ${_tip} ; Import Survey Points\\.\\.\\. ${_tip} ; Import Vector Data\\.\\.\\. ${_tip} ; Import Raster\\.\\.\\. ${_tip} ; Import Point Cloud\\.\\.\\. ${_tip} ; Online Data\\.\\.\\. ${_tip}[\r\n]+fileExportMenu: Export: Export Drawing\\.\\.\\. ${_tip} ; Export IFC\\.\\.\\. ${_tip} ; Export View as Image\\.\\.\\. ${_tip} ; Export Survey Points\\.\\.\\. ${_tip} ; Export Surface as DEM\\.\\.\\. ${_tip} ; Export Point Cloud\\.\\.\\. ${_tip}[\r\n]+surveyMenu: Survey: [^\r\n]*Coordinate Converter\\.\\.\\. ${_tip} ; Survey Code Manager\\.\\.\\. ${_tip} ; Apply Survey Codes ${_tip} ; Process Linework ${_tip} ; Draw Utility Schedule\\.\\.\\. [^\r\n]*[\r\n].*helpMenu: Help: "
            "-DFORBID=12[dD]|no menu item is named"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/file_menus_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_the_gis_items_that_ask_for_a_file_name_their_verbs_in_a_headless_run_headless
                         qt_file_keeps_every_import_and_export_in_two_submenus_and_settings_above_quit_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
    # A customisation file is loaded by the typed line - which File >
    # Settings' Import runs - and the two items that opened a file dialog
    # for one are gone: the line loads the symbol fixture (4 definitions, by
    # hand), and a trigger of either removed item is refused as the name of
    # no menu item - the run stops there, exit 1 (REFUSED).
    set(_loaded_first "customisation name=test_symbols origin=loaded kept=no definitions=4 codes=0 rules=0 colours=0")
    add_test(NAME qt_load_customisation_is_no_menu_item_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE \"${_twins}/test_symbols.customisation.json\"|*loadCustomisation"
            "-DREFUSED=${_loaded_first}.*--trigger loadCustomisation failed: [^\r\n]*no such menu item"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/load_customisation_no_item_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_replace_customisation_is_no_menu_item_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>CUSTOMISE \"${_twins}/test_symbols.customisation.json\"|*replaceCustomisation"
            "-DREFUSED=${_loaded_first}.*--trigger replaceCustomisation failed: [^\r\n]*no such menu item"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/replace_customisation_no_item_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_load_customisation_is_no_menu_item_headless
                         qt_replace_customisation_is_no_menu_item_headless
        PROPERTIES ENVIRONMENT_MODIFICATION
            "PATH=path_list_prepend:${KATANA_RUNTIME_BIN};QT_QPA_PLATFORM=set:offscreen"
        TIMEOUT 180)
endif()

# Edit > Select by ID and the Edit and View actions' object names
# (docs/desktop.md, "The plan view's shortcut menu"). Three points; Select All
# by its action; the dialog, non-modal, selects #2 and 3 through the executor
# and says what SELECT answered, and frames them by the plan view's Zoom to
# Selection line through the same executor - the two points' box is
# (20,20)-(40,40), so the view is centred on (30,30); Grid is reached by its
# name and turned off; Erase by its name erases the two, and Undo by its name
# brings them back.
if(TARGET katana)
    add_test(NAME qt_select_by_id_selects_and_frames_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=>POINT 1,1|>POINT 20,20|>POINT 40,40|*editSelectAll|@editSelectById|selectByIdIds=#2, 3|!selectByIdSelect|?selectByIdStatus|*viewGrid|?viewGrid|*editErase|>LIST|*editUndo|>LIST"
            "-DEXPECT=3 selected.*--dialog editSelectById opened selectByIdDialog \"Select by ID\"[\r\n]+2 selected[\r\n]+scope=selection matched=2[\r\n]+view=1 kind=plan centre=30,30 [^\r\n]*[\r\n]+selectByIdStatus: 2 selected.*viewGrid: Grid, unchecked[\r\n]+2 erased[\r\n]+1 entities[\r\n]+1 +Point +layer=0 +at 1,1.*3 entities"
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

# The vertex tools in the real window, driven with the pointer a headless run
# has no mouse for - "~X,Y" hovers and "+X,Y" clicks at a model point
# (docs/headless.md) - and read back from the record each pointer step
# prints: what Insert Vertex shows before the click, that the click puts the
# vertex ON the line, and that a vertex clicked before the tool is the one it
# works beside (docs/drawing.md, "What a vertex tool acts on"). Sample
# polyline 2 is the square (40,35) (110,35) (110,85) (40,85), whose right
# edge x = 110 has nothing else within reach. A pointer's model point comes
# back from its pixel within a few units in the last place, so a coordinate
# the view made from it is matched to ten decimals; a caption's middle dot is
# matched as any characters, as a platform may pass it on in another encoding.
if(TARGET katana)
    add_test(NAME qt_pointer_steps_hover_and_click_the_plan_view_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>PLINE|+100,110|+110,110|~110,115|>|<LIST"
            "-DEXPECT=pointer: action=hover x=110 y=115 tool=draw.polyline expects=point.*vertices=2 +open +length=10"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/pointer_steps_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_insert_vertex_shows_where_the_vertex_goes_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 2|>INSERTVERTEX|~110.3,50"
            "-DEXPECT=pointer: action=hover x=110.3 y=50 tool=draw.vertex.insert [^\r\n]*target=1 added=1 removed=0 enter=0 focus=4 refused=no caption=\"new vertex between 1 and 2 [^0-9\"]+ 15[.]000 from 1\" prompt=\"Insert Vertex: Click on polyline 2 where the new vertex goes\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/insert_vertex_hover_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_insert_vertex_puts_it_on_the_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 2|>INSERTVERTEX|~110.3,50|+110.3,50|<VERTEX LIST 2"
            "-DEXPECT=vertex 2 added to polyline 2 on segment 1 [(]5 vertices[)].*index=2 x=110 y=(50|49[.]9999999999[0-9]*|50[.]0000000000[0-9]*) z=none bulge=0 bearing=0 distance=(35|34[.]9999999999[0-9]*|35[.]0000000000[0-9]*)[\r\n]"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/insert_vertex_click_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    add_test(NAME qt_insert_vertex_beside_the_vertex_clicked_first_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 2|+110,85|>INSERTVERTEX|~111,70|>|<VERTEX LIST 2"
            "-DEXPECT=pointer: action=hover x=111 y=70 tool=draw.vertex.insert [^\r\n]*target=2 added=1 removed=0 enter=1 [^\r\n]*caption=\"new vertex between 1 and 2 [^0-9\"]+ 35[.]000 from 1\" prompt=\"Insert Vertex: Press Enter for a vertex at the marked middle of segment 2, or click beside vertex 2 of polyline 2\".*vertices=5.*index=3 x=75 y=85 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/insert_vertex_beside_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The same chosen vertex 2 (110,85), the pointer beyond its corner at
    # (140,110): both of its segments are nearest at the vertex itself, and
    # Insert said "too close to vertex 2; zoom in", in red, at any distance -
    # the first thing it showed. A click there takes nothing, so the preview
    # is Enter's - its place the middle of segment 2, (75,85), 35 from vertex
    # 2 - the click is refused, and Enter then adds there.
    add_test(NAME qt_insert_vertex_beyond_a_chosen_corner_shows_enter_not_a_refusal_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 2|+110,85,shift|>INSERTVERTEX|~140,110|+140,110|>|<VERTEX LIST 2"
            "-DEXPECT=pointer: action=hover x=140 y=110 tool=draw.vertex.insert [^\r\n]*enter=1 [^\r\n]*refused=no caption=\"Enter: new vertex between 2 and 3 [^0-9\"]+ 35[.]000 from 2\".*vertices=5 .*index=3 x=75 y=85 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/insert_vertex_beyond_the_corner_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Vertex 2 chosen for Insert, then vertex 1 moved by a verb line (as the
    # Vertices panel or an agent would): the preview is the refusal, not the
    # old edge, and the click is refused with it; the square keeps 4.
    add_test(NAME qt_insert_vertex_refuses_a_choice_edited_since_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 2|+110,85|>INSERTVERTEX|<VERTEX MOVE 2 1 125,30|~111,70|+111,70|<VERTEX LIST 2"
            "-DEXPECT=pointer: action=hover x=111 y=70 tool=draw.vertex.insert [^\r\n]*target=0 added=0 [^\r\n]*refused=yes caption=\"polyline 2 changed since it was picked; pick it again\".*! polyline 2 changed since it was picked; pick it again.*vertices=4 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/insert_vertex_stale_choice_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # A vertex made hot with Shift+click answers Delete Vertex: the prompt
    # says Enter deletes it, and Enter does, leaving the square's three others.
    add_test(NAME qt_delete_vertex_takes_the_chosen_vertex_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 2|+110,85,shift|>DELETEVERTEX|~110,85|>|<VERTEX LIST 2"
            "-DEXPECT=pointer: action=hover x=110 y=85 tool=draw.vertex.delete [^\r\n]*removed=[1-9][^\r\n]*prompt=\"Delete Vertex: Press Enter to delete vertex 2 of polyline 2, or click another vertex\".*vertices=3 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/delete_vertex_chosen_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # The same chosen vertex 2 (110,85), with the pointer over vertex 0
    # (40,35) instead: a click there picks anew and deletes vertex 0, so the
    # preview is that deletion, not Enter's - it marked vertex 2 while the
    # click deleted vertex 0 - with vertex 2, which Enter would delete, still
    # marked as Enter's (enter=1). The click leaves (110,35) (110,85) (40,85).
    add_test(NAME qt_delete_vertex_previews_the_click_not_the_chosen_vertex_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 2|+110,85,shift|>DELETEVERTEX|~40.2,35.2|+40.2,35.2|<VERTEX LIST 2"
            "-DEXPECT=pointer: action=hover x=40[.]2 y=35[.]2 tool=draw.vertex.delete [^\r\n]*removed=3 enter=1 [^\r\n]*caption=\"delete vertex 0 [^\"]*\".*vertex 0 deleted.*vertices=3 .*index=0 x=110 y=35 .*index=1 x=110 y=85 .*index=2 x=40 y=85 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/delete_vertex_previews_the_click_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # After the click that adds a vertex the pointer is on it, where the
    # restarted tool would refuse another: the view answers the click with
    # what it did, not in red, until the pointer moves.
    add_test(NAME qt_insert_vertex_is_answered_with_what_it_did_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 2|>INSERTVERTEX|~110.3,50|+110.3,50"
            "-DEXPECT=pointer: action=click x=110[.]3 y=50 tool=draw.vertex.insert [^\r\n]*refused=no caption=\"vertex 2 added to polyline 2 on segment 1 [(]5 vertices[)]\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/insert_vertex_answered_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # With the snaps on - their default - a line's end 4.5 above the
    # square's top edge sits in the snap aperture of a pointer on the edge:
    # it took the cursor off the line, and Insert showed nothing. Now only a
    # snap ON the line is taken, and the preview is there.
    add_test(NAME qt_insert_vertex_with_snaps_on_stays_on_the_line_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP ON|>ZOOM E|>LINE 60,89.5 60,100|>SELECT 2|>INSERTVERTEX|~60,85.2|+60,85.2|<VERTEX LIST 2"
            "-DEXPECT=pointer: action=hover x=60 y=85[.]2 tool=draw.vertex.insert [^\r\n]*target=1 added=1 [^\r\n]*refused=no .*vertices=5 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/insert_vertex_snaps_on_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Straighten on the closed boundary, polyline 1 (0,0) (160,0) (175,90)
    # (80,130) (0,100), from vertex 1 to vertex 3: forward that takes out
    # vertex 2 alone, the other way 4 and 0, so the shorter side goes - its X
    # and its two segments, the three removed marks.
    add_test(NAME qt_straighten_shows_what_goes_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 1|>STRAIGHTEN|+160,0|~80.5,129.5"
            "-DEXPECT=pointer: action=hover x=80[.]5 y=129[.]5 tool=draw.vertex.straighten [^\r\n]*target=2 added=0 removed=3 [^\r\n]*caption=\"straighten 1 to 3 [^0-9\"]+ removes 1 vertex\" prompt=\"Straighten: Click the other vertex to keep [[]Other side[]]\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/straighten_shows_what_goes_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Segment to Arc on the square's right edge, (110,35)-(110,85), through
    # (125,62): the centre (cx,60) with (cx-110)^2 + 625 = (cx-125)^2 + 4, so
    # cx = 96.8 and the radius sqrt(799.24) = 28.271.
    add_test(NAME qt_segment_to_arc_shows_the_arc_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 2|>SEGMENTARC|+110.2,60|~125,62"
            "-DEXPECT=pointer: action=hover x=125 y=62 tool=draw.vertex.arc expects=point shapes=1 [^\r\n]*target=1 [^\r\n]*caption=\"segment 1 [^0-9\"]+ radius 28[.]271\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/segment_to_arc_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Fillet Vertex refuses an end of the open polyline 3 at the pick, in the
    # preview and at the click, and asks for no radius: still the pick.
    add_test(NAME qt_fillet_refuses_an_end_vertex_at_the_pick_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 3|>FILLETVERTEX|~60.2,35|+60.2,35"
            "-DEXPECT=pointer: action=hover x=60[.]2 y=35 tool=draw.vertex.fillet [^\r\n]*refused=yes caption=\"vertex 0 is an end of the polyline; a corner is where two segments meet\" prompt=\"Fillet Vertex: Click the corner vertex of polyline 3 to round\".*pointer: action=click x=60[.]2 y=35 tool=draw.vertex.fillet expects=entity [^\r\n]*prompt=\"Fillet Vertex: Click the corner vertex of polyline 3 to round\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/fillet_refuses_an_end_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Move Vertex's caption ends in a bearing, 0°00'00" for due north, and
    # the pointer record quotes it as every reply's text is quoted: the
    # seconds mark written \", where by hand it closed the value early. The
    # regex's "." is that backslash.
    add_test(NAME qt_pointer_record_quotes_a_bearing_as_a_reply_does_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SNAP OFF|>ZOOM E|>SELECT 2|>MOVEVERTEX|+110,85|~110,95"
            "-DEXPECT=pointer: action=hover x=110 y=95 tool=draw.vertex.move [^\r\n]*caption=\"vertex 2 [^0-9\"]+ moves 10[.]000 at 0[^0-9]+00'00.\"\" prompt=\"Move Vertex: Specify its new position\""
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/pointer_record_bearing_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # Polyline 2 selected, then its layer hidden: Close or Open opened it
    # where nobody could see it. Now Enter says why, and it stays closed.
    add_test(NAME qt_close_or_open_leaves_a_selected_polyline_on_a_hidden_layer_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SELECT 2|<LAYER HIDE BUILDING|>CLOSEOPEN|>|<VERTEX LIST 2"
            "-DEXPECT=! the selected polylines are on hidden or locked layers; select others, then press Enter.*id=2 kind=polyline closed=yes vertices=4 "
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/close_or_open_hidden_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    # MOVE with a relative point typed with a dz: a line has no height, and
    # the dz is dropped as the z of x,y,z is - it was refused, and the road
    # line 4, (5,15)-(120,15), stayed where it was.
    add_test(NAME qt_move_takes_a_relative_point_with_a_dz_in_plan_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${PROJECT_SOURCE_DIR}/samples/site_plan"
            "-DDRIVE=>SELECT 4|>MOVE|>0,0|>@0,5,0|<INFO 4"
            "-DEXPECT=1 entity moved.*Line +layer=ROAD +5,20 -> 120,20"
            "-DOUTPUT=${CMAKE_CURRENT_BINARY_DIR}/move_relative_dz_headless.png"
            -P "${PROJECT_SOURCE_DIR}/tools/check_screenshot.cmake")
    set_tests_properties(qt_pointer_steps_hover_and_click_the_plan_view_headless
        qt_insert_vertex_shows_where_the_vertex_goes_headless
        qt_insert_vertex_puts_it_on_the_line_headless
        qt_insert_vertex_beside_the_vertex_clicked_first_headless
        qt_insert_vertex_beyond_a_chosen_corner_shows_enter_not_a_refusal_headless
        qt_insert_vertex_refuses_a_choice_edited_since_headless
        qt_delete_vertex_takes_the_chosen_vertex_headless
        qt_delete_vertex_previews_the_click_not_the_chosen_vertex_headless
        qt_insert_vertex_is_answered_with_what_it_did_headless
        qt_insert_vertex_with_snaps_on_stays_on_the_line_headless
        qt_straighten_shows_what_goes_headless
        qt_segment_to_arc_shows_the_arc_headless
        qt_fillet_refuses_an_end_vertex_at_the_pick_headless
        qt_pointer_record_quotes_a_bearing_as_a_reply_does_headless
        qt_close_or_open_leaves_a_selected_polyline_on_a_hidden_layer_headless
        qt_move_takes_a_relative_point_with_a_dz_in_plan_headless
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
# Set runs CRS SET through the window's command runner (since 2026-09-29), so
# its reply is logged as a typed line's is: the first "crs id" line is Set's,
# the second the >CRS step's.
if(TARGET katana)
    add_test(NAME qt_the_project_crs_dialog_is_driven_and_sets_one_step_headless
        COMMAND ${CMAKE_COMMAND}
            "-DAPP=$<TARGET_FILE:katana>"
            "-DPROJECT=${CMAKE_CURRENT_BINARY_DIR}/no_such_project"
            "-DDRIVE=@fileProjectCrs|?projectCrsCurrent|projectCrsPlace=115.86,-31.95|!projectCrsSuggest|?projectCrsList|projectCrsText=EPSG:7850|?projectCrsCheck|!projectCrsApply|>CRS|>UNDO|>CRS"
            "-DEXPECT=--dialog fileProjectCrs opened projectCrsDialog \"Project Coordinate System\"[\r\n]+projectCrsCurrent: Now: no coordinate system[\r\n]+projectCrsList: Suggested for this place [^\r\n]*[\r\n]+projectCrsCheck: EPSG:7850 GDA2020 / MGA zone 50 - projected, metre[^\r\n]*[\r\n]+crs id=EPSG:7850 name=\"GDA2020 / MGA zone 50\"[^\r\n]*[\r\n]+crs id=EPSG:7850 name=\"GDA2020 / MGA zone 50\"[^\r\n]*[\r\n]+1 undone[\r\n]+crs id=none"
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
