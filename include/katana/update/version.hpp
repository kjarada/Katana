#pragma once

// Versions as the release workflow writes them (.github/workflows/release.yml
// reads project(Katana VERSION) with sed on digits and dots only): MAJOR.MINOR,
// MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH.BUILD, each component digits.
//
// Compared component by component as integers, never as text ("0.10" is
// newer than "0.9"). A missing trailing component is zero, so "0.3" and
// "0.3.0" are the same version; how many were written is kept only so that
// toString gives the text back with as many components (a leading zero is not kept).
//
// There is no pre-release syntax ("-beta") on purpose: the workflow cannot
// produce one, and a version the updater cannot order is one it must not offer.

#include <array>
#include <compare>
#include <cstdint>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::update {

struct Version {
    std::array<std::uint32_t, 4> components{};
    std::uint8_t written = 2; // 2 to 4: how many components the text had

    [[nodiscard]] friend bool operator==(const Version& a, const Version& b)
    {
        return a.components == b.components;
    }
    [[nodiscard]] friend std::strong_ordering operator<=>(const Version& a, const Version& b)
    {
        return a.components <=> b.components;
    }
};

// 2 to 4 components separated by single dots, each 1 to 9 ASCII digits
// (so it fits 32 bits), no sign, no space, no empty component, leading zeros
// read as decimal ("1.02" is 1.2). ParseFailure otherwise; the context is the
// text. Version text comes from a signed manifest AND from this program's own
// build, so it is held to one rule.
[[nodiscard]] core::Result<Version> parseVersion(std::string_view text);

// The text as written: `written` components, joined by dots.
[[nodiscard]] std::string toString(const Version& version);

// The version this program was built as: project(Katana VERSION ...), passed
// to the update library as KATANA_VERSION (src/katana_update/CMakeLists.txt).
// katana_cli has no definition of its own, which is why the library carries
// one. parseVersion of it cannot fail in a correct build; a failure is
// Internal.
[[nodiscard]] std::string_view programVersionText();
[[nodiscard]] core::Result<Version> programVersion();

} // namespace katana::update
