find_package(SQLite3 REQUIRED)

add_library(katana_storage STATIC
    sqlite_database.cpp
    project_store.cpp
)
katana_target_defaults(katana_storage)
# CMake 3.24+ names the imported target SQLite::SQLite3.
target_link_libraries(katana_storage
    PUBLIC katana_entity katana_survey
    PRIVATE SQLite::SQLite3
)
