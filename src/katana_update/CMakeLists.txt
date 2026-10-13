# The auto-update engine (include/katana/update, docs/update.md): the manifest,
# the signature check, the plan, the staged download, the journalled swap and its
# rollback. Layer `update`: it may include `core` and nothing else, so it links
# neither GDAL nor Qt, and the helper program (src/katana_updater) can be built
# from this library and katana_core alone.
#
# The network is not here. A Fetch function is injected (feed.hpp); katana_app
# adapts gis::httpFetch to it. The seams to the machine - FsOps, ProcessOps,
# Launcher, RegistryOps, CredentialStore - are interfaces, so the swap, its
# rollback and its recovery are driven in-process by tests.

# ---- what a release is built against ---------------------------------------------

# Where releases are fetched from: kjarada/Katana, which the owner will make public, and
# which needs no token then. Until then a person who holds a read-only token reaches it
# through the REST API (include/katana/update/feed.hpp, FeedRoute::Api).
# -DKATANA_UPDATE_FEED=https://host/dir replaces GitHub with the owner's own host; both
# empty is a build with no auto-update.
set(KATANA_UPDATE_REPO "kjarada/Katana" CACHE STRING
    "owner/name of the GitHub repository releases are fetched from (empty with an empty KATANA_UPDATE_FEED: no auto-update)")
set(KATANA_UPDATE_FEED "" CACHE STRING
    "Base URL of a release host of the owner's own, instead of GitHub (it must honour Range)")

# ---- sources -------------------------------------------------------------------

set(_katana_update_sources
    version.cpp
    manifest.cpp
    paths.cpp
    signature.cpp
    trust.cpp
    plan.cpp
    inflate.cpp
    ranges.cpp
    state.cpp
    journal.cpp
    apply.cpp
    recover.cpp
    fault_fs.cpp
    feed.cpp
    check.cpp
    stage.cpp
    credential_env.cpp
    # FINISHED, with tests (tests/update/test_contract.cpp): the pure functions of the contract
    # that the skeleton settles itself - see the top of each file.
    contract.cpp
    arguments.cpp
    not_implemented.hpp
)
# One implementation of each seam per platform. The process scan and the registry
# live under src/katana_updater, where the helper's own task owns them, but THIS
# library compiles them: the programs read the registry to say `kind=installer`
# and scan processes to say `blocked`, and runningPrograms() is the engine's API.
if(WIN32)
    list(APPEND _katana_update_sources
        fsops_win.cpp
        spawn_win.cpp
        credential_win.cpp
        ../katana_updater/process_scan_win.cpp
        ../katana_updater/registry_win.cpp)
else()
    list(APPEND _katana_update_sources
        fsops_posix.cpp
        spawn_posix.cpp
        ../katana_updater/process_scan_posix.cpp
        ../katana_updater/registry_none.cpp)
endif()

add_library(katana_update STATIC ${_katana_update_sources})
katana_target_defaults(katana_update)

# ---- third parties, all behind src/ --------------------------------------------------

