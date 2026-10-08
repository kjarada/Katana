# The cache entry KATANA_BUILTIN_CUSTOMISATION: THE built-in customisation, the
# one a program starts with when nothing says otherwise (src/katana_cad/CMakeLists.txt,
# docs/building.md).
#
#   katana_declare_builtin_default(<file>)
#
# declares the entry with <file> as its default. It exists for one thing a
# plain set(... CACHE) cannot do: a cache keeps what it was given, and Katana
# Standard became the default after build directories had been configured with
# the NSW file as it. Such a directory kept NSW as the default and offered
# Katana Standard nowhere, so the tests of the new default skipped and the
# suite stayed green while it tested none of it. So the first configure that
# sees the earlier default MOVES it, once, and says so.
#
# What is moved, and what is not, is told apart by what CMake knows about the
# entry:
#
#   no entry yet                 a new build directory: the default as given
#   an entry of type FILEPATH    left by an earlier configure of this project
#   an entry of no type          given with -D on this command line: a choice
#
# Only the second, holding the NSW file, and only before the entry has been
# settled (KATANA_BUILTIN_DEFAULT_SETTLED, set by the first configure that
# runs this), is moved. A person who wants NSW as the default says so with -D
# and keeps it: a -D with no type is a choice, and one with a type, after the
# first configure, finds the entry settled. The one case this cannot tell from
# the earlier default is -DKATANA_BUILTIN_CUSTOMISATION:FILEPATH=<the NSW file>
# on the very first configure with this module of a directory that had it
# already; the message says how to get it back.
#
# A macro, not a function: the entry is set in the caller's scope.
macro(katana_declare_builtin_default katana_builtin_default_file)
    set(_katana_builtin_moved FALSE)
    if(DEFINED CACHE{KATANA_BUILTIN_CUSTOMISATION} AND NOT KATANA_BUILTIN_DEFAULT_SETTLED)
        get_property(_katana_builtin_type CACHE KATANA_BUILTIN_CUSTOMISATION PROPERTY TYPE)
        if(_katana_builtin_type STREQUAL "FILEPATH"
           AND "${KATANA_BUILTIN_CUSTOMISATION}" MATCHES "/nsw\\.customisation\\.json$")
            set(_katana_builtin_moved TRUE)
            set(_katana_builtin_was "${KATANA_BUILTIN_CUSTOMISATION}")
        endif()
    endif()
    set(KATANA_BUILTIN_CUSTOMISATION "${katana_builtin_default_file}"
        CACHE FILEPATH "The Katana customisation file compiled into the program as THE built-in (the default); none when it does not exist")
    if(_katana_builtin_moved)
        set(KATANA_BUILTIN_CUSTOMISATION "${katana_builtin_default_file}"
            CACHE FILEPATH "The Katana customisation file compiled into the program as THE built-in (the default); none when it does not exist" FORCE)
        message(STATUS
            "Built-in customisation: KATANA_BUILTIN_CUSTOMISATION held the NSW file, the default "
            "before Katana Standard (${_katana_builtin_was}); it is now ${katana_builtin_default_file}. "
            "NSW stays compiled in as an alternate (KATANA_ALTERNATE_CUSTOMISATIONS). To make NSW "
            "the default again, configure with -DKATANA_BUILTIN_CUSTOMISATION=<the NSW file>.")
    endif()
    set(KATANA_BUILTIN_DEFAULT_SETTLED ON CACHE INTERNAL
        "Whether this build directory has had its built-in default decided (cmake/KatanaBuiltInDefault.cmake)")
endmacro()
