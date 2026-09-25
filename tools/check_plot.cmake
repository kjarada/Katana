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

# The window's PLOT verb (plotting/plot_drawing_dialog.hpp), run by a script
# (--script, a batch run) as File > Plot to PDF runs it: the same fitted
# sheet in colour, greyscale and monochrome, and with every line weight
# times 4 and times 0.25. Rendered at 20 dpi in colour, what each prints is
# held to what its style means, compared plot with plot rather than with
# counts from an earlier run:
#   - the colour plot has pixels whose channels differ; the greyscale and
#     monochrome plots have none (a grey is R = G = B);
#   - the monochrome plot has fewer mid greys than the greyscale one: its
#     fills are black or paper, and what grey is left is the edges'
#     anti-aliasing;
#   - heavier line weights put down more ink, lighter ones less.
if(PDFTOPPM)
    set(script "${work}/plot_verb.kcs")
    file(WRITE "${script}" "")
    foreach(style colour grey mono)
        file(APPEND "${script}" "PLOT \"${work}/plot_verb_${style}.pdf\" paper=A3 style=${style}\n")
    endforeach()
    file(APPEND "${script}" "PLOT \"${work}/plot_verb_heavy.pdf\" paper=A3 lineweight=4\n")
    file(APPEND "${script}" "PLOT \"${work}/plot_verb_light.pdf\" paper=A3 lineweight=0.25\n")
    execute_process(
        COMMAND "${APP}" "${copy}" --script "${script}"
        RESULT_VARIABLE rc OUTPUT_VARIABLE out ERROR_VARIABLE err TIMEOUT 120)
    if(NOT rc EQUAL 0)
        message(FATAL_ERROR "the PLOT script exited with ${rc}\n${out}\n${err}")
    endif()
    foreach(said "style=greyscale lineweight=1" "style=monochrome lineweight=1"
                 "style=colour lineweight=4" "style=colour lineweight=0.25")
        if(NOT "${out}${err}" MATCHES "${said}")
            message(FATAL_ERROR "no PLOT record said ${said}:\n${out}\n${err}")
        endif()
    endforeach()

    # Every grey a pixel can be, as the six hex digits of R = G = B, and the
    # middle half of them.
    set(greys)
    set(middle)
    foreach(level RANGE 0 255)
        math(EXPR hex "${level}" OUTPUT_FORMAT HEXADECIMAL)
        string(SUBSTRING "${hex}" 2 -1 hex)
        string(TOLOWER "${hex}" hex)
        string(LENGTH "${hex}" digits)
        if(digits EQUAL 1)
            set(hex "0${hex}")
        endif()
        list(APPEND greys "${hex}${hex}${hex}")
        if(level GREATER_EQUAL 64 AND level LESS_EQUAL 191)
            list(APPEND middle "${hex}${hex}${hex}")
        endif()
    endforeach()
    list(JOIN greys "|" grey_pattern)
    list(JOIN middle "|" middle_pattern)

    # The pixels of a plot rendered at 20 dpi, a six-digit hex triple each.
    function(plot_pixels style result)
        set(pdf "${work}/plot_verb_${style}.pdf")
        execute_process(COMMAND "${PDFTOPPM}" -r 20 -singlefile "${pdf}" "${work}/plot_verb_${style}"
                        RESULT_VARIABLE rc)
        set(ppm "${work}/plot_verb_${style}.ppm")
        if(NOT rc EQUAL 0 OR NOT EXISTS "${ppm}")
            message(FATAL_ERROR "pdftoppm could not render ${pdf}")
        endif()
        # A binary PPM: "P6", width, height, 255, one whitespace, then three
        # bytes a pixel.
        file(READ "${ppm}" header LIMIT 20)
        if(NOT header MATCHES "^(P6[ \n]+[0-9]+[ \n]+[0-9]+[ \n]+255[ \n])")
            message(FATAL_ERROR "not a PPM: ${ppm}")
        endif()
        string(LENGTH "${CMAKE_MATCH_1}" offset)
        file(READ "${ppm}" hex OFFSET ${offset} HEX)
        string(REGEX MATCHALL "[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]" pixels "${hex}")
        set(${result} "${pixels}" PARENT_SCOPE)
    endfunction()
    # How many of `pixels` are coloured, mid grey, and ink (a channel below
    # 250 - lighter than that is paper to the eye).
    function(count_pixels pixels coloured mid ink)
        set(list ${pixels})
        list(LENGTH list all)
        set(grey_list ${list})
        list(FILTER grey_list INCLUDE REGEX "^(${grey_pattern})$")
        list(LENGTH grey_list grey)
        math(EXPR c "${all} - ${grey}")
        set(${coloured} ${c} PARENT_SCOPE)
        set(mid_list ${grey_list})
        list(FILTER mid_list INCLUDE REGEX "^(${middle_pattern})$")
        list(LENGTH mid_list m)
        set(${mid} ${m} PARENT_SCOPE)
        set(paper_list ${list})
        list(FILTER paper_list INCLUDE REGEX "^f[a-f]f[a-f]f[a-f]$")
        list(LENGTH paper_list paper)
        math(EXPR i "${all} - ${paper}")
        set(${ink} ${i} PARENT_SCOPE)
    endfunction()

    foreach(style colour grey mono heavy light)
        plot_pixels(${style} pixels)
        count_pixels("${pixels}" coloured_${style} mid_${style} ink_${style})
        message(STATUS "plot_headless: PLOT ${style}: ${coloured_${style}} coloured, "
                       "${mid_${style}} mid grey, ${ink_${style}} inked pixels at 20 dpi")
    endforeach()
    if(coloured_colour EQUAL 0)
        message(FATAL_ERROR "the colour plot printed no colour")
    endif()
    if(NOT coloured_grey EQUAL 0 OR NOT coloured_mono EQUAL 0)
        message(FATAL_ERROR "a greyscale or monochrome plot printed colour (${coloured_grey}, ${coloured_mono} pixels)")
    endif()
    if(NOT mid_mono LESS mid_grey)
        message(FATAL_ERROR "the monochrome plot has as many mid greys as the greyscale one (${mid_mono}, ${mid_grey})")
    endif()
    if(NOT ink_heavy GREATER ink_colour OR NOT ink_light LESS ink_colour)
        message(FATAL_ERROR "line weights times 4 and times 0.25 did not print more and less ink (${ink_heavy}, ${ink_colour}, ${ink_light})")
    endif()
endif()
