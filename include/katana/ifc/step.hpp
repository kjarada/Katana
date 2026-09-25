#pragma once

// The ISO 10303-21 ("STEP physical file") encodings an IFC file is written in,
// and the IFC globally unique identifier. Public so that the tests can check
// each against a worked example; the writer (export.hpp) is their one user.
//
// Everything here is deterministic (Rule 7): the same value gives the same
// text on every platform, which is what lets two exports of an unchanged
// drawing be compared byte for byte.

#include <cstdint>
#include <string>
#include <string_view>

namespace katana::ifc {

// The 22-character IfcGloballyUniqueId of a 128-bit value, `high` holding its
// first eight bytes. The encoding is the one the IFC documentation gives for
// IfcGloballyUniqueId: the first byte as two characters (so the first is
// always 0 to 3), then each following three bytes as four, from the alphabet
// 0-9 A-Z a-z _ $.
[[nodiscard]] std::string compressGuid(std::uint64_t high, std::uint64_t low);

// A GlobalId for the object `key` names inside `space`, the same for the same
// two strings every time. `space` is the project (ExportOptions::
// guidNamespace), `key` what the object is in Katana - "entity/42",
// "alignment/MC01", "utility/W-0001/segment/2" - so re-exporting a project
// gives each object the GlobalId it had before, which is how an IFC
// consumer recognises a changed object rather than a deleted one and a new
// one. The 128 bits are two independent 64-bit FNV-1a hashes, each through
// the SplitMix64 finaliser: not cryptographic, and not meant to be - the
// chance of two of a million keys colliding is below 1e-26.
[[nodiscard]] std::string guidFor(std::string_view space, std::string_view key);

// A string literal: quoted, a quote doubled, a backslash doubled, and every
// character outside printable ASCII written as \X2\hhhh\X0\ (UTF-16, for the
// Basic Multilingual Plane) or \X4\hhhhhhhh\X0\ (beyond it), the encodings
// ISO 10303-21 gives for them. Invalid UTF-8 is written as U+FFFD, the
// replacement character, rather than passed through as bytes a reader would
// have to guess at.
[[nodiscard]] std::string stepString(std::string_view utf8);

// A REAL: the shortest text that reads back as the same double, with the
// decimal point the format requires ("1." not "1", "1.E-05" not "1e-05"),
// and never "-0.". A non-finite value has no STEP form; the writer checks
// for one before it gets here (see StepFile::real).
[[nodiscard]] std::string stepReal(double value);

// How many characters `utf8` has, for the IfcLabel and IfcIdentifier limit
// of 255 (the schema's STRING(255) counts characters, not bytes).
[[nodiscard]] std::size_t characterCount(std::string_view utf8);

} // namespace katana::ifc
