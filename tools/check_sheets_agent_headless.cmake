# Drives every sheet feature headlessly the way an agent does (docs/plotting.md,
# "Sheets on the command line"): one run of --command lines lays a sheet out
# with a legend, a grid and a key plan, adds the drawing register, sets the
# page setup, arranges and checks the sheets and plots them as PNGs with
# PLOTSHEETS; then --plot-sheets plots the set to one PDF. Invoked by the
# qt_sheets_agent_headless test with:
#   -DAPP=<katana executable>  -DPROJECT=<sample project dir>  -DOUTPUT=<pdf path>
# under QT_QPA_PLATFORM=offscreen.
#
# It checks what an agent reads back: each reply on the log (stderr), the
# SHEETS CHECK lines in the form "severity code sheet view message=...", the
# file="..." lines PLOTSHEETS logs for what it wrote, the PNGs themselves and
# the PDF --plot-sheets names on stdout.
#
# The sample project is COPIED before it is opened, as in check_plot.cmake: a
# test must never change the data checked into the repository.

foreach(variable APP PROJECT OUTPUT)
    if(NOT DEFINED ${variable})
        message(FATAL_ERROR "check_sheets_agent_headless.cmake needs -D${variable}")
    endif()
endforeach()

get_filename_component(work "${OUTPUT}" DIRECTORY)
set(copy "${work}/sheets_agent_project")
set(pngs "${work}/sheets_agent_png")
file(REMOVE_RECURSE "${copy}" "${pngs}")
file(COPY "${PROJECT}/" DESTINATION "${copy}")
file(REMOVE "${OUTPUT}")

execute_process(
    COMMAND "${APP}" "${copy}"
        --command "GENERATE fit legend=on"
        --command "TITLEBLOCK organisation \"Example Surveys\""
        --command "TITLEBLOCK REVISION ADD A 25/09/26 \"FIRST ISSUE\" KJ"
        --command "VIEW SET vp1 grid=ticks gridinterval=auto"
        --command "VIEW ADD 1 keyplan"
        --command "ARRANGE 1"
        --command "VIEW SET vp2 legend=whole_set"
        --command "GENERATE register"
        --command "SHEETS PAGESETUP style=grey pattern=\"{n:02} {name}\""
        --command "SHEETS CHECK"
        --command "PLOTSHEETS format=png folder=\"${pngs}\" dpi=50 style=mono"
        --plot-sheets "${OUTPUT}"
    RESULT_VARIABLE rc
    OUTPUT_VARIABLE out
    ERROR_VARIABLE err
    TIMEOUT 300)
if(NOT rc EQUAL 0)
    message(FATAL_ERROR "the headless agent run exited with ${rc}\n${out}\n${err}")
endif()

foreach(reply
        "generated 1 sheet: 1"
        "grid=ticks gridinterval=auto"
        "added view vp[0-9]+ to sheet 1\nview id=vp[0-9]+ kind=key_plan"
        "arranged sheet 1 moved="
        "legend=whole_set"
        "generated 1 sheet: 1 \\(the drawing register\\)"
        "pagesetup style=greyscale lineweight=1 dpi=300 pattern=\"{n:02} {name}\""
        "checked 2 sheets: "
        "\n(error|warning|info) [a-z.-]+ ([0-9]+|-) (vp[0-9]+|-) message=\""
        "file=\"[^\"]*01 DRAWING REGISTER.png\""
        "file=\"[^\"]*02 PLAN.png\"")
    if(NOT err MATCHES "${reply}")
        message(FATAL_ERROR "the log has no reply matching '${reply}':\n${err}")
    endif()
endforeach()

# The PNGs PLOTSHEETS wrote, and the PDF --plot-sheets names on stdout.
foreach(png "${pngs}/01 DRAWING REGISTER.png" "${pngs}/02 PLAN.png")
    if(NOT EXISTS "${png}")
        message(FATAL_ERROR "PLOTSHEETS did not write ${png}\n${err}")
    endif()
    file(READ "${png}" head LIMIT 4 HEX)
    if(NOT head STREQUAL "89504e47")
        message(FATAL_ERROR "${png} is not a PNG (first bytes ${head})")
    endif()
endforeach()
file(READ "${OUTPUT}" head LIMIT 4 HEX)
if(NOT head STREQUAL "25504446") # "%PDF"
    message(FATAL_ERROR "${OUTPUT} does not start with %PDF (first bytes ${head})")
endif()
get_filename_component(pdfName "${OUTPUT}" NAME)
if(NOT out MATCHES "${pdfName}")
    message(FATAL_ERROR "--plot-sheets did not name ${pdfName} on stdout:\n${out}")
endif()
file(STRINGS "${OUTPUT}" pageObjects REGEX "/Type[ ]*/Page([^s]|$)")
list(LENGTH pageObjects pages)
if(NOT pages EQUAL 2)
    message(FATAL_ERROR "${OUTPUT} has ${pages} pages, not the register and the plan")
endif()
