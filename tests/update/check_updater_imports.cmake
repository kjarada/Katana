# The helper program imports nothing but the operating system's DLLs.
#
#   cmake -DOBJDUMP=<objdump> -DPROGRAM=<katana_updater.exe> -P check_updater_imports.cmake
#   cmake -DLISTING=<an objdump -p listing> -P check_updater_imports.cmake      (the fixtures)
#
# Why it matters: the helper never runs from bin/. The engine copies it to a temporary
# folder and runs the copy, so that at commit nothing under the install root is loaded
# and no loaded file is renamed. A copy that imported libstdc++-6.dll would fail to
# start there (the DLL is in bin/, not beside the copy) - and one that found it in bin/
# would hold open the very file the swap must replace. -static (src/katana_updater/CMakeLists.txt)
# takes the toolchain's runtime in; what is left must be Windows'.
#
# "Operating system's" is a list of names, below. KERNEL32, ADVAPI32, USER32, WS2_32 and
# CRYPT32 are what static OpenSSL brings; the api-ms-win-crt-* set is the Universal C
# Runtime, part of Windows 10. A DLL the helper really needs that is also Windows' is
# added here, with the reason.

if(LISTING)
    file(READ "${LISTING}" _text)
elseif(OBJDUMP AND PROGRAM)
    execute_process(COMMAND "${OBJDUMP}" -p "${PROGRAM}"
        OUTPUT_VARIABLE _text RESULT_VARIABLE _status ERROR_VARIABLE _error)
    if(NOT _status EQUAL 0)
        message(FATAL_ERROR "${OBJDUMP} -p ${PROGRAM} failed (${_status}): ${_error}")
    endif()
else()
    message(FATAL_ERROR "Pass -DOBJDUMP=<objdump> -DPROGRAM=<exe>, or -DLISTING=<objdump -p output>")
endif()

string(REGEX MATCHALL "DLL Name: [^\r\n]+" _entries "${_text}")
if(NOT _entries)
    message(FATAL_ERROR "No imported DLL was found in the listing: the check would pass with nothing to check")
endif()

set(_allowed
    "^(kernel32|kernelbase|advapi32|user32|shell32|ole32|oleaut32|ws2_32|crypt32|bcrypt|ncrypt|ntdll|rpcrt4|secur32|userenv|version|shlwapi|iphlpapi|wintrust|psapi|cfgmgr32|sechost|msvcrt|ucrtbase|api-ms-win-[a-z0-9-]+)[.]dll$")
set(_foreign "")
set(_count 0)
foreach(_entry IN LISTS _entries)
    string(REGEX REPLACE "^DLL Name: " "" _name "${_entry}")
    string(STRIP "${_name}" _name)
    string(TOLOWER "${_name}" _lower)
    math(EXPR _count "${_count} + 1")
    if(NOT _lower MATCHES "${_allowed}")
        list(APPEND _foreign "${_name}")
    endif()
endforeach()
if(_foreign)
    list(JOIN _foreign ", " _text)
    message(FATAL_ERROR "The helper imports DLLs that are not the operating system's: ${_text}")
endif()
message(STATUS "The helper imports ${_count} DLLs, all of them the operating system's.")
