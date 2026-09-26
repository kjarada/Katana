#pragma once

// Numbers as the desktop application shows them to a person. One definition,
// so the command log, the panels and the GIS dialogs group digits alike.

#include <QLocale>
#include <QString>

#include <cstdint>

#include "katana/geometry/primitives2d.hpp"

namespace katana::qt {

// A coordinate, a length or an angle as the status bar and the Properties
// panel show it: four places, in the C locale, so a value copied from the
// panel types back on the command line.
[[nodiscard]] inline QString planNumber(double value) { return QString::number(value, 'f', 4); }

// "x, y" at four places: the cursor's readout and a vertex in the panel.
[[nodiscard]] inline QString planPoint(const katana::geometry::Point2& p)
{
    return planNumber(p.x) + ", " + planNumber(p.y);
}

// Thousands grouped in the user's locale: "2,000,000" or "2 000 000".
[[nodiscard]] inline QString grouped(std::uint64_t value)
{
    return QLocale().toString(static_cast<qulonglong>(value));
}

} // namespace katana::qt
