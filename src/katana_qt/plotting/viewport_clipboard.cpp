#include "plotting/viewport_clipboard.hpp"

#include <QClipboard>
#include <QGuiApplication>
#include <QMimeData>

#include "katana/cad/plotting/sheet_json.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

Result<QByteArray> viewportsToClipboardBytes(const std::vector<plotting::Viewport>& viewports)
{
    if (viewports.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no viewports to copy");
    }
    plotting::SheetSet holder;
    plotting::Sheet sheet;
    sheet.id = "clipboard";
    sheet.viewports = viewports;
    holder.sheets.push_back(std::move(sheet));
    auto json = plotting::sheetSetToJson(holder);
    if (!json) {
        return json.error();
    }
    return QByteArray::fromStdString(*json);
}

Result<std::vector<plotting::Viewport>> viewportsFromClipboardBytes(const QByteArray& bytes)
{
    auto set = plotting::sheetSetFromJson(bytes.toStdString());
    if (!set) {
        return set.error();
    }
    if (set->sheets.size() != 1 || set->sheets.front().viewports.empty()) {
        return makeError(ErrorCode::ParseFailure, "the clipboard holds no viewports");
    }
    return std::move(set->sheets.front().viewports);
}

Status copyViewportsToClipboard(const std::vector<plotting::Viewport>& viewports)
{
    auto bytes = viewportsToClipboardBytes(viewports);
    if (!bytes) {
        return bytes.error();
    }
    QClipboard* clipboard = QGuiApplication::clipboard();
    if (clipboard == nullptr) {
        return makeError(ErrorCode::InvalidState, "there is no clipboard");
    }
    auto* mime = new QMimeData();
    mime->setData(QString::fromLatin1(kViewportMimeType), *bytes);
    clipboard->setMimeData(mime); // the clipboard owns it
    return {};
}

Result<std::vector<plotting::Viewport>> viewportsOnClipboard(const QMimeData* mime)
{
    if (mime == nullptr) {
        if (const QClipboard* clipboard = QGuiApplication::clipboard()) {
            mime = clipboard->mimeData();
        }
    }
    const QString type = QString::fromLatin1(kViewportMimeType);
    if (mime == nullptr || !mime->hasFormat(type)) {
        return makeError(ErrorCode::NotFound, "nothing to paste: copy a view first");
    }
    return viewportsFromClipboardBytes(mime->data(type));
}

} // namespace katana::qt
