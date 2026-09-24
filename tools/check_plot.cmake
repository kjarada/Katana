# Runs the application headlessly with --plot and checks that a PDF came out
# with the drawing in it, one page, titled, and - at a fixed scale larger
# than the sheet - stopping at the margin (pdfinfo and pdftoppm, when found).
# Invoked by the qt_plot_headless test with:
#   -DAPP=<katana executable>  -DPROJECT=<sample project dir>  -DOUTPUT=<pdf path>
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
    message(FATAL_ERROR "katana --plot exited with ${rc}\n${out}\n${err}")
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

# What the PDF says about itself, when poppler's tools are there to ask (they
# are, in MSYS2's ucrt64): one page, a title (the project's name) and Katana
# as its creator - a viewer's tab and a document list used to show nothing.
find_program(PDFINFO pdfinfo)
find_program(PDFTOPPM pdftoppm)
if(PDFINFO)
    execute_process(COMMAND "${PDFINFO}" "${OUTPUT}" OUTPUT_VARIABLE info RESULT_VARIABLE rc)
    if(NOT rc EQUAL 0)
        message(FATAL_ERROR "pdfinfo could not read ${OUTPUT}")
    endif()
    if(NOT info MATCHES "Pages:[ ]+1
")
        message(FATAL_ERROR "the plot is not one page:
${info}")
    endif()
    if(NOT info MATCHES "Creator:[ ]+Katana")
        message(FATAL_ERROR "the plot does not name Katana as its creator:
${info}")
    endif()
    if(NOT info MATCHES "Title:[ ]+plot_headless_project")
        message(FATAL_ERROR "the plot is not titled with the project's name:
${info}")
    endif()
endif()

# At a fixed scale the drawing is larger than the sheet and must stop at the
# 10 mm margin, not run to the paper's edge. At 1 : 50 about the view's
# centre the site plan's hatched building covers the whole A3 sheet; rendered
# at 10 dpi (165 x 117 px, the margin about 4 px) the outer two pixels all
# round must be white and the middle of the sheet must not.
if(PDFTOPPM)
    set(fixed "${work}/plot_headless_fixed.pdf")
    file(REMOVE "${fixed}")
    execute_process(
        COMMAND "${APP}" "${copy}" --plot "${fixed}" --scale 50 --paper A3
        RESULT_VARIABLE rc OUTPUT_VARIABLE out ERROR_VARIABLE err TIMEOUT 120)
    if(NOT rc EQUAL 0 OR NOT EXISTS "${fixed}")
        message(FATAL_ERROR "katana --plot --scale 50 exited with ${rc}
${out}
${err}")
    endif()
    execute_process(
        COMMAND "${PDFTOPPM}" -r 10 -gray -singlefile "${fixed}" "${work}/plot_headless_fixed"
        RESULT_VARIABLE rc)
    set(pgm "${work}/plot_headless_fixed.pgm")
    if(NOT rc EQUAL 0 OR NOT EXISTS "${pgm}")
        message(FATAL_ERROR "pdftoppm could not render ${fixed}")
    endif()
    # A binary PGM: "P5", width, height, 255, one whitespace, then a byte a
    # pixel, row by row.
    file(READ "${pgm}" header LIMIT 20)
    if(NOT header MATCHES "^(P5[ 
]+([0-9]+)[ 
]+([0-9]+)[ 
]+255[ 
])")
        message(FATAL_ERROR "not a PGM: ${pgm}")
    endif()
    string(LENGTH "${CMAKE_MATCH_1}" offset)
    set(width ${CMAKE_MATCH_2})
    set(height ${CMAKE_MATCH_3})
    file(READ "${pgm}" pixels OFFSET ${offset} HEX)
    # The byte at (x, y), as two hex digits.
    macro(pixel_at x y result)
        math(EXPR _at "2 * (${y} * ${width} + ${x})")
        string(SUBSTRING "${pixels}" ${_at} 2 ${result})
    endmacro()
    math(EXPR last_x "${width} - 1")
    math(EXPR last_y "${height} - 1")
    math(EXPR inner_x "${width} - 2")
    math(EXPR inner_y "${height} - 2")
    foreach(x RANGE 0 ${last_x})
        foreach(y 0 1 ${inner_y} ${last_y})
            pixel_at(${x} ${y} value)
            if(NOT value STREQUAL "ff")
                message(FATAL_ERROR "ink at (${x}, ${y}) in the margin of the fixed-scale plot")
            endif()
        endforeach()
    endforeach()
    foreach(y RANGE 0 ${last_y})
        foreach(x 0 1 ${inner_x} ${last_x})
            pixel_at(${x} ${y} value)
            if(NOT value STREQUAL "ff")
                message(FATAL_ERROR "ink at (${x}, ${y}) in the margin of the fixed-scale plot")
            endif()
        endforeach()
    endforeach()
    math(EXPR middle_x "${width} / 2")
    math(EXPR middle_y "${height} / 2")
    pixel_at(${middle_x} ${middle_y} value)
    if(value STREQUAL "ff")
        message(FATAL_ERROR "nothing drawn in the middle of the fixed-scale plot")
    endif()
    message(STATUS "plot_headless: the 1 : 50 plot stops at its margin (${width} x ${height} at 10 dpi)")
endif()
