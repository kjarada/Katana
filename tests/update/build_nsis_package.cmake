# Builds the Windows installer from the build's REAL CPack configuration over a small stand-in
# tree, then holds the script CPack generated to check_nsis_script.cmake:
#
#   cmake -DCPACK_COMMAND=<cpack> -DCPACK_CONFIG=<build>/CPackConfig.cmake -DWORK=<folder>
#         -DCHECK=<check_nsis_script.cmake> -DEXPECT_VERSION=<x.y.z>
#         [-DREGISTRY_HEADER=<include/katana/update/registry.hpp>] -P build_nsis_package.cmake
#
# What this proves that the sample values of check_nsis_script.cmake cannot: that CPack takes
# cmake/NSIS.template.in and not its own (cmake/ is on CPACK_MODULE_PATH; a template that
# went missing is not an error to CPack, only another installer), that the settings of
# KatanaPackaging.cmake reach the script intact (the install root's backslash survives
# CPackConfig.cmake, which a first version of that setting did not), that the install record
# script runs and its file is in the installer's list, and that makensis accepts the result:
# the installer is compiled, which is the only syntax check an NSIS script has.
#
# The tree stands for the installed one: the programs and a few data files, as text. Nothing
# is installed or run; the installer is built into WORK and left there. Signing is switched
# off for the run, so a signing certificate in the environment cannot be applied to the
# stand-in files.

foreach(_required CPACK_COMMAND CPACK_CONFIG WORK CHECK EXPECT_VERSION)
    if(NOT ${_required})
        message(FATAL_ERROR "Pass -D${_required}=...")
    endif()
endforeach()
if(NOT EXISTS "${CPACK_CONFIG}")
    message(FATAL_ERROR "${CPACK_CONFIG} does not exist: configure the build first")
endif()

file(REMOVE_RECURSE "${WORK}")
set(_files
    bin/katana.exe bin/katana_cli.exe bin/katana_mcp.exe bin/katana_updater.exe
    bin/platforms/qwindows.dll
    etc/ssl/certs/ca-bundle.crt
    share/katana/samples/site/project.json)
foreach(_file IN LISTS _files)
    file(WRITE "${WORK}/tree/${_file}" "stand-in for ${_file}\n")
endforeach()

execute_process(
    COMMAND "${CMAKE_COMMAND}" -E env --unset=KATANA_SIGN_PFX --unset=KATANA_SIGN_IDENTITY
            --unset=KATANA_SIGN_PASSWORD
            "${CPACK_COMMAND}" --config "${CPACK_CONFIG}" -G NSIS -B "${WORK}/out"
            -D "CPACK_INSTALL_CMAKE_PROJECTS="
            -D "CPACK_INSTALLED_DIRECTORIES=${WORK}/tree;."
    RESULT_VARIABLE _status OUTPUT_VARIABLE _output ERROR_VARIABLE _error)
if(NOT _status EQUAL 0)
    message(FATAL_ERROR "cpack -G NSIS failed (${_status}); makensis did not accept the script, or "
        "CPack could not read the configuration:\n${_output}\n${_error}")
endif()

file(GLOB_RECURSE _scripts "${WORK}/out/project.nsi")
file(GLOB _installers "${WORK}/out/Katana-*.exe")
list(LENGTH _scripts _script_count)
list(LENGTH _installers _installer_count)
if(NOT _script_count EQUAL 1 OR NOT _installer_count EQUAL 1)
    message(FATAL_ERROR "expected one project.nsi and one installer in ${WORK}/out, found "
        "${_script_count} and ${_installer_count}:\n${_output}")
endif()
file(SIZE "${_installers}" _installer_size)
# Seven stand-in files of a few bytes are about 200 KB of installer (NSIS's own stub and
# the language files); a few bytes would mean the compiler made an empty file.
if(_installer_size LESS 100000)
    message(FATAL_ERROR "${_installers} is only ${_installer_size} bytes")
endif()

# CPack's own list of what the installer puts down is what its uninstaller deletes: every
# stand-in file, and the install record the pre-build script writes into the staged tree.
set(_expected ${_files} share/katana/install.json)
# The header that names the helper's RunOnce value, handed on when the caller gave it.
set(_registry_header_argument "")
if(REGISTRY_HEADER)
    set(_registry_header_argument "-DREGISTRY_HEADER=${REGISTRY_HEADER}")
endif()
execute_process(
    COMMAND "${CMAKE_COMMAND}" "-DSCRIPT=${_scripts}" "-DEXPECT_VERSION=${EXPECT_VERSION}"
            "-DEXPECT_FILES=${_expected}" ${_registry_header_argument} -P "${CHECK}"
    RESULT_VARIABLE _check_status OUTPUT_VARIABLE _check_output ERROR_VARIABLE _check_error)
message("${_check_output}${_check_error}")
if(NOT _check_status EQUAL 0)
    message(FATAL_ERROR "the generated installer script is not per user (above)")
endif()
message(STATUS "Installer built from the real configuration: ${_installers} (${_installer_size} bytes), "
    "compiled by makensis; ${_script_count} script, checked.")
