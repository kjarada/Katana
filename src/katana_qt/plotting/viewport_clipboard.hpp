#pragma once

// Viewports on the system clipboard (docs/plotting.md, "Editing on the
// canvas"). A copy is stored as a sheet set's JSON (sheet_json.hpp) holding
// one sheet of the copied viewports, under a MIME type of its own: the form a
// project stores its sheets in, so a paste reads back exactly what was
// copied - in the same project, in another project, or in another Katana
// running beside this one.

#include <vector>

#include <QByteArray>

#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"

class QMimeData;

namespace katana::qt {

// The MIME type copied viewports travel under.
inline constexpr char kViewportMimeType[] = "application/x-katana-viewports";

// `viewports` as the clipboard carries them. InvalidArgument when there are
// none, or for what JSON cannot hold (sheetSetToJson).
[[nodiscard]] katana::core::Result<QByteArray>
viewportsToClipboardBytes(const std::vector<katana::cad::plotting::Viewport>& viewports);
// And back. ParseFailure for bytes that are not copied viewports.
[[nodiscard]] katana::core::Result<std::vector<katana::cad::plotting::Viewport>>
viewportsFromClipboardBytes(const QByteArray& bytes);

// Puts `viewports` on the system clipboard.
[[nodiscard]] katana::core::Status
copyViewportsToClipboard(const std::vector<katana::cad::plotting::Viewport>& viewports);
// The viewports on `mime`, or on the system clipboard when null; NotFound
// when it holds none.
[[nodiscard]] katana::core::Result<std::vector<katana::cad::plotting::Viewport>>
viewportsOnClipboard(const QMimeData* mime = nullptr);

} // namespace katana::qt
