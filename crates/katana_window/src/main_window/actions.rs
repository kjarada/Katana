//! The main window's menus and toolbars as rows (drive::action): its own actions - made once each
//! in the C++ (makeAction) and shared by a menu and a toolbar - and every workbench's, composed in
//! the C++'s order (buildActions, buildGisActions, buildSurveyActions, buildFormatActions,
//! buildToolActions, buildViewMenu, buildDocks, buildWindowMenu).
//!
//! A row carries what a QAction carried that changes - its check, whether it is enabled, Undo's
//! text - so the rows are made again from the window's state whenever they are asked for (each
//! frame, and by `--report`, `--check-menus` and `--check-shortcuts`), where the C++ kept the
//! QActions and set them as the state changed (refreshViewMenu, refreshAll). There is nothing to
//! keep in step.
//!
//! A submenu is a menu item too, which `--check-menus` holds to an icon and a status tip; the
//! `drive::Menu` row was frozen without either, so each submenu's are kept beside the rows
//! (`signs`). The tip was added to the row later, since a parent menu's `--report` shows it, and
//! is set from `signs` once every row is composed (`tip_submenus`): one source still.

use std::collections::BTreeMap;

use egui::{Key, KeyboardShortcut, Modifiers};
use katana_cad::snapping::SnapModes;
use katana_cad::view_link::linkable;
use katana_cad::view_verbs::{ZoomRequestKind, zoom_takes};
use katana_cad::viewport_layout::{LayoutKind, ViewKind};
use katana_entity::EnumName;
use katana_render::camera::{StandardView, to_string as standard_view_name};

use super::executor::{RECENT_SCRIPTS, RECENT_SCRIPTS_KEY, SNAP_MENU_MODES};
use super::window::MainWindow;
use crate::customisation::customisation_workbench::CustomisationWorkbench;
use crate::drive::action::{Action, Menu, MenuItem, ToolBar, ToolBarItem};
use crate::gis_online::OnlineDataWorkbench;
use crate::shell::Workbench;
use crate::terrain::main_window_surface_display::surface_styles_action;
use crate::terrain::surface_edit_ui::SurfaceEditUi;
use crate::theme::palette::{TEXT_SIZES, TextSize, text_size_steps};

/// Where a toolbar is (main_window::docking): a line of one of the window's sides - a row along
/// the top or the bottom, a column down the left or the right, each numbered from the window's
/// edge inwards - or floating; the Drafting bar has its own row over the status bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BarPlace {
    Top(usize),
    Left(usize),
    Right(usize),
    /// A row along the bottom, over the Drafting bar.
    Bottom(usize),
    /// Floating over the views, its buttons wrapped in its width.
    Floating(super::docking::FloatAt),
    /// The Drafting bar's own row, which is not docked elsewhere (main_window::docking).
    Drafting,
    /// In no toolbar area: a layout left it out.
    Nowhere,
}

/// The window's rows as composed for a frame.
pub struct Composed {
    /// The menu bar's menus in their order, with their submenus.
    pub menus: Vec<Menu>,
    /// The toolbars makeToolBar made, in the order made, each with its place; the Drafting bar
    /// (drawing_ui's own) last.
    pub bars: Vec<(ToolBar, BarPlace)>,
    /// Each submenu's icon and status tip, by its object name.
    pub signs: BTreeMap<String, (&'static str, String)>,
    /// The panels' own actions, which no menu or toolbar shows (the Properties tree's Copy):
    /// what findChildren<QAction*> found beside the menus' and toolbars' in the C++, after the
    /// menus' and before the toolbars'. Only the shortcut table and its clash check read them;
    /// a panel triggers its own.
    pub panel_actions: Vec<Action>,
}

impl Composed {
    /// Every action of the window, menus and toolbars alike, each once, in the menus' order and
    /// then the toolbars': what findChild<QAction*> found.
    pub fn actions(&self) -> Vec<Action> {
        let mut seen: Vec<Action> = Vec::new();
        let mut add = |action: &Action| {
            if !action.name.is_empty() && !seen.iter().any(|each| each.name == action.name) {
                seen.push(action.clone());
            }
        };
        for menu in &self.menus {
            for action in menu.actions() {
                add(action);
            }
        }
        for (bar, _) in &self.bars {
            for action in bar.actions() {
                add(action);
            }
        }
        seen
    }

    /// The action `name`, wherever it is shown.
    pub fn action(&self, name: &str) -> Option<Action> {
        self.actions().into_iter().find(|action| action.name == name)
    }
}

fn key(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, key)
}

fn ctrl(k: Key) -> KeyboardShortcut {
    key(Modifiers::COMMAND, k)
}

fn ctrl_shift(k: Key) -> KeyboardShortcut {
    key(Modifiers::COMMAND | Modifiers::SHIFT, k)
}

fn plain(k: Key) -> KeyboardShortcut {
    key(Modifiers::NONE, k)
}

/// makeAction: an action with its icon, text, tip and object name.
fn act(name: &str, icon: &'static str, text: &str, tip: &str) -> Action {
    Action::new(name, text).icon(icon).tip(tip)
}

// ---- File -----------------------------------------------------------------------------------

pub fn new_action() -> Action {
    act("fileNew", "New", "&New", "Start a new, empty drawing").shortcut(ctrl(Key::N))
}

pub fn open_action() -> Action {
    act("fileOpen", "Open", "&Open Project...", "Open a Katana project directory")
        .shortcut(ctrl(Key::O))
}

pub fn save_action() -> Action {
    act("fileSave", "Save", "&Save", "Save the project").shortcut(ctrl(Key::S))
}

pub fn save_as_action() -> Action {
    act(
        "fileSaveAs",
        "SaveAs",
        "Save &As...",
        "Save the project under another name",
    )
    .shortcut(ctrl_shift(Key::S))
}

pub fn import_action() -> Action {
    act(
        "fileImport",
        "Import",
        "Import &Any File...",
        "Import a drawing, an image or a point cloud (DXF, IFC, SHP, GeoTIFF, LAS ...)",
    )
    .shortcut(ctrl(Key::I))
}

pub fn export_action() -> Action {
    act(
        "fileExportVector",
        "Export",
        "Export Dra&wing...",
        "Export the drawing, or a scope of it, to a file: DXF, an archive, IFC 4.3 or a GIS \
         vector format (GeoPackage, GeoJSON, SHP ...)",
    )
    .shortcut(ctrl_shift(Key::E))
}

pub fn plot_action() -> Action {
    act(
        "filePlot",
        "Plot",
        "&Plot to PDF...",
        "Plot the drawing to a sheet at a standard scale",
    )
    .shortcut(ctrl(Key::P))
    .opens("plotDrawingDialog")
}

pub fn export_image_action() -> Action {
    act(
        "fileExportViewImage",
        "ExportImage",
        "Export View as I&mage...",
        "Save the plan or 3D view as a PNG, JPEG or TIFF, at the view's size or larger",
    )
    .opens("viewImageDialog")
}

pub fn run_script_action() -> Action {
    act(
        "fileRunScript",
        "CommandLine",
        "&Run Script...",
        "Run a script of commands (.kcs), a line at a time, as katana_cli runs it",
    )
}

pub fn import_ifc_action() -> Action {
    act(
        "fileImportIfc",
        "ImportIfc",
        "Import I&FC...",
        "Import an IFC file: alignments as PIs and PVIs, elements and annotations with their \
         property sets, terrain as surfaces",
    )
}

pub fn export_ifc_action() -> Action {
    act(
        "fileExportIfc",
        "ExportIfc",
        "Export I&FC...",
        "Export IFC 4.3: alignments, the drawing by class and an AS 5488 utility investigation, \
         with a preview of the class each object becomes",
    )
}

