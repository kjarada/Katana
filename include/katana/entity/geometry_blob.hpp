#pragma once

// Binary encoding of entity geometry (PLAN.MD Phase 07, and the decision
// recorded in docs/storage.md).
//
// WHY. Opening a 50 000-entity project cost 891 ms, of which 602 ms - 68% -
// was parsing the JSON that geometry was stored as, against only 289 ms for
// SQLite and rebuilding the model. An eight-vertex polyline is 128 bytes of
// doubles; as JSON it is around 350 bytes of text that has to be lexed,
// unescaped and converted decimal to binary on every load. That measurement is
// also why the database itself was left alone: changing it would have competed
// for the smaller third.
//
// FORMAT. Little-endian, length-prefixed, and versioned in its first byte so a
// later change can be migrated rather than guessed at:
//
//     u8   format version (kBlobVersion)
//     u8   geometry kind  (the Geometry variant index, i.e. EntityType)
//     ...  payload, per kind, all doubles in IEEE-754 little-endian
//
// Strings are u32 length then bytes, and are still required to be valid UTF-8
// - the same rule the JSON path enforces - so that a blob can be re-encoded as
// JSON for an export without the writer throwing.
//
// WHAT IT IS NOT. This is a storage format, not a wire protocol: it carries no
// checksum, because SQLite already protects the page it lives in, and no
// compression, because the geometry is mostly incompressible doubles and
// PLAN.MD section 33 says to measure before adding machinery.
//
// EXACTNESS. Doubles are stored by their bit pattern, so a round trip is bit
// exact - including negative zero, infinities and NaN payloads. That is
// required by Rule 7: a saved drawing must reload as the same drawing, not as
// one that agrees to fifteen digits.

#include <cstddef>
#include <cstdint>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"

namespace katana::entity {

// Bumped only when the layout changes. Readers accept every version they know
// how to read; writers always emit the newest.
inline constexpr std::uint8_t kBlobVersion = 1;

// Encodes `geometry`. Fails with InvalidArgument for text that is not valid
// UTF-8 or a string longer than the format can express - both of which are
// already rejected on the way into the model, so reaching either means a guard
// was bypassed rather than that a user typed something odd.
[[nodiscard]] katana::core::Result<std::vector<std::byte>>
geometryToBlob(const Geometry& geometry);

// Decodes a blob produced by geometryToBlob.
//
// Fails with ParseFailure for anything malformed: an unknown version or kind,
// a truncated payload, a length that runs past the end, a string that is not
// valid UTF-8, or trailing bytes after a complete record. Trailing bytes are
// an error rather than ignored - they mean the writer and reader disagree
// about the layout, and continuing would decode later fields wrongly.
//
// This runs on data from disk, which may be damaged or may have been written
// by a different version, so every length is checked against what remains
// before it is used. Nothing here can read out of bounds.
[[nodiscard]] katana::core::Result<Geometry> geometryFromBlob(std::span<const std::byte> blob);

} // namespace katana::entity
