#pragma once

// Editing a project's sheets (docs/plotting.md). Every function here is ONE
// undoable step: it builds the new sheet set from the document's and hands it
// to Document::setSheetSet, which records the set before and after. So undo
// and redo are exact, a failed edit changes nothing, and there is one door
// in - the same arrangement as every other edit of the drawing.

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"

namespace katana::cad::plotting {

// Appends `sheets` - typically a generator's output - renumbering their ids
// so they join the set without clashing (prepareForAppend).
[[nodiscard]] core::Status addSheets(Document& document, std::vector<Sheet> sheets,
                                     std::string stepName = "ADD_SHEETS");
// Inserts one sheet before `at` (at the end when not given).
[[nodiscard]] core::Status addSheet(Document& document, Sheet sheet,
                                    std::optional<std::size_t> at = std::nullopt);
// Removes a sheet. The last one can go too: an empty set is a valid set.
[[nodiscard]] core::Status removeSheet(Document& document, std::size_t index);
// Moves the sheet at `from` so that it ends up at `to`.
[[nodiscard]] core::Status moveSheet(Document& document, std::size_t from, std::size_t to);
// A deep copy placed after the original, named "<name> (copy)", with new
// sheet and viewport ids and none of the original's per-sheet sheet number.
[[nodiscard]] core::Status duplicateSheet(Document& document, std::size_t index);
// Applies `edit` to a copy of sheet `index` and stores the result as one
// step. The edit may refuse by returning an error; nothing changes then.
[[nodiscard]] core::Status editSheet(Document& document, std::size_t index,
                                     const std::function<core::Status(Sheet&)>& edit,
                                     std::string stepName = "EDIT_SHEET");
// The same for the viewport with id `viewportId`, wherever it is.
[[nodiscard]] core::Status editViewport(Document& document, std::string_view viewportId,
                                        const std::function<core::Status(Viewport&)>& edit,
                                        std::string stepName = "EDIT_VIEWPORT");
// The same for the whole set: the shared title-block values, the numbering,
// the revisions.
[[nodiscard]] core::Status editSheetSet(Document& document,
                                        const std::function<core::Status(SheetSet&)>& edit,
                                        std::string stepName = "EDIT_SHEETS");

// The largest logo file accepted: a logo is drawn 53 x 12 mm, which at 600 dpi
// is 1250 x 283 pixels - a few hundred kilobytes at most. A bigger file is a
// photograph, and would ride along in every PDF of the set.
inline constexpr std::uintmax_t kMaximumLogoBytes = 4u * 1024u * 1024u;

// Copies `image` (PNG, JPEG, GIF or BMP, recognised by its content, at most
// kMaximumLogoBytes) into the project's assets/ directory and makes it the
// set's logo, as one undoable step; returns the name it is stored under. The
// file stays in assets/ after an undo - it is the name that is undone - so a
// redo finds it. InvalidState for a drawing not saved to a project yet (the
// logo is kept with the project); Unsupported for another kind of file.
[[nodiscard]] core::Result<std::string> importLogo(Document& document,
                                                   const std::filesystem::path& image);

// The copy importLogo makes, for any image a sheet shows (an Image view's
// file): `image` checked as above against `maximumBytes` and copied into the
// project's assets/ directory under a name of its own, the same image found
// and reused; returns that name. `what` names the image in the errors ("a
// logo must be..."). Not an edit: nothing refers to the file until the caller
// stores its name, as one undoable step.
[[nodiscard]] core::Result<std::string>
importImageAsset(const Document& document, const std::filesystem::path& image,
                 std::uintmax_t maximumBytes = kMaximumLogoBytes, std::string_view what = "logo");

// Where the set's logo is on disk, when it has one and the project is saved.
[[nodiscard]] std::optional<std::filesystem::path> logoPath(const Document& document);

// The project's contribution to the title-block fields: its name,
// description, coordinate system and file name, with `plotDate` as given.
[[nodiscard]] FieldContext fieldContextFor(const Document& document, std::string plotDate);

// A date as the frame prints one: dd/mm/yy.
[[nodiscard]] std::string frameDate(std::chrono::year_month_day date);

} // namespace katana::cad::plotting
