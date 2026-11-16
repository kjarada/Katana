//! The application's icons, drawn in code.
//!
//! Every icon is a small vector drawing on a 24-unit grid, painted through the canvas at whatever
//! size and device-pixel ratio it is shown at. There are no image files behind them,
//! deliberately:
//!
//! ```text
//! * They are crisp at every size and on every HiDPI scale, because nothing is ever resampled -
//!   a 20 px toolbar icon and a 40 px one at 200% are both painted directly.
//! * They follow the theme. The two tones are the palette's colours (icon_colours), so changing
//!   the theme recolours every icon; a bitmap set would need redrawing per theme.
//! * They cannot go missing. A resource path that is wrong at run time is a blank button; a
//!   function that is wrong does not compile.
//! ```
//!
//! The visual language is one thing, applied everywhere (the design system of 2026-11-16): line
//! drawings on the 24-unit grid with round caps and joins, in two weights only - STROKE, 1.5 px
//! at a toolbar's 20 px, and FINE, 1 px, for detail inside a shape - held at those pixels at the
//! smaller sizes a menu (16 px) and a title bar (14 px) show (tool_icons::weight_at). Areas take
//! one light tint (TINT) under their outline, never a solid fill; solid marks are only the small
//! ones a drawing has - a node, a dot, an arrowhead. No gradients, no shading.
//!
//! Two tones. Each mark is drawn in the QUIET tone (the object) or the BRIGHT one (the part the
//! command acts on - the arrow of Import, the new geometry of a draw tool, the cut line of a
//! section); the code calls the bright one "accent", its name from before. Which colours the two
//! tones are is the mode's (icon_colours): at rest the secondary text colour and the text colour,
//! so a toolbar is quiet and its icons still say what each command acts on; ACTIVE - the tool
//! that runs, the toggle that is on - the text colour and the accent, the one place the accent
//! is; disabled, both the disabled text colour. Until 2026-11-16 every icon's bright part was the
//! accent blue at rest, and a toolbar of them was a field of blue in which the running tool did
//! not stand out. An icon that needs a sentence to explain belongs in a menu, not on a toolbar.
//!
//! The application icon (the katana on its tile) is painted by the same code, which is what
//! `katana_make_icons` writes out as resources/katana.ico for the executable's Windows resource.
//!
//! THE ICON SET BY NAME. An action row names its icon (drive::Action::icon, a `&'static str`):
//! an `Icon` by its C++ enumerator's name ("Import", "SurveyInverse"; `Icon::name`), or a
//! catalogue tool by its id ("draw.circle"; tools::tool_icons). A tool id always holds a '.', an
//! icon's name never does, so the two never meet. `paint_named_icon` paints either.
//!
//! THE CACHE (decided here, the plan's section 4.4). The C++ QIcon painted on demand at each size
//! Qt asked for (QIconEngine::pixmap). Here an icon is painted once for each size in device pixels
//! and mode it is shown at, through the raster canvas, and kept as an egui texture under its
//! name, size and mode, in the egui context's own store (`install` puts the rasteriser there, and
//! `texture` paints on first use): a texture egui keeps is what a frame draws, and the soft
//! renderer of a headless run finds it among the frame's textures. The same store keeps the tool
//! buttons' painted faces (tools::flyout_button), each keyed by everything that draws it. A
//! context without a rasteriser draws no icons, and says so by answering None.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::bim::road_object_icons::paint_road_object_icon;
use crate::canvas::{
    Brush, Canvas, Color, Image, Path, Pen, PenCap, PenJoin, PenStyle, PointF, RectF, RenderHint,
    Transform,
};
use crate::theme::palette;
use crate::tools::icons_terrain::draw_tin;
use crate::tools::tool_icons::{
    FINE, GRID, STROKE, TINT, ToolInk, arc, circle, paint_tool_icon_weighted, polyline, pt,
    rectangle, start, weight_at,
};

/// The application's icons, in their declaration order (icons.hpp).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Icon {
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
    // Geoprocessing (docs/geoprocessing.md): data in, through a run, to a
    // result - the one icon the geoprocessing tools share until they have
    // their own.
    Processing,
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
    // A view's Link button (cad/view_link.hpp): an open chain, its two links
    // apart, while the view pans and zooms on its own; a closed chain in the
    // accent while it moves with the views linked to it.
    ViewUnlinked,
    ViewLinked,
    // A view's zoom tools: the lens of Zoom To with the accent plus and minus,
    // and the lens over the dashed accent box of a selection.
    ZoomIn,
    ZoomOut,
    ZoomSelection,
    // File > Settings: three sliders, each track neutral and its knob the
    // accent - what is set. Not a gear: at a menu's 16 px its teeth close up
    // into a ring, and a ring is already a survey mark here.
    Settings,
    // Survey > Process Linework: three surveyed marks, and in the accent the
    // line the run strings through them.
    SurveyLinework,
    // ---- volumes (docs/volumes.md) ----
    // Terrain > Volumes: a block of ground, its top face in the accent - the
    // quantity between two surfaces.
    Volumes,
    // The Volumes dialog's Make Cut/Fill Map: a map split along the line
    // where cut meets fill, the fill side in the accent.
    CutFillMap,
    // ---- end of volumes ----
    // ---- road objects (docs/road_objects.md) ----
    // BIM > Road Objects: each object as it stands beside a road, the rail,
    // the reflector, the plate, the lantern, the pipe, the wall's section,
    // the wires and the marks in the accent; and the manager's list. Painted
    // in bim/road_object_icons.cpp.
    RoadBarrier,
    RoadGuidePosts,
    RoadSign,
    RoadLightPole,
    RoadCulvert,
    RoadWall,
    RoadFence,
    RoadLineMarking,
    RoadObjects,
    // ---- end road objects ----
    // ---- surface editing (docs/surface_editing.md) ----
    // Terrain > Edit Surface: a small TIN, one vertex in the accent - the
    // surface as a thing to work on.
    SurfaceEdit,
    // Raise, Lower or Set Level: a ground line and, in the accent, the
    // up-and-down arrow of a change of level.
    SurfaceLevels,
    // Smooth: a jagged line, and in the accent the smooth curve through it.
    SurfaceSmooth,
    // Thin: a row of vertices, the middle ones in the accent struck out.
    SurfaceThin,
    // Delete Long Triangles: a long thin triangle on the rim, struck through
    // in the accent.
    SurfaceDeleteLong,
    // Combine Surfaces: a ground outline, and in the accent a pad laid into it.
    SurfaceCombine,
    // ---- the drawing's surfaces (docs/terrain.md, "Surfaces in the drawing") ----
    // The Surfaces panel: a patch of triangulated ground, one triangle in the
    // accent - a surface picked out of the drawing's.
    SurfacesPanel,
    // ---- road model (docs/road_design.md) ----
    // BIM > Road Template Manager: a road's cross-section - the crowned
    // finished surface in the accent over a course, a kerb at each edge.
    RoadTemplate,
    // BIM > Corridors: two kerb lines curving away together, the centreline
    // dashed in the accent between them.
    Corridor,
    // BIM > Show Road Model: a block seen from above a corner, its top face
    // in the accent - a solid, which is what the road model is made of.
    RoadModel,
    // ---- end road model ----
    // ---- surface display (docs/surface_display.md) ----
    // Terrain > Surface Styles: a patch of ground cut into bands of height,
    // the middle band filled in the accent - what a style does to a
    // surface's faces - under its outline and one triangle's edge.
    SurfaceStyles,
    // Help > Check for Updates: a cloud, and in the accent the arrow that
    // brings the new version down out of it. Not Import's tray and arrow:
    // that is a file coming into the drawing, and this is the program itself.
    Download,
    // ---- icons of their own (the look of 2026-11-16, round two) ----
    // Items that shared another's icon - two printers side by side on the File
    // toolbar, three Properties sliders in File's Project section, Import's and
    // Export's trays three times each - each given its own, in the same line
    // language: what the item works on in the quiet tone, what it does in the
    // bright one.
    // File > Sheets: a stack of sheets, the front one's title block bright.
    Sheets,
    // File > Plot Sheets to PDF: Plot's printer, a stack of pages coming out.
    PlotSheets,
    // File > Drawing Summary: a clipboard, its rows' marks bright.
    DrawingSummary,
    // File > Project Coordinate System: a north and an east axis, a point and its
    // coordinates bright.
    ProjectCrs,
    // File > Start Screen: a window, its first tile bright.
    StartScreen,
    // Help > Command Reference: a page, the command line's prompt bright on it.
    CommandReference,
    // Help > Keyboard Shortcuts: a keyboard, its space bar bright.
    Keyboard,
    // File > Import IFC and Export IFC: a building element (a block in
    // isometric), with Import's and Export's arrow down the right as the GIS
    // data's icons have it.
    ImportIfc,
    ExportIfc,
    // File > Export View as Image: a picture, and Export's arrow.
    ExportImage,
    // Survey > Apply Survey Codes: a code's tag, and the bright arrow to the
    // point it is applied to.
    ApplyCodes,
    // Survey > Parcel Report: the parcel, and the report's page beside it (Area of
    // Selection keeps the parcel alone).
    SurveyParcelReport,
    // ---- one icon an item (the look's fifth round, the third audit's G) ----
    // GIS's eleven processing items, Terrain's Analysis and DEM items and the Reference Data
    // panel's Build Overviews shared Processing's two boxes; View > Toolbars' icon sizes and Text
    // Size's four items one icon a menu; four of Edit Surface's items another item's. A menu whose
    // items look alike is read word by word, so each draws what it does, in the same language:
    // the data in the quiet tone, what the command makes of it in the bright one.
    // GIS > Formats: a file's page and, bright, the tag of its format.
    GisFormats,
    // GIS > Buffer: a line with its nodes, and the bright band at a distance round it.
    GisBuffer,
    // GIS > Dissolve: two areas, the edge between them dashed away, their one outline bright.
    GisDissolve,
    // GIS > Overlay: two areas over each other, the part they share bright.
    GisOverlay,
    // GIS > Boundary Around Features: a scatter of points and the bright boundary round them.
    GisHull,
    // GIS > Clip to Boundary: an area and a dashed frame, the part inside the frame bright.
    GisClip,
    // GIS > Query with SQL: a table and the bright mark of a question.
    GisSql,
    // GIS > Check Geometry: an area and the bright tick of a check.
    GisCheck,
    // GIS > Repair Geometry: a bow-tie, the invalid area, and the bright wrench mending it.
    GisRepair,
    // GIS > Gaps and Overlaps: two areas and the bright sliver of the gap between them.
    GisCoverage,
    // Terrain > Analysis > Contours: three closed contours, the middle one bright.
    TerrainContours,
    // Terrain Shading: a hill under the bright sun, its far flank hatched in shade.
    TerrainShading,
    // Slope and Aspect: a slope and, bright, the angle it makes.
    TerrainSlope,
    // Statistics by Area: an area and, bright, the bars of what is counted inside it.
    TerrainZonal,
    // Drape and Sample Heights: points above the ground and the bright drops onto it.
    TerrainDrape,
    // Viewshed and Line of Sight: an eye, the ground and the bright fan of what it sees.
    TerrainViewshed,
    // Terrain > DEM > Grid Points to DEM: scattered points and the bright grid made of them.
    GridDem,
    // DEM Tools: a grid of cells and the bright wrench that works on it.
    DemTools,
    // Reference Data > Build Overviews: a raster's levels, smaller up a pyramid, the top bright.
    BuildOverviews,
    // View > Toolbars > Small, Standard and Large Icons: a toolbar with its first button bright,
    // at each size.
    ToolbarIconsSmall,
    ToolbarIconsStandard,
    ToolbarIconsLarge,
    // View > Text Size: a bright A at each of the four sizes over a quiet baseline (the
    // submenu keeps TextSize's two letters).
    TextSizeSmall,
    TextSizeStandard,
    TextSizeLarge,
    TextSizeExtraLarge,
    // Terrain > Edit Surface's items that act by a scope, on the Surface Edit tools' fine TIN
    // (tools::icons_terrain): points inside a dashed polygon struck out; triangles inside a
    // polygon lit; points of the drawing added; a breakline dashed away.
    SurfaceDeletePoints,
    SurfaceDeleteTriangles,
    SurfaceAddPoints,
    SurfaceDeleteBreakline,
    // GIS > Online Data: a globe, the web's data, and Import's arrow down the right. It had Import
    // Raster's grid, the item above it in the GIS menu.
    OnlineData,
}

/// Every value of Icon, in declaration order, for the contact sheet and for the test that every
/// icon paints something.
pub const ALL_ICONS: [Icon; 197] = [
    Icon::New,
    Icon::Open,
    Icon::Save,
    Icon::SaveAs,
    Icon::Import,
    Icon::Export,
    Icon::Plot,
    Icon::Undo,
    Icon::Redo,
    Icon::Erase,
    Icon::SelectAll,
    Icon::Select,
    Icon::Point,
    Icon::Line,
    Icon::Polyline,
    Icon::Rectangle,
    Icon::Circle,
    Icon::Arc,
    Icon::Move,
    Icon::Copy,
    Icon::ZoomExtents,
    Icon::Grid,
    Icon::Snap,
    Icon::Layers,
    Icon::Properties,
    Icon::SurfaceFromCloud,
    Icon::SurfaceFromRaster,
    Icon::SurfaceFromDrawing,
    Icon::Section,
    Icon::SectionAlignment,
    Icon::CorridorQuantities,
    Icon::CorridorSurface,
    Icon::ImportVector,
    Icon::ImportRaster,
    Icon::ImportPointCloud,
    Icon::ExportPointCloud,
    Icon::ExportDem,
    Icon::ConvertCopc,
    Icon::DatasetInfo,
    Icon::Processing,
    Icon::Minimise,
    Icon::Float,
    Icon::Dock,
    Icon::Maximise,
    Icon::Restore,
    Icon::Close,
    Icon::CommandLine,
    Icon::ReferenceData,
    Icon::ViewPlan,
    Icon::View3D,
    Icon::ViewSection,
    Icon::ViewElevation,
    Icon::ViewLayersFiltered,
    Icon::LayerNew,
    Icon::LayerNewChild,
    Icon::Rename,
    Icon::ZoomTo,
    Icon::Help,
    Icon::About,
    Icon::SurveyInverse,
    Icon::SurveyForward,
    Icon::SurveyArea,
    Icon::SurveyAngle,
    Icon::SurveyTraverse,
    Icon::SurveyLevelBook,
    Icon::SurveyConverter,
    Icon::SurveyImport,
    Icon::SurveyExport,
    Icon::SurveyPointManager,
    Icon::SurveyPointReport,
    Icon::FormatStyles,
    Icon::FormatSymbols,
    Icon::FormatSurveyCodes,
    Icon::Purge,
    Icon::GlobalModify,
    Icon::RecentScripts,
    Icon::Quit,
    Icon::Deselect,
    Icon::SelectById,
    Icon::CopyViewImage,
    Icon::Attributes,
    Icon::SnapEndpoint,
    Icon::SnapMidpoint,
    Icon::SnapCenter,
    Icon::SnapIntersection,
    Icon::SnapPerpendicular,
    Icon::SnapTangent,
    Icon::SnapNearest,
    Icon::SnapGrid,
    Icon::ThinLines,
    Icon::LayoutSingle,
    Icon::LayoutSplitVertical,
    Icon::LayoutSplitHorizontal,
    Icon::LayoutThreeLeft,
    Icon::LayoutThreeTop,
    Icon::LayoutQuad,
    Icon::ViewTop,
    Icon::ViewBottom,
    Icon::ViewFront,
    Icon::ViewBack,
    Icon::ViewLeft,
    Icon::ViewRight,
    Icon::ViewIsoSouthWest,
    Icon::ViewIsoSouthEast,
    Icon::ViewIsoNorthEast,
    Icon::ViewIsoNorthWest,
    Icon::Perspective,
    Icon::VerticalExaggeration,
    Icon::Panels,
    Icon::Toolbars,
    Icon::TextSize,
    Icon::ResetLayout,
    Icon::EditText,
    Icon::EditLabel,
    Icon::LabelLayout,
    Icon::LeaderManager,
    Icon::LeadersForSelection,
    Icon::ArrangeLeaders,
    Icon::TextStyles,
    Icon::LabelStyles,
    Icon::DimensionStyles,
    Icon::LayerVisible,
    Icon::LayerLocked,
    Icon::LayerColour,
    Icon::ViewUnlinked,
    Icon::ViewLinked,
    Icon::ZoomIn,
    Icon::ZoomOut,
    Icon::ZoomSelection,
    Icon::Settings,
    Icon::SurveyLinework,
    Icon::Volumes,
    Icon::CutFillMap,
    Icon::RoadBarrier,
    Icon::RoadGuidePosts,
    Icon::RoadSign,
    Icon::RoadLightPole,
    Icon::RoadCulvert,
    Icon::RoadWall,
    Icon::RoadFence,
    Icon::RoadLineMarking,
    Icon::RoadObjects,
    Icon::SurfaceEdit,
    Icon::SurfaceLevels,
    Icon::SurfaceSmooth,
    Icon::SurfaceThin,
    Icon::SurfaceDeleteLong,
    Icon::SurfaceCombine,
    Icon::SurfacesPanel,
    Icon::RoadTemplate,
    Icon::Corridor,
    Icon::RoadModel,
    Icon::SurfaceStyles,
    Icon::Download,
    Icon::Sheets,
    Icon::PlotSheets,
    Icon::DrawingSummary,
    Icon::ProjectCrs,
    Icon::StartScreen,
    Icon::CommandReference,
    Icon::Keyboard,
    Icon::ImportIfc,
    Icon::ExportIfc,
    Icon::ExportImage,
    Icon::ApplyCodes,
    Icon::SurveyParcelReport,
    Icon::GisFormats,
    Icon::GisBuffer,
    Icon::GisDissolve,
    Icon::GisOverlay,
    Icon::GisHull,
    Icon::GisClip,
    Icon::GisSql,
    Icon::GisCheck,
    Icon::GisRepair,
    Icon::GisCoverage,
    Icon::TerrainContours,
    Icon::TerrainShading,
    Icon::TerrainSlope,
    Icon::TerrainZonal,
    Icon::TerrainDrape,
    Icon::TerrainViewshed,
    Icon::GridDem,
    Icon::DemTools,
    Icon::BuildOverviews,
    Icon::ToolbarIconsSmall,
    Icon::ToolbarIconsStandard,
    Icon::ToolbarIconsLarge,
    Icon::TextSizeSmall,
    Icon::TextSizeStandard,
    Icon::TextSizeLarge,
    Icon::TextSizeExtraLarge,
    Icon::SurfaceDeletePoints,
    Icon::SurfaceDeleteTriangles,
    Icon::SurfaceAddPoints,
    Icon::SurfaceDeleteBreakline,
    Icon::OnlineData,
];

/// Every value of Icon, in declaration order (allIcons).
pub fn all_icons() -> &'static [Icon] {
    &ALL_ICONS
}