pub fn sheets_action() -> Action {
    act(
        "fileSheets",
        "Sheets",
        "S&heets...",
        "Lay the drawing out on sheets with a title block, and plot them",
    )
    .shortcut(ctrl_shift(Key::P))
}

pub fn plot_sheets_action() -> Action {
    act(
        "filePlotSheets",
        "PlotSheets",
        "Plot Sheets to P&DF...",
        "Plot every sheet of the project to one PDF",
    )
}

pub fn apply_codes_action() -> Action {
    act(
        "applySurveyCodes",
        "ApplyCodes",
        "Appl&y Survey Codes",
        "Give the selected entities that carry a field code - every one in the drawing when \
         nothing is selected - the layer, style and attributes the loaded survey codes say: the \
         CODE line, one undo step",
    )
}

pub fn linework_action() -> Action {
    act(
        "surveyLinework",
        "SurveyLinework",
        "P&rocess Linework",
        "Join the selected coded points into lines - every coded point in the drawing when \
         nothing is selected - by their codes and the linework control codes: the LINEWORK line, \
         one undo step",
    )
}

pub fn start_screen_action() -> Action {
    act(
        "fileStartScreen",
        "StartScreen",
        "Start Screen...",
        "Choose the customisation and the coordinate system this session works with, as a new \
         session does",
    )
    .opens("startDialog")
}

pub fn project_crs_action() -> Action {
    act(
        "fileProjectCrs",
        "ProjectCrs",
        "Project &Coordinate System...",
        "Set the coordinate system the project is in (EPSG code, WKT or PROJ); online data and \
         reprojection need one",
    )
    .opens("projectCrsDialog")
}

pub fn summary_action() -> Action {
    act(
        "fileDrawingSummary",
        "DrawingSummary",
        "Drawing S&ummary...",
        "The drawing at a glance: project, contents, what is current, the history and the \
         customisation loaded, and what it leaves undefined",
    )
    .opens("drawingSummaryDialog")
}

/// File > Quit. The C++ gave it QKeySequence::Quit, the platform's own key, which Qt's Windows
/// keyboard scheme leaves empty (Alt+F4 is the system's) and the X11 and macOS schemes make
/// Ctrl+Q. The Windows scheme is also what Qt's offscreen platform uses, so a headless run has no
/// key for it on any system: `headless` says which.
pub fn quit_action(headless: bool) -> Action {
    let quit = act(
        "fileQuit",
        "Quit",
        "&Quit",
        "Close Katana, asking first about anything unsaved",
    );
    if headless || cfg!(windows) {
        quit
    } else {
        quit.shortcut(ctrl(Key::Q))
    }
}

// ---- Edit -----------------------------------------------------------------------------------

/// What the window names Format > Layers' manager (main_window.cpp: the action's name plus
/// "Dialog"), where a manager made on its own names itself layerManager.
pub const FORMAT_LAYERS_DIALOG: &str = "formatLayersDialog";

pub fn layers_action() -> Action {
    act(
        "formatLayers",
        "Layers",
        "&Layers...",
        "The drawing's layers: colour, linetype, weight, what is on each",
    )
    .shortcut(ctrl(Key::L))
    // --dialog formatLayers finds it by this.
    .opens(FORMAT_LAYERS_DIALOG)
}

// ---- Terrain --------------------------------------------------------------------------------

fn surface_from(name: &str, icon: &'static str, text: &str, tip: &str) -> Action {
    act(name, icon, text, tip).opens("surfaceFromDialog")
}

fn alignment_manager_action() -> Action {
    act(
        "terrainAlignmentManager",
        "SectionAlignment",
        "Alignment &Manager...",
        "Define and edit alignments: PIs and curves, the design profile, the setting-out table \
         and chainage labels",
    )
    .opens("alignmentManagerDialog")
}

fn section_along_alignment() -> Action {
    act(
        "terrainSectionAlongAlignment",
        "SectionAlignment",
        "Cut Section Along &Alignment...",
        "Long section down a named alignment, with its design profile",
    )
    .shortcut(ctrl_shift(Key::K))
}

fn section_along_selection() -> Action {
    act(
        "terrainSectionAlongSelection",
        "Section",
        "&Cut Section Along Selection",
        "Long section along the selected line or polyline",
    )
    .shortcut(ctrl(Key::K))
}

fn volumes_action() -> Action {
    act(
        "terrainVolumes",
        "Volumes",
        "&Volumes...",
        "Cut and fill between surfaces, to a level, of a stockpile, along an alignment or on a \
         grid, inside polygons; and the cut/fill map and the envelope",
    )
    .opens("volumesDialog")
}

// ---- GIS ------------------------------------------------------------------------------------

fn import_vector_action() -> Action {
    act(
        "importVectorData",
        "ImportVector",
        "Import Vector Da&ta...",
        "Import a Shapefile, GeoPackage, GeoJSON, KML, GML, DXF or MapInfo file through GDAL, \
         choosing its layers and where they go",
    )
}

fn import_raster_action() -> Action {
    act(
        "importRaster",
        "ImportRaster",
        "Import &Raster...",
        "Import a GeoTIFF, ASCII grid, IMG or image through GDAL, as a backdrop",
    )
}

fn import_cloud_action() -> Action {
    act(
        "importPointCloud",
        "ImportPointCloud",
        "Import &Point Cloud...",
        "Import a LAS, LAZ or COPC point cloud through PDAL, choosing how many points and which \
         classes",
    )
}

fn export_cloud_action() -> Action {
    act(
        "exportPointCloud",
        "ExportPointCloud",
        "Export Point &Cloud...",
        "Write an imported point cloud to LAS or LAZ through PDAL",
    )
}

fn export_dem_action() -> Action {
    act(
        "exportSurfaceDem",
        "ExportDem",
        "Export Surface as &DEM...",
        "Write a surface to a GeoTIFF, ASCII grid or IMG elevation raster through GDAL",
    )
    .opens("surfaceRasterDialog")
}

fn copc_action() -> Action {
    act(
        "convertCopc",
        "ConvertCopc",
        "Convert Point Cloud to C&OPC...",
        "Rewrite a LAS or LAZ as a Cloud Optimised Point Cloud, which can be read at any level \
         of detail",
    )
}

fn dataset_info_action() -> Action {
    act(
        "datasetInformation",
        "DatasetInfo",
        "Dataset &Information...",
        "What a GIS file or point cloud holds - layers, size, extent, coordinate system - \
         without importing it",
    )
}

// ---- the menus ------------------------------------------------------------------------------

/// Fills `menu` with actions found by name among `pool` (fillMenuByName): a section's title -
/// empty for items under no title - and the names of its items. A name no action carries is left
/// out and said (`problems`).
fn fill_by_name(
    menu: &mut Menu,
    parts: &[(&str, &[&str])],
    pool: &[Action],
    problems: &mut Vec<String>,
) {
    let place = format!("File > {}", menu.title.replace('&', ""));
    for (section, names) in parts {
        if !section.is_empty() {
            menu.items.push(MenuItem::Section(String::from(*section)));
        }
        for name in *names {
            match pool.iter().find(|action| action.name == *name) {
                Some(action) => menu.items.push(MenuItem::Action(action.clone())),
                None => problems.push(format!(
                    "{place}: no menu item is named {name}, so it is left out of this menu."
                )),
            }
        }
    }
}

/// Every action of `menus`, depth first.
fn pool_of(menus: &[&Menu]) -> Vec<Action> {
    menus
        .iter()
        .flat_map(|menu| menu.actions().into_iter().cloned())
        .collect()
}

/// The items of `from` appended to `to`.
fn append(to: &mut Menu, from: &Menu) {
    to.items.extend(from.items.iter().cloned());
}

/// The workbench's menu of `name` among `menus`, or an empty one.
fn menu_named(menus: Vec<Menu>, name: &str) -> Option<Menu> {
    menus.into_iter().find(|menu| menu.name == name)
}

