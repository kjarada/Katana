# Drives the sheets headlessly the way an agent does (docs/plotting.md,
# "Sheets on the command line"): --command lines lay the sheets out, fill the
# title block, plot one sheet with PLOTSHEETS and SAVE the project; then
# --sheets-json and --plot-sheets write what they made, all in one run with
# no window. Invoked by the qt_sheets_headless test with:
#   -DAPP=<katana executable>  -DPROJECT=<sample project dir>  -DOUTPUT=<json path>
# under QT_QPA_PLATFORM=offscreen.
#
# Four runs:
#   1. the verbs, the JSON and the PDFs: the JSON holds what the verbs made,
#      the PDFs start with %PDF and PLOTSHEETS sheets=2 is one page;
#   2. --sheets-json - alone on the saved copy: the same JSON on stdout, so
#      SAVE kept the sheets in the project;
#   3. a line that is refused: the run fails, naming it, and writes nothing;
#   4. a PLOTSHEETS whose sheet the painter reports a problem in: the PDF is
#      written and the run goes on.
#
# The sample project is COPIED before it is opened, as in check_plot.cmake: a
# test must never change the data checked into the repository.

foreach(variable APP PROJECT OUTPUT)
    if(NOT DEFINED ${variable})
        message(FATAL_ERROR "check_sheets_headless.cmake needs -D${variable}")
    endif()
endforeach()

get_filename_component(work "${OUTPUT}" DIRECTORY)
set(copy "${work}/sheets_headless_project")
set(one "${work}/sheets_headless_one.pdf")
set(all "${work}/sheets_headless_all.pdf")
file(REMOVE_RECURSE "${copy}")
file(COPY "${PROJECT}/" DESTINATION "${copy}")
file(REMOVE "${OUTPUT}" "${one}" "${all}")

execute_process(
    COMMAND "${APP}" "${copy}"
        --command "GENERATE grid scale=500 overlap=5 keyplan=on"
        --command "TITLEBLOCK organisation \"Example Surveys\""
        --command "SHEET NEW NOTES paper=A4 at=1"
        --command "VIEW ADD 1 notes text=\"1. LEVELS IN METRES.\""
        --command "PLOTSHEETS \"${one}\" sheets=2 dpi=100"
        --command "SAVE"
        --sheets-json "${OUTPUT}"
        --plot-sheets "${all}"
    RESULT_VARIABLE rc
    OUTPUT_VARIABLE out
    ERROR_VARIABLE err
    TIMEOUT 150)
if(NOT rc EQUAL 0)
    message(FATAL_ERROR "the headless sheet run exited with ${rc}\n${out}\n${err}")
endif()
foreach(file "${OUTPUT}" "${one}" "${all}")
    if(NOT EXISTS "${file}")
        message(FATAL_ERROR "nothing was written to ${file}\n${err}")
    endif()
endforeach()

# The replies went to the log, and so to stderr: one fact per line.
foreach(reply "generated [0-9]+ sheets?: 1" "organisation=\"Example Surveys\""
              "added sheet 1 id=s[0-9]+ name=\"NOTES\" paper=A4"
              "added view vp[0-9]+ to sheet 1" "Plotted 1 sheet to")
    if(NOT err MATCHES "${reply}")
        message(FATAL_ERROR "the log has no reply matching '${reply}':\n${err}")
    endif()
endforeach()

# The JSON is written compact, as the project stores it.
file(READ "${OUTPUT}" json)
foreach(part "\"format\":\"katana-sheets\"" "\"organisation\":\"Example Surveys\""
             "\"kind\":\"key_plan\"" "\"kind\":\"notes\"" "\"name\":\"NOTES\"")
    string(FIND "${json}" "${part}" at)
    if(at EQUAL -1)
        message(FATAL_ERROR "the sheets JSON has no ${part}:\n${json}")
    endif()
endforeach()

