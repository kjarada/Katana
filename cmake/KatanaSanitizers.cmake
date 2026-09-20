# Sanitizer support.
#
#   GCC/Clang (ELF, macOS) : AddressSanitizer + UndefinedBehaviorSanitizer
#   MSVC                   : AddressSanitizer
#   MinGW                  : libsanitizer is not shipped, so ASan is unavailable.
#                            UBSan still works in trap mode (no runtime needed):
#                            undefined behaviour aborts the process, which fails
#                            the test that triggered it.

set(KATANA_SANITIZER_COMPILE_FLAGS "")
set(KATANA_SANITIZER_LINK_FLAGS "")

if(KATANA_ENABLE_SANITIZERS)
    if(MSVC)
        set(KATANA_SANITIZER_COMPILE_FLAGS /fsanitize=address)
        message(STATUS "Katana sanitizers: MSVC AddressSanitizer")
    elseif(MINGW)
        set(KATANA_SANITIZER_COMPILE_FLAGS
            -fsanitize=undefined -fno-sanitize=vptr -fsanitize-undefined-trap-on-error)
        message(STATUS "Katana sanitizers: UBSan (trap mode). ASan is unavailable on MinGW.")
    else()
        set(KATANA_SANITIZER_COMPILE_FLAGS
            -fsanitize=address,undefined -fno-omit-frame-pointer -fno-sanitize-recover=undefined)
        set(KATANA_SANITIZER_LINK_FLAGS -fsanitize=address,undefined)
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
