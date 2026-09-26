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
    // The workspace (docs/cad.md). The window buttons on every panel's and
    // view's title bar are drawn in neutral only: they act on the window, not
    // on the drawing, and an accent there would compete with the active view's
    // accent marker for the eye.
    Minimise,
    Float,
    Dock,
    Maximise,
    Restore,
    Close,
    // The panels without an icon of their own until now.
    CommandLine,
    ReferenceData,
    // What a view shows: the kind switcher on its title bar.
    ViewPlan,
    View3D,
    ViewSection,
    ViewElevation,
    // A view's Layers button when that view hides something: the Layers
    // glyph beside the accent funnel of a filter, so a filtered view is never
    // mistaken for missing data.
    ViewLayersFiltered,
    // The compact tool buttons of the Layers and Reference Data panels.
    LayerNew,
    LayerNewChild,
    Rename,
    ZoomTo,
    // Help
    Help,
    About,
    // Survey (PLAN.MD 45 slice 9), the Survey menu's tools. Survey marks are
    // rings with a centre dot, control stations triangles, and a thin north
    // line with its arrowhead where a direction is measured FROM north. The
    // accent is what the tool computes: the measured line and its angle, the
    // new point, the area, the adjusted legs, the sight line, the conversion.
    SurveyInverse,
    SurveyForward,
    SurveyArea,
    SurveyAngle,
    SurveyTraverse,
    SurveyLevelBook,
    SurveyConverter,
    // Survey points (PLAN.MD 45 slices 10, 11 and 13): the import wizard and
    // the export, a page and a mark with the accent arrow running the way the
    // points go; the Point Manager's table; the Point Report's page.
    SurveyImport,
    SurveyExport,
    SurveyPointManager,
    SurveyPointReport,
    // Format (the customisation managers). Styles and Linetypes: three lines
    // as a linetype table shows them, the accent one dashed with a symbol at
    // its vertex. Symbol Library: a grid of plan symbols, the accent one the
    // symbol chosen. Survey Code Manager: a field code's tag and the accent
    // linework it becomes. Purge Unused: a broom, sweeping the accent away.
    // Global Modify: a point, a line and an area gathered by the accent
    // bracket, and the accent arrow into the one look they are all given.
    FormatStyles,
    FormatSymbols,
    FormatSurveyCodes,
    Purge,
    GlobalModify,
    // The menu items that had no icon until 2026-09-26 (MainWindow::menuGaps
    // and --check-menus find any item still without one). File and Edit: a
    // script with a clock, a door with the way out, a selection cleared, a
    // selection by number, a view copied as a picture, an attribute tree.
    RecentScripts,
    Quit,
    Deselect,
    SelectById,
    CopyViewImage,
    Attributes,
    // The snap modes, each as the plan view marks it (viewport_widget.cpp,
    // drawSnapMarker) in the accent on the geometry it snaps to: a square at
    // an end, a triangle at a middle, a ring at a centre, a cross where two
    // lines meet, a right angle, a ring on a tangent, an hourglass on a
    // curve, a crosshair on the grid.
    SnapEndpoint,
    SnapMidpoint,
    SnapCenter,
    SnapIntersection,
    SnapPerpendicular,
    SnapTangent,
    SnapNearest,
    SnapGrid,
    ThinLines,
    // The viewport layouts (cad::layoutRects), the first view - the main one
    // - in the accent.
    LayoutSingle,
    LayoutSplitVertical,
    LayoutSplitHorizontal,
    LayoutThreeLeft,
    LayoutThreeTop,
    LayoutQuad,
    // The standard 3D views. An orthographic view is the object in plan with
    // the accent on the face seen and an arrow from where the eye is; Top is
    // that face filled, Bottom the same face dashed, seen through. An
    // isometric view is the cube with the accent arrow from its corner.
    ViewTop,
    ViewBottom,
    ViewFront,
    ViewBack,
    ViewLeft,
    ViewRight,
    ViewIsoSouthWest,
    ViewIsoSouthEast,
    ViewIsoNorthEast,
    ViewIsoNorthWest,
    Perspective,
    VerticalExaggeration,
    // The window: its panels, its toolbars, its text size, its layout put
    // back.
    Panels,
    Toolbars,
    TextSize,
    ResetLayout,
    // Annotation's editors and managers beside the tools, and the Format
    // menu's annotation styles.
    EditText,
    EditLabel,
    LabelLayout,
    LeaderManager,
    LeadersForSelection,
    ArrangeLeaders,
    TextStyles,
    LabelStyles,
    DimensionStyles,
    // The Layers panel's column heads: shown (an eye), locked (a padlock),
    // colour (swatches).
    LayerVisible,
    LayerLocked,
    LayerColour,
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
