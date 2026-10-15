# The Ed25519 public keys the update system trusts, and the check that a build
# that is to be RELEASED does not still hold placeholders.
#
#   include(KatanaUpdateKeys)                 from the top CMakeLists.txt
#   cmake -DKATANA_REQUIRE_UPDATE_KEYS=ON -P cmake/KatanaUpdateKeys.cmake
#                                             the same check, as a script (tests/update)
#
# Two keys are pinned, `active` and `spare` (docs/update.md): either verifies a
# release's manifest, and CI signs with the active one. Public keys are
# public data: this file is committed. The private halves are never in the
# repository (*.pem and update-signing* are git-ignored) and never in an agent's
# hands; the owner generates them (tools/update_keytool.py --help).
#
# ======================================================================================
# The keys below are the owner's real public keys, pinned on 2026-10-14 (745606c4).
# Before that they were PLACEHOLDERS: each 32 bytes that spell "PLACEHOLDER-..." in ASCII
# and are NOT the encoding of any point on the curve, so no private key exists that
# matches either, and a build that carries them verifies nothing: update/trust.hpp makes
# verifyManifest refuse. It cannot be updated, and it cannot be forged for. The tests
# still use such keys, as fixtures.
#
# KATANA_REQUIRE_UPDATE_KEYS (OFF by default; ON in .github/workflows/release.yml) makes
# the configure step FAIL while placeholders are in place, so a release can never ship
# with them.
#
# To replace the keys (the owner; docs/update.md, "What the owner does", has the sequence):
#   openssl genpkey -algorithm ed25519 -out update-signing.pem     (and update-spare.pem)
#   python tools/update_keytool.py pub update-signing.pem update-spare.pem
# which rewrites ONLY the block between the two marker lines below.
# ======================================================================================

# The one policy this file needs, said here and not left to whoever runs it. It is included
# from a configure, where the root CMakeLists.txt has set the policies of its minimum, and it
# is run as a script (the second line of the header), which starts with none set: a CMake older
# than 4.0 then does not know `if(<value> IN_LIST <list>)` below and stops at it (seen with
# CMake 3.31.6, which the release workflow's first job runs under). A policy set here ends with
# the file, so the includer's are not changed. tests/update/check_keys_policy.cmake holds it.
cmake_policy(SET CMP0057 NEW)

# >>> keys: rewritten by tools/update_keytool.py between these two lines, and nowhere else
set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS OFF)
set(KATANA_UPDATE_KEY_ACTIVE "4b5436f13549eee7555b471eb9e6461e9cdea8e62032f415ff6df38f272d0d73")
set(KATANA_UPDATE_KEY_SPARE "f5f20acdd524798aab2f07b803d2c0129b4c0decb37ca954e5d651a805037dfc")
# <<< keys

# Tests only: a script run of this file may take the keys from a fixture, so that the
# refusals below are proved on keys that are not the ones in the tree
# (tests/update/key_fixtures). A configure never reads it.
if(CMAKE_SCRIPT_MODE_FILE AND KATANA_UPDATE_KEYS_FIXTURE)
    include("${KATANA_UPDATE_KEYS_FIXTURE}")
endif()

# What the placeholders spell: "PLACEHOLDER" in ASCII.
set(_katana_placeholder_prefix "504c414345484f4c444552")

foreach(_katana_which IN ITEMS ACTIVE SPARE)
    set(_katana_key "${KATANA_UPDATE_KEY_${_katana_which}}")
    string(LENGTH "${_katana_key}" _katana_key_length)
    if(NOT _katana_key MATCHES "^[0-9a-f]+$" OR NOT _katana_key_length EQUAL 64)
        message(FATAL_ERROR
            "KATANA_UPDATE_KEY_${_katana_which} must be 64 lower-case hexadecimal digits (an Ed25519 "
            "public key), not '${_katana_key}'. Do not edit cmake/KatanaUpdateKeys.cmake by hand: "
            "python tools/update_keytool.py pub writes it.")
    endif()
endforeach()
# The public halves of RFC 8032 section 7.1, TEST 1, 2 and 3: published, so anyone holds the private
# halves and can sign an update that a build pinning one would accept. The same list as
# TEST_PUBLIC_KEYS in tools/update_make_manifest.py (which update_keytool.py refuses them by); a
# test holds the two to the RFC's own values. Refused whatever KATANA_REQUIRE_UPDATE_KEYS says: no
# build, release or not, has a reason to pin them (the tests pass a test trust root as a
# parameter, update/trust.hpp), and a hand edit or a tool run with --allow-test-keys on the
# tree's own file is how one would get in.
set(_katana_published_test_keys
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c"
    "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025")
foreach(_katana_which IN ITEMS ACTIVE SPARE)
    if("${KATANA_UPDATE_KEY_${_katana_which}}" IN_LIST _katana_published_test_keys)
        message(FATAL_ERROR
            "KATANA_UPDATE_KEY_${_katana_which} is a published RFC 8032 test key. Anyone holds its "
            "private half and can sign an update that this build would accept. Generate real keys "
            "(docs/update.md, the owner's steps) and run python tools/update_keytool.py pub.")
    endif()
endforeach()
if(KATANA_UPDATE_KEY_ACTIVE STREQUAL KATANA_UPDATE_KEY_SPARE)
    message(FATAL_ERROR
        "the spare key must be a different key from the active key: a spare that is the active key "
        "is no way to recover from losing it")
endif()

# The flag and the bytes must agree, in both directions. A flag that says OFF over bytes
# that spell PLACEHOLDER would let a release through with keys nobody holds; one that says
# ON over real keys would refuse a release for no reason anyone could find.
set(_katana_spells_placeholder OFF)
if(KATANA_UPDATE_KEY_ACTIVE MATCHES "^${_katana_placeholder_prefix}"
   OR KATANA_UPDATE_KEY_SPARE MATCHES "^${_katana_placeholder_prefix}")
    set(_katana_spells_placeholder ON)
endif()
if(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS AND NOT _katana_spells_placeholder)
    message(FATAL_ERROR
        "KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS says the keys are placeholders, but they do not spell "
        "PLACEHOLDER. Run python tools/update_keytool.py pub, which sets both together.")
elseif(NOT KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS AND _katana_spells_placeholder)
    message(FATAL_ERROR
        "KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS says the keys are real, but they spell PLACEHOLDER. "
        "A key that spells that is no key anyone holds.")
endif()

if(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS)
    if(KATANA_REQUIRE_UPDATE_KEYS)
        message(FATAL_ERROR
            "KATANA_REQUIRE_UPDATE_KEYS is ON, but cmake/KatanaUpdateKeys.cmake still holds the "
            "PLACEHOLDER update keys. A release built with them could never be updated and would "
            "carry keys nobody holds. Generate the keys and run tools/update_keytool.py pub "
            "(docs/update.md, the owner's steps), then commit the file.")
    endif()
    message(STATUS "Update keys: PLACEHOLDERS - this build can neither verify nor receive an update "
                   "(KATANA_REQUIRE_UPDATE_KEYS=ON refuses it; docs/update.md has the owner's steps)")
else()
    string(SUBSTRING "${KATANA_UPDATE_KEY_ACTIVE}" 0 16 _katana_active_id)
    string(SUBSTRING "${KATANA_UPDATE_KEY_SPARE}" 0 16 _katana_spare_id)
    message(STATUS "Update keys: pinned, active ${_katana_active_id}, spare ${_katana_spare_id}")
endif()
