#pragma once

// The application's icons, drawn in code.
//
// Every icon is a small vector drawing on a 24-unit grid, painted by a
// QIconEngine at whatever size and device-pixel ratio Qt asks for. There are
// no image files behind them, deliberately:
//
//   * They are crisp at every size and on every HiDPI scale, because nothing
//     is ever resampled - a 20 px toolbar icon and a 40 px one at 200% are
//     both painted directly.
//   * They follow the theme. The neutral strokes take the palette's text
//     colour and the accent takes theme::accent(), so changing the theme
//     recolours every icon; a bitmap set would need redrawing per theme.
//   * They cannot go missing. A resource path that is wrong at run time is a
//     blank button; a function that is wrong does not compile.
//   * The toolchain here has no Qt SVG module and no image tools, so an SVG
//     or PNG set would have been a dependency added for decoration.
//
// The visual language is one thing, applied everywhere: a 1.7-unit stroke with
// round caps and joins, neutral for the object and ACCENT for the part the
// command acts on - the arrow of Import, the new geometry of a draw tool, the
// cut line of a section. An icon that needs a sentence to explain belongs in a
// menu, not on a toolbar.
//
// The application icon (the katana on its tile) is painted by the same code,
// which is what `katana_make_icons` writes out as resources/katana.ico for
// the executable's Windows resource.

#include <QIcon>
#include <QImage>

#include <vector>

class QPainter;
class QRectF;

namespace katana::qt {

enum class Icon {
    // File
    New,
    Open,
    Save,
    SaveAs,
    Import,
    Export,
    Plot,
    // Edit
    Undo,
    Redo,
    Erase,
    SelectAll,
    // Draw
    Select,
    Point,
    Line,
    Polyline,
    Rectangle,
    Circle,
    Arc,
    // Modify
    Move,
    Copy,
    // View
    ZoomExtents,
    Grid,
    Snap,
    Layers,
    Properties,
    // Terrain and civil
    SurfaceFromCloud,
    SurfaceFromRaster,
    SurfaceFromDrawing,
    Section,
    SectionAlignment,
    CorridorQuantities,
    CorridorSurface,
    // GIS: GDAL and PDAL data. One glyph per kind of data - a polygon with
    // its vertices, a grid of cells, a scatter of points - with the accent
    // arrow of Import and Export, so the family reads as one.
    ImportVector,
    ImportRaster,
    ImportPointCloud,
    ExportPointCloud,
    ExportDem,
    ConvertCopc,
    DatasetInfo,
    // Help
    Help,
    About,
};

// Every value of Icon, in declaration order, for the contact sheet and for
// the test that every icon paints something.
[[nodiscard]] const std::vector<Icon>& allIcons();

// A QIcon backed by the painting code. Cheap to copy; paints on demand.
[[nodiscard]] QIcon icon(Icon which);

// Paints `which` into `rect` with the given colours. Exposed so that the
// contact sheet and the tests can paint without going through QIcon.
void paintIcon(QPainter& painter, Icon which, const QRectF& rect, const QColor& neutral,
               const QColor& accent);

// The application icon at `size` pixels square, on a transparent ground.
[[nodiscard]] QImage applicationIconImage(int size);
[[nodiscard]] QIcon applicationIcon();

} // namespace katana::qt