impl Icon {
    /// The icon's name in the window's icon set: the C++ enumerator's.
    pub const fn name(self) -> &'static str {
        match self {
            Self::New => "New",
            Self::Open => "Open",
            Self::Save => "Save",
            Self::SaveAs => "SaveAs",
            Self::Import => "Import",
            Self::Export => "Export",
            Self::Plot => "Plot",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Erase => "Erase",
            Self::SelectAll => "SelectAll",
            Self::Select => "Select",
            Self::Point => "Point",
            Self::Line => "Line",
            Self::Polyline => "Polyline",
            Self::Rectangle => "Rectangle",
            Self::Circle => "Circle",
            Self::Arc => "Arc",
            Self::Move => "Move",
            Self::Copy => "Copy",
            Self::ZoomExtents => "ZoomExtents",
            Self::Grid => "Grid",
            Self::Snap => "Snap",
            Self::Layers => "Layers",
            Self::Properties => "Properties",
            Self::SurfaceFromCloud => "SurfaceFromCloud",
            Self::SurfaceFromRaster => "SurfaceFromRaster",
            Self::SurfaceFromDrawing => "SurfaceFromDrawing",
            Self::Section => "Section",
            Self::SectionAlignment => "SectionAlignment",
            Self::CorridorQuantities => "CorridorQuantities",
            Self::CorridorSurface => "CorridorSurface",
            Self::ImportVector => "ImportVector",
            Self::ImportRaster => "ImportRaster",
            Self::ImportPointCloud => "ImportPointCloud",
            Self::ExportPointCloud => "ExportPointCloud",
            Self::ExportDem => "ExportDem",
            Self::ConvertCopc => "ConvertCopc",
            Self::DatasetInfo => "DatasetInfo",
            Self::Processing => "Processing",
            Self::Minimise => "Minimise",
            Self::Float => "Float",
            Self::Dock => "Dock",
            Self::Maximise => "Maximise",
            Self::Restore => "Restore",
            Self::Close => "Close",
            Self::CommandLine => "CommandLine",
            Self::ReferenceData => "ReferenceData",
            Self::ViewPlan => "ViewPlan",
            Self::View3D => "View3D",
            Self::ViewSection => "ViewSection",
            Self::ViewElevation => "ViewElevation",
            Self::ViewLayersFiltered => "ViewLayersFiltered",
            Self::LayerNew => "LayerNew",
            Self::LayerNewChild => "LayerNewChild",
            Self::Rename => "Rename",
            Self::ZoomTo => "ZoomTo",
            Self::Help => "Help",
            Self::About => "About",
            Self::SurveyInverse => "SurveyInverse",
            Self::SurveyForward => "SurveyForward",
            Self::SurveyArea => "SurveyArea",
            Self::SurveyAngle => "SurveyAngle",
            Self::SurveyTraverse => "SurveyTraverse",
            Self::SurveyLevelBook => "SurveyLevelBook",
            Self::SurveyConverter => "SurveyConverter",
            Self::SurveyImport => "SurveyImport",
            Self::SurveyExport => "SurveyExport",
            Self::SurveyPointManager => "SurveyPointManager",
            Self::SurveyPointReport => "SurveyPointReport",
            Self::FormatStyles => "FormatStyles",
            Self::FormatSymbols => "FormatSymbols",
            Self::FormatSurveyCodes => "FormatSurveyCodes",
            Self::Purge => "Purge",
            Self::GlobalModify => "GlobalModify",
            Self::RecentScripts => "RecentScripts",
            Self::Quit => "Quit",
            Self::Deselect => "Deselect",
            Self::SelectById => "SelectById",
            Self::CopyViewImage => "CopyViewImage",
            Self::Attributes => "Attributes",
            Self::SnapEndpoint => "SnapEndpoint",
            Self::SnapMidpoint => "SnapMidpoint",
            Self::SnapCenter => "SnapCenter",
            Self::SnapIntersection => "SnapIntersection",
            Self::SnapPerpendicular => "SnapPerpendicular",
            Self::SnapTangent => "SnapTangent",
            Self::SnapNearest => "SnapNearest",
            Self::SnapGrid => "SnapGrid",
            Self::ThinLines => "ThinLines",
            Self::LayoutSingle => "LayoutSingle",
            Self::LayoutSplitVertical => "LayoutSplitVertical",
            Self::LayoutSplitHorizontal => "LayoutSplitHorizontal",
            Self::LayoutThreeLeft => "LayoutThreeLeft",
            Self::LayoutThreeTop => "LayoutThreeTop",
            Self::LayoutQuad => "LayoutQuad",
            Self::ViewTop => "ViewTop",
            Self::ViewBottom => "ViewBottom",
            Self::ViewFront => "ViewFront",
            Self::ViewBack => "ViewBack",
            Self::ViewLeft => "ViewLeft",
            Self::ViewRight => "ViewRight",
            Self::ViewIsoSouthWest => "ViewIsoSouthWest",
            Self::ViewIsoSouthEast => "ViewIsoSouthEast",
            Self::ViewIsoNorthEast => "ViewIsoNorthEast",
            Self::ViewIsoNorthWest => "ViewIsoNorthWest",
            Self::Perspective => "Perspective",
            Self::VerticalExaggeration => "VerticalExaggeration",
            Self::Panels => "Panels",
            Self::Toolbars => "Toolbars",
            Self::TextSize => "TextSize",
            Self::ResetLayout => "ResetLayout",
            Self::EditText => "EditText",
            Self::EditLabel => "EditLabel",
            Self::LabelLayout => "LabelLayout",
            Self::LeaderManager => "LeaderManager",
            Self::LeadersForSelection => "LeadersForSelection",
            Self::ArrangeLeaders => "ArrangeLeaders",
            Self::TextStyles => "TextStyles",
            Self::LabelStyles => "LabelStyles",
            Self::DimensionStyles => "DimensionStyles",
            Self::LayerVisible => "LayerVisible",
            Self::LayerLocked => "LayerLocked",
            Self::LayerColour => "LayerColour",
            Self::ViewUnlinked => "ViewUnlinked",
            Self::ViewLinked => "ViewLinked",
            Self::ZoomIn => "ZoomIn",
            Self::ZoomOut => "ZoomOut",
            Self::ZoomSelection => "ZoomSelection",
            Self::Settings => "Settings",
            Self::SurveyLinework => "SurveyLinework",
            Self::Volumes => "Volumes",
            Self::CutFillMap => "CutFillMap",
            Self::RoadBarrier => "RoadBarrier",
            Self::RoadGuidePosts => "RoadGuidePosts",
            Self::RoadSign => "RoadSign",
            Self::RoadLightPole => "RoadLightPole",
            Self::RoadCulvert => "RoadCulvert",
            Self::RoadWall => "RoadWall",
            Self::RoadFence => "RoadFence",
            Self::RoadLineMarking => "RoadLineMarking",
            Self::RoadObjects => "RoadObjects",
            Self::SurfaceEdit => "SurfaceEdit",
            Self::SurfaceLevels => "SurfaceLevels",
            Self::SurfaceSmooth => "SurfaceSmooth",
            Self::SurfaceThin => "SurfaceThin",
            Self::SurfaceDeleteLong => "SurfaceDeleteLong",
            Self::SurfaceCombine => "SurfaceCombine",
            Self::SurfacesPanel => "SurfacesPanel",
            Self::RoadTemplate => "RoadTemplate",
            Self::Corridor => "Corridor",
            Self::RoadModel => "RoadModel",
            Self::SurfaceStyles => "SurfaceStyles",
            Self::Download => "Download",
            Self::Sheets => "Sheets",
            Self::PlotSheets => "PlotSheets",
            Self::DrawingSummary => "DrawingSummary",
            Self::ProjectCrs => "ProjectCrs",
            Self::StartScreen => "StartScreen",
            Self::CommandReference => "CommandReference",
            Self::Keyboard => "Keyboard",
            Self::ImportIfc => "ImportIfc",
            Self::ExportIfc => "ExportIfc",
            Self::ExportImage => "ExportImage",
            Self::ApplyCodes => "ApplyCodes",
            Self::SurveyParcelReport => "SurveyParcelReport",
            Self::GisFormats => "GisFormats",
            Self::GisBuffer => "GisBuffer",
            Self::GisDissolve => "GisDissolve",
            Self::GisOverlay => "GisOverlay",
            Self::GisHull => "GisHull",
            Self::GisClip => "GisClip",
            Self::GisSql => "GisSql",
            Self::GisCheck => "GisCheck",
            Self::GisRepair => "GisRepair",
            Self::GisCoverage => "GisCoverage",
            Self::TerrainContours => "TerrainContours",
            Self::TerrainShading => "TerrainShading",
            Self::TerrainSlope => "TerrainSlope",
            Self::TerrainZonal => "TerrainZonal",
            Self::TerrainDrape => "TerrainDrape",
            Self::TerrainViewshed => "TerrainViewshed",
            Self::GridDem => "GridDem",
            Self::DemTools => "DemTools",
            Self::BuildOverviews => "BuildOverviews",
            Self::ToolbarIconsSmall => "ToolbarIconsSmall",
            Self::ToolbarIconsStandard => "ToolbarIconsStandard",
            Self::ToolbarIconsLarge => "ToolbarIconsLarge",
            Self::TextSizeSmall => "TextSizeSmall",
            Self::TextSizeStandard => "TextSizeStandard",
            Self::TextSizeLarge => "TextSizeLarge",
            Self::TextSizeExtraLarge => "TextSizeExtraLarge",
            Self::SurfaceDeletePoints => "SurfaceDeletePoints",
            Self::SurfaceDeleteTriangles => "SurfaceDeleteTriangles",
            Self::SurfaceAddPoints => "SurfaceAddPoints",
            Self::SurfaceDeleteBreakline => "SurfaceDeleteBreakline",
            Self::OnlineData => "OnlineData",
        }
    }

    /// The icon named `name`; None for any other text.
    pub fn from_name(name: &str) -> Option<Self> {
        ALL_ICONS.into_iter().find(|icon| icon.name() == name)
    }
}

// ---- the marks icons are built from ------------------------------------------------------------

/// A rectangle, its corners rounded by `radius` when it has one.
fn rounded(x: f64, y: f64, w: f64, h: f64, radius: f64) -> Path {
    let mut path = Path::new();
    if radius > 0.0 {
        path.add_rounded_rect(RectF::new(x, y, w, h), radius, radius);
    } else {
        path.add_rect(RectF::new(x, y, w, h));
    }
    path
}

/// The floppy disk shared by Save and Save As.
fn draw_disk(ink: &mut ToolInk<'_>, with_label: bool) {
    ink.stroke(
        &polyline(
            &[
                pt(4.0, 4.0),
                pt(17.0, 4.0),
                pt(20.0, 7.0),
                pt(20.0, 20.0),
                pt(4.0, 20.0),
            ],
            true,
        ),
        false,
        STROKE,
    );
    ink.stroke(
        &polyline(
            &[pt(8.0, 4.0), pt(8.0, 9.0), pt(15.0, 9.0), pt(15.0, 4.0)],
            false,
        ),
        false,
        STROKE,
    );
    if with_label {
        ink.stroke(
            &polyline(
                &[pt(7.0, 20.0), pt(7.0, 13.5), pt(17.0, 13.5), pt(17.0, 20.0)],
                false,
            ),
            true,
            STROKE,
        );
    }
}

/// The tray shared by Import and Export.
fn draw_tray(ink: &mut ToolInk<'_>) {
    ink.stroke(
        &polyline(
            &[pt(4.0, 14.0), pt(4.0, 20.0), pt(20.0, 20.0), pt(20.0, 14.0)],
            false,
        ),
        false,
        STROKE,
    );
}

// The three kinds of GIS data, drawn in the lower-left 15 units so that an arrow fits down the
// right-hand side. Neutral: the data is the object, the arrow is what the command does to it.

fn draw_vector_glyph(ink: &mut ToolInk<'_>) {
    let corners = [
        pt(3.0, 20.0),
        pt(3.0, 11.0),
        pt(9.0, 7.5),
        pt(14.5, 12.5),
        pt(12.0, 20.0),
    ];
    ink.stroke(&polyline(&corners, true), false, STROKE);
    for p in corners {
        ink.node(p.x, p.y, false);
    }
}

fn draw_raster_glyph(ink: &mut ToolInk<'_>) {
    ink.stroke(&rectangle(3.0, 8.0, 12.0, 12.0), false, STROKE);
    ink.line(7.0, 8.0, 7.0, 20.0, false, FINE);
    ink.line(11.0, 8.0, 11.0, 20.0, false, FINE);
    ink.line(3.0, 12.0, 15.0, 12.0, false, FINE);
    ink.line(3.0, 16.0, 15.0, 16.0, false, FINE);
    ink.fill(&rectangle(3.0, 8.0, 4.0, 4.0), false, TINT);
    ink.fill(&rectangle(7.0, 12.0, 4.0, 4.0), false, TINT);
    ink.fill(&rectangle(11.0, 16.0, 4.0, 4.0), false, TINT);
}

fn draw_cloud_glyph(ink: &mut ToolInk<'_>) {
    for p in [
        pt(4.0, 19.5),
        pt(8.0, 20.0),
        pt(12.5, 19.0),
        pt(5.5, 15.5),
        pt(10.0, 15.5),
        pt(14.0, 14.5),
        pt(4.0, 11.5),
        pt(8.5, 11.0),
        pt(12.0, 9.0),
    ] {
        ink.dot(p.x, p.y, 1.3, false);
    }
}

/// The down arrow of an import, down the right.
fn draw_arrow_in(ink: &mut ToolInk<'_>) {
    ink.line(18.5, 3.0, 18.5, 12.5, true, STROKE);
    ink.stroke(
        &polyline(&[pt(15.5, 9.5), pt(18.5, 12.5), pt(21.5, 9.5)], false),
        true,
        STROKE,
    );
}

/// The up arrow of an export, down the right.
fn draw_arrow_out(ink: &mut ToolInk<'_>) {
    ink.line(18.5, 12.5, 18.5, 3.0, true, STROKE);
    ink.stroke(
        &polyline(&[pt(15.5, 6.0), pt(18.5, 3.0), pt(21.5, 6.0)], false),
        true,
        STROKE,
    );
}

// ---- survey marks ------------------------------------------------------------------------------

/// A survey point: a ring with a dot at its centre, as a mark is drawn on a plan. In the accent
/// for a point the command creates.
fn draw_mark(ink: &mut ToolInk<'_>, x: f64, y: f64, accent: bool) {
    ink.stroke(&circle(x, y, 2.2), accent, FINE);
    ink.dot(x, y, 0.8, accent);
}

/// A control or traverse station: a filled triangle, the usual plan symbol.
fn draw_station(ink: &mut ToolInk<'_>, x: f64, y: f64) {
    ink.fill(
        &polyline(
            &[pt(x, y - 2.8), pt(x + 2.5, y + 1.6), pt(x - 2.5, y + 1.6)],
            true,
        ),
        false,
        255,
    );
}

/// Grid north above a mark: a thin line from `from` up to `to` with its head.
fn draw_north(ink: &mut ToolInk<'_>, x: f64, from: f64, to: f64) {
    ink.line(x, from, x, to, false, FINE);
    ink.stroke(
        &polyline(
            &[pt(x - 1.7, to + 2.5), pt(x, to), pt(x + 1.7, to + 2.5)],
            false,
        ),
        false,
        FINE,
    );
}

/// The curved arrow of Undo; Redo is its mirror image.
fn draw_turn(ink: &mut ToolInk<'_>, mirrored: bool) {
    ink.canvas().save();
    if mirrored {
        ink.canvas().translate(GRID, 0.0);
        ink.canvas().scale(-1.0, 1.0);
    }
    let mut sweep = start(5.5, 9.0);
    sweep.line_to(pt(14.0, 9.0));
    sweep.cubic_to(pt(17.5, 9.0), pt(19.5, 11.5), pt(19.5, 14.2));
    sweep.cubic_to(pt(19.5, 17.2), pt(17.2, 19.5), pt(14.0, 19.5));
    sweep.line_to(pt(10.0, 19.5));
    ink.stroke(&sweep, false, STROKE);
    ink.stroke(
        &polyline(&[pt(9.5, 5.0), pt(5.5, 9.0), pt(9.5, 13.0)], false),
        true,
        STROKE,
    );
    ink.canvas().restore();
}

// ---- the menu items' marks ---------------------------------------------------------------------

/// The window every layout, panel and toolbar icon is drawn in.
fn draw_frame(ink: &mut ToolInk<'_>) {
    ink.stroke(&rounded(2.5, 4.0, 19.0, 16.0, 1.5), false, STROKE);
}

/// A pencil in the accent at the lower right: the icon edits what it is on.
fn draw_pencil(ink: &mut ToolInk<'_>) {
    ink.stroke(
        &polyline(
            &[
                pt(12.5, 21.5),
                pt(13.2, 18.6),
                pt(19.4, 12.4),
                pt(21.6, 14.6),
                pt(15.4, 20.8),
            ],
            true,
        ),
        true,
        STROKE,
    );
}

/// A label: a rounded note with its words, and its leader down to what it labels.
fn draw_note(ink: &mut ToolInk<'_>, x: f64, y: f64, accent: bool) {
    ink.stroke(&rounded(x, y, 11.0, 7.0, 1.5), accent, STROKE);
    ink.line(x + 2.5, y + 3.5, x + 8.5, y + 3.5, accent, FINE);
}

/// A capital A drawn in strokes, `height` tall with its foot at (x, bottom).
fn draw_letter_a(ink: &mut ToolInk<'_>, x: f64, bottom: f64, height: f64, accent: bool) {
    let half = height * 0.38;
    ink.stroke(
        &polyline(
            &[
                pt(x - half, bottom),
                pt(x, bottom - height),
                pt(x + half, bottom),
            ],
            false,
        ),
        accent,
        STROKE,
    );
    ink.line(
        x - half * 0.62,
        bottom - height * 0.36,
        x + half * 0.62,
        bottom - height * 0.36,
        accent,
        STROKE,
    );
}

/// An arrowhead at (x, y) pointing along (dx, dy), a unit direction.
fn draw_head(ink: &mut ToolInk<'_>, x: f64, y: f64, dx: f64, dy: f64) {
    let back = 3.0;
    let side = 2.6;
    ink.stroke(
        &polyline(
            &[
                pt(x - back * dx - side * dy, y - back * dy + side * dx),
                pt(x, y),
                pt(x - back * dx + side * dy, y - back * dy - side * dx),
            ],
            false,
        ),
        true,
        STROKE,
    );
}

/// An open-ended wrench in the bright tone: its jaw a ring of radius 2.6 about (x, y), open on the
/// side away from its handle, and the handle running from the ring towards `toward` (degrees
/// counter-clockwise from east, as on paper). Open, because a closed ring with a handle is a lens:
/// Zoom To's.
fn draw_wrench(ink: &mut ToolInk<'_>, x: f64, y: f64, toward: f64) {
    const JAW: f64 = 2.6;
    const HANDLE: f64 = 7.5;
    // The gap is 80 degrees wide, centred opposite the handle.
    ink.stroke(&arc(x, y, JAW, toward + 180.0 + 40.0, 280.0), true, STROKE);
    let (dx, dy) = (toward.to_radians().cos(), -toward.to_radians().sin());
    ink.line(
        x + JAW * dx,
        y + JAW * dy,
        x + (JAW + HANDLE) * dx,
        y + (JAW + HANDLE) * dy,
        true,
        STROKE,
    );
}

/// An ellipse inside the rectangle (x, y, w, h).
fn ellipse(x: f64, y: f64, w: f64, h: f64) -> Path {
    let mut path = Path::new();
    path.add_ellipse(RectF::new(x, y, w, h));
    path
}

