#pragma once

// DIMSTYLE's fields as the command line types them (docs/annotation.md,
// "Dimension styles"): the one statement of what each field is called, how
// its value is read and how it is written back. The interpreter's DIMSTYLE
// reads its NEW and SET lines with it and writes INFO's record with it, and
// the window's Dimension Styles manager builds its lines with it - so the
// dialog cannot write a line the verb would read differently.
//
//   TEXT GAP EXTOFF EXTBEYOND ARROW   the sizes: model units, or paper mm for a
//                                     paper-sized style
//   HEAD                              None | Tick | ClosedFilled | Open | Dot
//   SCALE DECIMALS ROUND              the unit factor, the places, the step
//   PREFIX SUFFIX                     text before and after the number
//   TRIM PAPER                        on | off
//
// Field names are case-insensitive; a line gives them in upper case and a
// record in lower case, as the other annotation verbs' keys are.

#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad::annotation {

// The fields, upper case, in the order a line and a record give them.
[[nodiscard]] const std::vector<std::string_view>& dimensionStyleFields();

// Sets one field of `style` from its typed value. InvalidArgument for a field
// that does not exist (naming the fields that do), ParseFailure for a value
// that does not read - a number, a head's name, on or off. Whether the value
// is allowed (a text height of zero) is validate()'s, which the table command
// applies.
[[nodiscard]] katana::core::Status setDimensionStyleField(katana::entity::DimensionStyle& style,
                                                          std::string_view field,
                                                          std::string_view value);

// `words` as "FIELD value FIELD value ...", applied in order onto `style`, ALL
// OR NOTHING: on any failure `style` is as it was. InvalidArgument for an odd
// count, which would leave a field with no value.
[[nodiscard]] katana::core::Status
setDimensionStyleFields(katana::entity::DimensionStyle& style, std::span<const std::string> words);

// One field's value as a line and a record give it: numbers exact
// (core::formatExactReal, so 0.625 reads back as 0.625), the head by name,
// on or off, a prefix or suffix as it is. Empty for an unknown field.
[[nodiscard]] std::string dimensionStyleFieldValue(const katana::entity::DimensionStyle& style,
                                                   std::string_view field);

// The "FIELD value" pairs that turn `from` into `to`, each value one word of a
// command line (command_words.hpp), in field order, separated by blanks:
// what a SET line needs to carry. Empty when the two agree field for field
// (the name is not a field). InvalidArgument when a prefix or suffix that
// differs holds a double quote, which no command line can carry.
[[nodiscard]] katana::core::Result<std::string>
dimensionStyleChanges(const katana::entity::DimensionStyle& from,
                      const katana::entity::DimensionStyle& to);

// The layers that name the style, ascending - what DIMSTYLE DELETE is refused
// for. A layer that names none is drawn with the default style but does not
// name it, and is not counted.
[[nodiscard]] std::vector<std::string> layersUsingDimensionStyle(const katana::entity::Model& model,
                                                                 std::string_view name);

// DIMSTYLE INFO's record: name=, every field in lower case (key=value, a value
// quoted when it holds a blank or is empty), layers=N (layersUsingDimensionStyle)
// and reads= - what a dimension of ten units reads as, the question anyone
// setting a style is asking.
[[nodiscard]] std::string describeDimensionStyle(const katana::entity::Model& model,
                                                 const katana::entity::DimensionStyle& style);

} // namespace katana::cad::annotation