foreach(pdf "${one}" "${all}")
    file(READ "${pdf}" head LIMIT 4 HEX)
    if(NOT head STREQUAL "25504446") # "%PDF"
        message(FATAL_ERROR "${pdf} does not start with %PDF (first bytes ${head})")
    endif()
endforeach()
# PLOTSHEETS sheets=2 wrote one page: pdfinfo says so when it is installed;
# without it the page objects are counted (a page's dictionary is not
# compressed, and /Type /Pages is the page tree, not a page).
find_program(PDFINFO pdfinfo)
if(PDFINFO)
    execute_process(COMMAND "${PDFINFO}" "${one}" OUTPUT_VARIABLE info RESULT_VARIABLE rc)
    if(NOT rc EQUAL 0 OR NOT info MATCHES "Pages:[ ]+1\n")
        message(FATAL_ERROR "PLOTSHEETS sheets=2 did not plot one page:\n${info}")
    endif()
endif()
foreach(pdf "${one}" "${all}")
    file(STRINGS "${pdf}" pageObjects REGEX "/Type[ ]*/Page([^s]|$)")
    list(LENGTH pageObjects pages)
    if(pdf STREQUAL one)
        if(NOT pages EQUAL 1)
            message(FATAL_ERROR "PLOTSHEETS sheets=2 wrote ${pages} pages, not 1")
        endif()
    elseif(pages LESS 2)
        message(FATAL_ERROR "${pdf} has ${pages} pages; the set has more than one sheet")
    endif()
endforeach()

# 2. The project kept what SAVE wrote: the same JSON, on stdout.
execute_process(
    COMMAND "${APP}" "${copy}" --sheets-json -
    RESULT_VARIABLE rc
    OUTPUT_VARIABLE printed
    ERROR_VARIABLE err
    TIMEOUT 120)
if(NOT rc EQUAL 0)
    message(FATAL_ERROR "--sheets-json - exited with ${rc}\n${err}")
endif()
# Stripped: stdout is a text stream, so on Windows its last line ends "\r\n".
string(STRIP "${printed}" printed)
string(STRIP "${json}" json)
if(NOT printed STREQUAL json)
    message(FATAL_ERROR "the saved project's sheets are not what was written:\n"
                        "written:\n${json}\nprinted:\n${printed}")
endif()

# 3. A refused line fails the run and names what was refused.
set(refused "${work}/sheets_headless_refused.json")
file(REMOVE "${refused}")
execute_process(
    COMMAND "${APP}" "${copy}" --command "SHEET REMOVE 99" --sheets-json "${refused}"
    RESULT_VARIABLE rc
    OUTPUT_VARIABLE out
    ERROR_VARIABLE err
    TIMEOUT 120)
if(rc EQUAL 0 OR EXISTS "${refused}")
    message(FATAL_ERROR "a refused --command did not fail the run (exit ${rc})\n${err}")
endif()
if(NOT err MATCHES "no sheet 99" OR NOT err MATCHES "--command SHEET REMOVE 99 was refused")
    message(FATAL_ERROR "the refusal does not say what was refused:\n${err}")
endif()
# 4. A plot the painter reports a problem in (an image view whose file is not
#    in the project) is still written: PLOTSHEETS is not refused, and the run
#    goes on to --sheets-json.
set(warned "${work}/sheets_headless_warned.pdf")
set(after "${work}/sheets_headless_after.json")
file(REMOVE "${warned}" "${after}")
execute_process(
    COMMAND "${APP}" "${copy}" --command "VIEW ADD 1 image text=nowhere.png"
        --command "PLOTSHEETS \"${warned}\" sheets=1" --sheets-json "${after}"
    RESULT_VARIABLE rc
    OUTPUT_VARIABLE out
    ERROR_VARIABLE err
    TIMEOUT 120)
if(NOT rc EQUAL 0 OR NOT EXISTS "${warned}" OR NOT EXISTS "${after}")
    message(FATAL_ERROR "a plot with a problem in it stopped the run (exit ${rc})\n${err}")
endif()
message(STATUS "sheets_headless: the verbs, SAVE, --sheets-json and both plots agree")
