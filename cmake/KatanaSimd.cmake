# SIMD kernel files: compiled for AVX2 or NEON, linked into an ordinary
# baseline target.
#
#   katana_add_simd_sources(<target> AVX2 <dir/name_avx2.cpp>... NEON <dir/name_neon.cpp>...)
#
# The shipped program is built for baseline x86-64 and must start on any 64-bit
# PC, so AVX2 code may only ever run after core::activeSimdLevel() said so. That
# rules out the obvious ways of getting it:
#
#   - -march=x86-64-v3 on the whole build ships a program that faults on older
#     machines (and was measured to buy nothing on its own: docs/performance.md).
#   - target_clones needs ifunc, which MinGW does not have: it does not compile.
#   - -mavx2 on an ORDINARY source file is the inline-copy hazard. Every inline
#     function it uses from a shared header (a std::vector member, a
#     katana::math operator, std::span::data) is emitted into its object as an
#     AVX2 copy, the linker keeps one copy of each such function for the whole
#     program, and it may keep this one - so baseline code calls AVX2
#     instructions. It was demonstrated with this toolchain.
#
# So kernels live in files of their own, named *_avx2.cpp, with raw pointers at
# the boundary, internal linkage inside, C-linkage katana_avx2_ entry points,
# and no includes but intrinsics, <cstddef>, <cstdint> and their own
# *_kernels.hpp declarations. Each target's kernel files are compiled into an
# OBJECT library with -mavx2 -mfma added to the project's usual flags - which
# keep -ffp-contract=off, so the compiler fuses no multiply-add and a kernel can
# match its scalar reference bit for bit - and the objects are linked into the
# target.
#
# NEON (64-bit ARM: Apple silicon, Linux aarch64, Windows ARM64) has no such
# hazard - Advanced SIMD is in every AArch64 processor, so the kernels need no
# -m flag and an inline copy compiled beside them is ordinary baseline code.
# The *_neon.cpp files keep the same discipline all the same (their own OBJECT
# library, the same include allowlist with <arm_neon.h>, C-linkage katana_neon_
# entries), for two reasons: one set of rules for every kernel is easier to
# hold than two, and the object check's third rule - no fused multiply-add -
# matters MORE on AArch64, where fmla/fmadd are ordinary baseline instructions
# the compiler would use freely if -ffp-contract=off were ever lost.
#
# Two ctest checks hold the rules (tools/check_simd_kernels.cmake):
#   simd_kernel_sources  every *_avx2.cpp and *_neon.cpp includes only the
#                        allowlist, and no build file turns on OpenMP
#                        threading;
#   simd_kernel_objects  every kernel OBJECT defines no code visible to the
#                        linker but katana_avx2_ (or katana_neon_) entries, no
#                        static initialiser, and no fused multiply-add
#                        instruction.
# The second is the one that matters: it inspects what the compiler actually
# produced, in whatever configuration was built.

option(KATANA_SIMD_KERNELS "Compile the SIMD kernels (AVX2 on x86-64, NEON on 64-bit ARM); OFF leaves only the scalar references" ON)

# Which kernel set this build compiles: "avx2", "neon" or "" (none). The
# processor names are what CMake reports for the target: AMD64 on Windows,
# x86_64 on Linux and macOS; arm64 on macOS (Apple silicon), ARM64 on Windows,
# aarch64 on Linux. A cross build sets CMAKE_SYSTEM_PROCESSOR in its toolchain
# file, so this is the TARGET's processor, never the build machine's.
set(KATANA_SIMD_KERNEL_SET "")
if(KATANA_SIMD_KERNELS)
    if(CMAKE_SYSTEM_PROCESSOR MATCHES "^(AMD64|amd64|x86_64|X86_64)$")
        set(KATANA_SIMD_KERNEL_SET "avx2")
    elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "^(arm64|ARM64|aarch64|AARCH64)$")
        set(KATANA_SIMD_KERNEL_SET "neon")
    endif()