/// One snap mode's row of View > Snap Modes.
fn snap_mode_action(mode: katana_cad::snapping::SnapMode, modes: SnapModes) -> Action {
    let (icon, tip) = match mode.name() {
        "Endpoint" => (
            "SnapEndpoint",
            "Snap to the ends of lines, arcs and polyline segments",
        ),
        "Midpoint" => (
            "SnapMidpoint",
            "Snap to the middle of a line, an arc or a segment",
        ),
        "Center" => ("SnapCenter", "Snap to the centre of a circle or an arc"),
        "Intersection" => ("SnapIntersection", "Snap to where two entities cross"),
        "Perpendicular" => (
            "SnapPerpendicular",
            "Snap to the foot of the perpendicular from the last point",
        ),
        "Tangent" => (
            "SnapTangent",
            "Snap to the point where a line from the last point touches a circle",
        ),
        "Nearest" => ("SnapNearest", "Snap to the nearest point on any entity"),
        _ => ("SnapGrid", "Snap to the grid's points"),
    };
    // The mode's name is the cad layer's, and the action's; what a person reads is British.
    let text = match mode.name() {
        "Center" => "Centre",
        name => name,
    };
    act(&format!("viewSnapMode{}", mode.name()), icon, text, tip)
    .checkable(modes & (mode as SnapModes) != 0)
}

/// View > Text Size's items by size: viewTextSizeSmall ... viewTextSizeExtraLarge.
pub fn text_size_action_name(size: TextSize) -> &'static str {
    match size {
        TextSize::Small => "viewTextSizeSmall",
        TextSize::Standard => "viewTextSizeStandard",
        TextSize::Large => "viewTextSizeLarge",
        TextSize::ExtraLarge => "viewTextSizeExtraLarge",
    }
}

/// The layouts View > Viewport Layout offers: kind, name, icon, tip.
pub const LAYOUTS: [(LayoutKind, &str, &str, &str); 6] = [
    (
        LayoutKind::Single,
        "Single",
        "LayoutSingle",
        "One view, the whole window",
    ),
    (
        LayoutKind::SplitVertical,
        "SplitVertical",
        "LayoutSplitVertical",
        "Two views side by side",
    ),
    (
        LayoutKind::SplitHorizontal,
        "SplitHorizontal",
        "LayoutSplitHorizontal",
        "Two views, one above the other",
    ),
    (
        LayoutKind::ThreeLeft,
        "ThreeLeft",
        "LayoutThreeLeft",
        "Three views: a large one on the left, two stacked on the right",
    ),
    (
        LayoutKind::ThreeTop,
        "ThreeTop",
        "LayoutThreeTop",
        "Three views: a wide one on top, two side by side below",
    ),
    (
        LayoutKind::Quad,
        "Quad",
        "LayoutQuad",
        "Four views of equal size",
    ),
];

/// The kinds View > Active Viewport Shows offers.
pub const KINDS: [(ViewKind, &str, &str); 4] = [
    (ViewKind::Plan, "ViewPlan", "Show the plan in the active view"),
    (
        ViewKind::Model3D,
        "View3D",
        "Show the model in 3D in the active view",
    ),
    (
        ViewKind::Section,
        "ViewSection",
        "Show the section cut last in the active view",
    ),
    (
        ViewKind::Elevation,
        "ViewElevation",
        "Show an elevation of the model in the active view",
    ),
];

/// The standard 3D views: view, name, icon, tip.
pub const STANDARD_VIEWS: [(StandardView, &str, &str, &str); 10] = [
    (StandardView::Top, "Top", "ViewTop", "Look straight down on the model"),
    (
        StandardView::Bottom,
        "Bottom",
        "ViewBottom",
        "Look straight up at the model from below",
    ),
    (
        StandardView::Front,
        "Front",
        "ViewFront",
        "Look north at the model, from the south",
    ),
    (
        StandardView::Back,
        "Back",
        "ViewBack",
        "Look south at the model, from the north",
    ),
    (
        StandardView::Left,
        "Left",
        "ViewLeft",
        "Look east at the model, from the west",
    ),
    (
        StandardView::Right,
        "Right",
        "ViewRight",
        "Look west at the model, from the east",
    ),
    (
        StandardView::IsoSouthWest,
        "IsoSouthWest",
        "ViewIsoSouthWest",
        "Look at the model from above its south-west corner",
    ),
    (
        StandardView::IsoSouthEast,
        "IsoSouthEast",
        "ViewIsoSouthEast",
        "Look at the model from above its south-east corner",
    ),
    (
        StandardView::IsoNorthEast,
        "IsoNorthEast",
        "ViewIsoNorthEast",
        "Look at the model from above its north-east corner",
    ),
    (
        StandardView::IsoNorthWest,
        "IsoNorthWest",
        "ViewIsoNorthWest",
        "Look at the model from above its north-west corner",
    ),
];

/// The toolbar icon sizes View > Toolbars offers: pixels, text, name.
pub const ICON_SIZES: [(i32, &str, &str); 3] = [
    (16, "&Small Icons", "viewToolBarIconsSmall"),
    (20, "S&tandard Icons", "viewToolBarIconsStandard"),
    (28, "&Large Icons", "viewToolBarIconsLarge"),
];

/// Each icon size's own icon in View > Toolbars, in ICON_SIZES' order: the three shared Toolbars'
/// window, and read only by their words (the third look audit's G).
const ICON_SIZE_ICONS: [&str; 3] = ["ToolbarIconsSmall", "ToolbarIconsStandard", "ToolbarIconsLarge"];

/// Each text size's own icon in View > Text Size, in TEXT_SIZES' order: an A at each size.
const TEXT_SIZE_ICONS: [&str; 4] = [
    "TextSizeSmall",
    "TextSizeStandard",
    "TextSizeLarge",
    "TextSizeExtraLarge",
];

/// A toolbar's toggle in View > Toolbars: viewToolBar<title without blanks>, with the icon of the
/// bar's first button (`tool_bar_icon`).
pub fn tool_bar_toggle(bar: &ToolBar, shown: bool, icon: &'static str) -> Action {
    act(
        &format!("viewToolBar{}", bar.title.replace(' ', "")),
        icon,
        &bar.title,
        &format!("Show or hide the {} toolbar", bar.title),
    )
    .checkable(shown)
}

