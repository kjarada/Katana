# Sanitizer support.
#
#   GCC/Clang (ELF, macOS) : AddressSanitizer + UndefinedBehaviorSanitizer
#   MSVC                   : AddressSanitizer
#   MinGW                  : libsanitizer is not shipped, so ASan is unavailable.
#                            UBSan still works in trap mode (no runtime needed):
#                            undefined behaviour aborts the process, which fails
#                            the test that triggered it.
#
# float-cast-overflow is named explicitly because GCC's -fsanitize=undefined
# does NOT include it, and it is the UB that let the rasteriser drop every
# triangle crossing the eye plane (a float beyond INT_MAX converted to int)
# without any sanitizer run noticing (PLAN.MD Phase 15, 2026-09-23).

set(KATANA_SANITIZER_COMPILE_FLAGS "")
set(KATANA_SANITIZER_LINK_FLAGS "")

if(KATANA_ENABLE_SANITIZERS)
    if(MSVC)
        set(KATANA_SANITIZER_COMPILE_FLAGS /fsanitize=address)
        message(STATUS "Katana sanitizers: MSVC AddressSanitizer")
    elseif(MINGW)
        set(KATANA_SANITIZER_COMPILE_FLAGS
            -fsanitize=undefined,float-cast-overflow -fno-sanitize=vptr
            -fsanitize-undefined-trap-on-error)
        message(STATUS "Katana sanitizers: UBSan (trap mode). ASan is unavailable on MinGW.")
    else()
        set(KATANA_SANITIZER_COMPILE_FLAGS
            -fsanitize=address,undefined,float-cast-overflow -fno-omit-frame-pointer
            -fno-sanitize-recover=undefined,float-cast-overflow)
        set(KATANA_SANITIZER_LINK_FLAGS -fsanitize=address,undefined,float-cast-overflow)
        message(STATUS "Katana sanitizers: AddressSanitizer + UndefinedBehaviorSanitizer")
    endif()
endif()

function(katana_apply_sanitizers target)
    if(KATANA_SANITIZER_COMPILE_FLAGS)
        target_compile_options(${target} PRIVATE ${KATANA_SANITIZER_COMPILE_FLAGS})
    endif()
    if(KATANA_SANITIZER_LINK_FLAGS)
        target_link_options(${target} PUBLIC ${KATANA_SANITIZER_LINK_FLAGS})
    endif()
endfunction()
