#pragma once

// Numbers as the desktop application shows them to a person. One definition,
// so the command log, the panels and the GIS dialogs group digits alike.

#include <QLocale>
#include <QString>

#include <cstdint>

namespace katana::qt {

// Thousands grouped in the user's locale: "2,000,000" or "2 000 000".
[[nodiscard]] inline QString grouped(std::uint64_t value)
{
    return QLocale().toString(static_cast<qulonglong>(value));
}

} // namespace katana::qt