endif()
set(KATANA_SIMD_KERNELS_ACTIVE OFF)
if(KATANA_SIMD_KERNEL_SET)
    set(KATANA_SIMD_KERNELS_ACTIVE ON)
endif()

# The options a kernel OBJECT library gets on top of katana_target_defaults.
#
# -fno-sanitize=address: AddressSanitizer registers each object's globals from
# a module constructor (.ctors.65436 on MinGW, .init_array.00099 on Linux) - a
# static initialiser in the kernel object, which simd_kernel_objects refuses
# because it runs at start-up on every machine. So in the ASan build (the
# linux-sanitize preset) ASan leaves the kernels alone, as it would a
# hand-written assembly routine; UBSan adds no constructor and stays. The
# price: a kernel reading past the end of its input would not be reported by
# ASan. The kernels guard against that by structure - whole blocks only while
# a whole block remains, then a scalar tail - and the tests run lengths across
# every block edge at every level. It is unconditional because it is a no-op
# without -fsanitize=address, and so that simd_kernel_objects_under_asan
# (tests/CMakeLists.txt) checks these very options in every build.
#
# NEON needs no -m flag: Advanced SIMD is the AArch64 baseline, and every
# AArch64 compiler enables it by default.
function(katana_simd_kernel_options objects)
    if(KATANA_SIMD_KERNEL_SET STREQUAL "neon")
        if(NOT MSVC)
            target_compile_options(${objects} PRIVATE -fno-sanitize=address)
        endif()
    elseif(MSVC)
        target_compile_options(${objects} PRIVATE /arch:AVX2)
    else()
        target_compile_options(${objects} PRIVATE -mavx2 -mfma -fno-sanitize=address)
    endif()
endfunction()

function(katana_add_simd_sources target)
    cmake_parse_arguments(ARG "" "" "AVX2;NEON" ${ARGN})
    if(NOT ARG_AVX2)
        message(FATAL_ERROR "katana_add_simd_sources(${target}): no AVX2 files given")
    endif()
    # Every kernel family has both: a family with AVX2 alone would leave the
    # ARM builds on its scalar reference without anyone deciding so.
    if(NOT ARG_NEON)
        message(FATAL_ERROR "katana_add_simd_sources(${target}): no NEON files given")
    endif()
    foreach(_set AVX2 NEON)
        string(TOLOWER "${_set}" _suffix)
        foreach(_file IN LISTS ARG_${_set})
            # The name is what the source check finds kernel files by; a
            # kernel compiled under any other name would escape it.
            if(NOT _file MATCHES "_${_suffix}\\.cpp$")
                message(FATAL_ERROR
                    "katana_add_simd_sources(${target}): '${_file}' must be named *_${_suffix}.cpp, "
                    "so that tools/check_simd_kernels.cmake holds it to the kernel-file rules")
            endif()
        endforeach()
    endforeach()
    if(NOT KATANA_SIMD_KERNELS_ACTIVE)
        return()
    endif()

    string(TOUPPER "${KATANA_SIMD_KERNEL_SET}" _set)
    set(_files ${ARG_${_set}})
    set(_objects ${target}_${KATANA_SIMD_KERNEL_SET}_kernels)
    add_library(${_objects} OBJECT ${_files})
    katana_target_defaults(${_objects})
    katana_simd_kernel_options(${_objects})
    target_sources(${target} PRIVATE $<TARGET_OBJECTS:${_objects}>)
    # Tells the target's dispatch code that the entries exist to be called:
    # KATANA_HAVE_AVX2_KERNELS or KATANA_HAVE_NEON_KERNELS.
    target_compile_definitions(${target} PRIVATE KATANA_HAVE_${_set}_KERNELS=1)
    set_property(GLOBAL APPEND PROPERTY KATANA_SIMD_OBJECT_LIBRARIES ${_objects})
    foreach(_file IN LISTS _files)
        get_filename_component(_path "${_file}" ABSOLUTE)
        set_property(GLOBAL APPEND PROPERTY KATANA_SIMD_KERNEL_SOURCES "${_path}")
    endforeach()
endfunction()
