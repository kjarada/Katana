# Verifies the one-way dependency rule of PLAN.MD section 2 and the library
# isolation rule (Rule 4).
#
#   cmake -DKATANA_ROOT=<source dir> -P tools/check_layering.cmake
#
# 1. A header or source belonging to layer X may only include "katana/<Y>/..."
#    when Y is X itself or is listed in KATANA_ALLOWED_<X>.
# 2. Public headers (include/) must not include third-party library headers;
#    those stay behind internal interfaces inside src/.

if(NOT KATANA_ROOT)
    message(FATAL_ERROR "Pass -DKATANA_ROOT=<source dir>")
endif()

# Layer -> layers it may depend on (transitively closed by hand, lowest first).
set(KATANA_ALLOWED_core "")
set(KATANA_ALLOWED_math "core")
set(KATANA_ALLOWED_geometry "core;math")
set(KATANA_ALLOWED_geodesy "core;math")
set(KATANA_ALLOWED_survey "core;math")
# surveyio reads manufacturer field data - Leica, Trimble, Topcon, LandXML, a
# coordinate CSV - and produces katana::survey values, nothing else.
#
# geodesy is deliberately ABSENT, and that is the module's whole discipline:
# surveyio records the coordinate system a file DECLARES and never transforms
# anything. A transformation needs both of its ends, and an importer knows only
# one of them, so transforming on import moves the survey silently. See
# survey::DeclaredCoordinateSystem for the argument in full.
set(KATANA_ALLOWED_surveyio "core;math;geometry;survey")
set(KATANA_ALLOWED_terrain "core;math;geometry")
set(KATANA_ALLOWED_entity "core;math;geometry")
set(KATANA_ALLOWED_commands "core;math;geometry;entity")
# The archive module: the .12da Archive, and the survey coding libraries beside it.
# Pure text handling plus the mapping onto entities, alignments and surfaces, so
# it sits beside commands and needs no GDAL.
set(KATANA_ALLOWED_archive12d "core;math;geometry;terrain;entity")
set(KATANA_ALLOWED_storage "core;math;geometry;entity;survey")
# gis and pointcloud are both implemented by src/katana_io.
set(KATANA_ALLOWED_gis "core;pointcloud")
set(KATANA_ALLOWED_pointcloud "core")
# interop converts what gis and pointcloud read into the domain model, so it is
# the only layer allowed to see both the external readers and the entity model.
set(KATANA_ALLOWED_interop
    "core;math;geometry;terrain;entity;commands;archive12d;gis;pointcloud")
# render sits beside terrain: it consumes a DrawList of plain geometry and has
# no idea an Entity or a Document exists, which is Rule 3 made structural.
set(KATANA_ALLOWED_render "core;math;geometry")
# cad deliberately may NOT see interop: keeping GDAL and PDAL out of the core
# application layer is what lets it build with -DKATANA_BUILD_IO=OFF, which the
# sanitizer job depends on. Reference data is owned by app/qt instead.
#
# cad deliberately may NOT see surveyio either, for the same shape of reason: the
# CAD core must not depend on manufacturer structures. app/qt own the parsers, and
# what reaches cad is katana::survey values - points, observations, stations - so
# that adding a fifth instrument vendor cannot change anything cad compiles
# against, and removing one cannot break it.
set(KATANA_ALLOWED_cad
    "core;math;geometry;geodesy;survey;terrain;render;entity;commands;storage")
set(KATANA_ALLOWED_app
    "core;math;geometry;geodesy;survey;surveyio;terrain;render;entity;commands;storage;cad;archive12d;gis;pointcloud;interop")
set(KATANA_ALLOWED_qt
    "core;math;geometry;geodesy;survey;surveyio;terrain;render;entity;commands;storage;cad;archive12d;gis;pointcloud;interop;app")
# The GPU renderer (src/katana_qt/gpu, docs/gpu.md) lives under katana_qt for
# Qt's sake, but it is the software rasteriser's twin behind the same seam: it
# may see a DrawList, a Camera and the point cloud's plain types, and never a
# Document - so it gets render's rule, not qt's.
set(KATANA_ALLOWED_gpu "core;math;geometry;render;pointcloud")