/// The icon of a toolbar's first button, which its toggle in View > Toolbars shows: all thirteen
/// toggles showed one window icon, so the list was read word by word (the third look audit's G).
/// A control the bar draws itself is asked of `widget_icon` (a tool family's button: its first
/// tool's icon) or is the icon of what it does (Edit's Undo button, the Properties bar's style
/// choice); one with neither is passed over. Toolbars' window when the bar shows no icon at all.
pub fn tool_bar_icon(bar: &ToolBar, widget_icon: &dyn Fn(&str) -> Option<&'static str>) -> &'static str {
    bar.items
        .iter()
        .find_map(|item| match item {
            ToolBarItem::Action(action) if !action.icon.is_empty() => Some(action.icon),
            ToolBarItem::Widget(name) => match name.as_str() {
                "editUndoButton" => Some("Undo"),
                "CurrentStyleCombo" => Some("Properties"),
                _ => widget_icon(name),
            },
            _ => None,
        })
        .unwrap_or("Toolbars")
}

/// A toolbar's own Place submenu (View > Toolbars > Place, and its right-click menu): Dock Top,
/// Left, Right and Bottom and Float, the one it is at checked - its dock actions,
/// `viewToolBarDock<Bar><Side>` (main_window::docking), named by the key its toggle is named by.
pub fn tool_bar_place_menu(bar: &ToolBar, place: &BarPlace, icon: &'static str) -> Menu {
    use super::docking::{Side, dock_action_name};
    let key = bar.title.replace(' ', "");
    let at = place.docked().map(|(side, _)| side);
    let mut menu = Menu::new(&format!("viewToolBarPlace{key}"), &bar.title)
        .icon(icon)
        .tip(&format!(
            "Where the {} toolbar is: docked along a side of the window, or floating over the \
             views",
            bar.title
        ));
    for side in Side::ALL {
        // Each side its own icon, an arrow into a box from that side, so the four are told apart
        // without reading them (the third look audit's G).
        let (text, icon) = match side {
            Side::Top => ("Dock &Top", "ViewBack"),
            Side::Left => ("Dock &Left", "ViewLeft"),
            Side::Right => ("Dock &Right", "ViewRight"),
            Side::Bottom => ("Dock &Bottom", "ViewFront"),
        };
        let along = if side.vertical() { "down" } else { "along" };
        menu.items.push(MenuItem::Action(
            act(
                &dock_action_name(&key, Some(side)),
                icon,
                text,
                &format!(
                    "Dock the {} toolbar {along} the {} of the window",
                    bar.title,
                    side.word()
                ),
            )
            .checkable(at == Some(side)),
        ));
    }
    menu.items.push(MenuItem::Action(
        act(
            &dock_action_name(&key, None),
            "Float",
            "&Float",
            &format!(
                "Float the {} toolbar over the views, its buttons wrapped in rows a person widens \
                 by its edge",
                bar.title
            ),
        )
        .checkable(matches!(place, BarPlace::Floating(_))),
    ));
    menu
}

/// View > Toolbars > Reset Toolbars: every toolbar back where a new window has it, and shown.
pub fn reset_tool_bars_action() -> Action {
    act(
        super::docking::RESET_ACTION,
        "ResetLayout",
        "&Reset Toolbars",
        "Put every toolbar back where a new window has it, docked and shown",
    )
}

/// The toolbars at the places `places` gives them (main_window::docking), in its order: a bar it
/// does not hold keeps its built place, after those it does.
pub fn apply_places(bars: &mut Vec<(ToolBar, BarPlace)>, places: &[(String, BarPlace)]) {
    let order = |name: &str| places.iter().position(|(bar, _)| bar == name);
    for (bar, place) in bars.iter_mut() {
        if let Some(at) = order(&bar.name) {
            *place = places[at].1;
        }
    }
    let mut keyed: Vec<(usize, (ToolBar, BarPlace))> = std::mem::take(bars)
        .into_iter()
        .enumerate()
        .map(|(built, entry)| {
            let key = order(&entry.0.name).unwrap_or(places.len() + built);
            (key, entry)
        })
        .collect();
    keyed.sort_by_key(|(key, _)| *key);
    bars.extend(keyed.into_iter().map(|(_, entry)| entry));
}

/// The window's menus and toolbars, composed from its own rows and every workbench's, in the
/// C++'s order (see the module comment).
pub fn compose(window: &MainWindow) -> Composed {
    compose_with(window, true)
}

/// `compose`, with the rows a restored layout gave the top toolbars when `restored`, else as
/// built.
pub fn compose_with(window: &MainWindow, restored: bool) -> Composed {
    let exec = &window.exec;
    let ui = &window.ui;
    let document = &exec.document;
    let mut signs: BTreeMap<String, (&'static str, String)> = BTreeMap::new();
    let mut problems: Vec<String> = Vec::new();

    // ---- the workbenches' rows --------------------------------------------------------------
    let format_rows = ui.format.format_menu();
    let annotation_format = ui.annotation.format_menu();
    let survey_menu = menu_named(ui.survey.menus(), "surveyMenu");
    let utility_menu = menu_named(ui.utilities.menus(), "surveyMenu");
    let road = exec.road.as_deref();
    let online = OnlineDataWorkbench::action();
    let update = ui.update.action();

    // ---- File -------------------------------------------------------------------------------
    let mut file = Menu::new("fileMenu", "&File")
        .section("Drawing")
        .action(new_action())
        .action(open_action())
        .action(save_action())
        .action(save_as_action())
        .section("Import and export");
    let mut import_menu = Menu::new("fileImportMenu", "&Import");
    let mut export_menu = Menu::new("fileExportMenu", "&Export");
    signs.insert(
        String::from("fileImportMenu"),
        (
            "Import",
            String::from(
                "Bring a file into the drawing: any file as it comes, IFC, survey points, or GIS \
                 data with its options",
            ),
        ),
    );
    signs.insert(
        String::from("fileExportMenu"),
        (
            "Export",
            String::from(
                "Write the drawing, a view's picture, survey points, a surface or a point cloud \
                 to a file",
            ),
        ),
    );
    let mut recent = Menu::new("fileRecentScripts", "Recen&t Scripts");
    signs.insert(
        String::from("fileRecentScripts"),
        (
            "RecentScripts",
            String::from("Run one of the last scripts run again"),
        ),
    );
    let listed: Vec<String> = if exec.headless {
        Vec::new()
    } else {
        exec.settings
            .list(RECENT_SCRIPTS_KEY)
            .map(<[String]>::to_vec)
            .unwrap_or_default()
    };
    for (at, path) in listed.iter().take(RECENT_SCRIPTS).enumerate() {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        recent.items.push(MenuItem::Action(act(
            &format!("recentScript{}", at + 1),
            "RecentScripts",
            &format!("&{} {name}", at + 1),
            &format!(
                "Run {}",
                crate::ifc_dialogs::to_native_separators(path)
            ),
        )));
    }
    if listed.is_empty() {
        recent.items.push(MenuItem::Action(
            act(
                "",
                "RecentScripts",
                crate::drive::controls::NO_CHOICE,
                "No script has been run yet: File > Run Script runs one",
            )
            .enabled(false),
        ));
    } else {
        recent.items.push(MenuItem::Separator);
        recent.items.push(MenuItem::Action(act(
            "recentScriptsClear",
            "Erase",
            "&Clear the List",
            "Forget the scripts listed here; the files are not touched",
        )));
    }

    // ---- Edit -------------------------------------------------------------------------------
    let history = document.history();
    let undo_text = if history.can_undo() {
        format!("&Undo {}", history.undo_name())
    } else {
        String::from("&Undo")
    };
    let undo = act("editUndo", "Undo", &undo_text, "Undo the last command")
        .shortcut(ctrl(Key::Z))
        .enabled(history.can_undo());
    let redo = act(
        "editRedo",
        "Redo",
        "&Redo",
        "Redo the command that was undone",
    )
    .shortcut(ctrl(Key::Y))
    .enabled(history.can_redo());
    let select_all = act(
        "editSelectAll",
        "SelectAll",
        "Select &All",
        "Select every entity on an unlocked layer",
    )
    .shortcut(ctrl(Key::A));
    let erase = act(
        "editErase",
        "Erase",
        "&Erase Selection",
        "Erase the selected entities",
    )
    .shortcut(plain(Key::Delete));
    let edit = Menu::new("editMenu", "&Edit")
        .action(undo.clone())
        .action(redo.clone())
        .separator()
        .action(select_all.clone())
        .action(
            act(
                "editDeselect",
                "Deselect",
                "&Deselect",
                "Clear the selection, and end the running tool",
            )
            .shortcut(plain(Key::Escape)),
        )
        .action(
            act(
                "editSelectById",
                "SelectById",
                "Select by &ID...",
                "Select entities by the ids LIST and INFO print, and find them",
            )
            .opens("selectByIdDialog"),
        )
        .action(erase.clone())
        .action(act(
            "editCopyViewImage",
            "CopyViewImage",
            "Copy &View as Image",
            "Copy the plan view, as it is on screen, to the clipboard as an image (SNAPSHOT \
             CLIPBOARD)",
        ))
        .separator()
        .action(
            act(
                "editAttributes",
                "Attributes",
                "A&ttributes...",
                "The selection's attributes as a tree: add, change, rename and remove them, each \
                 one undo step",
            )
            .shortcut(ctrl(Key::Num1)),
        );

    // ---- View -------------------------------------------------------------------------------
    let active = exec.workspace.state(exec.workspace.active_id());
    let takes = |request: ZoomRequestKind| {
        active
            .as_ref()
            .is_some_and(|state| zoom_takes(state.kind, request))
    };
    let extents = act(
        "viewZoomExtents",
        "ZoomExtents",
        "Zoom &Extents",
        "Fit the whole drawing in the view (ZOOM EXTENTS)",
    )
    .shortcut(ctrl(Key::E));
    let grid = act("viewGrid", "Grid", "&Grid", "Show or hide the grid")
        .shortcut(plain(Key::F7))
        .checkable(exec.workspace.grid_visible());
    let snap = act(
        "viewSnap",
        "Snap",
        "Object &Snap",
        "Snap the cursor to endpoints, midpoints, centres and intersections",
    )
    .shortcut(plain(Key::F3))
    .checkable(crate::view_workspace::ViewWorkspace::snap_enabled(document));
    let mut snap_menu = Menu::new("viewSnapModes", "Snap &Modes");
    signs.insert(
        String::from("viewSnapModes"),
        (
            "Snap",
            String::from(
                "Which points Object Snap finds: ends, middles, centres, crossings and more",
            ),
        ),
    );
    for mode in SNAP_MENU_MODES {
        snap_menu
            .items
            .push(MenuItem::Action(snap_mode_action(mode, document.drafting().snap_modes)));
    }
    // ---- meshplan: View > Meshes Seen from Above (VIEWS MESHES), the window's one switch ----
    let mut mesh_menu = Menu::new("viewMeshesMenu", "Meshes Seen &from Above");
    signs.insert(
        String::from("viewMeshesMenu"),
        (
            "ViewTop",
            String::from(
                "How plan views, plots and sheets draw imported meshes: their faces, their \
                 footprints, or not at all",
            ),
        ),
    );
    let meshes_now = crate::plan_painter::mesh_plan_display();
    for (display, icon, text, tip) in [
        (
            katana_cad::scene::MeshPlanDisplay::Faces,
            "ViewTop",
            "&Faces",
            "Each face seen from above, filled in its colour and shaded by its slope (VIEWS \
             MESHES FACES)",
        ),
        (
            katana_cad::scene::MeshPlanDisplay::Footprint,
            "GisHull",
            "F&ootprints",
            "Each mesh as the dashed outline of its hull (VIEWS MESHES FOOTPRINT)",
        ),
        (
            katana_cad::scene::MeshPlanDisplay::Hidden,
            "LayerVisible",
            "&Hidden",
            "Meshes drawn in the 3D views only (VIEWS MESHES HIDDEN)",
        ),
    ] {
        mesh_menu.items.push(MenuItem::Action(
            act(&format!("viewMeshes{display:?}"), icon, text, tip)
                .checkable(meshes_now == display),
        ));
    }
    // ---- end meshplan ----
    let mut view = Menu::new("viewMenu", "&View")
        .section("Display")
        .action(extents.clone())
        .action(act(
            "viewZoomIn",
            "ZoomIn",
            "Zoom &In",
            "Twice as close about the centre of the active view (ZOOM IN)",
        )
        .enabled(takes(ZoomRequestKind::In)))
        .action(
            act(
                "viewZoomOut",
                "ZoomOut",
                "Zoom &Out",
                "Twice as far about the centre of the active view (ZOOM OUT)",
            )
            .enabled(takes(ZoomRequestKind::Out)),
        )
        .action(
            act(
                "viewZoomSelection",
                "ZoomSelection",
                "Zoom to Sele&ction",
                "Frame what is selected in the active view (ZOOM SELECTION)",
            )
            .enabled(takes(ZoomRequestKind::Scope)),
        )
        .action(
            act(
                "viewZoomTo",
                "ZoomTo",
                "&Zoom To...",
                "Frame what a scope takes - layers, a filter, the selection - in a view (ZOOM \
                 <scope>)",
            )
            .opens("zoomToDialog"),
        )
        .action(
            act(
                "viewPaperScale",
                "ViewPlan",
                "Paper Style Sc&ale...",
                "Size paper-based linestyles and symbols in the active plan view without zooming \
                 (VIEWS SCALE)",
            )
            .opens("viewScaleDialog")
            .enabled(active.as_ref().is_some_and(|state| state.kind == ViewKind::Plan)),
        )
        .action(grid.clone())
        .action(snap.clone())
        .submenu(snap_menu)
        .action(
            act(
                "ThinScreenLinesAction",
                "ThinLines",
                "&Thin Screen Lines (Faster)",
                "Draw plan-view lines one pixel wide, which repaints large drawings several times \
                 faster",
            )
            .checkable(crate::viewport_widget::thin_screen_lines()),
        )
        .submenu(mesh_menu) // meshplan
        .section("Viewports");
    let mut layout_menu = Menu::new("viewLayoutMenu", "Viewport &Layout");
    signs.insert(
        String::from("viewLayoutMenu"),
        (
            "LayoutQuad",
            String::from("Arrange the open views: one, two, three or four, side by side or stacked"),
        ),
    );
    for (kind, name, icon, tip) in LAYOUTS {
        layout_menu.items.push(MenuItem::Action(act(
            &format!("viewLayout{name}"),
            icon,
            kind.name(),
            tip,
        )));
    }
    view.items.push(MenuItem::Menu(layout_menu));
    let mut kind_menu = Menu::new("viewKindMenu", "Active Viewport S&hows");
    signs.insert(
        String::from("viewKindMenu"),
        (
            "View3D",
            String::from(
                "What the active view shows: the plan, the model in 3D, a section or an elevation",
            ),
        ),
    );
    let active_kind = exec.workspace.active_view_kind();
    for (kind, icon, tip) in KINDS {
        kind_menu.items.push(MenuItem::Action(
            act(&format!("viewShows{}", kind.name()), icon, kind.name(), tip)
                .checkable(active.is_some() && active_kind == kind),
        ));
    }
    view.items.push(MenuItem::Menu(kind_menu));
    let linkable_now = active.as_ref().is_some_and(|state| linkable(state.kind));
    view.items.push(MenuItem::Action(
        act(
            "viewLinkActive",
            "ViewLinked",
            "Lin&k This View",
            "Pan and zoom the active plan view together with the other linked views, or take it \
             out of the link (VIEWS LINK, VIEWS UNLINK)",
        )
        .shortcut(ctrl_shift(Key::L))
        .checkable(linkable_now && active.as_ref().is_some_and(|state| state.linked))
        .enabled(linkable_now),
    ));
    view.items.push(MenuItem::Action(act(
        "viewUnlinkAll",
        "ViewUnlinked",
        "U&nlink All Views",
        "Take every view out of the link, so each pans and zooms on its own (VIEWS UNLINK ALL)",
    )));
    view.items.push(MenuItem::Action(
        act(
            "viewSelectionGhosts",
            "LayerVisible",
            "Show the Selection on Hi&dden Layers",
            "Draw what is selected faintly in the active view where it hides the layer, or not at \
             all (VIEWS SET <id> ghosts=on|off)",
        )
        .checkable(active.as_ref().is_some_and(|state| state.selection_ghosts))
        .enabled(active.is_some()),
    ));
    view.items.push(MenuItem::Section(String::from("3D views")));
    let mut standard = Menu::new("viewStandardMenu", "Standard &3D Views");
    signs.insert(
        String::from("viewStandardMenu"),
        (
            "ViewIsoSouthWest",
            String::from(
                "Turn the 3D view to look from a side, from above or below, or from a corner",
            ),
        ),
    );
    for (standard_view, name, icon, tip) in STANDARD_VIEWS {
        standard.items.push(MenuItem::Action(act(
            &format!("viewStandard{name}"),
            icon,
            standard_view_name(standard_view),
            tip,
        )));
    }
    view.items.push(MenuItem::Menu(standard));
    view.items.push(MenuItem::Action(
        act(
            "viewTogglePerspective",
            "Perspective",
            "Toggle Pe&rspective",
            "Switch the active 3D view between perspective and a true-scale orthographic view",
        )
        .shortcut(plain(Key::F9)),
    ));
    view.items.push(MenuItem::Action(act(
        "viewVerticalExaggeration",
        "VerticalExaggeration",
        "&Vertical Exaggeration...",
        "Stretch heights in the 3D and section views, so a gentle grade can be seen \
         (EXAGGERATION)",
    )));
    // The Window section (buildDocks, then buildWindowMenu).
    view.items.push(MenuItem::Section(String::from("Window")));
    let mut panels = Menu::new("viewPanels", "&Panels");
    signs.insert(
        String::from("viewPanels"),
        (
            "Panels",
            String::from(
                "Show or hide the panels: Layers, Properties, Command Line, Reference Data and \
                 Vertices",
            ),
        ),
    );
    for (name, icon, title, shown) in [
        ("viewPanelLayers", "Layers", "Layers", ui.docks.layers),
        ("viewPanelProperties", "Properties", "Properties", ui.docks.properties),
        ("viewPanelCommandLine", "CommandLine", "Command Line", ui.docks.command_line),
        (
            "viewPanelReferenceData",
            "ReferenceData",
            "Reference Data",
            ui.docks.reference,
        ),
    ] {
        panels.items.push(MenuItem::Action(
            act(
                name,
                icon,
                title,
                &format!("Show or hide the {title} panel"),
            )
            .checkable(shown),
        ));
    }
    panels
        .items
        .push(MenuItem::Action(ui.drawing.panel_action()));
    let surfaces_panel = crate::terrain::surfaces_panel::surfaces_panel_ui();
    panels.items.push(MenuItem::Action(
        surfaces_panel.toggle.clone().checkable(ui.docks.surfaces),
    ));
    view.items.push(MenuItem::Menu(panels));

    // ---- Draw, Modify, Annotate and the tools' own menus -------------------------------------
    let mut draw = ui.tool_rows.draw.clone();
    let mut modify = ui.tool_rows.modify.clone();
    let mut annotate = ui.tool_rows.annotate.clone();
    ui.tools.refresh_menu(&mut draw);
    ui.tools.refresh_menu(&mut modify);
    ui.tools.refresh_menu(&mut annotate);
    // Annotate > Edit Text, Edit Label, Label Layout Report and the Leaders, the annotation
    // workbench's, after the catalogue's tools.
    append(&mut annotate, &ui.annotation.annotate_menu());
    for family in ui.tools.families() {
        // The family's submenu shows its first variant's picture, as the C++ gave it.
        let icon = ui
            .tools
            .action(&family.first)
            .map_or("Processing", |action| action.icon);
        signs.insert(family.menu.name.clone(), (icon, family.status_tip.clone()));
    }
    let made: Vec<Menu> = ui
        .tool_rows
        .made
        .iter()
        .map(|menu| {
            let mut menu = menu.clone();
            ui.tools.refresh_menu(&mut menu);
            menu
        })
        .collect();

    // ---- Format -----------------------------------------------------------------------------
    let mut format = format_rows;
    append(&mut format, &annotation_format);

    // ---- Survey -----------------------------------------------------------------------------
    let mut survey = survey_menu.unwrap_or_else(|| Menu::new("surveyMenu", "&Survey"));
    if let Some(utilities) = &utility_menu {
        append(&mut survey, utilities);
    }

    // ---- Terrain ----------------------------------------------------------------------------
    let mut terrain = Menu::new("terrainMenu", "&Terrain")
        .section("Alignments")
        .action(alignment_manager_action())
        .section("Surfaces")
        .action(surface_from(
            "surfaceFromPointCloud",
            "SurfaceFromCloud",
            "Surface From &Point Cloud...",
            "Triangulate a surface from an imported point cloud's ground returns",
        ))
        .action(surface_from(
            "surfaceFromRaster",
            "SurfaceFromRaster",
            "Surface From &Raster...",
            "Triangulate a surface from an elevation raster's true values, read through GDAL",
        ))
        .action(surface_from(
            "surfaceFromDrawing",
            "SurfaceFromDrawing",
            "Surface From &Drawing...",
            "Triangulate a surface from the levelled points and lines of the drawing, a scope of \
             it or a filter",
        ));
    // Terrain > Surfaces Panel, after Surface From Drawing (installSurfacesPanel).
    terrain
        .items
        .push(MenuItem::Action(surfaces_panel.terrain_item.clone()));
    // Edit Surface and Combine Surfaces (surface_edit_ui's finish).
    for item in &ui.terrain_edit_items {
        terrain.items.push(item.clone());
    }
    signs.insert(
        ui.surface_edit.menu().name.clone(),
        (
            SurfaceEditUi::menu_icon(),
            String::from(SurfaceEditUi::menu_tip()),
        ),
    );
    terrain
        .items
        .push(MenuItem::Action(surface_styles_action()));
    terrain = terrain
        .section("Sections")
        .action(section_along_selection())
        .action(section_along_alignment());
    if let Some(road) = road {
        terrain = terrain
            .section("Corridors")
            .action(road.corridors_action())
            .action(road.quantities_action());
    }
    terrain = terrain.section("Volumes").action(volumes_action());
    // The geoprocessing packages' Analysis and DEM submenus (menu_table).
    append(&mut terrain, ui.geo_menus.terrain());
    for name in ["terrainAnalysisMenu", "terrainDemMenu"] {
        if let Some((icon, tip)) = ui.geo_menus.submenu_sign(name) {
            signs.insert(String::from(name), (icon, String::from(tip)));
        }
    }

    // ---- BIM --------------------------------------------------------------------------------
    let mut bim = Menu::new("bimMenu", "&BIM");
    if let Some(road) = road {
        append(&mut bim, &road.menu());
    }
    append(&mut bim, &ui.road_objects.menu());

    // ---- GIS --------------------------------------------------------------------------------
    let mut gis = Menu::new("gisMenu", "&GIS")
        .section("Vector")
        .action(import_vector_action())
        .action(export_action())
        .section("Raster")
        .action(import_raster_action())
        .action(export_dem_action())
        .section("Point cloud")
        .action(import_cloud_action())
        .action(export_cloud_action())
        .action(copc_action())
        // Dataset Information reads a vector file, a raster or a point cloud, so it is under none
        // of the three: a section of its own, where a bare rule left it under Point Cloud's.
        .section("Any dataset")
        .action(dataset_info_action())
        .section("Online")
        .action(online.clone());
    append(&mut gis, ui.geo_menus.gis());

    // ---- Help -------------------------------------------------------------------------------
    let help = Menu::new("helpMenu", "&Help")
        .action(
            act(
                "helpCommandReference",
                "CommandReference",
                "&Command Reference...",
                "Every command the command line accepts, searchable",
            )
            .shortcut(plain(Key::F1))
            .opens("commandReferenceDialog"),
        )
        .action(
            act(
                "helpSheetCommands",
                "Help",
                "&Sheets and Plotting Commands...",
                "The sheet verbs and every option they take (HELP SHEETS)",
            )
            .opens("commandReferenceDialog"),
        )
        .action(
            act(
                "helpKeyboardShortcuts",
                "Keyboard",
                "&Keyboard Shortcuts...",
                "Every key the window answers to, where its command is and what it does",
            )
            .opens("keyboardShortcutsDialog"),
        )
        .separator()
        .action(update)
        .action(act(
            "helpAbout",
            "About",
            "&About Katana...",
            "Version and build information",
        ));

    // ---- File, finished ---------------------------------------------------------------------
    // File's two submenus take File's own items and then Survey's and GIS's by name.
    let mut pool = pool_of(&[&gis, &survey]);
    pool.extend([
        import_action(),
        import_ifc_action(),
        export_action(),
        export_ifc_action(),
        export_image_action(),
    ]);
    fill_by_name(
        &mut import_menu,
        &[
            ("", &["fileImport", "fileImportIfc"]),
            ("Survey", &["surveyImport"]),
            (
                "GIS",
                &["importVectorData", "importRaster", "importPointCloud", "onlineData"],
            ),
        ],
        &pool,
        &mut problems,
    );
    fill_by_name(
        &mut export_menu,
        &[
            ("", &["fileExportVector", "fileExportIfc", "fileExportViewImage"]),
            ("Survey", &["surveyExport"]),
            ("GIS", &["exportSurfaceDem", "exportPointCloud"]),
        ],
        &pool,
        &mut problems,
    );
    file.items.push(MenuItem::Menu(import_menu));
    file.items.push(MenuItem::Menu(export_menu));
    file = file
        .section("Scripts")
        .action(run_script_action())
        .submenu(recent)
        .section("Plot")
        .action(plot_action())
        .action(sheets_action())
        .action(plot_sheets_action())
        .section("Project")
        .action(start_screen_action())
        .action(project_crs_action())
        .action(summary_action())
        // A section of its own, as every group of File is: a bare rule among headed sections read
        // as a section that had lost its title (the fourth look audit).
        .section("Application")
        .action(CustomisationWorkbench::settings_action())
        .action(quit_action(exec.headless));

    // ---- the menu bar, in the order a CAD user reads it --------------------------------------
    let mut menus = vec![file, edit, view, draw, modify, annotate];
    menus.extend(made);
    menus.extend([format, survey, terrain, bim, gis, help]);

    // ---- the toolbars, in the order makeToolBar made them -------------------------------------
    let mut bars: Vec<(ToolBar, BarPlace)> = Vec::new();
    let mut file_bar = ToolBar::new("FileToolBar", "File");
    file_bar.items = vec![
        ToolBarItem::Action(new_action()),
        ToolBarItem::Action(open_action()),
        ToolBarItem::Action(save_action()),
        ToolBarItem::Separator,
        ToolBarItem::Action(import_action()),
        ToolBarItem::Action(export_action()),
        ToolBarItem::Action(plot_action()),
        ToolBarItem::Action(sheets_action()),
    ];
    bars.push((file_bar, BarPlace::Top(0)));
    let mut edit_bar = ToolBar::new("EditToolBar", "Edit");
    edit_bar.items = vec![
        ToolBarItem::Widget(String::from("editUndoButton")),
        ToolBarItem::Widget(String::from("editRedoButton")),
        ToolBarItem::Separator,
        ToolBarItem::Action(select_all),
        ToolBarItem::Action(erase),
    ];
    bars.push((edit_bar, BarPlace::Top(0)));
    let mut properties_bar = ToolBar::new("PropertiesToolBar", "Properties");
    properties_bar.items = vec![ToolBarItem::Widget(String::from("CurrentStyleCombo"))];
    bars.push((properties_bar, BarPlace::Top(0)));
    let mut view_bar = ToolBar::new("ViewToolBar", "View");
    view_bar.items = vec![
        ToolBarItem::Action(extents),
        ToolBarItem::Separator,
        ToolBarItem::Action(grid),
        ToolBarItem::Action(snap),
    ];
    bars.push((view_bar, BarPlace::Top(0)));
    // The Surface Edit toolbar, made before the tools filled it; seatThirdToolBarRow puts it,
    // with BIM, on a third row.
    let mut surface_edit_bar = ui.surface_edit.tool_bar().clone();
    ui.tools.refresh_tool_bar(&mut surface_edit_bar);
    let mut draw_bar = ToolBar::new("DrawToolBar", "Draw");
    draw_bar.items.push(ToolBarItem::Action(
        act(
            "toolSelect",
            "Select",
            "&Select",
            "Pick entities, or drag a window or crossing box; stops the running tool",
        )
        .checkable(exec.workspace.active_tool_id().is_empty()),
    ));
    draw_bar.items.push(ToolBarItem::Separator);
    let mut drawn = ui.tool_rows.draw_bar.clone();
    ui.tools.refresh_tool_bar(&mut drawn);
    draw_bar.items.extend(drawn.items);
    let mut annotate_bar = ui.tool_rows.annotate_bar.clone();
    ui.tools.refresh_tool_bar(&mut annotate_bar);
    let mut modify_bar = ui.tool_rows.modify_bar.clone();
    ui.tools.refresh_tool_bar(&mut modify_bar);
    bars.push((draw_bar, BarPlace::Left(0)));
    bars.push((annotate_bar, BarPlace::Left(0)));
    bars.push((modify_bar, BarPlace::Right(0)));
    let mut format_bar = ToolBar::new("FormatToolBar", "Format");
    format_bar.items.push(ToolBarItem::Action(layers_action()));
    format_bar
        .items
        .extend(ui.format.format_tool_bar().items);
    format_bar
        .items
        .extend(ui.annotation.format_tool_bar().items);
    bars.push((format_bar, BarPlace::Top(0)));
    for survey_bar in ui.survey.tool_bars() {
        bars.push((survey_bar, BarPlace::Top(1)));
    }
    let mut terrain_bar = ToolBar::new("TerrainToolBar", "Terrain");
    terrain_bar.items = vec![
        ToolBarItem::Action(alignment_manager_action()),
        ToolBarItem::Separator,
        ToolBarItem::Action(surface_from(
            "surfaceFromPointCloud",
            "SurfaceFromCloud",
            "Surface From &Point Cloud...",
            "Triangulate a surface from an imported point cloud's ground returns",
        )),
        ToolBarItem::Action(surface_from(
            "surfaceFromRaster",
            "SurfaceFromRaster",
            "Surface From &Raster...",
            "Triangulate a surface from an elevation raster's true values, read through GDAL",
        )),
        ToolBarItem::Action(surface_from(
            "surfaceFromDrawing",
            "SurfaceFromDrawing",
            "Surface From &Drawing...",
            "Triangulate a surface from the levelled points and lines of the drawing, a scope of \
             it or a filter",
        )),
        ToolBarItem::Action(surface_styles_action()),
        ToolBarItem::Separator,
        ToolBarItem::Action(section_along_selection()),
        ToolBarItem::Action(section_along_alignment()),
        ToolBarItem::Separator,
        ToolBarItem::Action(volumes_action()),
    ];
    bars.push((terrain_bar, BarPlace::Top(1)));
    let mut bim_bar = ToolBar::new("BIMToolBar", "BIM");
    if let Some(road) = road {
        bim_bar.items.extend(road.tool_bar().items);
    }
    bim_bar
        .items
        .extend(ui.road_objects.tool_bar().items);
    let mut gis_bar = ToolBar::new("GISToolBar", "GIS");
    gis_bar.items = vec![
        ToolBarItem::Action(import_vector_action()),
        ToolBarItem::Action(import_raster_action()),
        ToolBarItem::Action(import_cloud_action()),
        ToolBarItem::Separator,
        ToolBarItem::Action(export_cloud_action()),
        ToolBarItem::Action(export_dem_action()),
        ToolBarItem::Action(copc_action()),
        ToolBarItem::Separator,
        ToolBarItem::Action(dataset_info_action()),
    ];
    bars.push((gis_bar, BarPlace::Top(1)));
    // seatThirdToolBarRow: Surface Edit and BIM on a row of their own.
    bars.push((surface_edit_bar, BarPlace::Top(2)));
    bars.push((bim_bar, BarPlace::Top(2)));
    // The places a restored layout or a move gave the toolbars, in their order along their lines;
    // one the places do not hold (a toolbar added by a later build) stays as built, after them.
    // Before View > Toolbars is made, whose Place items are checked by them.
    if restored && let Some(places) = &ui.bars.places {
        apply_places(&mut bars, places);
    }
    let drafting = ui.drawing.drafting_bar(document);

    // ---- View > Toolbars, Text Size, Reset Window Layout (buildWindowMenu) -------------------
    let mut toolbars = Menu::new("viewToolBars", "Tool&bars").section("Show");
    signs.insert(
        String::from("viewToolBars"),
        (
            "Toolbars",
            String::from(
                "Show or hide each toolbar, choose their icons' size, and whether they show their \
                 names",
            ),
        ),
    );
    // In the order makeToolBar made them, which is the order the C++ lists them in: Surface Edit
    // was made before the tools filled it, BIM before GIS.
    const MADE: [&str; 13] = [
        "FileToolBar",
        "EditToolBar",
        "PropertiesToolBar",
        "ViewToolBar",
        "SurfaceEditToolBar",
        "DrawToolBar",
        "AnnotateToolBar",
        "ModifyToolBar",
        "FormatToolBar",
        "SurveyToolBar",
        "TerrainToolBar",
        "BIMToolBar",
        "GISToolBar",
    ];
    // A tool family's button on a bar shows its first tool's icon, as its submenu does.
    let family_icon = |name: &str| -> Option<&'static str> {
        let family = ui
            .tools
            .families()
            .iter()
            .find(|family| family.button.as_deref() == Some(name))?;
        ui.tools.action(&family.first).map(|action| action.icon)
    };
    for name in MADE {
        if let Some((bar, _)) = bars.iter().find(|(bar, _)| bar.name == name) {
            toolbars.items.push(MenuItem::Action(tool_bar_toggle(
                bar,
                ui.bars.shown(&bar.name),
                tool_bar_icon(bar, &family_icon),
            )));
        }
    }
    // Where each toolbar is: docked along a side, or floating (main_window::docking). A submenu a
    // toolbar, in the order of the toggles above, each of its items one of its dock actions.
    toolbars.items.push(MenuItem::Section(String::from("Place")));
    for name in MADE {
        if let Some((bar, place)) = bars.iter().find(|(bar, _)| bar.name == name) {
            let icon = tool_bar_icon(bar, &family_icon);
            let menu = tool_bar_place_menu(bar, place, icon);
            signs.insert(menu.name.clone(), (icon, menu.tip.clone()));
            toolbars.items.push(MenuItem::Menu(menu));
        }
    }
    toolbars.items.push(MenuItem::Section(String::from("Buttons")));
    toolbars.items.push(MenuItem::Action(
        act(
            "viewToolBarNames",
            "Rename",
            "Show Toolbar &Names",
            "Show each toolbar's name before its buttons, so the groups are told apart without \
             hovering over them",
        )
        .checkable(ui.bars.names_shown),
    ));
    for ((pixels, text, name), icon) in ICON_SIZES.into_iter().zip(ICON_SIZE_ICONS) {
        toolbars.items.push(MenuItem::Action(
            act(
                name,
                icon,
                text,
                &format!("Toolbar icons {pixels} pixels square"),
            )
            .checkable(pixels == ui.bars.icon_size),
        ));
    }
    toolbars.items.push(MenuItem::Section(String::from("Layout")));
    toolbars.items.push(MenuItem::Action(reset_tool_bars_action()));
    let mut text_size = Menu::new("viewTextSize", "Te&xt Size");
    signs.insert(
        String::from("viewTextSize"),
        (
            "TextSize",
            String::from(
                "How large the window's text is: menus, panels, dialogs and the command line",
            ),
        ),
    );
    for ((size, label), icon) in TEXT_SIZES
        .into_iter()
        .zip(["&Small", "S&tandard", "&Large", "&Extra Large"])
        .zip(TEXT_SIZE_ICONS)
    {
        let steps = text_size_steps(size);
        let tip = if steps == 0 {
            String::from("The platform's own text size")
        } else {
            format!(
                "The platform's text size {} {} point{}",
                if steps > 0 { "and" } else { "less" },
                steps.abs(),
                if steps.abs() == 1 { "" } else { "s" }
            )
        };
        text_size.items.push(MenuItem::Action(
            act(text_size_action_name(size), icon, label, &tip)
                .checkable(size == crate::theme::text_size()),
        ));
    }
    if let Some(view_menu) = menus.iter_mut().find(|menu| menu.name == "viewMenu") {
        view_menu.items.push(MenuItem::Menu(toolbars));
        view_menu.items.push(MenuItem::Menu(text_size));
        view_menu.items.push(MenuItem::Action(act(
            "viewResetLayout",
            "ResetLayout",
            "Reset &Window Layout",
            "Put the panels and toolbars back where a new window has them",
        )));
    }
    bars.push((drafting, BarPlace::Drafting));
    // What fill_by_name could not find: logged by the window once its log exists.
    let _ = problems;
    // Each submenu's status tip on its row too, where a parent menu's report reads it, as Qt's
    // reportWidget reads the statusTip of every item, a submenu's menuAction() among them.
    for menu in &mut menus {
        tip_submenus(menu, &signs);
    }
    Composed {
        menus,
        bars,
        signs,
        panel_actions: vec![crate::property_panel::copy_action()],
    }
}

/// The steps a history list shows each way, the next first, as far as a list can be read: a
/// longer history is said in a last line and reached by typing UNDO n (refreshHistoryMenus).
const HISTORY_LISTED: usize = 25;

/// MainWindow::refreshHistoryMenus: the lists the Edit toolbar's Undo and Redo buttons drop down
/// (editUndoMenu, editRedoMenu), untitled as the C++'s QMenus were. The k-th item, undoStep k or
/// redoStep k, runs UNDO k or REDO k through the one executor (MainWindow::trigger). Made from the
/// history whenever they are asked for, where the C++ refilled them as it changed.
pub fn history_menus(document: &katana_cad::Document) -> [Menu; 2] {
    let history = document.history();
    // `which` says where the k steps are: the last ones for Undo, the next for Redo.
    let fill = |name: &str, names: Vec<&str>, verb: &str, item: &str, which: &str| {
        let mut menu = Menu::new(name, "");
        for (k, step) in names.iter().take(HISTORY_LISTED).enumerate() {
            let k = k + 1;
            let line = format!("{verb} {k}");
            let tip = if k == 1 {
                format!("{line}: {step}")
            } else {
                format!("{line}: the {which} {k} steps, down to {step}")
            };
            // '&&' so a name's own '&' is shown, not taken for a mnemonic.
            menu.items.push(MenuItem::Action(
                Action::new(&format!("{item}{k}"), &format!("{k}  {}", step.replace('&', "&&")))
                    .tip(&tip),
            ));
        }
        if names.len() > HISTORY_LISTED {
            menu.items.push(MenuItem::Action(
                Action::new(
                    "",
                    &format!("{} more: type {verb} n", names.len() - HISTORY_LISTED),
                )
                .enabled(false),
            ));
        }
        menu
    };
    [
        fill("editUndoMenu", history.undo_names(), "UNDO", "undoStep", "last"),
        fill("editRedoMenu", history.redo_names(), "REDO", "redoStep", "next"),
    ]
}

/// Gives every submenu of `menu`, at any depth, the icon and the status tip `signs` keeps for it
/// by its name: the icon its item shows in the menu, as every action's item shows its own.
fn tip_submenus(menu: &mut Menu, signs: &BTreeMap<String, (&'static str, String)>) {
    for item in &mut menu.items {
        if let MenuItem::Menu(sub) = item {
            if let Some((icon, tip)) = signs.get(&sub.name) {
                sub.tip.clone_from(tip);
                sub.icon = icon;
            }
            tip_submenus(sub, signs);
        }
    }
}

