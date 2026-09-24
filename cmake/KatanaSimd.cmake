# SIMD kernel files: compiled for AVX2, linked into an ordinary baseline target.
#
#   katana_add_simd_sources(<target> AVX2 <dir/name_avx2.cpp>...)
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
# Two ctest checks hold the rules (tools/check_simd_kernels.cmake):
#   simd_kernel_sources  every *_avx2.cpp includes only the allowlist, and no
#                        build file turns on OpenMP threading;
#   simd_kernel_objects  every kernel OBJECT defines no code visible to the
#                        linker but katana_avx2_ entries, no static
#                        initialiser, and no fused multiply-add instruction.
# The second is the one that matters: it inspects what the compiler actually
# produced, in whatever configuration was built.

option(KATANA_SIMD_KERNELS "Compile the AVX2 kernels (x86-64 only); OFF leaves only the scalar references" ON)

set(KATANA_SIMD_KERNELS_ACTIVE OFF)
if(KATANA_SIMD_KERNELS AND CMAKE_SYSTEM_PROCESSOR MATCHES "^(AMD64|amd64|x86_64|X86_64)$")
    set(KATANA_SIMD_KERNELS_ACTIVE ON)
endif()

function(katana_add_simd_sources target)
    cmake_parse_arguments(ARG "" "" "AVX2" ${ARGN})
    if(NOT ARG_AVX2)
        message(FATAL_ERROR "katana_add_simd_sources(${target}): no AVX2 files given")
    endif()
    foreach(_file IN LISTS ARG_AVX2)
        # The name is what the source check finds kernel files by; a kernel
        # compiled under any other name would escape it.
        if(NOT _file MATCHES "_avx2\\.cpp$")
            message(FATAL_ERROR
                "katana_add_simd_sources(${target}): '${_file}' must be named *_avx2.cpp, "
                "so that tools/check_simd_kernels.cmake holds it to the kernel-file rules")
        endif()
    endforeach()
    if(NOT KATANA_SIMD_KERNELS_ACTIVE)
        return()
    endif()

    set(_objects ${target}_avx2_kernels)
    add_library(${_objects} OBJECT ${ARG_AVX2})
    katana_target_defaults(${_objects})
    if(MSVC)
        target_compile_options(${_objects} PRIVATE /arch:AVX2)
    else()
        target_compile_options(${_objects} PRIVATE -mavx2 -mfma)
    endif()
    target_sources(${target} PRIVATE $<TARGET_OBJECTS:${_objects}>)
    # Tells the target's dispatch code that the entries exist to be called.
    target_compile_definitions(${target} PRIVATE KATANA_HAVE_AVX2_KERNELS=1)
    set_property(GLOBAL APPEND PROPERTY KATANA_SIMD_OBJECT_LIBRARIES ${_objects})
endfunction()
