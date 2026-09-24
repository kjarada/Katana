#pragma once

// The small encodings DXF uses for colour, lineweight and text, in one place
// so that the reader and the writer cannot disagree about them - and exposed
// so that tests check them against values worked out by hand.

#include <optional>
#include <string>
#include <string_view>

#include "katana/entity/entity.hpp"

namespace katana::dxf {

// ---- colour ----------------------------------------------------------------

// Group 62 values that are not colours.
inline constexpr int kColourByBlock = 0;
inline constexpr int kColourByLayer = 256;
// Index 7 is the drawing's foreground: white on a dark background, black on a
// light one. Katana draws it white.
inline constexpr int kColourForeground = 7;

// The RGB of indexed colour 1-255. nullopt for 0 (ByBlock), 256 (ByLayer) and
// anything outside the range.
//
// 1-9 are the named colours (red, yellow, green, cyan, blue, magenta, white,
// grey 128, grey 192), 250-255 a grey ramp, and 10-249 twenty-four hues 15
// degrees apart, each at five brightnesses (255, 189, 129, 104, 79), full and
// then a third saturated. The hues are computed rather than tabulated: the
// published table rounds some of them a unit or two differently, which no
// screen shows and which cannot move a colour to a different index.
[[nodiscard]] std::optional<katana::entity::Color> indexedColour(int index);

// The index whose RGB is nearest `colour` (squared distance; the lower index
// on a tie). Black is the foreground, 7, since no index is black and the
// nearest dark red would be a surprise.
[[nodiscard]] int nearestIndexedColour(const katana::entity::Color& colour);

// Group 420: 0x00RRGGBB.
[[nodiscard]] katana::entity::Color trueColour(long value);

// ---- lineweight --------------------------------------------------------------

// A layer's or entity's group 370 is one of a fixed set of hundredths of a
// millimetre (0, 5, 9, 13 ... 211), or -1 ByLayer, -2 ByBlock, -3 default.
// The nearest of the set to `millimetres`.
[[nodiscard]] int nearestLineweight(double millimetres);

// ---- text --------------------------------------------------------------------

// A TEXT string as it reads: %%d, %%p and %%c are the degree, plus-minus and
// diameter signs, %%% a percent sign, %%nnn the character with that code,
// %%u, %%o and %%k (underline, overline, strike-through toggles) nothing, and
// \U+XXXX the character it names.
[[nodiscard]] std::string plainText(std::string_view raw);

// An MTEXT string as it reads. \P (and \N, a column break) end a paragraph and
// become a line break; \~ is a no-break space; \\ \{ \} are the characters;
// \S1/2; (a stacked fraction) is "1/2", with ^ and # read as /; the font,
// height, colour, width, tracking, slant, alignment and paragraph codes
// (\f \F \H \C \c \W \T \Q \A \p ... up to their ';') and the toggles
// (\L \l \O \o \K \k) are dropped; a brace that is not escaped groups and is
// dropped. The TEXT codes above apply too.
[[nodiscard]] std::string plainMText(std::string_view raw);

// The inverse of plainText for a string going into a file: ASCII as it is,
// the three signs as %%d %%p %%c, a percent sign that another follows as %%%,
// every other character as \U+XXXX (and one beyond the basic plane as '?',
// which R2000 has no escape for). Control characters become spaces.
[[nodiscard]] std::string encodeText(std::string_view utf8);

} // namespace katana::dxf
