# Builds the real main window headlessly with --screenshot and checks that a PNG
# came out. Invoked by the qt_screenshot_headless test with:
#   -DAPP=<katana executable>  -DPROJECT=<sample project dir>  -DOUTPUT=<png path>
# under QT_QPA_PLATFORM=offscreen, so nothing is ever shown.
#
# What this protects is construction, not appearance: the window is built with
# the theme applied, every toolbar icon is painted by the code in icons.cpp,
# and the viewport draws the sample with its hatch and its alignment overlay.
# A painter that throws, an action wired to a null pointer or a dock that fails
# to build ends the process before it writes anything - and that is caught
# here rather than on a user's first launch. Whether the result LOOKS right is
# a question for a person; the PNG is left in the build tree for that.
#
# The sample project is COPIED before it is opened, as in check_plot.cmake: a
# test must never change the data checked into the repository.

foreach(variable APP PROJECT OUTPUT)
    if(NOT DEFINED ${variable})
        message(FATAL_ERROR "check_screenshot.cmake needs -D${variable}")
    endif()
endforeach()

get_filename_component(work "${OUTPUT}" DIRECTORY)
set(copy "${work}/screenshot_headless_project")
file(REMOVE_RECURSE "${copy}")
file(COPY "${PROJECT}/" DESTINATION "${copy}")
file(REMOVE "${OUTPUT}")

execute_process(
    COMMAND "${APP}" "${copy}" --screenshot "${OUTPUT}"
    RESULT_VARIABLE rc
    OUTPUT_VARIABLE out
    ERROR_VARIABLE err
    TIMEOUT 120)
if(NOT rc EQUAL 0)
    message(FATAL_ERROR "katana --screenshot exited with ${rc}\n${out}\n${err}")
endif()
if(NOT EXISTS "${OUTPUT}")
    message(FATAL_ERROR "no PNG was written to ${OUTPUT}\n${out}\n${err}")
endif()

# A 1360 x 860 window with a drawing in it compresses to tens of kilobytes; a
# window that painted nothing but its background is a few. Compared as hex for
# the reason given in check_plot.cmake.
file(SIZE "${OUTPUT}" size)
if(size LESS 20000)
    message(FATAL_ERROR "the PNG is only ${size} bytes: the window painted next to nothing")
endif()
file(READ "${OUTPUT}" head LIMIT 4 HEX)
if(NOT head STREQUAL "89504e47") # the PNG signature, \x89 P N G
    message(FATAL_ERROR "the output is not a PNG (first bytes ${head})")
endif()
message(STATUS "screenshot_headless: ${size} bytes of PNG at ${OUTPUT}")