# src/<directory> -> layer
set(KATANA_SRC_LAYER_katana_core core)
set(KATANA_SRC_LAYER_katana_math math)
set(KATANA_SRC_LAYER_katana_geometry geometry)
set(KATANA_SRC_LAYER_katana_geodesy geodesy)
set(KATANA_SRC_LAYER_katana_survey survey)
set(KATANA_SRC_LAYER_katana_surveyio surveyio)
set(KATANA_SRC_LAYER_katana_terrain terrain)
set(KATANA_SRC_LAYER_katana_render render)
set(KATANA_SRC_LAYER_katana_entity entity)
set(KATANA_SRC_LAYER_katana_commands commands)
set(KATANA_SRC_LAYER_katana_archive12d archive12d)
set(KATANA_SRC_LAYER_katana_storage storage)
set(KATANA_SRC_LAYER_katana_cad cad)
set(KATANA_SRC_LAYER_katana_io gis)
set(KATANA_SRC_LAYER_katana_interop interop)
set(KATANA_SRC_LAYER_katana_app app)
set(KATANA_SRC_LAYER_katana_qt qt)
# src/<directory>/<subdirectory> -> a narrower layer than its directory's.
set(KATANA_SRC_SUBLAYERS "katana_qt/gpu=gpu")

set(KATANA_THIRD_PARTY_PATTERN
    "#[ \t]*include[ \t]*[<\"](Eigen/|CGAL/|proj\\.h|sqlite3\\.h|gdal|ogr|cpl_|pdal/|nlohmann/|Q[A-Z]|vulkan/)")

set(_violations "")

function(_check_file file layer is_public)
    file(STRINGS "${file}" _lines REGEX "#[ \t]*include")
    file(RELATIVE_PATH _rel "${KATANA_ROOT}" "${file}")
    foreach(_line IN LISTS _lines)
        if(_line MATCHES "#[ \t]*include[ \t]*\"katana/([a-z_]+)/")
            set(_dep "${CMAKE_MATCH_1}")
            if(NOT _dep STREQUAL layer)
                list(FIND KATANA_ALLOWED_${layer} "${_dep}" _index)
                if(_index EQUAL -1)
                    list(APPEND _violations
                        "${_rel}: layer '${layer}' must not depend on '${_dep}'")
                endif()
            endif()
        endif()
        if(is_public AND _line MATCHES "${KATANA_THIRD_PARTY_PATTERN}")
            list(APPEND _violations
                "${_rel}: public header exposes a third-party header (${_line})")
        endif()
    endforeach()
    set(_violations "${_violations}" PARENT_SCOPE)
endfunction()

file(GLOB _layer_dirs LIST_DIRECTORIES true "${KATANA_ROOT}/include/katana/*")
foreach(_dir IN LISTS _layer_dirs)
    if(IS_DIRECTORY "${_dir}")
        get_filename_component(_layer "${_dir}" NAME)
        if(NOT DEFINED KATANA_ALLOWED_${_layer})
            list(APPEND _violations "include/katana/${_layer}: layer is not declared in check_layering.cmake")
            continue()
        endif()
        file(GLOB_RECURSE _headers "${_dir}/*.hpp")
        foreach(_header IN LISTS _headers)
            _check_file("${_header}" "${_layer}" TRUE)
        endforeach()
    endif()
endforeach()

file(GLOB _src_dirs LIST_DIRECTORIES true "${KATANA_ROOT}/src/*")
foreach(_dir IN LISTS _src_dirs)
    if(IS_DIRECTORY "${_dir}")
        get_filename_component(_name "${_dir}" NAME)
        if(NOT DEFINED KATANA_SRC_LAYER_${_name})
            list(APPEND _violations "src/${_name}: module is not declared in check_layering.cmake")
            continue()
        endif()
        set(_layer "${KATANA_SRC_LAYER_${_name}}")
        file(GLOB_RECURSE _sources "${_dir}/*.cpp" "${_dir}/*.hpp")
        foreach(_source IN LISTS _sources)
            set(_source_layer "${_layer}")
            foreach(_sublayer IN LISTS KATANA_SRC_SUBLAYERS)
                string(REPLACE "=" ";" _pair "${_sublayer}")
                list(GET _pair 0 _prefix)
                list(GET _pair 1 _narrower)
                if(_source MATCHES "/src/${_prefix}/")
                    set(_source_layer "${_narrower}")
                endif()
            endforeach()
            _check_file("${_source}" "${_source_layer}" FALSE)
        endforeach()
    endif()
endforeach()

if(_violations)
    list(JOIN _violations "\n  " _text)
    message(FATAL_ERROR "Layering violations:\n  ${_text}")
endif()
message(STATUS "Layering check passed.")