/// A toolbar band with its first button bright, `side` grid units square, and quiet dots for the
/// buttons after it: View > Toolbars' icon sizes.
fn draw_toolbar_size(ink: &mut ToolInk<'_>, side: f64) {
    ink.stroke(&rounded(2.5, 5.0, 19.0, 14.0, 1.5), false, STROKE);
    ink.stroke(&rounded(5.0, 12.0 - side / 2.0, side, side, 1.0), true, STROKE);
    let mut x = 5.0 + side + 3.0;
    while x < 19.5 {
        ink.dot(x, 12.0, 0.9, false);
        x += 3.0;
    }
}

/// View > Text Size's items: a bright A `height` tall standing on a quiet baseline.
fn draw_text_size(ink: &mut ToolInk<'_>, height: f64) {
    ink.line(3.0, 21.0, 21.0, 21.0, false, FINE);
    draw_letter_a(ink, 12.0, 19.5, height, true);
}

/// An orthographic standard view: the object in plan, the face seen in the accent and the eye's
/// arrow coming at it along (dx, dy).
fn draw_orthographic_view(ink: &mut ToolInk<'_>, dx: f64, dy: f64) {
    ink.stroke(&rectangle(7.0, 7.0, 10.0, 10.0), false, STROKE);
    // The face that meets the arrow: the edge the arrow ends on.
    let cx = 12.0 - 5.0 * dx;
    let cy = 12.0 - 5.0 * dy;
    ink.line(
        cx - 5.0 * dy,
        cy - 5.0 * dx,
        cx + 5.0 * dy,
        cy + 5.0 * dx,
        true,
        STROKE,
    );
    let tip_x = 12.0 - 6.2 * dx;
    let tip_y = 12.0 - 6.2 * dy;
    ink.line(
        tip_x - 4.8 * dx,
        tip_y - 4.8 * dy,
        tip_x,
        tip_y,
        true,
        STROKE,
    );
    draw_head(ink, tip_x, tip_y, dx, dy);
}

/// `path` with every point mapped by `transform` (QTransform::map(QPainterPath)).
fn mapped(transform: &Transform, path: &Path) -> Path {
    use crate::canvas::PathElement;
    let mut out = path.clone();
    for element in &mut out.elements {
        match element {
            PathElement::MoveTo(point) | PathElement::LineTo(point) => {
                *point = transform.map(*point);
            }
            PathElement::CubicTo(c1, c2, end) => {
                *c1 = transform.map(*c1);
                *c2 = transform.map(*c2);
                *end = transform.map(*end);
            }
            PathElement::Close => {}
            PathElement::Text { origin, .. } => *origin = transform.map(*origin),
        }
    }
    out
}

/// A link of a chain: a stadium `length` long and `width` wide about (x, y), lying on the diagonal
/// that rises to the right, as the Link button's chain lies.
fn chain_link(x: f64, y: f64, length: f64, width: f64) -> Path {
    let mut link = Path::new();
    link.add_rounded_rect(
        RectF::new(-0.5 * length, -0.5 * width, length, width),
        0.5 * width,
        0.5 * width,
    );
    let turn = Transform::IDENTITY.translated(x, y).rotated(-45.0);
    mapped(&turn, &link)
}

/// An isometric standard view: the cube, and the eye's arrow from its corner along (dx, dy) -
/// both components +-1, so the arrow runs on a diagonal.
fn draw_isometric_view(ink: &mut ToolInk<'_>, dx: f64, dy: f64) {
    let top = polyline(
        &[pt(12.0, 6.0), pt(17.5, 9.0), pt(12.0, 12.0), pt(6.5, 9.0)],
        true,
    );
    ink.fill(&top, false, TINT);
    ink.stroke(
        &polyline(
            &[
                pt(12.0, 6.0),
                pt(17.5, 9.0),
                pt(17.5, 15.0),
                pt(12.0, 18.0),
                pt(6.5, 15.0),
                pt(6.5, 9.0),
            ],
            true,
        ),
        false,
        STROKE,
    );
    ink.line(12.0, 12.0, 12.0, 18.0, false, STROKE);
    ink.stroke(&top, false, STROKE);
    let unit = 1.0 / 2.0_f64.sqrt();
    let tip_x = 12.0 - 7.8 * dx;
    let tip_y = 12.0 - 7.8 * dy;
    ink.line(
        tip_x - 3.6 * dx,
        tip_y - 3.6 * dy,
        tip_x,
        tip_y,
        true,
        STROKE,
    );
    draw_head(ink, tip_x, tip_y, dx * unit, dy * unit);
}

// ---- the icons ---------------------------------------------------------------------------------

/// Paints `which` into `rect` with the given colours. Exposed so that the contact sheet and the
/// tests can paint without going through the cache. `rect` is taken as the icon's size on screen
/// for its weights (weight_at): right wherever a canvas pixel is a screen pixel.
pub fn paint_icon(
    canvas: &mut dyn Canvas,
    which: Icon,
    rect: RectF,
    neutral: Color,
    accent: Color,
) {
    paint_icon_weighted(canvas, which, rect, neutral, accent, weight_at(rect.width));
}

