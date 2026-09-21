# Runs the application headlessly with --plot and checks that a PDF came out
# (PLAN.MD Phase 22). Invoked by the qt_plot_headless test with:
#   -DAPP=<katana_qt_app>  -DPROJECT=<sample project dir>  -DOUTPUT=<pdf path>
# under QT_QPA_PLATFORM=offscreen, so no window is ever shown.
#
# The sample project is COPIED before it is opened. Opening a project can
# touch its directory (backups, a migration), and a test must never change
# the data checked into the repository.

foreach(variable APP PROJECT OUTPUT)
    if(NOT DEFINED ${variable})
        message(FATAL_ERROR "check_plot.cmake needs -D${variable}")
    endif()
endforeach()

get_filename_component(work "${OUTPUT}" DIRECTORY)
set(copy "${work}/plot_headless_project")
file(REMOVE_RECURSE "${copy}")
file(COPY "${PROJECT}/" DESTINATION "${copy}")
file(REMOVE "${OUTPUT}")

execute_process(
    COMMAND "${APP}" "${copy}" --plot "${OUTPUT}" --fit --paper A3
    RESULT_VARIABLE rc
    OUTPUT_VARIABLE out
    ERROR_VARIABLE err
    TIMEOUT 120)
if(NOT rc EQUAL 0)
    message(FATAL_ERROR "katana_qt_app --plot exited with ${rc}\n${out}\n${err}")
endif()
if(NOT EXISTS "${OUTPUT}")
    message(FATAL_ERROR "no PDF was written to ${OUTPUT}\n${out}\n${err}")
endif()

# A PDF with the site plan drawn in it is tens of kilobytes; an empty page is
# under two. The threshold is deliberately far below the real size so that
# it catches "nothing was drawn" and not "a little less was drawn".
file(SIZE "${OUTPUT}" size)
if(size LESS 2000)
    message(FATAL_ERROR "the PDF is only ${size} bytes: nothing was drawn")
endif()
# Compared as hex, not as text. A plain file(READ ... LIMIT 4) on this
# platform returned "%PDF" plus a newline - five characters - and the string
# comparison failed on a perfectly good PDF; HEX is byte-exact.
file(READ "${OUTPUT}" head LIMIT 4 HEX)
if(NOT head STREQUAL "25504446") # "%PDF"
    message(FATAL_ERROR "the output does not start with %PDF (first bytes ${head})")
endif()
message(STATUS "plot_headless: ${size} bytes of PDF at ${OUTPUT}")
