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
get_filename_component(stem "${OUTPUT}" NAME_WE)
set(copy "${work}/${stem}_project")
file(REMOVE_RECURSE "${copy}")
if(IS_DIRECTORY "${PROJECT}")
    file(COPY "${PROJECT}/" DESTINATION "${copy}")
else()
    # No project at all: the application must say so in its log and carry on
    # with the imports, not stop on a box nobody can close.
    file(MAKE_DIRECTORY "${copy}")
endif()
file(REMOVE "${OUTPUT}")

# With -DTOGGLE_LAYER=<name> the layer's visibility box is flipped through the
# panel first (see --toggle-layer in main.cpp); the application refuses to
# take the screenshot if it did not come through that cleanly.
# With -DIMPORT=<file> that file is imported into the project first - which,
# for a 12d archive holding a tin, opens the 3D view and so replaces the plan
# viewport. The toggle after that once called a listener of the viewport that
# had been destroyed.
# With -DSTYLE_MANAGER=ON the styles and linetypes manager is opened and IT
# is what the PNG holds, so the dialog is built and painted by a test.
set(extra)
if(DEFINED IMPORT)
    list(APPEND extra "${IMPORT}")
endif()
if(DEFINED TOGGLE_LAYER)
    list(APPEND extra --toggle-layer "${TOGGLE_LAYER}")
endif()
if(STYLE_MANAGER)
    list(APPEND extra --style-manager)
endif()
# -DATTRIBUTES=<entity id> opens the attribute manager on that entity.
if(DEFINED ATTRIBUTES)
    list(APPEND extra --attributes "${ATTRIBUTES}")
endif()
if(LAYER_MANAGER)
    list(APPEND extra --layer-manager)
endif()

execute_process(
    COMMAND "${APP}" "${copy}" ${extra} --screenshot "${OUTPUT}"
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
# window that painted nothing but its background is a few. The dialog is a
# smaller window of mostly flat panels, so it gets its own floor. Compared as
# hex for the reason given in check_plot.cmake.
set(floor 20000)
if(STYLE_MANAGER OR DEFINED ATTRIBUTES OR LAYER_MANAGER)
    set(floor 10000)
endif()
file(SIZE "${OUTPUT}" size)
if(size LESS floor)
    message(FATAL_ERROR "the PNG is only ${size} bytes: the window painted next to nothing")
endif()
file(READ "${OUTPUT}" head LIMIT 4 HEX)
if(NOT head STREQUAL "89504e47") # the PNG signature, \x89 P N G
    message(FATAL_ERROR "the output is not a PNG (first bytes ${head})")
endif()
message(STATUS "screenshot_headless: ${size} bytes of PNG at ${OUTPUT}")
