add_library(katana_core STATIC
    log.cpp
    task_pool.cpp
    xml.cpp
)
katana_target_defaults(katana_core)