/// paint_icon with the strokes `weight` times as wide as drawn (weight_at): for a caller that
/// knows the icon's size on screen, which `rect` in device pixels does not say.
pub fn paint_icon_weighted(
    canvas: &mut dyn Canvas,
    which: Icon,
    rect: RectF,
    neutral: Color,
    accent: Color,
    weight: f64,
) {
    canvas.save();
    canvas.set_render_hint(RenderHint::Antialiasing, true);
    canvas.translate(rect.x, rect.y);
    canvas.scale(rect.width / GRID, rect.height / GRID);
    {
        let mut ink = ToolInk::new(canvas, neutral, accent).weighted(weight);
        let ink = &mut ink;
        match which {
            Icon::New => {
                ink.stroke(
                    &polyline(
                        &[
                            pt(6.0, 3.0),
                            pt(14.0, 3.0),
                            pt(19.0, 8.0),
                            pt(19.0, 21.0),
                            pt(6.0, 21.0),
                        ],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(14.0, 3.0), pt(14.0, 8.0), pt(19.0, 8.0)], false),
                    false,
                    STROKE,
                );
                ink.line(12.5, 12.0, 12.5, 18.0, true, STROKE);
                ink.line(9.5, 15.0, 15.5, 15.0, true, STROKE);
            }
            Icon::Open => {
                ink.stroke(
                    &polyline(
                        &[
                            pt(3.0, 19.0),
                            pt(3.0, 6.0),
                            pt(9.0, 6.0),
                            pt(11.0, 8.0),
                            pt(19.0, 8.0),
                            pt(19.0, 11.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(
                        &[pt(3.0, 19.0), pt(6.5, 11.0), pt(22.0, 11.0), pt(18.5, 19.0)],
                        true,
                    ),
                    true,
                    STROKE,
                );
            }
            Icon::Save => {
                draw_disk(ink, true);
            }
            Icon::SaveAs => {
                // The disk, and the pencil every editing icon has (draw_pencil) writing on
                // it: until 2026-11-16 a solid pencil of its own, the one filled glyph on
                // the File toolbar.
                draw_disk(ink, false);
                draw_pencil(ink);
            }
            Icon::Import => {
                draw_tray(ink);
                ink.line(12.0, 3.0, 12.0, 15.0, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(8.0, 11.0), pt(12.0, 15.0), pt(16.0, 11.0)], false),
                    true,
                    STROKE,
                );
            }
            Icon::Export => {
                draw_tray(ink);
                ink.line(12.0, 16.0, 12.0, 4.0, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(8.0, 8.0), pt(12.0, 4.0), pt(16.0, 8.0)], false),
                    true,
                    STROKE,
                );
            }
            Icon::Plot => {
                ink.stroke(
                    &polyline(
                        &[pt(7.0, 9.0), pt(7.0, 4.0), pt(17.0, 4.0), pt(17.0, 9.0)],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(
                        &[
                            pt(7.0, 17.0),
                            pt(4.0, 17.0),
                            pt(4.0, 9.0),
                            pt(20.0, 9.0),
                            pt(20.0, 17.0),
                            pt(17.0, 17.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(&rectangle(7.0, 14.0, 10.0, 7.0), true, STROKE);
                ink.line(9.5, 17.5, 14.5, 17.5, true, STROKE);
            }
            Icon::Undo => {
                draw_turn(ink, false);
            }
            Icon::Redo => {
                draw_turn(ink, true);
            }
            Icon::Erase => {
                ink.line(4.0, 7.0, 20.0, 7.0, false, STROKE);
                ink.stroke(
                    &polyline(
                        &[pt(9.0, 7.0), pt(9.0, 4.5), pt(15.0, 4.5), pt(15.0, 7.0)],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(
                        &[pt(6.0, 7.0), pt(7.0, 21.0), pt(17.0, 21.0), pt(18.0, 7.0)],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.line(10.0, 11.0, 10.0, 17.0, true, STROKE);
                ink.line(14.0, 11.0, 14.0, 17.0, true, STROKE);
            }
            Icon::SelectAll => {
                ink.dashed(&rectangle(3.5, 3.5, 17.0, 17.0), true);
                ink.stroke(&rectangle(7.0, 8.0, 5.5, 4.5), false, STROKE);
                ink.stroke(&circle(15.0, 15.5, 2.6), false, STROKE);
            }
            Icon::Select => {
                let arrow = polyline(
                    &[
                        pt(6.0, 3.0),
                        pt(6.0, 18.0),
                        pt(10.0, 14.5),
                        pt(12.8, 20.5),
                        pt(15.2, 19.4),
                        pt(12.5, 13.5),
                        pt(18.0, 13.5),
                    ],
                    true,
                );
                // An outlined pointer with the set's tint inside, not a solid one: at rest
                // it is as quiet as its neighbours, and active it is the accent's outline.
                ink.fill(&arrow, true, TINT);
                ink.stroke(&arrow, true, STROKE);
            }
            Icon::Point => {
                ink.line(12.0, 3.0, 12.0, 8.0, false, STROKE);
                ink.line(12.0, 16.0, 12.0, 21.0, false, STROKE);
                ink.line(3.0, 12.0, 8.0, 12.0, false, STROKE);
                ink.line(16.0, 12.0, 21.0, 12.0, false, STROKE);
                ink.dot(12.0, 12.0, 2.6, true);
            }
            Icon::Line => {
                ink.line(5.0, 19.0, 19.0, 5.0, true, STROKE);
                ink.node(5.0, 19.0, false);
                ink.node(19.0, 5.0, false);
            }
            Icon::Polyline => {
                ink.stroke(
                    &polyline(
                        &[pt(4.0, 18.0), pt(9.0, 8.0), pt(15.0, 15.0), pt(20.0, 5.0)],
                        false,
                    ),
                    true,
                    STROKE,
                );
                for p in [pt(4.0, 18.0), pt(9.0, 8.0), pt(15.0, 15.0), pt(20.0, 5.0)] {
                    ink.node(p.x, p.y, false);
                }
            }
            Icon::Rectangle => {
                ink.stroke(&rectangle(4.0, 6.0, 16.0, 12.0), true, STROKE);
                for p in [pt(4.0, 6.0), pt(20.0, 6.0), pt(20.0, 18.0), pt(4.0, 18.0)] {
                    ink.node(p.x, p.y, false);
                }
            }
            Icon::Circle => {
                // The radius goes off at 45 degrees. Drawn horizontally it made the
                // icon read as a minus sign in a ring.
                ink.stroke(&circle(12.0, 12.0, 8.0), true, STROKE);
                ink.line(12.0, 12.0, 17.66, 6.34, false, FINE);
                ink.node(12.0, 12.0, false);
                ink.node(17.66, 6.34, false);
            }
            Icon::Arc => {
                let mut arc = start(4.0, 18.0);
                arc.quad_to(pt(12.0, -1.0), pt(20.0, 18.0));
                ink.stroke(&arc, true, STROKE);
                ink.node(4.0, 18.0, false);
                ink.node(12.0, 8.5, false);
                ink.node(20.0, 18.0, false);
            }
            Icon::Move => {
                ink.line(12.0, 4.0, 12.0, 20.0, false, STROKE);
                ink.line(4.0, 12.0, 20.0, 12.0, false, STROKE);
                ink.stroke(
                    &polyline(&[pt(9.0, 6.0), pt(12.0, 3.0), pt(15.0, 6.0)], false),
                    true,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(9.0, 18.0), pt(12.0, 21.0), pt(15.0, 18.0)], false),
                    true,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(6.0, 9.0), pt(3.0, 12.0), pt(6.0, 15.0)], false),
                    true,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(18.0, 9.0), pt(21.0, 12.0), pt(18.0, 15.0)], false),
                    true,
                    STROKE,
                );
            }
            Icon::Copy => {
                ink.stroke(
                    &polyline(
                        &[
                            pt(8.5, 15.0),
                            pt(4.0, 15.0),
                            pt(4.0, 4.0),
                            pt(15.0, 4.0),
                            pt(15.0, 8.5),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(&rectangle(9.0, 9.0, 11.0, 11.0), true, STROKE);
            }
            Icon::ZoomExtents => {
                ink.stroke(
                    &polyline(&[pt(3.0, 8.0), pt(3.0, 3.0), pt(8.0, 3.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(16.0, 3.0), pt(21.0, 3.0), pt(21.0, 8.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(21.0, 16.0), pt(21.0, 21.0), pt(16.0, 21.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(8.0, 21.0), pt(3.0, 21.0), pt(3.0, 16.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(&rectangle(8.0, 8.0, 8.0, 8.0), true, STROKE);
            }
            Icon::Grid => {
                ink.stroke(&rectangle(3.0, 3.0, 18.0, 18.0), false, STROKE);
                ink.line(9.0, 3.0, 9.0, 21.0, false, FINE);
                ink.line(15.0, 3.0, 15.0, 21.0, false, FINE);
                ink.line(3.0, 9.0, 21.0, 9.0, false, FINE);
                ink.line(3.0, 15.0, 21.0, 15.0, false, FINE);
                ink.dot(15.0, 9.0, 2.2, true);
            }
            Icon::Snap => {
                // A horseshoe magnet in outline - the outer and inner curves of one bent
                // bar, true semicircles about (12, 13) - and its two poles tinted and
                // banded in the bright tone, where it pulls. It was one 4.2-unit stroke with
                // a pen of its own, the heaviest glyph on the toolbar.
                let mut magnet = start(5.0, 4.0);
                magnet.line_to(pt(5.0, 13.0));
                magnet.arc_to(RectF::new(5.0, 6.0, 14.0, 14.0), 180.0, 180.0);
                magnet.line_to(pt(19.0, 4.0));
                magnet.line_to(pt(14.0, 4.0));
                magnet.line_to(pt(14.0, 13.0));
                magnet.arc_to(RectF::new(10.0, 11.0, 4.0, 4.0), 0.0, -180.0);
                magnet.line_to(pt(10.0, 4.0));
                magnet.close_subpath();
                ink.fill(&rectangle(5.0, 4.0, 5.0, 4.5), true, TINT);
                ink.fill(&rectangle(14.0, 4.0, 5.0, 4.5), true, TINT);
                ink.stroke(&magnet, false, STROKE);
                ink.line(5.0, 8.5, 10.0, 8.5, true, STROKE);
                ink.line(14.0, 8.5, 19.0, 8.5, true, STROKE);
            }
            Icon::Layers => {
                ink.stroke(
                    &polyline(&[pt(3.0, 16.0), pt(12.0, 21.0), pt(21.0, 16.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(3.0, 12.0), pt(12.0, 17.0), pt(21.0, 12.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(
                        &[pt(12.0, 3.0), pt(21.0, 8.0), pt(12.0, 13.0), pt(3.0, 8.0)],
                        true,
                    ),
                    true,
                    STROKE,
                );
            }
            Icon::Properties => {
                ink.line(4.0, 7.0, 20.0, 7.0, false, STROKE);
                ink.line(4.0, 12.0, 20.0, 12.0, false, STROKE);
                ink.line(4.0, 17.0, 20.0, 17.0, false, STROKE);
                ink.dot(9.0, 7.0, 2.3, true);
                ink.dot(15.5, 12.0, 2.3, true);
                ink.dot(7.5, 17.0, 2.3, true);
            }
            Icon::SurfaceFromCloud => {
                for p in [
                    pt(5.0, 7.0),
                    pt(9.5, 4.5),
                    pt(14.5, 7.5),
                    pt(19.0, 5.0),
                    pt(7.5, 11.0),
                    pt(12.0, 10.0),
                    pt(17.5, 11.5),
                ] {
                    ink.dot(p.x, p.y, 1.15, true);
                }
                ink.stroke(
                    &polyline(
                        &[pt(3.0, 20.5), pt(8.0, 15.5), pt(13.0, 18.5), pt(21.0, 14.0)],
                        false,
                    ),
                    false,
                    STROKE,
                );
            }
            Icon::SurfaceFromRaster => {
                ink.stroke(&rectangle(3.0, 3.0, 10.0, 10.0), false, STROKE);
                ink.line(8.0, 3.0, 8.0, 13.0, false, FINE);
                ink.line(3.0, 8.0, 13.0, 8.0, false, FINE);
                ink.fill(&rectangle(3.0, 3.0, 5.0, 5.0), false, TINT);
                ink.fill(&rectangle(8.0, 8.0, 5.0, 5.0), false, TINT);
                ink.stroke(
                    &polyline(
                        &[
                            pt(8.0, 21.0),
                            pt(14.5, 12.0),
                            pt(17.5, 16.5),
                            pt(19.0, 14.5),
                            pt(22.0, 21.0),
                        ],
                        true,
                    ),
                    true,
                    STROKE,
                );
            }
            Icon::SurfaceFromDrawing => {
                ink.stroke(
                    &polyline(
                        &[
                            pt(3.0, 19.0),
                            pt(10.0, 6.0),
                            pt(21.0, 9.0),
                            pt(21.0, 20.0),
                            pt(15.0, 17.0),
                        ],
                        true,
                    ),
                    true,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(10.0, 6.0), pt(15.0, 17.0), pt(21.0, 9.0)], false),
                    true,
                    STROKE,
                );
                for p in [
                    pt(3.0, 19.0),
                    pt(10.0, 6.0),
                    pt(21.0, 9.0),
                    pt(21.0, 20.0),
                    pt(15.0, 17.0),
                ] {
                    ink.dot(p.x, p.y, 1.5, false);
                }
            }
            Icon::Section => {
                ink.stroke(
                    &polyline(
                        &[
                            pt(3.0, 15.0),
                            pt(8.0, 11.0),
                            pt(12.0, 14.0),
                            pt(17.0, 8.0),
                            pt(21.0, 11.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.line(3.0, 20.0, 21.0, 20.0, false, FINE);
                ink.dashed(&polyline(&[pt(12.0, 3.5), pt(12.0, 20.0)], false), true);
            }
            Icon::SectionAlignment => {
                let mut alignment = start(3.0, 20.0);
                alignment.cubic_to(pt(10.0, 19.0), pt(9.0, 11.0), pt(13.0, 9.0));
                alignment.cubic_to(pt(16.0, 7.5), pt(18.0, 6.0), pt(21.0, 4.0));
                ink.stroke(&alignment, false, STROKE);
                ink.line(7.5, 8.5, 16.5, 14.5, true, STROKE);
                ink.line(6.7, 9.7, 8.3, 7.3, true, STROKE);
                ink.line(15.7, 15.7, 17.3, 13.3, true, STROKE);
            }
            Icon::CorridorQuantities => {
                // Ground falling left to right, the road template cut into it, and the
                // cut between them shaded - which is the quantity.
                let cut = polyline(
                    &[pt(5.5, 8.4), pt(8.5, 15.0), pt(15.5, 15.0), pt(18.5, 11.3)],
                    true,
                );
                ink.fill(&cut, true, TINT);
                ink.line(3.0, 7.8, 21.0, 11.9, false, STROKE);
                ink.stroke(
                    &polyline(
                        &[pt(5.5, 8.4), pt(8.5, 15.0), pt(15.5, 15.0), pt(18.5, 11.3)],
                        false,
                    ),
                    true,
                    STROKE,
                );
                ink.line(3.0, 20.0, 21.0, 20.0, false, FINE);
            }
            Icon::CorridorSurface => {
                let mut left = start(4.0, 21.0);
                left.quad_to(pt(5.0, 10.0), pt(15.0, 3.0));
                let mut right = start(11.0, 21.0);
                right.quad_to(pt(12.0, 13.0), pt(21.0, 7.0));
                ink.stroke(&left, false, STROKE);
                ink.stroke(&right, false, STROKE);
                ink.line(4.5, 17.0, 11.4, 17.6, true, FINE);
                ink.line(6.6, 11.6, 13.4, 13.4, true, FINE);
                ink.line(10.6, 6.6, 17.0, 9.6, true, FINE);
            }
            Icon::ImportVector => {
                draw_vector_glyph(ink);
                draw_arrow_in(ink);
            }
            Icon::ImportRaster => {
                draw_raster_glyph(ink);
                draw_arrow_in(ink);
            }
            Icon::ImportPointCloud => {
                draw_cloud_glyph(ink);
                draw_arrow_in(ink);
            }
            Icon::ExportPointCloud => {
                draw_cloud_glyph(ink);
                draw_arrow_out(ink);
            }
            Icon::ExportDem => {
                // A surface's profile over the grid it becomes: the accent is the
                // surface, since that is what is being written out.
                draw_raster_glyph(ink);
                ink.stroke(
                    &polyline(
                        &[pt(3.0, 7.0), pt(7.5, 3.5), pt(11.0, 6.0), pt(15.0, 2.5)],
                        false,
                    ),
                    true,
                    STROKE,
                );
                draw_arrow_out(ink);
            }
            Icon::ConvertCopc => {
                // An octree seen from above: a square quartered, one quarter quartered
                // again, points in the cells - the structure COPC writes into the file.
                ink.stroke(&rectangle(3.0, 3.0, 18.0, 18.0), false, STROKE);
                ink.line(12.0, 3.0, 12.0, 21.0, false, FINE);
                ink.line(3.0, 12.0, 21.0, 12.0, false, FINE);
                ink.line(16.5, 12.0, 16.5, 21.0, true, FINE);
                ink.line(12.0, 16.5, 21.0, 16.5, true, FINE);
                for p in [
                    pt(7.5, 7.5),
                    pt(16.5, 7.5),
                    pt(7.5, 16.5),
                    pt(14.25, 14.25),
                    pt(18.75, 14.25),
                    pt(14.25, 18.75),
                    pt(18.75, 18.75),
                ] {
                    ink.dot(p.x, p.y, 1.2, true);
                }
            }
            Icon::DatasetInfo => {
                // A file's page, and the "i" of information beside it rather than on
                // it, so the two never cross at 16 px.
                ink.stroke(&rectangle(3.0, 3.0, 10.0, 13.0), false, STROKE);
                ink.line(5.5, 7.0, 10.5, 7.0, false, FINE);
                ink.line(5.5, 10.0, 10.5, 10.0, false, FINE);
                ink.line(5.5, 13.0, 10.5, 13.0, false, FINE);
                ink.stroke(&circle(17.5, 17.5, 4.0), true, STROKE);
                ink.dot(17.5, 15.6, 0.9, true);
                ink.line(17.5, 17.6, 17.5, 19.8, true, STROKE);
            }
            Icon::Processing => {
                // A dataset in, the accent path of a run, and the result out: the
                // same two boxes as any conversion, joined by the step between them.
                ink.stroke(&rectangle(3.0, 3.0, 8.0, 8.0), false, STROKE);
                ink.line(3.0, 7.0, 11.0, 7.0, false, FINE);
                ink.line(7.0, 3.0, 7.0, 11.0, false, FINE);
                ink.stroke(&rectangle(13.0, 13.0, 8.0, 8.0), false, STROKE);
                ink.stroke(
                    &polyline(&[pt(7.0, 13.0), pt(7.0, 17.0), pt(12.0, 17.0)], false),
                    true,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(10.0, 15.0), pt(12.0, 17.0), pt(10.0, 19.0)], false),
                    true,
                    STROKE,
                );
            }
            // ---- window buttons: quiet only, in the set's STROKE. They were drawn heavier
            // (2.0 to 2.2 units) for a title bar's 14 px; weight_at now holds every icon's
            // strokes at 1.5 px there.
            Icon::Minimise => {
                ink.line(6.0, 17.0, 18.0, 17.0, false, STROKE);
            }
            Icon::Maximise => {
                // The window and the line under its title bar: the bar was a solid band.
                ink.stroke(&rounded(5.0, 5.0, 14.0, 14.0, 1.5), false, STROKE);
                ink.line(5.0, 9.0, 19.0, 9.0, false, STROKE);
            }
            Icon::Restore => {
                // Two windows, the front one whole and the back one showing only
                // where it is not covered - the convention every desktop uses.
                ink.stroke(
                    &polyline(
                        &[
                            pt(9.0, 8.5),
                            pt(9.0, 4.0),
                            pt(20.0, 4.0),
                            pt(20.0, 15.0),
                            pt(16.0, 15.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(&rounded(4.0, 9.0, 12.0, 11.0, 1.5), false, STROKE);
                ink.line(4.0, 12.5, 16.0, 12.5, false, STROKE);
            }
            Icon::Close => {
                ink.line(6.5, 6.5, 17.5, 17.5, false, STROKE);
                ink.line(17.5, 6.5, 6.5, 17.5, false, STROKE);
            }
            Icon::Float => {
                // The main window open at one corner, and an arrow leaving through it.
                // Dock is the same box with the arrow coming back in, so the pair
                // reads as one control in two states.
                ink.stroke(
                    &polyline(
                        &[
                            pt(11.0, 4.0),
                            pt(4.0, 4.0),
                            pt(4.0, 20.0),
                            pt(20.0, 20.0),
                            pt(20.0, 13.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.line(10.0, 14.0, 20.0, 4.0, false, STROKE);
                ink.stroke(
                    &polyline(&[pt(14.0, 4.0), pt(20.0, 4.0), pt(20.0, 10.0)], false),
                    false,
                    STROKE,
                );
            }
            Icon::Dock => {
                ink.stroke(
                    &polyline(
                        &[
                            pt(11.0, 4.0),
                            pt(4.0, 4.0),
                            pt(4.0, 20.0),
                            pt(20.0, 20.0),
                            pt(20.0, 13.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.line(20.0, 4.0, 10.0, 14.0, false, STROKE);
                ink.stroke(
                    &polyline(&[pt(10.0, 8.0), pt(10.0, 14.0), pt(16.0, 14.0)], false),
                    false,
                    STROKE,
                );
            }
            // ---- panels
            Icon::CommandLine => {
                ink.stroke(&rounded(3.0, 5.0, 18.0, 14.0, 2.0), false, STROKE);
                ink.stroke(
                    &polyline(&[pt(7.0, 9.5), pt(10.0, 12.0), pt(7.0, 14.5)], false),
                    true,
                    STROKE,
                );
                ink.line(12.0, 14.5, 17.0, 14.5, true, STROKE);
            }
            Icon::ReferenceData => {
                // An image and a scatter of points: the two kinds of reference data.
                ink.stroke(&rectangle(3.0, 3.0, 13.0, 11.0), false, STROKE);
                ink.stroke(
                    &polyline(
                        &[
                            pt(3.8, 12.0),
                            pt(7.0, 8.0),
                            pt(10.0, 10.5),
                            pt(12.5, 8.5),
                            pt(15.2, 11.5),
                        ],
                        false,
                    ),
                    true,
                    STROKE,
                );
                for p in [
                    pt(12.0, 18.5),
                    pt(15.5, 17.0),
                    pt(19.0, 18.5),
                    pt(16.5, 20.8),
                    pt(20.2, 14.8),
                    pt(13.0, 21.5),
                ] {
                    ink.dot(p.x, p.y, 1.2, false);
                }
            }
            // ---- what a view shows
            Icon::ViewPlan => {
                ink.stroke(&rounded(3.0, 3.0, 18.0, 18.0, 1.5), false, STROKE);
                ink.stroke(
                    &polyline(
                        &[pt(6.5, 16.5), pt(10.0, 9.0), pt(14.5, 13.0), pt(17.5, 7.0)],
                        false,
                    ),
                    true,
                    STROKE,
                );
                for p in [pt(6.5, 16.5), pt(10.0, 9.0), pt(14.5, 13.0), pt(17.5, 7.0)] {
                    ink.node(p.x, p.y, false);
                }
            }
            Icon::View3D => {
                let top = polyline(
                    &[pt(12.0, 3.0), pt(20.0, 7.5), pt(12.0, 12.0), pt(4.0, 7.5)],
                    true,
                );
                ink.fill(&top, true, TINT);
                ink.stroke(
                    &polyline(
                        &[
                            pt(12.0, 3.0),
                            pt(20.0, 7.5),
                            pt(20.0, 16.5),
                            pt(12.0, 21.0),
                            pt(4.0, 16.5),
                            pt(4.0, 7.5),
                        ],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.line(12.0, 12.0, 12.0, 21.0, false, STROKE);
                ink.stroke(&top, true, STROKE);
            }
            Icon::ViewSection => {
                // A profile on its axes: a section is a measured graph, which is what
                // tells it apart from the Section command's cut through the ground.
                ink.stroke(
                    &polyline(&[pt(4.0, 3.5), pt(4.0, 20.0), pt(20.5, 20.0)], false),
                    false,
                    STROKE,
                );
                for x in [8.5, 12.5, 16.5] {
                    ink.line(x, 20.0, x, 18.3, false, FINE);
                }
                ink.stroke(
                    &polyline(
                        &[
                            pt(4.0, 14.0),
                            pt(8.5, 10.0),
                            pt(12.5, 13.0),
                            pt(16.5, 7.0),
                            pt(20.5, 10.0),
                        ],
                        false,
                    ),
                    true,
                    STROKE,
                );
            }
            Icon::ViewElevation => {
                // A building's front: the architectural meaning of an elevation.
                ink.line(2.5, 20.0, 21.5, 20.0, false, STROKE);
                ink.stroke(
                    &polyline(
                        &[
                            pt(5.0, 20.0),
                            pt(5.0, 10.0),
                            pt(12.0, 4.0),
                            pt(19.0, 10.0),
                            pt(19.0, 20.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(&rectangle(9.5, 13.0, 5.0, 7.0), true, STROKE);
            }
            Icon::ViewLayersFiltered => {
                // The Layers glyph moved left, and the funnel everyone reads as
                // "filtered" in the accent: this view is not showing everything.
                ink.stroke(
                    &polyline(&[pt(2.0, 16.0), pt(8.5, 20.0), pt(15.0, 16.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(2.0, 12.0), pt(8.5, 16.0), pt(15.0, 12.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(
                        &[pt(8.5, 4.5), pt(15.0, 8.2), pt(8.5, 12.0), pt(2.0, 8.2)],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.fill(
                    &polyline(
                        &[
                            pt(14.5, 2.5),
                            pt(22.5, 2.5),
                            pt(19.8, 6.5),
                            pt(19.8, 10.5),
                            pt(17.2, 11.8),
                            pt(17.2, 6.5),
                        ],
                        true,
                    ),
                    true,
                    255,
                );
            }
            // ---- panel tools
            Icon::LayerNew => {
                ink.stroke(
                    &polyline(&[pt(2.5, 15.5), pt(9.5, 19.5), pt(16.5, 15.5)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(
                        &[pt(9.5, 7.5), pt(16.5, 11.5), pt(9.5, 15.5), pt(2.5, 11.5)],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.line(19.0, 2.5, 19.0, 9.5, true, STROKE);
                ink.line(15.5, 6.0, 22.5, 6.0, true, STROKE);
            }
            Icon::LayerNewChild => {
                ink.stroke(&rounded(3.0, 3.0, 9.0, 5.0, 1.0), false, STROKE);
                ink.stroke(
                    &polyline(&[pt(7.0, 8.0), pt(7.0, 17.0), pt(11.0, 17.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(&rounded(11.0, 14.5, 10.0, 5.0, 1.0), true, STROKE);
                ink.line(18.5, 3.0, 18.5, 10.0, true, STROKE);
                ink.line(15.0, 6.5, 22.0, 6.5, true, STROKE);
            }
            Icon::Rename => {
                // A name field with the text cursor in it.
                ink.stroke(&rounded(2.5, 7.0, 19.0, 10.0, 1.5), false, STROKE);
                ink.line(6.0, 12.0, 11.0, 12.0, false, STROKE);
                ink.line(15.0, 4.5, 15.0, 19.5, true, STROKE);
                ink.line(13.0, 4.5, 17.0, 4.5, true, STROKE);
                ink.line(13.0, 19.5, 17.0, 19.5, true, STROKE);
            }
            Icon::ZoomTo => {
                ink.stroke(&circle(10.0, 10.0, 6.5), false, STROKE);
                ink.line(14.8, 14.8, 20.5, 20.5, false, STROKE);
                ink.stroke(&rectangle(7.5, 7.5, 5.0, 5.0), true, STROKE);
            }
            Icon::Help => {
                ink.stroke(&circle(12.0, 12.0, 9.0), false, STROKE);
                let mut mark = start(9.0, 9.6);
                mark.cubic_to(pt(9.0, 5.6), pt(15.0, 5.6), pt(15.0, 9.4));
                mark.cubic_to(pt(15.0, 11.6), pt(12.0, 11.8), pt(12.0, 14.4));
                ink.stroke(&mark, true, STROKE);
                ink.dot(12.0, 17.6, 1.2, true);
            }
            Icon::About => {
                ink.stroke(&circle(12.0, 12.0, 9.0), false, STROKE);
                ink.dot(12.0, 7.6, 1.25, true);
                ink.line(12.0, 11.2, 12.0, 17.4, true, STROKE);
            }
            // ---- Survey ------------------------------------------------------------------------
            Icon::SurveyInverse => {
                // Two marks, north at the first, and what the inverse measures: the
                // line between them and its azimuth, swept clockwise from north.
                draw_north(ink, 6.0, 15.8, 4.0);
                // (6, 18) to (19, 7): unit vector (13, -11) / 17.03, trimmed by the
                // marks' 2.2 radius at each end.
                ink.line(7.68, 16.58, 17.32, 8.42, true, STROKE);
                let mut sweep = Path::new();
                // Radius 10 about (6, 18): large enough to read as the angle at 20 px.
                let round = RectF::new(-4.0, 8.0, 20.0, 20.0);
                sweep.arc_move_to(round, 90.0);
                // From north (90 degrees in Qt's anticlockwise sense) to the line,
                // atan2(11, 13) = 40.2 degrees: 49.8 degrees clockwise.
                sweep.arc_to(round, 90.0, -49.8);
                ink.stroke(&sweep, true, FINE);
                draw_mark(ink, 6.0, 18.0, false);
                draw_mark(ink, 19.0, 7.0, false);
            }
            Icon::SurveyForward => {
                // From a known mark along a direction to a NEW point, which is the
                // accent: what the command adds to the drawing.
                draw_north(ink, 5.0, 16.8, 4.0);
                // (5, 19) to (18, 7): unit u = (13, -12) / 17.69, trimmed by 2.2 and
                // 2.4. The arrowhead at the tip T = (16.24, 8.63), its wings at
                // T - 3u +- 2.2 n with n = (0.678, 0.735) across the line, says the
                // direction runs OUT to the new point - which the inverse's has not.
                ink.line(6.62, 17.51, 16.24, 8.63, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(12.55, 9.04), pt(16.24, 8.63), pt(15.53, 12.28)], false),
                    true,
                    STROKE,
                );
                draw_mark(ink, 5.0, 19.0, false);
                draw_mark(ink, 18.0, 7.0, true);
            }
            Icon::SurveyArea => {
                let parcel = polyline(
                    &[
                        pt(3.0, 19.0),
                        pt(5.0, 6.0),
                        pt(14.0, 3.0),
                        pt(21.0, 9.0),
                        pt(18.0, 20.0),
                    ],
                    true,
                );
                ink.fill(&parcel, true, TINT);
                ink.stroke(&parcel, false, STROKE);
                for p in [
                    pt(3.0, 19.0),
                    pt(5.0, 6.0),
                    pt(14.0, 3.0),
                    pt(21.0, 9.0),
                    pt(18.0, 20.0),
                ] {
                    ink.node(p.x, p.y, false);
                }
            }
            Icon::SurveyAngle => {
                // Two rays from a vertex, the angle between them, and the degree mark.
                ink.line(4.0, 19.0, 21.0, 19.0, false, STROKE);
                ink.line(4.0, 19.0, 14.0, 5.4, false, STROKE);
                let mut sweep = Path::new();
                let round = RectF::new(-5.0, 10.0, 18.0, 18.0); // radius 9 about (4, 19)
                sweep.arc_move_to(round, 0.0);
                // The second ray's direction, atan2(13.6, 10) = 53.7 degrees.
                sweep.arc_to(round, 0.0, 53.7);
                ink.stroke(&sweep, true, STROKE);
                ink.stroke(&circle(18.0, 8.0, 2.0), true, FINE);
            }
            Icon::SurveyTraverse => {
                // The legs, which the adjustment corrects, over the stations.
                ink.stroke(
                    &polyline(
                        &[pt(4.0, 18.0), pt(10.0, 7.0), pt(15.0, 15.0), pt(20.0, 5.0)],
                        false,
                    ),
                    true,
                    STROKE,
                );
                for p in [pt(4.0, 18.0), pt(10.0, 7.0), pt(15.0, 15.0), pt(20.0, 5.0)] {
                    draw_station(ink, p.x, p.y);
                }
            }
            Icon::SurveyLevelBook => {
                // A level on its tripod sighting a staff: the sight line is the
                // reading the book records.
                ink.stroke(&rounded(2.5, 7.5, 7.0, 3.5, 0.8), false, STROKE);
                ink.line(6.0, 11.0, 3.0, 21.0, false, FINE);
                ink.line(6.0, 11.0, 9.0, 21.0, false, FINE);
                // Solid: at 20 px a dashed sight line this short is a single dash.
                ink.line(10.0, 9.25, 15.5, 9.25, true, STROKE);
                ink.stroke(&rectangle(16.0, 3.0, 5.0, 18.0), false, STROKE);
                for y in [6.0, 10.0, 14.0, 18.0] {
                    ink.fill(&rectangle(16.0, y, 2.5, 2.0), false, TINT);
                }
            }
            Icon::SurveyConverter => {
                // A globe and a grid, and the conversion between them both ways.
                ink.stroke(&circle(7.5, 7.5, 5.0), false, STROKE);
                ink.line(2.5, 7.5, 12.5, 7.5, false, FINE);
                let mut meridian = Path::new();
                meridian.add_ellipse(RectF::new(7.5 - 2.2, 7.5 - 5.0, 2.0 * 2.2, 2.0 * 5.0));
                ink.stroke(&meridian, false, FINE);
                ink.stroke(&rectangle(13.0, 13.0, 8.0, 8.0), false, STROKE);
                ink.line(17.0, 13.0, 17.0, 21.0, false, FINE);
                ink.line(13.0, 17.0, 21.0, 17.0, false, FINE);
                let mut there = start(14.5, 4.5);
                there.quad_to(pt(18.5, 4.5), pt(18.5, 10.5));
                ink.stroke(&there, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(16.5, 8.5), pt(18.5, 10.8), pt(20.5, 8.5)], false),
                    true,
                    STROKE,
                );
                let mut back = start(10.5, 19.5);
                back.quad_to(pt(5.5, 19.5), pt(5.5, 13.5));
                ink.stroke(&back, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(3.5, 15.5), pt(5.5, 13.2), pt(7.5, 15.5)], false),
                    true,
                    STROKE,
                );
            }
            Icon::SurveyImport => {
                // A file's rows, and the arrow INTO the drawing to the new point it
                // makes (the accent), beside a point already there.
                ink.stroke(&rounded(2.5, 3.0, 8.5, 12.0, 0.8), false, STROKE);
                for y in [6.5, 9.0, 11.5] {
                    ink.line(4.5, y, 9.0, y, false, FINE);
                }
                ink.line(12.5, 9.0, 17.6, 9.0, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(15.3, 6.8), pt(17.8, 9.0), pt(15.3, 11.2)], false),
                    true,
                    STROKE,
                );
                draw_mark(ink, 20.2, 9.0, true);
                draw_mark(ink, 15.5, 18.5, false);
            }
            Icon::SurveyExport => {
                // A point of the drawing, and the arrow OUT to the file it writes
                // (the file's rows are the accent).
                draw_mark(ink, 3.8, 9.0, false);
                draw_mark(ink, 8.5, 18.5, false);
                ink.line(6.4, 9.0, 11.3, 9.0, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(9.0, 6.8), pt(11.5, 9.0), pt(9.0, 11.2)], false),
                    true,
                    STROKE,
                );
                ink.stroke(&rounded(13.0, 3.0, 8.5, 12.0, 0.8), false, STROKE);
                for y in [6.5, 9.0, 11.5] {
                    ink.line(15.0, y, 19.5, y, true, FINE);
                }
            }
            Icon::SurveyPointManager => {
                // A table of points: a header row, a column of marks (the accent),
                // and rows of values.
                ink.stroke(&rounded(2.5, 3.5, 19.0, 17.0, 0.8), false, STROKE);
                ink.line(2.5, 8.0, 21.5, 8.0, false, FINE);
                ink.line(9.0, 3.5, 9.0, 20.5, false, FINE);
                for y in [11.2, 14.7, 18.2] {
                    ink.dot(5.7, y, 1.1, true);
                    ink.line(11.0, y, 19.5, y, false, FINE);
                }
            }
            Icon::SurveyPointReport => {
                // A page listing points: a mark heading it, then lines of text.
                ink.stroke(&rounded(4.0, 2.5, 16.0, 19.0, 0.8), false, STROKE);
                draw_mark(ink, 8.0, 7.0, true);
                ink.line(12.0, 6.0, 17.5, 6.0, false, FINE);
                ink.line(12.0, 8.5, 16.0, 8.5, false, FINE);
                for y in [12.5, 15.5, 18.5] {
                    ink.line(6.5, y, 17.5, y, false, FINE);
                }
            }
            Icon::FormatStyles => {
                // A linetype table's three rows: continuous, dashed, and in the
                // accent the one the manager is for - a library linestyle, dashed, with
                // a symbol (a ring) drawn at its vertex.
                ink.line(3.0, 5.5, 21.0, 5.5, false, STROKE);
                ink.dashed(&polyline(&[pt(3.0, 11.5), pt(21.0, 11.5)], false), false);
                ink.dashed(&polyline(&[pt(3.0, 18.5), pt(9.8, 18.5)], false), true);
                ink.dashed(&polyline(&[pt(14.2, 18.5), pt(21.0, 18.5)], false), true);
                ink.stroke(&circle(12.0, 18.5, 2.2), true, FINE);
            }
            Icon::FormatSymbols => {
                // Four plan symbols in a grid, as the library shows them: a survey
                // mark, a station, a cross and, in the accent, the one chosen.
                draw_mark(ink, 6.5, 6.5, false);
                draw_station(ink, 17.5, 7.0);
                ink.line(4.0, 15.0, 9.0, 20.0, false, FINE);
                ink.line(9.0, 15.0, 4.0, 20.0, false, FINE);
                ink.stroke(
                    &polyline(
                        &[
                            pt(17.5, 13.5),
                            pt(21.0, 17.5),
                            pt(17.5, 21.5),
                            pt(14.0, 17.5),
                        ],
                        true,
                    ),
                    true,
                    STROKE,
                );
                ink.dot(17.5, 17.5, 1.1, true);
            }
            Icon::FormatSurveyCodes => {
                // A field code's tag, and the linework and mark the code manager
                // turns it into (the accent).
                ink.stroke(
                    &polyline(
                        &[
                            pt(3.0, 4.0),
                            pt(13.0, 4.0),
                            pt(17.0, 8.0),
                            pt(13.0, 12.0),
                            pt(3.0, 12.0),
                        ],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.dot(6.0, 8.0, 1.1, false);
                ink.line(8.5, 8.0, 12.5, 8.0, false, FINE);
                ink.stroke(
                    &polyline(&[pt(3.0, 20.0), pt(9.0, 16.5), pt(15.0, 19.0)], false),
                    true,
                    STROKE,
                );
                draw_mark(ink, 19.2, 17.5, true);
            }
            Icon::Purge => {
                // A broom sweeping the unused away (the accent): not Erase's bin,
                // which deletes what is picked, where Purge clears what nothing uses.
                ink.line(20.5, 2.5, 12.5, 11.5, false, STROKE);
                ink.stroke(
                    &polyline(
                        &[pt(9.8, 9.6), pt(15.2, 13.8), pt(11.0, 20.5), pt(3.0, 16.5)],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.line(7.4, 13.3, 5.8, 17.9, false, FINE);
                ink.line(10.4, 15.0, 8.8, 19.4, false, FINE);
                ink.dot(16.5, 19.5, 1.3, true);
                ink.dot(20.0, 17.0, 1.3, true);
                ink.dot(20.5, 21.0, 1.3, true);
            }
            Icon::GlobalModify => {
                // Three kinds of object - a point, a line, an area - gathered by
                // the accent bracket, and the accent arrow into the one look every
                // one of them is given.
                ink.dot(4.5, 4.5, 1.6, false);
                ink.line(2.5, 12.0, 7.5, 12.0, false, STROKE);
                ink.stroke(
                    &polyline(
                        &[pt(2.5, 17.0), pt(7.5, 17.0), pt(7.5, 21.5), pt(2.5, 21.5)],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(
                        &[pt(10.0, 2.5), pt(12.0, 2.5), pt(12.0, 21.5), pt(10.0, 21.5)],
                        false,
                    ),
                    true,
                    STROKE,
                );
                ink.line(14.0, 12.0, 18.5, 12.0, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(16.2, 9.7), pt(18.5, 12.0), pt(16.2, 14.3)], false),
                    true,
                    STROKE,
                );
                ink.dot(21.0, 12.0, 1.9, true);
            }
            // ---- menu items: File and Edit -------------------------------------------------------
            Icon::RecentScripts => {
                ink.stroke(&rounded(2.5, 3.5, 15.0, 11.0, 1.5), false, STROKE);
                ink.stroke(
                    &polyline(&[pt(5.5, 7.0), pt(8.0, 9.0), pt(5.5, 11.0)], false),
                    false,
                    STROKE,
                );
                ink.line(9.5, 11.0, 12.5, 11.0, false, STROKE);
                ink.stroke(&circle(17.0, 16.5, 5.0), true, STROKE);
                ink.stroke(
                    &polyline(&[pt(17.0, 13.8), pt(17.0, 16.5), pt(19.2, 17.8)], false),
                    true,
                    STROKE,
                );
            }
            Icon::Quit => {
                ink.stroke(
                    &polyline(
                        &[
                            pt(13.0, 7.5),
                            pt(13.0, 3.0),
                            pt(4.0, 3.0),
                            pt(4.0, 21.0),
                            pt(13.0, 21.0),
                            pt(13.0, 16.5),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.line(8.5, 12.0, 21.0, 12.0, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(17.5, 8.5), pt(21.0, 12.0), pt(17.5, 15.5)], false),
                    true,
                    STROKE,
                );
            }
            Icon::Deselect => {
                ink.dashed(&rectangle(3.0, 3.0, 14.0, 14.0), false);
                ink.line(15.0, 15.0, 21.0, 21.0, true, STROKE);
                ink.line(21.0, 15.0, 15.0, 21.0, true, STROKE);
            }
            Icon::SelectById => {
                ink.dashed(&rectangle(3.0, 3.0, 18.0, 18.0), false);
                ink.line(10.2, 7.0, 9.2, 17.0, true, STROKE);
                ink.line(14.8, 7.0, 13.8, 17.0, true, STROKE);
                ink.line(7.0, 10.3, 17.2, 10.3, true, STROKE);
                ink.line(6.6, 13.9, 16.8, 13.9, true, STROKE);
            }
            Icon::CopyViewImage => {
                ink.stroke(
                    &polyline(
                        &[pt(3.0, 14.5), pt(3.0, 3.0), pt(16.0, 3.0), pt(16.0, 6.5)],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(&rounded(7.5, 8.0, 14.0, 12.0, 1.0), true, STROKE);
                ink.stroke(
                    &polyline(
                        &[
                            pt(9.5, 18.0),
                            pt(13.0, 13.5),
                            pt(15.5, 16.0),
                            pt(17.5, 14.0),
                            pt(20.0, 18.0),
                        ],
                        false,
                    ),
                    true,
                    FINE,
                );
            }
            Icon::Attributes => {
                // A tree of attributes: the group at the top, its values beneath.
                ink.dot(5.0, 5.0, 1.8, false);
                ink.line(8.5, 5.0, 20.5, 5.0, false, STROKE);
                ink.line(5.0, 5.0, 5.0, 18.5, false, STROKE);
                ink.line(5.0, 11.5, 9.0, 11.5, false, STROKE);
                ink.line(5.0, 18.5, 9.0, 18.5, false, STROKE);
                ink.line(11.5, 11.5, 20.5, 11.5, true, STROKE);
                ink.line(11.5, 18.5, 20.5, 18.5, true, STROKE);
            }
            // ---- menu items: View, the snap modes ------------------------------------------------
            Icon::SnapEndpoint => {
                ink.line(3.0, 21.0, 16.0, 8.0, false, STROKE);
                ink.stroke(&rectangle(13.0, 5.0, 6.0, 6.0), true, STROKE);
            }
            Icon::SnapMidpoint => {
                ink.line(3.0, 19.0, 21.0, 5.0, false, STROKE);
                ink.stroke(
                    &polyline(&[pt(12.0, 7.8), pt(15.8, 14.4), pt(8.2, 14.4)], true),
                    true,
                    STROKE,
                );
            }
            Icon::SnapCenter => {
                ink.stroke(&circle(12.0, 12.0, 8.5), false, STROKE);
                ink.stroke(&circle(12.0, 12.0, 3.0), true, STROKE);
            }
            Icon::SnapIntersection => {
                ink.line(3.0, 6.0, 21.0, 18.0, false, STROKE);
                ink.line(3.0, 18.0, 21.0, 6.0, false, STROKE);
                ink.line(9.0, 9.0, 15.0, 15.0, true, STROKE);
                ink.line(9.0, 15.0, 15.0, 9.0, true, STROKE);
            }
            Icon::SnapPerpendicular => {
                ink.line(3.0, 19.0, 21.0, 19.0, false, STROKE);
                ink.line(10.0, 4.0, 10.0, 19.0, false, STROKE);
                ink.stroke(
                    &polyline(&[pt(10.0, 14.5), pt(14.5, 14.5), pt(14.5, 19.0)], false),
                    true,
                    STROKE,
                );
            }
            Icon::SnapTangent => {
                ink.stroke(&circle(11.0, 14.5, 6.5), false, STROKE);
                ink.line(2.0, 8.0, 22.0, 8.0, false, STROKE);
                ink.stroke(&circle(11.0, 8.0, 2.4), true, STROKE);
            }
            Icon::SnapNearest => {
                let mut curve = start(3.0, 19.0);
                curve.cubic_to(pt(8.0, 8.0), pt(15.0, 20.0), pt(21.0, 7.0));
                ink.stroke(&curve, false, STROKE);
                ink.stroke(
                    &polyline(
                        &[pt(9.0, 9.5), pt(15.0, 9.5), pt(9.0, 16.5), pt(15.0, 16.5)],
                        true,
                    ),
                    true,
                    STROKE,
                );
            }
            Icon::SnapGrid => {
                for x in [5.0, 12.0, 19.0] {
                    for y in [5.0, 12.0, 19.0] {
                        ink.dot(x, y, 1.2, false);
                    }
                }
                ink.line(12.0, 7.5, 12.0, 16.5, true, STROKE);
                ink.line(7.5, 12.0, 16.5, 12.0, true, STROKE);
            }
            Icon::ThinLines => {
                // Three line weights, heavy to thin, the thin one bright: the one icon whose
                // meaning is weight, so the one that draws other weights than STROKE and FINE.
                ink.line(3.0, 7.0, 21.0, 7.0, false, 2.0 * STROKE);
                ink.line(3.0, 13.0, 21.0, 13.0, false, STROKE);
                ink.line(3.0, 18.5, 21.0, 18.5, true, FINE);
            }
            // ---- the viewport layouts: the main view in the accent ------------------------------
            Icon::LayoutSingle => {
                ink.fill(&rounded(2.5, 4.0, 19.0, 16.0, 1.5), true, TINT);
                draw_frame(ink);
            }
            Icon::LayoutSplitVertical => {
                ink.fill(&rectangle(2.5, 4.0, 9.5, 16.0), true, TINT);
                draw_frame(ink);
                ink.line(12.0, 4.0, 12.0, 20.0, false, STROKE);
            }
            Icon::LayoutSplitHorizontal => {
                ink.fill(&rectangle(2.5, 4.0, 19.0, 8.0), true, TINT);
                draw_frame(ink);
                ink.line(2.5, 12.0, 21.5, 12.0, false, STROKE);
            }
            Icon::LayoutThreeLeft => {
                ink.fill(&rectangle(2.5, 4.0, 9.5, 16.0), true, TINT);
                draw_frame(ink);
                ink.line(12.0, 4.0, 12.0, 20.0, false, STROKE);
                ink.line(12.0, 12.0, 21.5, 12.0, false, STROKE);
            }
            Icon::LayoutThreeTop => {
                ink.fill(&rectangle(2.5, 4.0, 19.0, 8.0), true, TINT);
                draw_frame(ink);
                ink.line(2.5, 12.0, 21.5, 12.0, false, STROKE);
                ink.line(12.0, 12.0, 12.0, 20.0, false, STROKE);
            }
            Icon::LayoutQuad => {
                ink.fill(&rectangle(2.5, 4.0, 9.5, 8.0), true, TINT);
                draw_frame(ink);
                ink.line(12.0, 4.0, 12.0, 20.0, false, STROKE);
                ink.line(2.5, 12.0, 21.5, 12.0, false, STROKE);
            }
            // ---- the standard 3D views ----------------------------------------------------------
            Icon::ViewTop => {
                ink.fill(&rectangle(7.0, 7.0, 10.0, 10.0), true, TINT);
                ink.stroke(&rectangle(7.0, 7.0, 10.0, 10.0), true, STROKE);
                ink.dot(12.0, 12.0, 1.3, false);
            }
            Icon::ViewBottom => {
                ink.fill(&rectangle(7.0, 7.0, 10.0, 10.0), true, TINT);
                ink.dashed(&rectangle(7.0, 7.0, 10.0, 10.0), true);
            }
            Icon::ViewFront => {
                draw_orthographic_view(ink, 0.0, -1.0); // from the south, looking north
            }
            Icon::ViewBack => {
                draw_orthographic_view(ink, 0.0, 1.0);
            }
            Icon::ViewLeft => {
                draw_orthographic_view(ink, 1.0, 0.0); // from the west, looking east
            }
            Icon::ViewRight => {
                draw_orthographic_view(ink, -1.0, 0.0);
            }
            Icon::ViewIsoSouthWest => {
                draw_isometric_view(ink, 1.0, -1.0);
            }
            Icon::ViewIsoSouthEast => {
                draw_isometric_view(ink, -1.0, -1.0);
            }
            Icon::ViewIsoNorthEast => {
                draw_isometric_view(ink, -1.0, 1.0);
            }
            Icon::ViewIsoNorthWest => {
                draw_isometric_view(ink, 1.0, 1.0);
            }
            Icon::Perspective => {
                // Rails running to a vanishing point, the ties shortening.
                ink.line(3.0, 21.0, 10.5, 4.0, true, STROKE);
                ink.line(21.0, 21.0, 13.5, 4.0, true, STROKE);
                ink.line(4.8, 17.0, 19.2, 17.0, false, STROKE);
                ink.line(6.8, 12.5, 17.2, 12.5, false, STROKE);
                ink.line(8.6, 8.5, 15.4, 8.5, false, STROKE);
            }
            Icon::VerticalExaggeration => {
                ink.stroke(
                    &polyline(
                        &[
                            pt(2.5, 20.0),
                            pt(7.0, 16.5),
                            pt(11.0, 18.5),
                            pt(15.5, 13.5),
                            pt(21.5, 15.5),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.line(12.0, 3.0, 12.0, 11.5, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(9.5, 5.5), pt(12.0, 3.0), pt(14.5, 5.5)], false),
                    true,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(9.5, 9.0), pt(12.0, 11.5), pt(14.5, 9.0)], false),
                    true,
                    STROKE,
                );
            }
            // ---- the window ----------------------------------------------------------------------
            Icon::Panels => {
                ink.fill(&rectangle(2.5, 4.0, 6.0, 16.0), true, TINT);
                draw_frame(ink);
                ink.line(8.5, 4.0, 8.5, 20.0, false, STROKE);
                ink.line(17.0, 4.0, 17.0, 20.0, false, STROKE);
            }
            Icon::Toolbars => {
                ink.fill(&rectangle(2.5, 4.0, 19.0, 5.0), true, TINT);
                draw_frame(ink);
                ink.line(2.5, 9.0, 21.5, 9.0, false, STROKE);
                for x in [6.0, 10.0, 14.0] {
                    ink.dot(x, 6.5, 1.0, false);
                }
            }
            Icon::TextSize => {
                draw_letter_a(ink, 9.0, 20.5, 16.5, false);
                draw_letter_a(ink, 18.5, 20.5, 9.0, true);
            }
            Icon::ResetLayout => {
                ink.stroke(
                    &polyline(
                        &[
                            pt(12.0, 15.0),
                            pt(2.5, 15.0),
                            pt(2.5, 3.5),
                            pt(17.0, 3.5),
                            pt(17.0, 9.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.line(8.0, 3.5, 8.0, 15.0, false, STROKE);
                let mut turn = Path::new();
                let round = RectF::new(11.5, 11.0, 10.0, 10.0);
                turn.arc_move_to(round, 100.0);
                turn.arc_to(round, 100.0, 270.0);
                ink.stroke(&turn, true, STROKE);
                // The head where the sweep begins, pointing back along it.
                ink.stroke(
                    &polyline(&[pt(14.2, 9.7), pt(15.8, 11.1), pt(14.4, 13.0)], false),
                    true,
                    STROKE,
                );
            }
            // ---- annotation ----------------------------------------------------------------------
            Icon::EditText => {
                ink.line(3.5, 4.5, 14.5, 4.5, false, STROKE);
                ink.line(9.0, 4.5, 9.0, 18.5, false, STROKE);
                draw_pencil(ink);
            }
            Icon::EditLabel => {
                draw_note(ink, 2.5, 3.0, false);
                ink.line(5.0, 10.0, 3.5, 19.0, false, STROKE);
                ink.dot(3.5, 19.5, 1.3, false);
                draw_pencil(ink);
            }
            Icon::LabelLayout => {
                ink.stroke(
                    &polyline(
                        &[pt(5.0, 3.0), pt(19.0, 3.0), pt(19.0, 21.0), pt(5.0, 21.0)],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.line(8.0, 16.0, 16.0, 16.0, false, STROKE);
                ink.line(8.0, 18.5, 13.0, 18.5, false, STROKE);
                draw_note(ink, 6.5, 5.5, true);
            }
            Icon::LeaderManager => {
                ink.stroke(
                    &polyline(&[pt(4.0, 20.0), pt(9.5, 11.0), pt(12.5, 11.0)], false),
                    false,
                    STROKE,
                );
                ink.fill(
                    &polyline(&[pt(4.0, 20.0), pt(4.4, 16.2), pt(7.1, 17.9)], true),
                    false,
                    255,
                );
                ink.stroke(&rounded(12.5, 5.0, 9.0, 12.0, 1.5), true, STROKE);
                ink.line(14.8, 9.0, 19.2, 9.0, false, STROKE);
                ink.line(14.8, 13.0, 19.2, 13.0, false, STROKE);
            }
            Icon::LeadersForSelection => {
                ink.dashed(&rectangle(2.5, 12.5, 8.5, 9.0), false);
                ink.stroke(
                    &polyline(&[pt(7.0, 16.5), pt(13.0, 8.0), pt(16.0, 8.0)], false),
                    true,
                    STROKE,
                );
                draw_note(ink, 11.5, 2.5, true);
            }
            Icon::ArrangeLeaders => {
                ink.line(11.0, 2.0, 11.0, 22.0, true, FINE);
                for y in [3.0, 10.0, 17.0] {
                    ink.stroke(&rounded(12.0, y, 9.5, 4.5, 1.0), false, STROKE);
                }
                ink.line(3.0, 7.5, 12.0, 5.25, false, STROKE);
                ink.line(4.5, 13.5, 12.0, 12.25, false, STROKE);
                ink.line(2.5, 20.5, 12.0, 19.25, false, STROKE);
            }
            Icon::TextStyles => {
                draw_letter_a(ink, 8.5, 20.0, 15.0, false);
                ink.line(15.5, 7.0, 21.5, 7.0, true, STROKE);
                ink.line(15.5, 12.0, 21.5, 12.0, true, STROKE);
                ink.line(15.5, 17.0, 21.5, 17.0, true, FINE);
            }
            Icon::LabelStyles => {
                draw_note(ink, 2.0, 4.0, false);
                ink.line(4.5, 11.0, 3.0, 19.0, false, STROKE);
                ink.line(15.5, 12.0, 21.5, 12.0, true, STROKE);
                ink.line(15.5, 16.0, 21.5, 16.0, true, STROKE);
                ink.line(15.5, 20.0, 21.5, 20.0, true, FINE);
            }
            Icon::DimensionStyles => {
                ink.line(4.0, 21.0, 4.0, 8.0, false, STROKE);
                ink.line(20.0, 21.0, 20.0, 8.0, false, STROKE);
                ink.line(4.0, 11.0, 20.0, 11.0, true, STROKE);
                draw_head(ink, 4.0, 11.0, -1.0, 0.0);
                draw_head(ink, 20.0, 11.0, 1.0, 0.0);
                ink.line(9.0, 6.0, 15.0, 6.0, false, STROKE);
            }
            Icon::LayerVisible => {
                let mut eye = start(2.5, 12.0);
                eye.cubic_to(pt(7.0, 4.5), pt(17.0, 4.5), pt(21.5, 12.0));
                eye.cubic_to(pt(17.0, 19.5), pt(7.0, 19.5), pt(2.5, 12.0));
                ink.stroke(&eye, false, STROKE);
                ink.stroke(&circle(12.0, 12.0, 3.4), true, STROKE);
                ink.dot(12.0, 12.0, 1.3, true);
            }
            Icon::LayerLocked => {
                let mut shackle = start(7.5, 11.0);
                shackle.line_to(pt(7.5, 8.0));
                shackle.cubic_to(pt(7.5, 2.2), pt(16.5, 2.2), pt(16.5, 8.0));
                shackle.line_to(pt(16.5, 11.0));
                ink.stroke(&shackle, false, STROKE);
                ink.stroke(&rounded(4.5, 11.0, 15.0, 10.0, 1.5), true, STROKE);
                ink.dot(12.0, 16.0, 1.4, true);
            }
            Icon::LayerColour => {
                // Four swatches, two of them tinted: a palette, in outline. The bright one
                // was a solid square, the one filled block among the panel's column heads.
                ink.fill(&rounded(3.0, 3.0, 8.0, 8.0, 1.5), false, TINT);
                ink.fill(&rounded(13.0, 13.0, 8.0, 8.0, 1.5), true, TINT);
                ink.stroke(&rounded(3.0, 3.0, 8.0, 8.0, 1.5), false, STROKE);
                ink.stroke(&rounded(13.0, 3.0, 8.0, 8.0, 1.5), false, STROKE);
                ink.stroke(&rounded(3.0, 13.0, 8.0, 8.0, 1.5), false, STROKE);
                ink.stroke(&rounded(13.0, 13.0, 8.0, 8.0, 1.5), true, STROKE);
            }
            Icon::ViewUnlinked => {
                // Two links a gap apart: at 14 px the gap is two pixels, which is
                // what tells the pair from the closed chain beside it.
                ink.stroke(&chain_link(7.2, 16.8, 9.0, 6.0), false, STROKE);
                ink.stroke(&chain_link(16.8, 7.2, 9.0, 6.0), false, STROKE);
            }
            Icon::ViewLinked => {
                // Interlocked: each link runs a third of its length into the other.
                ink.stroke(&chain_link(9.3, 14.7, 11.5, 6.2), true, STROKE);
                ink.stroke(&chain_link(14.7, 9.3, 11.5, 6.2), true, STROKE);
            }
            Icon::ZoomIn | Icon::ZoomOut => {
                // Zoom To's lens and handle, with the sign where its square was.
                ink.stroke(&circle(10.0, 10.0, 6.5), false, STROKE);
                ink.line(14.8, 14.8, 20.5, 20.5, false, STROKE);
                ink.line(7.0, 10.0, 13.0, 10.0, true, STROKE);
                if which == Icon::ZoomIn {
                    ink.line(10.0, 7.0, 10.0, 13.0, true, STROKE);
                }
            }
            Icon::ZoomSelection => {
                // The selection's dashed box, as Select All draws it, under the lens.
                ink.dashed(&rectangle(3.0, 3.0, 12.5, 12.5), true);
                ink.stroke(&circle(15.0, 15.0, 4.6), false, STROKE);
                ink.line(18.3, 18.3, 21.5, 21.5, false, STROKE);
            }
            Icon::Settings => {
                // Three sliders, each knob somewhere else along its track, so that at
                // 16 px the three do not line up into a bar.
                for knob in [pt(15.5, 6.0), pt(8.0, 12.0), pt(13.0, 18.0)] {
                    ink.line(3.5, knob.y, 20.5, knob.y, false, STROKE);
                    ink.dot(knob.x, knob.y, 2.5, true);
                }
            }
            Icon::SurveyLinework => {
                // The line first, so that each mark's ring is drawn over its end.
                ink.stroke(
                    &polyline(&[pt(4.5, 17.5), pt(11.5, 6.5), pt(19.5, 14.5)], false),
                    true,
                    STROKE,
                );
                draw_mark(ink, 4.5, 17.5, false);
                draw_mark(ink, 11.5, 6.5, false);
                draw_mark(ink, 19.5, 14.5, false);
            }
            // ---- volumes (docs/volumes.md) ----
            Icon::Volumes => {
                // A block seen from above one corner: the top face filled, so at
                // 16 px it reads as a solid, not a wire box.
                let top = polyline(
                    &[pt(12.0, 3.0), pt(21.0, 7.5), pt(12.0, 12.0), pt(3.0, 7.5)],
                    true,
                );
                ink.fill(&top, true, TINT);
                ink.stroke(&top, true, STROKE);
                ink.stroke(
                    &polyline(
                        &[
                            pt(3.0, 7.5),
                            pt(3.0, 16.5),
                            pt(12.0, 21.0),
                            pt(21.0, 16.5),
                            pt(21.0, 7.5),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.line(12.0, 12.0, 12.0, 21.0, false, STROKE);
            }
            Icon::CutFillMap => {
                // The map's frame, the fill side shaded in the accent, the cut side
                // faintly in the neutral, and the line where they meet.
                let mut fill_side = start(3.5, 3.5);
                fill_side.line_to(pt(11.0, 3.5));
                fill_side.cubic_to(pt(14.0, 10.0), pt(9.0, 14.0), pt(13.0, 20.5));
                fill_side.line_to(pt(3.5, 20.5));
                fill_side.close_subpath();
                let mut cut_side = start(11.0, 3.5);
                cut_side.line_to(pt(20.5, 3.5));
                cut_side.line_to(pt(20.5, 20.5));
                cut_side.line_to(pt(13.0, 20.5));
                cut_side.cubic_to(pt(9.0, 14.0), pt(14.0, 10.0), pt(11.0, 3.5));
                ink.fill(&fill_side, true, TINT);
                ink.fill(&cut_side, false, TINT);
                let mut meet = start(11.0, 3.5);
                meet.cubic_to(pt(14.0, 10.0), pt(9.0, 14.0), pt(13.0, 20.5));
                ink.stroke(&meet, true, STROKE);
                ink.stroke(
                    &polyline(
                        &[pt(3.5, 3.5), pt(20.5, 3.5), pt(20.5, 20.5), pt(3.5, 20.5)],
                        true,
                    ),
                    false,
                    STROKE,
                );
            }
            // ---- end of volumes ----
            // ---- road objects (docs/road_objects.md): bim/road_object_icons.cpp ----
            Icon::RoadBarrier
            | Icon::RoadGuidePosts
            | Icon::RoadSign
            | Icon::RoadLightPole
            | Icon::RoadCulvert
            | Icon::RoadWall
            | Icon::RoadFence
            | Icon::RoadLineMarking
            | Icon::RoadObjects => {
                let _ = paint_road_object_icon(which, ink);
            }
            // ---- end road objects ----
            // ---- surface editing (docs/surface_editing.md) ----
            Icon::SurfaceEdit => {
                // Four triangles round a centre vertex, the vertex in the accent. The TIN in the
                // fine weight, as its tools draw it: in the full stroke its rim and spokes were a
                // crossed square at a menu's 16 px.
                draw_tin(ink, true);
                ink.dot(12.0, 12.0, 2.6, true);
            }
            Icon::SurfaceLevels => {
                ink.stroke(
                    &polyline(
                        &[pt(2.5, 18.0), pt(8.0, 15.0), pt(14.0, 17.0), pt(21.5, 13.0)],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.line(12.0, 3.0, 12.0, 12.5, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(9.0, 6.0), pt(12.0, 3.0), pt(15.0, 6.0)], false),
                    true,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(9.0, 9.5), pt(12.0, 12.5), pt(15.0, 9.5)], false),
                    true,
                    STROKE,
                );
            }
            Icon::SurfaceSmooth => {
                ink.stroke(
                    &polyline(
                        &[
                            pt(2.5, 14.0),
                            pt(6.0, 8.0),
                            pt(9.0, 17.0),
                            pt(13.0, 7.0),
                            pt(17.0, 16.0),
                            pt(21.5, 10.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                let mut smooth = start(2.5, 13.0);
                smooth.cubic_to(pt(8.0, 10.0), pt(14.0, 15.0), pt(21.5, 11.0));
                ink.stroke(&smooth, true, STROKE);
            }
            Icon::SurfaceThin => {
                ink.line(3.0, 15.0, 21.0, 11.0, false, STROKE);
                ink.node(3.0, 15.0, false);
                ink.node(21.0, 11.0, false);
                for x in [9.0, 15.0] {
                    let y = 15.0 - (x - 3.0) * 4.0 / 18.0;
                    ink.dot(x, y, 1.8, true);
                    ink.line(x - 2.5, y - 2.5, x + 2.5, y + 2.5, true, FINE);
                }
            }
            Icon::SurfaceDeleteLong => {
                ink.stroke(
                    &polyline(&[pt(2.5, 17.0), pt(21.5, 17.0), pt(12.0, 13.5)], true),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(12.0, 13.5), pt(7.0, 5.0), pt(17.0, 5.0)], true),
                    false,
                    STROKE,
                );
                ink.line(5.0, 21.0, 19.0, 10.0, true, STROKE);
            }
            Icon::SurfaceCombine => {
                ink.stroke(&rectangle(3.0, 4.0, 18.0, 16.0), false, STROKE);
                ink.fill(&rectangle(9.0, 9.0, 8.0, 7.0), true, TINT);
                ink.stroke(&rectangle(9.0, 9.0, 8.0, 7.0), true, STROKE);
            }
            Icon::SurfacesPanel => {
                // Four vertices of a TIN and the triangles between them; the middle
                // one filled in the accent, the row picked in the panel.
                let picked = polyline(&[pt(9.0, 7.0), pt(16.0, 14.0), pt(7.0, 17.0)], true);
                ink.fill(&picked, true, TINT);
                ink.stroke(
                    &polyline(
                        &[
                            pt(3.0, 12.0),
                            pt(9.0, 7.0),
                            pt(19.0, 4.0),
                            pt(21.0, 15.0),
                            pt(16.0, 14.0),
                            pt(13.0, 21.0),
                            pt(7.0, 17.0),
                        ],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(9.0, 7.0), pt(16.0, 14.0), pt(19.0, 4.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(3.0, 12.0), pt(7.0, 17.0), pt(9.0, 7.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(7.0, 17.0), pt(16.0, 14.0)], false),
                    false,
                    STROKE,
                );
            }
            // ---- road model (docs/road_design.md) ----
            Icon::RoadTemplate => {
                // The course under the road, then the crowned surface over it in the
                // accent, rising to a kerb at each edge.
                let course = polyline(
                    &[
                        pt(4.5, 13.0),
                        pt(12.0, 11.0),
                        pt(19.5, 13.0),
                        pt(19.5, 16.5),
                        pt(12.0, 14.5),
                        pt(4.5, 16.5),
                    ],
                    true,
                );
                ink.fill(&course, false, TINT);
                ink.stroke(&course, false, STROKE);
                ink.stroke(
                    &polyline(&[pt(4.5, 13.0), pt(12.0, 11.0), pt(19.5, 13.0)], false),
                    true,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(4.5, 13.0), pt(4.5, 9.5), pt(2.0, 9.5)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(19.5, 13.0), pt(19.5, 9.5), pt(22.0, 9.5)], false),
                    false,
                    STROKE,
                );
            }
            Icon::Corridor => {
                let mut left = start(3.0, 21.0);
                left.quad_to(pt(4.0, 9.0), pt(15.0, 3.0));
                let mut right = start(11.0, 21.0);
                right.quad_to(pt(12.0, 13.0), pt(21.0, 8.0));
                ink.stroke(&left, false, STROKE);
                ink.stroke(&right, false, STROKE);
                let mut centre = start(7.0, 21.0);
                centre.quad_to(pt(8.0, 11.0), pt(18.0, 5.5));
                ink.dashed(&centre, true);
            }
            Icon::RoadModel => {
                // A block seen from above one corner: its top in the accent.
                let top = polyline(
                    &[pt(12.0, 4.0), pt(20.0, 8.0), pt(12.0, 12.0), pt(4.0, 8.0)],
                    true,
                );
                ink.fill(&top, true, TINT);
                ink.stroke(&top, true, STROKE);
                ink.stroke(
                    &polyline(
                        &[
                            pt(4.0, 8.0),
                            pt(4.0, 16.0),
                            pt(12.0, 20.0),
                            pt(20.0, 16.0),
                            pt(20.0, 8.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.line(12.0, 12.0, 12.0, 20.0, false, STROKE);
            }
            // ---- end road model ----
            Icon::SurfaceStyles => {
                // The band's corners lie on the outline's sides, to a twentieth of a
                // unit: the left side x = 3 + 6 (17 - y) / 11 is 5.51 at y = 12.4 and
                // 7.47 at 8.8, the right x = 21 - 4 (y - 8) / 12 is 20.3 at 10.1 and
                // 19.1 at 13.7 - so the fill stops at the outline at every size.
                ink.fill(
                    &polyline(
                        &[pt(5.5, 12.4), pt(7.5, 8.8), pt(20.3, 10.1), pt(19.1, 13.7)],
                        true,
                    ),
                    true,
                    TINT,
                );
                ink.stroke(
                    &polyline(
                        &[pt(3.0, 17.0), pt(9.0, 6.0), pt(21.0, 8.0), pt(17.0, 20.0)],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.line(9.0, 6.0, 17.0, 20.0, false, FINE);
                ink.line(5.5, 12.4, 19.1, 13.7, true, FINE);
                ink.line(7.5, 8.8, 20.3, 10.1, true, FINE);
            }
            Icon::Download => {
                // The cloud is left open underneath, where the arrow comes out of it,
                // so that at 16 px the shaft does not cross a third line at its base.
                let mut cloud = start(7.5, 13.0);
                cloud.cubic_to(pt(3.5, 13.0), pt(3.5, 7.5), pt(7.5, 7.5));
                cloud.cubic_to(pt(8.0, 3.5), pt(15.0, 3.0), pt(16.0, 7.0));
                cloud.cubic_to(pt(20.5, 6.5), pt(21.0, 13.0), pt(16.5, 13.0));
                ink.stroke(&cloud, false, STROKE);
                ink.line(12.0, 10.0, 12.0, 20.5, true, STROKE);
                ink.stroke(
                    &polyline(&[pt(8.5, 17.0), pt(12.0, 20.5), pt(15.5, 17.0)], false),
                    true,
                    STROKE,
                );
            }
            // ---- icons of their own (the look of 2026-11-16, round two) ----
            Icon::Sheets => {
                // The sheet behind shows its top and right edges only, so the two
                // outlines never cross at 16 px; the front sheet's title block - what
                // makes a page a sheet of a set - is the bright part.
                ink.stroke(
                    &polyline(
                        &[
                            pt(7.5, 6.5),
                            pt(7.5, 3.0),
                            pt(20.5, 3.0),
                            pt(20.5, 17.0),
                            pt(16.5, 17.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(&rounded(3.5, 6.5, 13.0, 15.0, 0.8), false, STROKE);
                ink.line(3.5, 17.0, 16.5, 17.0, true, FINE);
                ink.line(10.5, 17.0, 10.5, 21.5, true, FINE);
                ink.line(6.0, 10.0, 14.0, 10.0, false, FINE);
                ink.line(6.0, 13.0, 11.5, 13.0, false, FINE);
            }
            Icon::PlotSheets => {
                // Plot's printer and its paper; out of it, where Plot has one page, a
                // page with another behind it.
                ink.stroke(
                    &polyline(
                        &[pt(7.0, 9.0), pt(7.0, 4.0), pt(17.0, 4.0), pt(17.0, 9.0)],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(
                        &[
                            pt(5.5, 16.0),
                            pt(3.0, 16.0),
                            pt(3.0, 9.0),
                            pt(21.0, 9.0),
                            pt(21.0, 16.0),
                            pt(19.5, 16.0),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(
                        &[
                            pt(9.5, 13.0),
                            pt(9.5, 11.5),
                            pt(18.5, 11.5),
                            pt(18.5, 19.0),
                            pt(16.5, 19.0),
                        ],
                        false,
                    ),
                    true,
                    STROKE,
                );
                ink.stroke(&rectangle(5.5, 13.0, 11.0, 8.0), true, STROKE);
                ink.line(8.0, 17.0, 14.0, 17.0, true, FINE);
            }
            Icon::DrawingSummary => {
                // A clipboard of rows, each with its mark: the drawing at a glance.
                ink.stroke(&rounded(4.5, 4.0, 15.0, 17.5, 1.5), false, STROKE);
                ink.stroke(&rounded(9.0, 2.5, 6.0, 3.5, 1.0), false, STROKE);
                for y in [10.0, 14.0, 18.0] {
                    ink.dot(8.0, y, 1.2, true);
                    ink.line(10.5, y, 16.5, y, false, FINE);
                }
            }
            Icon::ProjectCrs => {
                // A north axis and an east axis from their origin, and in the bright tone
                // a point and the lines down to its two coordinates.
                ink.line(5.0, 19.5, 5.0, 4.0, false, STROKE);
                ink.stroke(
                    &polyline(&[pt(2.6, 6.6), pt(5.0, 4.0), pt(7.4, 6.6)], false),
                    false,
                    STROKE,
                );
                ink.line(4.5, 19.0, 20.0, 19.0, false, STROKE);
                ink.stroke(
                    &polyline(&[pt(17.4, 16.6), pt(20.0, 19.0), pt(17.4, 21.4)], false),
                    false,
                    STROKE,
                );
                ink.line(5.0, 10.0, 14.5, 10.0, true, FINE);
                ink.line(14.5, 10.0, 14.5, 19.0, true, FINE);
                ink.dot(14.5, 10.0, 2.2, true);
            }
            Icon::StartScreen => {
                // A window, a tile to start from in the bright tone, and the choices
                // listed beside it.
                draw_frame(ink);
                ink.line(2.5, 8.0, 21.5, 8.0, false, FINE);
                ink.stroke(&rounded(5.0, 10.5, 6.5, 6.5, 1.0), true, STROKE);
                ink.line(14.0, 11.5, 19.0, 11.5, false, FINE);
                ink.line(14.0, 14.0, 19.0, 14.0, false, FINE);
                ink.line(14.0, 16.5, 17.0, 16.5, false, FINE);
            }
            Icon::CommandReference => {
                // A page, the command line's prompt on it in the bright tone, then the
                // lines that say what the command does.
                ink.stroke(&rounded(4.0, 2.5, 16.0, 19.0, 0.8), false, STROKE);
                ink.stroke(
                    &polyline(&[pt(7.0, 6.5), pt(9.5, 8.75), pt(7.0, 11.0)], false),
                    true,
                    STROKE,
                );
                ink.line(11.0, 11.0, 16.0, 11.0, true, STROKE);
                ink.line(7.0, 15.0, 17.0, 15.0, false, FINE);
                ink.line(7.0, 18.0, 14.0, 18.0, false, FINE);
            }
            Icon::Keyboard => {
                ink.stroke(&rounded(2.5, 6.5, 19.0, 11.0, 1.5), false, STROKE);
                for x in [6.0, 9.0, 12.0, 15.0, 18.0] {
                    ink.dot(x, 9.8, 0.9, false);
                }
                for x in [7.5, 10.5, 13.5, 16.5] {
                    ink.dot(x, 12.4, 0.9, false);
                }
                ink.line(8.0, 15.0, 16.0, 15.0, true, STROKE);
            }
            Icon::ImportIfc | Icon::ExportIfc => {
                // A building element - a block, seen from above a corner - in the lower
                // left 15 units, as the GIS data's icons draw theirs, and the arrow down
                // the right.
                let top = polyline(
                    &[pt(8.5, 8.0), pt(14.5, 11.0), pt(8.5, 14.0), pt(2.5, 11.0)],
                    true,
                );
                ink.fill(&top, false, TINT);
                ink.stroke(
                    &polyline(
                        &[
                            pt(8.5, 8.0),
                            pt(14.5, 11.0),
                            pt(14.5, 17.5),
                            pt(8.5, 20.5),
                            pt(2.5, 17.5),
                            pt(2.5, 11.0),
                        ],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.line(8.5, 14.0, 8.5, 20.5, false, STROKE);
                ink.stroke(&top, false, STROKE);
                if which == Icon::ImportIfc {
                    draw_arrow_in(ink);
                } else {
                    draw_arrow_out(ink);
                }
            }
            Icon::ExportImage => {
                // A picture - its frame, a hill and the sun - and Export's arrow.
                ink.stroke(&rectangle(3.0, 8.0, 12.0, 12.0), false, STROKE);
                ink.stroke(
                    &polyline(
                        &[
                            pt(3.0, 18.0),
                            pt(7.0, 13.5),
                            pt(10.0, 16.5),
                            pt(12.0, 14.5),
                            pt(15.0, 17.5),
                        ],
                        false,
                    ),
                    false,
                    FINE,
                );
                ink.dot(11.5, 11.2, 1.2, false);
                draw_arrow_out(ink);
            }
            Icon::ApplyCodes => {
                // A field code's tag, and the bright arrow to the point it gives a
                // layer, a style and attributes.
                ink.stroke(
                    &polyline(
                        &[
                            pt(3.0, 4.0),
                            pt(11.0, 4.0),
                            pt(14.5, 7.5),
                            pt(11.0, 11.0),
                            pt(3.0, 11.0),
                        ],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.dot(6.0, 7.5, 1.0, false);
                let unit = 1.0 / 2.0_f64.sqrt();
                ink.line(12.0, 13.0, 16.4, 17.4, true, STROKE);
                draw_head(ink, 16.4, 17.4, unit, unit);
                draw_mark(ink, 19.3, 20.3, true);
            }
            Icon::SurveyParcelReport => {
                // The report's page, and the parcel it is on in front of it.
                ink.stroke(&rounded(10.0, 2.5, 11.0, 14.0, 0.8), false, STROKE);
                for y in [6.0, 9.0, 12.0] {
                    ink.line(12.5, y, 18.5, y, false, FINE);
                }
                let corners = [pt(3.0, 21.0), pt(4.0, 11.0), pt(10.5, 9.5), pt(14.0, 20.0)];
                let parcel = polyline(&corners, true);
                ink.fill(&parcel, true, TINT);
                ink.stroke(&parcel, false, STROKE);
                for p in corners {
                    ink.node(p.x, p.y, true);
                }
            }
            // ---- one icon an item (the third audit's G) ------------------------------------
            Icon::GisFormats => {
                // A page with its corner folded, two lines of it, and the tag of its format.
                ink.stroke(
                    &polyline(
                        &[
                            pt(10.0, 20.0),
                            pt(4.0, 20.0),
                            pt(4.0, 3.0),
                            pt(12.0, 3.0),
                            pt(16.0, 7.0),
                            pt(16.0, 11.5),
                        ],
                        false,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(12.0, 3.0), pt(12.0, 7.0), pt(16.0, 7.0)], false),
                    false,
                    FINE,
                );
                ink.line(6.5, 9.0, 11.0, 9.0, false, FINE);
                ink.line(6.5, 12.5, 11.0, 12.5, false, FINE);
                ink.stroke(&rounded(11.5, 14.0, 10.0, 7.0, 1.5), true, STROKE);
                ink.line(14.0, 17.5, 19.0, 17.5, true, FINE);
            }
            Icon::GisBuffer => {
                // The line and, round it at a distance, the band: a stadium, its ends the round
                // caps a buffer has.
                ink.line(7.5, 12.0, 16.5, 12.0, false, STROKE);
                ink.node(7.5, 12.0, false);
                ink.node(16.5, 12.0, false);
                ink.stroke(&rounded(2.5, 6.5, 19.0, 11.0, 5.5), true, STROKE);
            }
            Icon::GisDissolve => {
                let outline = [
                    pt(3.0, 8.0),
                    pt(12.0, 5.0),
                    pt(20.5, 7.0),
                    pt(21.0, 17.5),
                    pt(12.0, 19.0),
                    pt(4.0, 18.0),
                ];
                ink.fill(&polyline(&outline, true), true, TINT);
                ink.stroke(&polyline(&outline, true), true, STROKE);
                // The edge the two areas shared, gone.
                ink.dashed(&polyline(&[pt(12.0, 5.0), pt(12.0, 19.0)], false), false);
            }
            Icon::GisOverlay => {
                ink.fill(&rectangle(9.0, 9.0, 6.5, 6.5), true, TINT);
                ink.stroke(&rectangle(3.0, 3.5, 12.5, 12.0), false, STROKE);
                ink.stroke(&rectangle(9.0, 9.0, 12.0, 11.5), false, STROKE);
                ink.stroke(&rectangle(9.0, 9.0, 6.5, 6.5), true, STROKE);
            }
            Icon::GisHull => {
                for p in [
                    pt(7.5, 9.5),
                    pt(12.0, 7.5),
                    pt(16.0, 11.0),
                    pt(9.5, 14.0),
                    pt(14.0, 15.5),
                    pt(11.5, 11.0),
                ] {
                    ink.dot(p.x, p.y, 1.2, false);
                }
                ink.stroke(
                    &polyline(
                        &[pt(3.5, 9.0), pt(11.5, 3.5), pt(20.0, 9.5), pt(16.0, 20.0), pt(6.5, 18.0)],
                        true,
                    ),
                    true,
                    STROKE,
                );
            }
            Icon::GisClip => {
                let shape = polyline(
                    &[pt(3.0, 12.0), pt(9.0, 3.5), pt(18.5, 6.0), pt(17.0, 17.5), pt(6.0, 19.5)],
                    true,
                );
                ink.stroke(&shape, false, FINE);
                // What lies inside the frame: what the clip keeps.
                ink.canvas().save();
                ink.canvas().set_clip_rect(RectF::new(10.0, 10.0, 11.5, 11.5), true);
                ink.fill(&shape, true, TINT);
                ink.stroke(&shape, true, STROKE);
                ink.canvas().restore();
                ink.dashed(&rectangle(10.0, 10.0, 11.5, 11.5), false);
            }
            Icon::GisSql => {
                ink.stroke(&rectangle(2.5, 4.0, 12.0, 15.0), false, STROKE);
                ink.line(2.5, 8.5, 14.5, 8.5, false, FINE);
                ink.line(2.5, 12.0, 14.5, 12.0, false, FINE);
                ink.line(2.5, 15.5, 14.5, 15.5, false, FINE);
                ink.line(7.5, 4.0, 7.5, 19.0, false, FINE);
                let mut mark = start(15.8, 11.0);
                mark.cubic_to(pt(15.8, 8.8), pt(17.2, 7.5), pt(18.7, 7.5));
                mark.cubic_to(pt(20.3, 7.5), pt(21.6, 8.7), pt(21.6, 10.4));
                mark.cubic_to(pt(21.6, 12.6), pt(18.7, 12.9), pt(18.7, 15.4));
                ink.stroke(&mark, true, STROKE);
                ink.dot(18.7, 19.0, 1.2, true);
            }
            Icon::GisCheck => {
                ink.stroke(
                    &polyline(
                        &[pt(3.0, 13.0), pt(5.0, 4.0), pt(14.0, 3.0), pt(17.0, 8.5), pt(10.5, 13.5)],
                        true,
                    ),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(10.5, 16.5), pt(14.0, 20.0), pt(21.0, 12.0)], false),
                    true,
                    STROKE,
                );
            }
            Icon::GisRepair => {
                // A bow-tie: the area that crosses itself, which a repair makes two triangles.
                ink.stroke(
                    &polyline(&[pt(3.0, 3.5), pt(13.0, 13.5), pt(13.0, 3.5), pt(3.0, 13.5)], true),
                    false,
                    STROKE,
                );
                draw_wrench(ink, 18.0, 14.5, 225.0);
            }
            Icon::GisCoverage => {
                let gap = polyline(
                    &[pt(10.0, 4.0), pt(14.0, 4.0), pt(13.0, 20.0), pt(10.5, 20.0)],
                    true,
                );
                ink.fill(&gap, true, TINT);
                ink.stroke(
                    &polyline(&[pt(10.0, 4.0), pt(3.0, 4.0), pt(3.0, 20.0), pt(10.5, 20.0)], false),
                    false,
                    STROKE,
                );
                ink.stroke(
                    &polyline(&[pt(14.0, 4.0), pt(21.0, 4.0), pt(21.0, 20.0), pt(13.0, 20.0)], false),
                    false,
                    STROKE,
                );
                ink.line(10.0, 4.0, 10.5, 20.0, true, STROKE);
                ink.line(14.0, 4.0, 13.0, 20.0, true, STROKE);
            }
            Icon::TerrainContours => {
                ink.stroke(&ellipse(2.5, 4.0, 19.0, 16.0), false, STROKE);
                ink.stroke(&ellipse(6.0, 7.5, 12.0, 9.5), true, STROKE);
                ink.stroke(&ellipse(9.5, 10.5, 5.0, 3.5), false, FINE);
            }
            Icon::TerrainShading => {
                // The far flank's shade first, under the hill's outline.
                for (x, top) in [(14.5, 10.5), (17.0, 13.0), (19.5, 16.0)] {
                    ink.line(x, top, x, 19.5, false, FINE);
                }
                let mut hill = start(2.5, 19.5);
                hill.cubic_to(pt(6.5, 12.0), pt(9.0, 7.5), pt(12.0, 7.5));
                hill.cubic_to(pt(15.0, 7.5), pt(18.0, 13.0), pt(21.5, 19.5));
                ink.stroke(&hill, false, STROKE);
                ink.line(2.5, 19.5, 21.5, 19.5, false, STROKE);
                ink.fill(&circle(5.0, 5.5, 2.3), true, TINT);
                ink.stroke(&circle(5.0, 5.5, 2.3), true, STROKE);
            }
            Icon::TerrainSlope => {
                ink.stroke(
                    &polyline(&[pt(3.0, 19.0), pt(21.0, 19.0), pt(21.0, 7.0)], false),
                    false,
                    FINE,
                );
                ink.line(3.0, 19.0, 21.0, 7.0, false, STROKE);
                // The angle at the foot: atan(12 / 18) = 33.7 degrees.
                ink.stroke(&arc(3.0, 19.0, 7.0, 0.0, 33.7), true, STROKE);
            }
            Icon::TerrainZonal => {
                ink.stroke(
                    &polyline(
                        &[pt(3.0, 6.5), pt(13.0, 3.0), pt(21.0, 8.0), pt(19.5, 20.5), pt(4.5, 19.5)],
                        true,
                    ),
                    false,
                    STROKE,
                );
                for (x, top) in [(8.5, 13.0), (12.0, 9.5), (15.5, 12.0)] {
                    ink.line(x, top, x, 17.0, true, STROKE);
                }
            }
            Icon::TerrainDrape => {
                ink.stroke(
                    &polyline(&[pt(2.5, 18.5), pt(8.0, 15.5), pt(14.0, 17.5), pt(21.5, 13.5)], false),
                    false,
                    STROKE,
                );
                // Each point, and the drop to where it meets the ground line: two, since three
                // drops' heads ran together into a zigzag at 16 px.
                for (x, ground) in [(6.5, 16.5), (17.0, 15.9)] {
                    ink.dot(x, 4.0, 1.4, false);
                    ink.line(x, 6.5, x, ground - 1.2, true, STROKE);
                    draw_head(ink, x, ground - 1.2, 0.0, 1.0);
                }
            }
            Icon::TerrainViewshed => {
                let fan = polyline(&[pt(11.0, 8.0), pt(21.5, 3.5), pt(21.5, 13.0)], true);
                ink.fill(&fan, true, TINT);
                ink.line(11.0, 8.0, 21.5, 3.5, true, FINE);
                ink.line(11.0, 8.0, 21.5, 13.0, true, FINE);
                let mut eye = start(2.0, 8.0);
                eye.cubic_to(pt(4.0, 4.8), pt(9.0, 4.8), pt(11.0, 8.0));
                eye.cubic_to(pt(9.0, 11.2), pt(4.0, 11.2), pt(2.0, 8.0));
                ink.stroke(&eye, false, STROKE);
                ink.dot(6.5, 8.0, 1.3, false);
                ink.stroke(
                    &polyline(&[pt(2.5, 20.0), pt(9.0, 16.5), pt(15.0, 18.5), pt(21.5, 15.5)], false),
                    false,
                    STROKE,
                );
            }
            Icon::GridDem => {
                // The grid made of the points is the result, so it is the bright part, and
                // COPC's octree (quiet frame, bright subdivision) is not mistaken for it.
                ink.stroke(&rectangle(3.0, 3.0, 18.0, 18.0), true, STROKE);
                for at in [9.0, 15.0] {
                    ink.line(at, 3.0, at, 21.0, true, FINE);
                    ink.line(3.0, at, 21.0, at, true, FINE);
                }
                for p in [
                    pt(6.0, 6.5),
                    pt(12.5, 5.0),
                    pt(17.5, 7.5),
                    pt(6.5, 12.5),
                    pt(11.0, 13.5),
                    pt(17.0, 12.0),
                    pt(5.5, 18.0),
                    pt(13.0, 17.5),
                    pt(18.0, 18.5),
                ] {
                    ink.dot(p.x, p.y, 1.1, false);
                }
            }
            Icon::DemTools => {
                draw_raster_glyph(ink);
                draw_wrench(ink, 18.5, 5.5, 270.0);
            }
            Icon::BuildOverviews => {
                ink.stroke(&rectangle(2.5, 15.0, 19.0, 5.5), false, STROKE);
                for x in [7.25, 12.0, 16.75] {
                    ink.line(x, 15.0, x, 20.5, false, FINE);
                }
                ink.stroke(&rectangle(6.0, 9.0, 12.0, 4.5), false, STROKE);
                ink.line(12.0, 9.0, 12.0, 13.5, false, FINE);
                ink.fill(&rectangle(9.0, 3.5, 6.0, 4.0), true, TINT);
                ink.stroke(&rectangle(9.0, 3.5, 6.0, 4.0), true, STROKE);
            }
            Icon::ToolbarIconsSmall => draw_toolbar_size(ink, 4.0),
            Icon::ToolbarIconsStandard => draw_toolbar_size(ink, 6.5),
            Icon::ToolbarIconsLarge => draw_toolbar_size(ink, 9.5),
            Icon::TextSizeSmall => draw_text_size(ink, 7.0),
            Icon::TextSizeStandard => draw_text_size(ink, 10.5),
            Icon::TextSizeLarge => draw_text_size(ink, 14.0),
            Icon::TextSizeExtraLarge => draw_text_size(ink, 17.5),
            Icon::SurfaceDeletePoints => {
                // The polygon solid: dashed at the full stroke inside the TIN it broke into
                // a scatter of dashes at 16 px.
                draw_tin(ink, false);
                ink.stroke(
                    &polyline(
                        &[pt(6.5, 7.5), pt(17.5, 7.0), pt(16.5, 17.0), pt(7.5, 16.0)],
                        true,
                    ),
                    true,
                    STROKE,
                );
                for (x, y) in [(10.5, 11.0), (13.5, 13.0)] {
                    ink.line(x - 1.7, y - 1.7, x + 1.7, y + 1.7, true, FINE);
                    ink.line(x + 1.7, y - 1.7, x - 1.7, y + 1.7, true, FINE);
                }
            }
            Icon::SurfaceDeleteTriangles => {
                draw_tin(ink, true);
                let polygon = polyline(&[pt(6.5, 9.0), pt(15.5, 7.0), pt(14.0, 17.0)], true);
                ink.fill(&polygon, true, TINT);
                ink.stroke(&polygon, true, STROKE);
            }
            Icon::SurfaceAddPoints => {
                draw_tin(ink, false);
                for (x, y) in [(9.0, 10.0), (14.5, 9.0), (12.0, 15.0)] {
                    ink.node(x, y, true);
                }
            }
            Icon::SurfaceDeleteBreakline => {
                draw_tin(ink, true);
                ink.dashed(&polyline(&[pt(3.0, 13.0), pt(21.0, 9.0)], false), true);
                ink.line(9.5, 8.5, 14.5, 13.5, true, STROKE);
                ink.line(14.5, 8.5, 9.5, 13.5, true, STROKE);
            }
            Icon::OnlineData => {
                // The globe in the lower left 15 units, as the GIS data's glyphs, so Import's
                // arrow fits down the right.
                ink.stroke(&circle(9.0, 13.5, 6.5), false, STROKE);
                ink.stroke(&ellipse(6.0, 7.0, 6.0, 13.0), false, FINE);
                ink.line(2.5, 13.5, 15.5, 13.5, false, FINE);
                draw_arrow_in(ink);
            }
        }
    }
    canvas.restore();
}

// ---- the application icon ----------------------------------------------------------------------

/// The sizes the application's icon is made at (applicationIcon): a taskbar's, a title bar's and
/// the largest Windows asks for.
pub const APPLICATION_ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// Paints the application icon at `size` pixels square, on what is already there (a transparent
/// ground for applicationIconImage).
pub fn paint_application_icon(canvas: &mut dyn Canvas, size: u32) {
    let size_f = f64::from(size);
    canvas.save();
    canvas.set_render_hint(RenderHint::Antialiasing, true);
    // Drawn on a 256 grid: the largest size Windows asks for, so the numbers below are pixels at
    // that size and proportions at every other.
    canvas.scale(size_f / 256.0, size_f / 256.0);

    // The tile. Flat slate - the window's own dark, as every icon of the set is flat since
    // 2026-11-16 (it was a gradient, lighter at the top) - with a hairline rim so it holds its
    // edge on a dark taskbar and a light desktop alike. The mark's colours are its own, not the
    // theme's: the icon is the same whatever theme the window is in.
    let mut tile = Path::new();
    tile.add_rounded_rect(RectF::new(8.0, 8.0, 240.0, 240.0), 54.0, 54.0);
    canvas.fill_path(&tile, &Brush::solid(Color::rgb(0x22, 0x27, 0x2e)));

    // A survey grid behind the blade, clipped to the tile: what the product is for, said quietly.
    // Dropped below 48 px, where it would only be noise.
    if size >= 48 {
        canvas.save();
        canvas.set_clip_path(&tile, false);
        canvas.set_pen(Pen {
            color: Color::rgba(255, 255, 255, 16),
            width: 2.0,
            ..Pen::default()
        });
        for i in 1..6_i32 {
            let at = 8.0 + 40.0 * f64::from(i);
            canvas.draw_line(pt(at, 8.0), pt(at, 248.0));
            canvas.draw_line(pt(8.0, at), pt(248.0, at));
        }
        canvas.restore();
    }
    canvas.set_pen(Pen {
        color: Color::rgba(255, 255, 255, 38),
        width: 2.0,
        ..Pen::default()
    });
    canvas.set_brush(Brush::none());
    canvas.draw_path(&tile);

    // Below 64 px the sword is drawn LARGER within the tile and heavier. At true proportions it
    // is a few pixels wide on a taskbar and disappears; an icon is a sign, not a scale drawing,
    // and every icon set that ships small sizes redraws them bolder for the same reason.
    let boost = if size >= 64 {
        1.0
    } else if size >= 32 {
        1.14
    } else {
        1.26
    };
    let heft = if size >= 64 {
        1.0
    } else if size >= 32 {
        1.25
    } else {
        1.55
    };
    canvas.save();
    canvas.set_clip_path(&tile, false);
    canvas.translate(128.0, 128.0);
    canvas.scale(boost, boost);
    canvas.translate(-128.0, -128.0);

    // The blade: a sliver along a shallow curve from the guard to the point, widest at the
    // guard. Built from offsets either side of a quadratic Bezier so the taper and the curve are
    // each one number.
    let root = pt(96.0, 166.0);
    let tip = pt(214.0, 40.0);
    // Pulls the curve towards the back of the blade.
    let bend = pt(142.0, 92.0);
    let on_curve = |t: f64| {
        let u = 1.0 - t;
        pt(
            u * u * root.x + 2.0 * u * t * bend.x + t * t * tip.x,
            u * u * root.y + 2.0 * u * t * bend.y + t * t * tip.y,
        )
    };
    let normal_at = |t: f64| {
        let d = (bend - root) * (2.0 * (1.0 - t)) + (tip - bend) * (2.0 * t);
        let length = d.x.hypot(d.y);
        pt(-d.y / length, d.x / length)
    };
    const SAMPLES: i32 = 28;
    let mut edge: Vec<PointF> = Vec::new();
    let mut back: Vec<PointF> = Vec::new();
    for i in 0..=SAMPLES {
        let t = f64::from(i) / f64::from(SAMPLES);
        // Full width for most of the length, then a quick run-out to the point.
        let half = 9.0 * heft * (if t < 0.82 { 1.0 } else { (1.0 - t) / 0.18 });
        edge.push(on_curve(t) + normal_at(t) * half);
        back.push(on_curve(t) - normal_at(t) * half);
    }
    let mut blade = Path::new();
    blade.add_polygon(&edge);
    for point in back.iter().rev() {
        blade.line_to(*point);
    }
    blade.close_subpath();
    // Flat steel; the bright cutting edge below is what gives it its shine.
    canvas.fill_path(&blade, &Brush::solid(Color::rgb(0xc9, 0xd2, 0xdc)));
    // The cutting edge, a brighter line along one side.
    canvas.set_pen(Pen::with(
        Color::rgba(255, 255, 255, 210),
        2.4,
        PenStyle::Solid,
        PenCap::Round,
        PenJoin::Bevel,
    ));
    canvas.draw_polyline(&edge[..(SAMPLES - 3) as usize]);

    // The handle, in the one warm colour of the mark, continuing the blade's own line BACK from
    // the root. `along` is the unit tangent pointing root to tip: with the normal
    // n = (-dy, dx) / L, the tangent is (n.y, -n.x). The first version had the sign wrong, so
    // the handle ran UP the blade and the guard ended up at the pommel - visible at once on the
    // contact sheet, which is what the sheet is for.
    let normal = normal_at(0.0);
    let along = pt(normal.y, -normal.x);
    let pommel = root - along * 66.0;
    canvas.set_pen(Pen::with(
        Color::rgb(0xe5, 0x48, 0x4d),
        22.0 * heft,
        PenStyle::Solid,
        PenCap::Round,
        PenJoin::Bevel,
    ));
    canvas.draw_line(root - along * 10.0, pommel);
    // The wrap: dark diamonds across the handle. Only where there are pixels enough to show
    // them.
    if size >= 64 {
        canvas.set_pen(Pen::with(
            Color::rgb(0x5a, 0x16, 0x1b),
            3.0,
            PenStyle::Solid,
            PenCap::Round,
            PenJoin::Bevel,
        ));
        let across = normal * 9.0;
        for i in 1..=4_i32 {
            let centre = root - along * (8.0 + 11.5 * f64::from(i));
            canvas.draw_line(centre - across - along * 5.0, centre + across + along * 5.0);
            canvas.draw_line(centre - across + along * 5.0, centre + across - along * 5.0);
        }
    }
    // The guard, across the blade at its root.
    canvas.set_pen(Pen::with(
        Color::rgb(0xd9, 0xb2, 0x4a),
        10.0 * heft,
        PenStyle::Solid,
        PenCap::Round,
        PenJoin::Bevel,
    ));
    canvas.draw_line(root - normal * 24.0, root + normal * 24.0);
    canvas.restore();
    canvas.restore();
}

/// The application icon at `size` pixels square, on a transparent ground.
pub fn application_icon_image(raster: &dyn Rasteriser, size: u32) -> Image {
    raster.paint(size, size, 1.0, &mut |canvas: &mut dyn Canvas| {
        paint_application_icon(canvas, size);
    })
}

/// The application icon at every size it is made at (applicationIcon's pixmaps).
pub fn application_icon(raster: &dyn Rasteriser) -> Vec<Image> {
    APPLICATION_ICON_SIZES
        .iter()
        .map(|&size| application_icon_image(raster, size))
        .collect()
}

// ---- the icon set by name ----------------------------------------------------------------------

/// How an icon is shown (QIcon::Mode, and the state of the button it is on).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IconMode {
    #[default]
    Normal,
    /// On a button whose tool runs or whose toggle is on (QIcon::On).
    Active,
    Disabled,
}

/// The quiet and the bright tone an icon is painted in (see the module comment): at rest the
/// secondary text colour and the text colour; active the text colour and the accent; disabled
/// both the disabled text colour. Hover is shown by the button's background, so that one icon
/// serves a toolbar and a menu without the two disagreeing.
pub fn icon_colours(mode: IconMode) -> (Color, Color) {
    match mode {
        IconMode::Normal => (palette::text_muted(), palette::text()),
        IconMode::Active => (palette::text(), palette::accent()),
        IconMode::Disabled => (palette::text_disabled(), palette::text_disabled()),
    }
}

/// Whether `name` is in the window's icon set: an icon's name, or a tool's id (any text with a
/// '.', as toolIcon takes any id and paints a placeholder for one no family draws).
pub fn is_named_icon(name: &str) -> bool {
    name.contains('.') || Icon::from_name(name).is_some()
}

/// Paints the icon named `name` (see the module comment) into `rect` in `mode`'s colours. False,
/// painting nothing, for a name that is not in the set. `rect` is taken as the icon's size on
/// screen; paint_named_icon_at says how many canvas pixels a screen pixel is.
pub fn paint_named_icon(canvas: &mut dyn Canvas, name: &str, rect: RectF, mode: IconMode) -> bool {
    paint_named_icon_at(canvas, name, rect, mode, 1.0)
}

/// paint_named_icon for a canvas whose pixels are `ratio` to the screen's (the device-pixel
/// ratio when `rect` is in device pixels), so the weights are those of the icon's size on screen:
/// a 16 px menu icon at 200 % is painted in 32 device pixels with a 16 px icon's weights.
pub fn paint_named_icon_at(
    canvas: &mut dyn Canvas,
    name: &str,
    rect: RectF,
    mode: IconMode,
    ratio: f64,
) -> bool {
    let (neutral, accent) = icon_colours(mode);
    let ratio = if ratio.is_finite() && ratio > 0.0 {
        ratio
    } else {
        1.0
    };
    let weight = weight_at(rect.width / ratio);
    if let Some(which) = Icon::from_name(name) {
        paint_icon_weighted(canvas, which, rect, neutral, accent, weight);
        return true;
    }
    if name.contains('.') {
        paint_tool_icon_weighted(canvas, name, rect, neutral, accent, weight);
        return true;
    }
    false
}

// ---- the raster canvas and the cache -----------------------------------------------------------

/// The raster canvas's door: a fresh, transparent image of `width` by `height` device pixels at
/// device-pixel ratio `ratio`, painted by `paint` through a canvas whose device transform scales
/// the painter's units by `ratio`. The window hands it to the icon cache, the toolbar check and
/// the developer tools; the raster canvas itself is C's (tiny-skia), and none of this names it.
pub trait Rasteriser {
    fn paint(
        &self,
        width: u32,
        height: u32,
        ratio: f64,
        paint: &mut dyn FnMut(&mut dyn Canvas),
    ) -> Image;
}

/// The raster canvas over tiny-skia (canvas::RasterCanvas) as a rasteriser: an image of the
/// given device pixels carrying `ratio`, which the canvas paints logical coordinates at.
pub struct CanvasRasteriser;

impl Rasteriser for CanvasRasteriser {
    fn paint(
        &self,
        width: u32,
        height: u32,
        ratio: f64,
        paint: &mut dyn FnMut(&mut dyn Canvas),
    ) -> Image {
        let mut image = Image::new(width, height);
        image.set_device_pixel_ratio(ratio);
        let mut canvas = crate::canvas::RasterCanvas::on(image);
        paint(&mut canvas);
        canvas.into_image()
    }
}

/// The program's rasteriser: the raster canvas over tiny-skia, the one place the window, the
/// toolbar check and the two developer tools take their rasteriser from.
pub fn default_rasteriser() -> Option<Arc<dyn Rasteriser + Send + Sync>> {
    Some(Arc::new(CanvasRasteriser))
}

/// The icon `name` painted at `size` logical pixels square for a screen of `ratio`
/// (QIcon::pixmap(size, ratio)): an image of size times ratio device pixels, rounded, painted at
/// that size and carrying the ratio. None for a name not in the set.
pub fn icon_image(
    raster: &dyn Rasteriser,
    name: &str,
    size: f64,
    ratio: f64,
    mode: IconMode,
) -> Option<Image> {
    if !is_named_icon(name) {
        return None;
    }
    let side = device_pixels(size * ratio);
    let mut image = raster.paint(side, side, 1.0, &mut |canvas: &mut dyn Canvas| {
        let bounds = RectF::new(0.0, 0.0, f64::from(side), f64::from(side));
        paint_named_icon_at(canvas, name, bounds, mode, ratio);
    });
    image.set_device_pixel_ratio(ratio);
    Some(image)
}

/// A length in device pixels as QSize rounds one (qRound: halves up), none below zero.
pub fn device_pixels(length: f64) -> u32 {
    if length.is_finite() && length > 0.0 {
        (length + 0.5).floor().min(f64::from(u32::MAX)) as u32
    } else {
        0
    }
}

/// What the egui context keeps for the window's painted images.
#[derive(Clone)]
struct PaintedCache {
    raster: Arc<dyn Rasteriser + Send + Sync>,
    textures: Arc<Mutex<HashMap<String, egui::TextureHandle>>>,
}

fn cache_id() -> egui::Id {
    egui::Id::new("katana_window::icons::cache")
}

/// Gives `ctx` the rasteriser its icons and tool buttons are painted with, and an empty cache.
pub fn install(ctx: &egui::Context, raster: Arc<dyn Rasteriser + Send + Sync>) {
    let cache = PaintedCache {
        raster,
        textures: Arc::default(),
    };
    ctx.data_mut(|data| data.insert_temp(cache_id(), cache));
}

/// The rasteriser `install` gave `ctx`, if any.
pub fn installed_rasteriser(ctx: &egui::Context) -> Option<Arc<dyn Rasteriser + Send + Sync>> {
    ctx.data_mut(|data| data.get_temp::<PaintedCache>(cache_id()))
        .map(|cache| cache.raster)
}

/// The texture kept under `key`, painted by `paint` into an image of `width` by `height` device
/// pixels at `ratio` the first time it is asked for. None when `ctx` has no rasteriser or the
/// image would be empty.
pub fn painted_texture(
    ctx: &egui::Context,
    key: &str,
    width: u32,
    height: u32,
    ratio: f64,
    paint: &mut dyn FnMut(&mut dyn Canvas),
) -> Option<egui::TextureHandle> {
    if width == 0 || height == 0 {
        return None;
    }
    let cache = ctx.data_mut(|data| data.get_temp::<PaintedCache>(cache_id()))?;
    let mut textures = cache.textures.lock().ok()?;
    if let Some(found) = textures.get(key) {
        return Some(found.clone());
    }
    let image = cache.raster.paint(width, height, ratio, paint);
    let texture = ctx.load_texture(key, image.to_color_image(), egui::TextureOptions::NEAREST);
    textures.insert(String::from(key), texture.clone());
    Some(texture)
}

/// The icon `name` at `size` points square in `mode`, painted for `ctx`'s pixels per point and
/// kept (see THE CACHE above). None for a name not in the set, or a context with no rasteriser.
pub fn texture(
    ctx: &egui::Context,
    name: &str,
    size: f32,
    mode: IconMode,
) -> Option<egui::TextureHandle> {
    if !is_named_icon(name) {
        return None;
    }
    let ratio = f64::from(ctx.pixels_per_point());
    let side = device_pixels(f64::from(size) * ratio);
    let key = format!("icon {name} {side} {ratio} {mode:?}");
    painted_texture(
        ctx,
        &key,
        side,
        side,
        1.0,
        &mut |canvas: &mut dyn Canvas| {
            let bounds = RectF::new(0.0, 0.0, f64::from(side), f64::from(side));
            paint_named_icon_at(canvas, name, bounds, mode, ratio);
        },
    )
}

/// Where an image of `width` by `height` device pixels is shown with its top left at `at`: the
/// corner moved onto the device's pixel grid (qRound, as Qt places a pixmap), so the image is
/// drawn one pixel for one and stays crisp.
pub fn device_rect(ctx: &egui::Context, at: egui::Pos2, width: u32, height: u32) -> egui::Rect {
    let ratio = ctx.pixels_per_point();
    let snap = |points: f32| ((points * ratio) + 0.5).floor() / ratio;
    egui::Rect::from_min_size(
        egui::pos2(snap(at.x), snap(at.y)),
        egui::vec2(width as f32 / ratio, height as f32 / ratio),
    )
}

/// Draws the icon `name` square in `rect` (its width the icon's size) with `painter`, in `mode`.
/// Nothing for a name not in the set or a context with no rasteriser.
pub fn show_icon(painter: &egui::Painter, rect: egui::Rect, name: &str, mode: IconMode) {
    let ctx = painter.ctx();
    let Some(texture) = texture(ctx, name, rect.width(), mode) else {
        return;
    };
    let [width, height] = texture.size();
    let at = device_rect(ctx, rect.min, width as u32, height as u32);
    let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.image(texture.id(), at, whole, egui::Color32::WHITE);
}