# OpenSSL for Ed25519 and zlib for raw deflate, STATIC on Windows: the helper program
# must import nothing but the operating system's DLLs (it runs from a copy in a
# temporary folder, with no bin/ beside it), and a static library cannot hand it a
# DLL it did not ask for. libcrypto.a and libz.a are in MSYS2 UCRT64; libcrypto needs
# ws2_32 and crypt32 after it. The resulting imports are MEASURED on tests/update's
# katana_update_link_probe, which uses Ed25519 through EVP, raw inflate and crc32 through
# this target with the helper's link options: ADVAPI32, CRYPT32, KERNEL32, USER32, WS2_32
# and the api-ms-win-crt set, nothing else (test katana_update_link_probe_imports_only_os_dlls).
# The helper itself, while it is a stub, references neither library and so says nothing about
# the link line; the probe does. (The helper adds SHELL32, for CommandLineToArgvW.)
#
# The static libraries are looked for BY NAME here, not through find_package(OpenSSL)
# with OPENSSL_USE_STATIC_LIBS: FindOpenSSL caches what it first found, and GDAL's and
# libcurl's own configuration files ask for OpenSSL too, so which of the DLL's import
# library and the static one the cache held would depend on who asked first. The
# probe's import test (tests/update, katana_update_link_probe_imports_only_os_dlls; and the
# helper's own once it uses them) is what would catch the wrong one - shown by linking the probe
# against the DLLs by hand, which that check refuses, naming libcrypto-3-x64.dll and zlib1.dll.
# Elsewhere the toolchain's own libraries are used.
if(WIN32 AND NOT MSVC)
    find_library(KATANA_UPDATE_CRYPTO_STATIC NAMES libcrypto.a)
    find_library(KATANA_UPDATE_ZLIB_STATIC NAMES libz.a)
    find_path(KATANA_UPDATE_OPENSSL_INCLUDE NAMES openssl/evp.h)
    if(NOT KATANA_UPDATE_CRYPTO_STATIC OR NOT KATANA_UPDATE_ZLIB_STATIC OR NOT KATANA_UPDATE_OPENSSL_INCLUDE)
        message(FATAL_ERROR
            "The update library needs OpenSSL and zlib as STATIC libraries on Windows "
            "(libcrypto.a, libz.a and openssl/evp.h; MSYS2: mingw-w64-<toolchain>-openssl, "
            "-zlib), so that the helper program imports nothing but the operating system's "
            "DLLs. Found: crypto '${KATANA_UPDATE_CRYPTO_STATIC}', zlib '${KATANA_UPDATE_ZLIB_STATIC}', "
            "headers '${KATANA_UPDATE_OPENSSL_INCLUDE}'.")
    endif()
    add_library(katana_update_third_party INTERFACE)
    target_include_directories(katana_update_third_party SYSTEM INTERFACE "${KATANA_UPDATE_OPENSSL_INCLUDE}")
    target_link_libraries(katana_update_third_party INTERFACE
        "${KATANA_UPDATE_CRYPTO_STATIC}" ws2_32 crypt32 "${KATANA_UPDATE_ZLIB_STATIC}")
else()
    find_package(OpenSSL REQUIRED)
    find_package(ZLIB REQUIRED)
    add_library(katana_update_third_party INTERFACE)
    target_link_libraries(katana_update_third_party INTERFACE OpenSSL::Crypto ZLIB::ZLIB)
endif()
find_package(nlohmann_json CONFIG REQUIRED)
target_link_libraries(katana_update
    PUBLIC katana_core
    PRIVATE katana_update_third_party nlohmann_json::nlohmann_json)
# For not_implemented.hpp, which the files under src/katana_updater share.
target_include_directories(katana_update PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

# ---- what is compiled in ----------------------------------------------------------------

# One place a version is written: project(Katana VERSION ...). katana_cli has no
# KATANA_VERSION of its own, so the library carries one, and update::programVersion()
# is how any program learns what it is.
target_compile_definitions(katana_update PRIVATE
    KATANA_VERSION="${PROJECT_VERSION}"
    KATANA_UPDATE_REPO="${KATANA_UPDATE_REPO}"
    KATANA_UPDATE_FEED="${KATANA_UPDATE_FEED}")

# The pinned keys reach trust.cpp ALONE, so changing a key recompiles one file and no
# other file can read them (cmake/KatanaUpdateKeys.cmake; trust.hpp).
if(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS)
    set(_katana_update_placeholder_flag 1)
else()
    set(_katana_update_placeholder_flag 0)
endif()
set_source_files_properties(trust.cpp PROPERTIES COMPILE_DEFINITIONS
    "KATANA_UPDATE_KEY_ACTIVE=\"${KATANA_UPDATE_KEY_ACTIVE}\";KATANA_UPDATE_KEY_SPARE=\"${KATANA_UPDATE_KEY_SPARE}\";KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS=${_katana_update_placeholder_flag}")
