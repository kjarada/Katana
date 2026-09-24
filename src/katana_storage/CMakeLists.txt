find_package(SQLite3 REQUIRED)

add_library(katana_storage STATIC
    sqlite_database.cpp
    project_store.cpp
)
katana_target_defaults(katana_storage)
# CMake 4.1 renamed FindSQLite3's imported target to SQLite3::SQLite3 and
# deprecated the SQLite::SQLite3 of 3.14-4.0, warning at every configure; use
# whichever name the running CMake provides.
if(TARGET SQLite3::SQLite3)
    set(_katana_sqlite_target SQLite3::SQLite3)
else()
    set(_katana_sqlite_target SQLite::SQLite3)
endif()
target_link_libraries(katana_storage
    PUBLIC katana_entity katana_survey
    PRIVATE ${_katana_sqlite_target}
)
