// STUB. Task T5 writes productionTrust(), verifyManifest() and isNewerThan()
// against include/katana/update/trust.hpp.
//
// The pinned keys reach THIS FILE ALONE as compile definitions
// (src/katana_update/CMakeLists.txt, from cmake/KatanaUpdateKeys.cmake), so a
// change of key recompiles one file. The definitions are in place now and the
// asserts below hold them to their shape at compile time; T5 turns the two hex
// strings into the Trust.
//
//   KATANA_UPDATE_KEY_ACTIVE, KATANA_UPDATE_KEY_SPARE
//        64 lower-case hexadecimal digits each (an Ed25519 public key)
//   KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS
//        1 while cmake/KatanaUpdateKeys.cmake still holds the placeholders, else 0

#include "katana/update/trust.hpp"

#include "not_implemented.hpp"

#if !defined(KATANA_UPDATE_KEY_ACTIVE) || !defined(KATANA_UPDATE_KEY_SPARE) || \
    !defined(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS)
#error "the update keys are defined for trust.cpp by src/katana_update/CMakeLists.txt"
#endif

namespace katana::update {

namespace {

constexpr std::string_view kPinnedActive = KATANA_UPDATE_KEY_ACTIVE;
constexpr std::string_view kPinnedSpare = KATANA_UPDATE_KEY_SPARE;

static_assert(kPinnedActive.size() == 64, "KATANA_UPDATE_KEY_ACTIVE is 64 hexadecimal digits");
static_assert(kPinnedSpare.size() == 64, "KATANA_UPDATE_KEY_SPARE is 64 hexadecimal digits");
static_assert(kPinnedActive != kPinnedSpare, "the active and the spare key are two keys");
static_assert(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS == 0 || KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS == 1,
              "KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS is 0 or 1");

} // namespace

const Trust& productionTrust()
{
    detail::notYet("productionTrust", "T5");
}

core::Result<Manifest> verifyManifest(std::string_view, std::string_view, const Trust&,
                                      const Install&)
{
    return detail::notImplemented("verifyManifest", "T5");
}

bool isNewerThan(const Manifest&, const Install&)
{
    detail::notYet("isNewerThan", "T5");
}

} // namespace katana::update
