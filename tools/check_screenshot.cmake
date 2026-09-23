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
# With -DCUSTOMISE_DIR=<directory> every 12d library and mapfile in it is
# loaded before anything is drawn, so the drawing appears as the customisation
# says it should (PLAN.MD 20.3). A DIRECTORY rather than a list of files,
# because a list of paths cannot survive being passed through `cmake -D`
# without its separators being escaped into the paths themselves - which is
# exactly what happened, and left this test passing while loading nothing.
#
# A customisation that is not in the checkout - the reference one carries its
# author's licence notice - is reported and the run goes on without it, since
# the suite must stay green in a checkout that does not include it.
if(DEFINED CUSTOMISE_DIR)
    file(GLOB _customisation "${CUSTOMISE_DIR}/*.4d" "${CUSTOMISE_DIR}/*.mapfile")
    if(_customisation)
        list(APPEND extra "--customise" ${_customisation})
        list(LENGTH _customisation _count)
        message(STATUS "customisation: ${_count} files from ${CUSTOMISE_DIR}")
    else()
        message(STATUS "no customisation files in ${CUSTOMISE_DIR}; running without them")
    endif()
endif()
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
# -DACTIONS=<name>[,<name>...] triggers those menu items, in order, after the
# import (see --action in main.cpp). Commas, not semicolons, for the reason
# CUSTOMISE_DIR is a directory: a CMake list does not survive `cmake -D`.
if(DEFINED ACTIONS)
    string(REPLACE "," ";" _actions "${ACTIONS}")
    foreach(_action IN LISTS _actions)
        list(APPEND extra --action "${_action}")
    endforeach()
endif()
# -DDATASET_INFO=<file> and -DIMPORT_OPTIONS=<file> grab GIS > Dataset
# Information and the GIS menu's import dialog for that file.
if(DEFINED DATASET_INFO)
    list(APPEND extra --dataset-info "${DATASET_INFO}")
endif()
if(DEFINED IMPORT_OPTIONS)
    list(APPEND extra --import-options "${IMPORT_OPTIONS}")
endif()
# -DSELECT_ALL=ON selects every entity before the actions run (--select-all),
# for a command that acts on the selection (surveyArea).
if(SELECT_ALL)
    list(APPEND extra --select-all)
endif()
# -DSURVEY_DIALOG=<action name> opens that Survey menu dialog through its
# action and grabs IT (--survey-dialog). -DFILL=<field>=<text>[|<field>=<text>...]
# types into its fields - '|' between them, because commas and blanks belong
# to the values (E,N and N 45 E) and a CMake list does not survive `cmake -D`;
# "\n" in a text is a line break, so a field book fits - and
# -DPRESS=<button>[,<button>...] clicks its buttons, in order, after the fills.
if(DEFINED SURVEY_DIALOG)
    list(APPEND extra --survey-dialog "${SURVEY_DIALOG}")
    if(DEFINED FILL)
        string(REPLACE "|" ";" _fills "${FILL}")
        foreach(_fill IN LISTS _fills)
            list(APPEND extra --fill "${_fill}")
        endforeach()
    endif()
    if(DEFINED PRESS)
        string(REPLACE "," ";" _presses "${PRESS}")
        foreach(_press IN LISTS _presses)
            list(APPEND extra --press "${_press}")
        endforeach()
    endif()
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
# -DEXPECT=<regex> must match what the run printed. A headless run echoes its
# command log to stderr, so this is how a test sees what a command REPORTED -
# a surface's elevation range, which returns a cloud's surface was built from
# - and not only that the window painted.
if(DEFINED EXPECT AND NOT "${out}${err}" MATCHES "${EXPECT}")
    message(FATAL_ERROR "the run did not report /${EXPECT}/:\n${out}\n${err}")
endif()

# A 1360 x 860 window with a drawing in it compresses to tens of kilobytes; a
# window that painted nothing but its background is a few. The dialog is a
# smaller window of mostly flat panels, so it gets its own floor. Compared as
# hex for the reason given in check_plot.cmake.
set(floor 20000)
if(STYLE_MANAGER OR DEFINED ATTRIBUTES OR LAYER_MANAGER)
    set(floor 10000)
endif()
# The GIS dialogs are small forms and a page of text: kilobytes of PNG when
# they paint, a few hundred bytes of flat background when they do not.
if(DEFINED DATASET_INFO OR DEFINED IMPORT_OPTIONS)
    set(floor 4000)
endif()
# So are the survey dialogs: a form, a report pane and a row of buttons.
if(DEFINED SURVEY_DIALOG)
    set(floor 4000)
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
