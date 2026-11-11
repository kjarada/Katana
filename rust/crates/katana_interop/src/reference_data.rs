//! Reference data: imported material a drawing is worked ON TOP OF rather
//! than material the drawing is made of - georeferenced imagery and point
//! clouds (include/katana/interop/reference_data.hpp,
//! src/katana_interop/reference_data.cpp).
//!
//! Why this is not part of the entity model. An entity is something the user
//! draws, selects, snaps to, edits and undoes. A 400-megapixel orthophoto and
//! a 2-million-point LiDAR scan are none of those things: they are backdrop.
//! Making them entities would put a hundred megabytes of pixels through the
//! command stack's before-image undo machinery for a visibility toggle, and
//! would give the user a "select all" that returns an orthophoto.
//!
//! So reference data lives beside the model, is not undoable, and its pixels
//! and points are not saved into the project database. The project records
//! each layer's SOURCE and its display settings (ReferenceSource, below, kept
//! in the project metadata's reference layers) and the front ends read them
//! again when it opens (REFS RESTORE, docs/interop.md "Reference layers").
//! The renderer paints this, it does not own it.
//!
//! `ReferenceData`'s Rust shape: plain data, a value the session holds (step
//! 6) and the window shows (step 7). No command changes it - it is not
//! undoable by design - so it is an ordinary struct with `&mut self`
//! methods, Clone and comparable, and holds no handle, no callback and no
//! lifetime. The C++'s `add` overloads are one `add` taking anything that is a
//! `ReferenceLayer`; its `findRaster`/`findPointCloud`, which hand back a
//! pointer to change a layer through, are `find_raster_mut`/
//! `find_point_cloud_mut`, beside a shared `find_raster`/`find_point_cloud`
//! for a reader.

use std::path::{Component, Path, PathBuf};

use katana_core::error::make_error;
use katana_core::json::{Json, JsonResult, OrderedJson};
use katana_core::path_text::path_exists;
use katana_core::{ErrorCode, Result};
use katana_entity::EnumName;
use katana_geometry::{Box2, Point2};
use katana_math::cpp;
use katana_pointcloud::{PointCloudBounds, PointCloudPoint};

/// The C++ `toString` overload set: `to_string(RasterRole::Imagery)`,
/// `to_string(PointColorMode::Flat)`.
pub use katana_entity::entity::to_string;

use crate::archive12d::{Archive12dImportOptions, import_archive12d};
use crate::import::{
    PointCloudImportOptions, RasterImportOptions, SourceKind, import_point_cloud, import_raster,
    kind_for_path,
};

pub type ReferenceId = u64;

// ---- raster -----------------------------------------------------------------------------------

/// What a raster is to the drawing: a picture to work over, heights, or a
/// product made from other data by a geoprocessing run
/// (docs/geoprocessing.md). Derived rasters are kept in the project's cache
/// and can be made again from their `derivation`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RasterRole {
    #[default]
    Imagery,
    Elevation,
    Derived,
}

/// How a raster's values are drawn: as they are, or shaded from its heights.
/// RASTER SHADE renders a shaded style into the picture itself, a derived
/// raster whose RGBA the views and the sheet painter draw as they draw any
/// other; the style says which picture it is (docs/terrain.md, "Shading").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RasterDisplayStyle {
    #[default]
    Plain,
    Hillshade,
    Relief,
    ReliefHillshade,
    Slope,
}

/// The facts of band 1 a reply or a legend needs, read at full precision -
/// never from the RGBA display copy. Absent is not zero: a band without a
/// no-data value, or whose range was never read, says so.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RasterBandFacts {
    pub data_type: String,
    pub no_data: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub unit: String,
    pub vertical_crs: String,
}

/// The C++ `toString(RasterRole)`: a word a record keeps and reads back
/// (raster_role_from_word).
impl EnumName for RasterRole {
    fn name(self) -> &'static str {
        match self {
            RasterRole::Imagery => "imagery",
            RasterRole::Elevation => "elevation",
            RasterRole::Derived => "derived",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RasterOverlay {
    pub id: ReferenceId,
    pub name: String,
    pub source: PathBuf,

    pub width: i32,
    pub height: i32,
    /// width * height * 4, top row first.
    pub rgba: Vec<u8>,

    /// Maps this image's pixel grid to world coordinates (GDAL order).
    pub geotransform: [f64; 6],
    pub has_geotransform: bool,
    pub projection_wkt: String,

    pub visible: bool,
    pub opacity: f64,

    /// Where a raster fetched from a web service came from, and the terms it
    /// came under (docs/gis_online.md, "Licences and attribution"): the
    /// service's address with any key removed, its licence, and the
    /// attribution the publisher asks for. Empty for a file.
    pub source_url: String,
    pub licence: String,
    pub attribution: String,

    pub role: RasterRole,
    pub facts: RasterBandFacts,
    /// The line that made a Derived raster ("GDAL raster hillshade ... FROM
    /// SURFACE ground CELL 1"); empty for any other.
    pub derivation: String,
    pub display_style: RasterDisplayStyle,
}

impl Default for RasterOverlay {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            source: PathBuf::new(),
            width: 0,
            height: 0,
            rgba: Vec::new(),
            geotransform: [0.0, 1.0, 0.0, 0.0, 0.0, -1.0],
            has_geotransform: false,
            projection_wkt: String::new(),
            visible: true,
            opacity: 1.0,
            source_url: String::new(),
            licence: String::new(),
            attribution: String::new(),
            role: RasterRole::Imagery,
            facts: RasterBandFacts::default(),
            derivation: String::new(),
            display_style: RasterDisplayStyle::Plain,
        }
    }
}

impl RasterOverlay {
    /// World position of a pixel corner (px, py), pixel units, origin
    /// top-left.
    pub fn pixel_to_world(&self, px: f64, py: f64) -> Point2 {
        let g = &self.geotransform;
        Point2::new(g[0] + px * g[1] + py * g[2], g[3] + px * g[4] + py * g[5])
    }

    /// World-space corners of the image. Computed through the full affine,
    /// so a rotated or north-up-negative geotransform is handled rather than
    /// assumed away: all four corners are transformed and the bounding box
    /// taken.
    pub fn world_bounds(&self) -> Box2 {
        let mut bounds = Box2::default();
        if self.width <= 0 || self.height <= 0 {
            return bounds;
        }
        // All four corners, not just two: a geotransform may carry rotation
        // terms (g[2], g[4]), and a north-up image has a NEGATIVE g[5], so
        // assuming the top-left pixel maps to the minimum corner is wrong in
        // both cases.
        let width = f64::from(self.width);
        let height = f64::from(self.height);
        bounds.expand(self.pixel_to_world(0.0, 0.0));
        bounds.expand(self.pixel_to_world(width, 0.0));
        bounds.expand(self.pixel_to_world(0.0, height));
        bounds.expand(self.pixel_to_world(width, height));
        bounds
    }
}

// ---- point cloud ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PointColorMode {
    /// ramp over the cloud's own z range
    #[default]
    Elevation,
    /// ramp over the cloud's own intensity range
    Intensity,
    /// ASPRS class colours
    Classification,
    /// the file's own RGB, where it has any
    SourceColor,
    Flat,
}

/// The C++ `toString(PointColorMode)`: the names the window's menus show.
impl EnumName for PointColorMode {
    fn name(self) -> &'static str {
        match self {
            PointColorMode::Elevation => "Elevation",
            PointColorMode::Intensity => "Intensity",
            PointColorMode::Classification => "Classification",
            PointColorMode::SourceColor => "Source colour",
            PointColorMode::Flat => "Flat",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointCloudLayer {
    pub id: ReferenceId,
    pub name: String,
    pub source: PathBuf,

    pub points: Vec<PointCloudPoint>,
    pub bounds: PointCloudBounds,
    pub projection_wkt: String,
    /// Points in the file before decimation, so the UI can be honest about
    /// showing a sample rather than the whole cloud.
    pub source_point_count: u64,
    pub decimation_step: u32,

    pub visible: bool,
    pub color_mode: PointColorMode,
    pub point_size: f64,
}

impl Default for PointCloudLayer {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            source: PathBuf::new(),
            points: Vec::new(),
            bounds: PointCloudBounds::default(),
            projection_wkt: String::new(),
            source_point_count: 0,
            decimation_step: 1,
            visible: true,
            color_mode: PointColorMode::Elevation,
            point_size: 1.0,
        }
    }
}

impl PointCloudLayer {
    pub fn world_bounds(&self) -> Box2 {
        let mut bounds = Box2::default();
        if self.bounds.empty() {
            return bounds;
        }
        bounds.expand(Point2::new(self.bounds.min_x, self.bounds.min_y));
        bounds.expand(Point2::new(self.bounds.max_x, self.bounds.max_y));
        bounds
    }

    pub fn is_decimated(&self) -> bool {
        self.decimation_step > 1 || (self.points.len() as u64) < self.source_point_count
    }
}

/// Colour for one point under a mode, given the ranges to normalise against.
/// Returned as 0-255 RGB so no rendering type appears in this layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// A five-stop ramp: blue, cyan, green, yellow, red. Perceptually ordered
/// enough for elevation and intensity, and unambiguous in print.
const RAMP: [Rgb; 5] = [
    Rgb::new(33, 68, 173),
    Rgb::new(43, 172, 190),
    Rgb::new(90, 176, 74),
    Rgb::new(233, 199, 63),
    Rgb::new(201, 62, 48),
];

fn ramp_at(normalised: f64) -> Rgb {
    if !normalised.is_finite() {
        return Rgb::new(128, 128, 128);
    }
    let clamped = cpp::clamp(normalised, 0.0, 1.0);
    let scaled = clamped * (RAMP.len() - 1) as f64;
    let lower = scaled.floor() as usize;
    let upper = (lower + 1).min(RAMP.len() - 1);
    let t = scaled - lower as f64;
    // std::lround, half away from zero, as f64::round rounds; the result is
    // within 0..=255 for t in [0, 1].
    let mix =
        |a: u8, b: u8| (f64::from(a) + t * (f64::from(b) - f64::from(a))).round() as i64 as u8;
    Rgb::new(
        mix(RAMP[lower].r, RAMP[upper].r),
        mix(RAMP[lower].g, RAMP[upper].g),
        mix(RAMP[lower].b, RAMP[upper].b),
    )
}

/// ASPRS standard classification colours (LAS 1.4 table 17); unknown classes
/// get a neutral grey rather than an arbitrary colour.
pub fn classification_color(classification: u8) -> Rgb {
    // ASPRS LAS 1.4 table 17. Classes the standard leaves reserved, and any
    // vendor-specific class above 18, get a neutral grey rather than an
    // arbitrary colour that would imply a meaning this code cannot know.
    match classification {
        0 => Rgb::new(150, 150, 150),  // created, never classified
        1 => Rgb::new(180, 180, 180),  // unclassified
        2 => Rgb::new(166, 124, 82),   // ground
        3 => Rgb::new(150, 200, 120),  // low vegetation
        4 => Rgb::new(95, 170, 80),    // medium vegetation
        5 => Rgb::new(40, 120, 50),    // high vegetation
        6 => Rgb::new(214, 104, 64),   // building
        7 => Rgb::new(220, 80, 200),   // low point (noise)
        9 => Rgb::new(60, 130, 220),   // water
        10 => Rgb::new(130, 100, 160), // rail
        11 => Rgb::new(90, 90, 95),    // road surface
        13 => Rgb::new(230, 200, 90),  // wire - guard
        14 => Rgb::new(230, 170, 60),  // wire - conductor
        15 => Rgb::new(170, 140, 110), // transmission tower
        16 => Rgb::new(200, 180, 140), // wire structure connector
        17 => Rgb::new(120, 140, 190), // bridge deck
        18 => Rgb::new(240, 60, 60),   // high noise
        _ => Rgb::new(128, 128, 128),
    }
}

pub fn color_for_point(
    point: &PointCloudPoint,
    mode: PointColorMode,
    min_value: f64,
    max_value: f64,
) -> Rgb {
    // A degenerate range (a perfectly flat surface, or a cloud with constant
    // intensity) would divide by zero; the midpoint of the ramp is the honest
    // answer, since every point really does have the same value.
    let span = max_value - min_value;
    let degenerate = !(span > 0.0) || !span.is_finite();
    let along = |value: f64| {
        if degenerate {
            0.5
        } else {
            (value - min_value) / span
        }
    };
    match mode {
        PointColorMode::Elevation => ramp_at(along(point.z)),
        PointColorMode::Intensity => ramp_at(along(point.intensity)),
        PointColorMode::Classification => classification_color(point.classification),
        PointColorMode::SourceColor => {
            // Falls back to the elevation ramp when the file carries no
            // colour, so choosing this mode on a colourless cloud shows
            // something rather than a uniform black.
            if point.has_color {
                Rgb::new(point.red, point.green, point.blue)
            } else {
                ramp_at(along(point.z))
            }
        }
        PointColorMode::Flat => Rgb::new(200, 200, 200),
    }
}

// ---- the collection ---------------------------------------------------------------------------

/// A layer read again from its source: the C++ `std::variant<RasterOverlay,
/// PointCloudLayer>`, and what ReferenceData::add takes.
#[allow(
    clippy::large_enum_variant,
    reason = "The C++ std::variant holds either layer inline; a layer is read once and moved into the collection, never stored in bulk."
)]
#[derive(Clone, Debug, PartialEq)]
pub enum ReferenceLayer {
    Raster(RasterOverlay),
    PointCloud(PointCloudLayer),
}

impl From<RasterOverlay> for ReferenceLayer {
    fn from(raster: RasterOverlay) -> Self {
        Self::Raster(raster)
    }
}

impl From<PointCloudLayer> for ReferenceLayer {
    fn from(cloud: PointCloudLayer) -> Self {
        Self::PointCloud(cloud)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceData {
    rasters: Vec<RasterOverlay>,
    point_clouds: Vec<PointCloudLayer>,
    missing: Vec<String>,
    next_id: ReferenceId,
}

impl Default for ReferenceData {
    fn default() -> Self {
        Self {
            rasters: Vec::new(),
            point_clouds: Vec::new(),
            missing: Vec::new(),
            next_id: 1,
        }
    }
}

impl ReferenceData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rasters(&self) -> &[RasterOverlay] {
        &self.rasters
    }

    pub fn point_clouds(&self) -> &[PointCloudLayer] {
        &self.point_clouds
    }

    /// Ids are monotonic and never reused, matching the entity database's
    /// rule, so a stale reference can never silently resolve to a different
    /// layer.
    pub fn add(&mut self, layer: impl Into<ReferenceLayer>) -> ReferenceId {
        let id = self.next_id;
        self.next_id += 1;
        match layer.into() {
            ReferenceLayer::Raster(mut raster) => {
                raster.id = id;
                self.rasters.push(raster);
            }
            ReferenceLayer::PointCloud(mut cloud) => {
                cloud.id = id;
                self.point_clouds.push(cloud);
            }
        }
        id
    }

    pub fn find_raster(&self, id: ReferenceId) -> Option<&RasterOverlay> {
        self.rasters.iter().find(|raster| raster.id == id)
    }

    pub fn find_raster_mut(&mut self, id: ReferenceId) -> Option<&mut RasterOverlay> {
        self.rasters.iter_mut().find(|raster| raster.id == id)
    }

    pub fn find_point_cloud(&self, id: ReferenceId) -> Option<&PointCloudLayer> {
        self.point_clouds.iter().find(|cloud| cloud.id == id)
    }

    pub fn find_point_cloud_mut(&mut self, id: ReferenceId) -> Option<&mut PointCloudLayer> {
        self.point_clouds.iter_mut().find(|cloud| cloud.id == id)
    }

    pub fn remove(&mut self, id: ReferenceId) -> bool {
        if let Some(at) = self.rasters.iter().position(|raster| raster.id == id) {
            self.rasters.remove(at);
            return true;
        }
        if let Some(at) = self.point_clouds.iter().position(|cloud| cloud.id == id) {
            self.point_clouds.remove(at);
            return true;
        }
        false
    }

    /// Every layer, and every record kept as missing: a drawing closed.
    pub fn clear(&mut self) {
        self.rasters.clear();
        self.point_clouds.clear();
        self.missing.clear();
        // next_id is deliberately NOT reset: ids are never reused, matching
        // the entity database, so a stale id cannot resolve to a different
        // layer.
    }

    /// The records (to_record) of layers a project names whose source could
    /// not be read when it opened: kept, so that saving does not drop a
    /// layer because its drive was not connected that day, and listed as
    /// missing.
    pub fn keep_missing(&mut self, record: String) {
        self.missing.push(record);
    }

    pub fn missing(&self) -> &[String] {
        &self.missing
    }

    /// Drops the missing record of that layer name, case-insensitively;
    /// false when none has it.
    pub fn forget_missing(&mut self, name: &str) -> bool {
        let found = self.missing.iter().position(|record| {
            parse_reference_record(record)
                .is_ok_and(|parsed| parsed.name.as_bytes().eq_ignore_ascii_case(name.as_bytes()))
        });
        match found {
            Some(at) => {
                self.missing.remove(at);
                true
            }
            None => false,
        }
    }

    pub fn empty(&self) -> bool {
        self.rasters.is_empty() && self.point_clouds.is_empty()
    }

    pub fn size(&self) -> usize {
        self.rasters.len() + self.point_clouds.len()
    }

    /// Bounds of every VISIBLE layer, for Zoom Extents. Empty when nothing is
    /// visible, which the caller must distinguish from "nothing is loaded".
    pub fn visible_bounds(&self) -> Box2 {
        let mut bounds = Box2::default();
        for raster in &self.rasters {
            if raster.visible {
                bounds.expand_box(raster.world_bounds());
            }
        }
        for cloud in &self.point_clouds {
            if cloud.visible {
                bounds.expand_box(cloud.world_bounds());
            }
        }
        bounds
    }
}

// ---- the words a line and a record use --------------------------------------------------------

const COLOUR_WORDS: [(PointColorMode, &str); 5] = [
    (PointColorMode::Elevation, "elevation"),
    (PointColorMode::Intensity, "intensity"),
    (PointColorMode::Classification, "classification"),
    (PointColorMode::SourceColor, "rgb"),
    (PointColorMode::Flat, "flat"),
];

const STYLE_WORDS: [(RasterDisplayStyle, &str); 5] = [
    (RasterDisplayStyle::Plain, "plain"),
    (RasterDisplayStyle::Hillshade, "hillshade"),
    (RasterDisplayStyle::Relief, "relief"),
    (RasterDisplayStyle::ReliefHillshade, "relief+hillshade"),
    (RasterDisplayStyle::Slope, "slope"),
];

/// The C++ `toWord` overload set: one word each, which a line and a record
/// use, unlike the names the window's menus show.
pub trait Word: Copy {
    fn word(self) -> &'static str;
}

/// elevation, intensity, classification, rgb, flat.
impl Word for PointColorMode {
    fn word(self) -> &'static str {
        COLOUR_WORDS
            .iter()
            .find(|(mode, _)| *mode == self)
            .map_or("elevation", |&(_, word)| word)
    }
}

/// plain, hillshade, relief, relief+hillshade, slope.
impl Word for RasterDisplayStyle {
    fn word(self) -> &'static str {
        STYLE_WORDS
            .iter()
            .find(|(style, _)| *style == self)
            .map_or("plain", |&(_, word)| word)
    }
}

/// `toWord(mode)` / `toWord(style)`.
pub fn to_word<T: Word>(value: T) -> &'static str {
    value.word()
}

pub fn point_color_mode_from_word(word: &str) -> Option<PointColorMode> {
    let lowered = word.to_ascii_lowercase();
    COLOUR_WORDS
        .iter()
        .find(|(_, each)| lowered == *each)
        .map(|&(mode, _)| mode)
}

pub fn display_style_from_word(word: &str) -> Option<RasterDisplayStyle> {
    let lowered = word.to_ascii_lowercase();
    STYLE_WORDS
        .iter()
        .find(|(_, each)| lowered == *each)
        .map(|&(style, _)| style)
}

/// imagery, elevation, derived: to_string's words, read back.
pub fn raster_role_from_word(word: &str) -> Option<RasterRole> {
    let lowered = word.to_ascii_lowercase();
    [
        RasterRole::Imagery,
        RasterRole::Elevation,
        RasterRole::Derived,
    ]
    .into_iter()
    .find(|role| lowered == to_string(*role))
}

/// A path as UTF-8 text with '/' between its names: the C++'s
/// `generic_u8string`. A path that is not UTF-8 is written with U+FFFD for
/// its bad bytes, as the record's JSON writer replaces them.
fn utf8(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

/// A file on disk, not a /vsi path or a URL GDAL reads.
fn on_disk(source: &Path) -> bool {
    let name = utf8(source);
    !name.starts_with("/vsi") && !name.contains("://")
}

/// std::filesystem::exists(path, error): false when it cannot tell.
fn exists(path: &Path) -> bool {
    path_exists(path).unwrap_or(false)
}

/// std::filesystem's lexically_normal on the components Rust's Path splits:
/// "." gone, a name followed by ".." gone with it, a ".." straight after the
/// root gone; "." for a path that normalises to nothing, and nothing for
/// nothing.
fn lexically_normal(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        return PathBuf::new();
    }
    let mut kept: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match kept.last() {
                Some(Component::Normal(_)) => {
                    kept.pop();
                }
                Some(Component::RootDir) => {}
                _ => kept.push(component),
            },
            other => kept.push(other),
        }
    }
    if kept.is_empty() {
        return PathBuf::from(".");
    }
    kept.iter().collect()
}

/// std::filesystem's `path.lexically_relative(base)` for two lexically normal
/// paths: empty for another root or another absoluteness, "." for the base
/// itself, else ".." for each of the base's names past the common start,
/// then the rest of the path.
fn lexically_relative(path: &Path, base: &Path) -> PathBuf {
    let path_parts: Vec<Component<'_>> = path.components().collect();
    let base_parts: Vec<Component<'_>> = base.components().collect();
    let root_of = |parts: &[Component<'_>]| match parts.first() {
        Some(Component::Prefix(prefix)) => Some(prefix.as_os_str().to_owned()),
        _ => None,
    };
    if root_of(&path_parts) != root_of(&base_parts)
        || path.is_absolute() != base.is_absolute()
        || (!path.has_root() && base.has_root())
    {
        return PathBuf::new();
    }
    let common = path_parts
        .iter()
        .zip(&base_parts)
        .take_while(|(a, b)| a == b)
        .count();
    if common == path_parts.len() && common == base_parts.len() {
        return PathBuf::from(".");
    }
    let mut climbs: i64 = 0;
    for part in &base_parts[common..] {
        match part {
            Component::ParentDir => climbs -= 1,
            Component::CurDir => {}
            _ => climbs += 1,
        }
    }
    if climbs < 0 {
        return PathBuf::new();
    }
    if climbs == 0 && common == path_parts.len() {
        return PathBuf::from(".");
    }
    let mut out = PathBuf::new();
    for _ in 0..climbs {
        out.push("..");
    }
    for part in &path_parts[common..] {
        out.push(part.as_os_str());
    }
    out
}

/// The record's version: a record of a newer one is refused, not half read.
const RECORD_VERSION: i32 = 1;

// ---- what a project keeps ---------------------------------------------------------------------

/// The C++ `ReferenceSource::Kind`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReferenceSourceKind {
    #[default]
    Raster,
    PointCloud,
}

/// A reference layer as the project records it: where it came from and how
/// it is shown, never its pixels or points. One line of versioned JSON a
/// layer (to_record): a field a newer build adds is skipped by an older one,
/// and a record of a newer version is refused by name rather than half read.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceSource {
    pub kind: ReferenceSourceKind,
    pub name: String,
    /// The file read again: a raster, a point cloud, or the archive a cloud
    /// came in with (the cloud of this name is taken from it). Recorded
    /// absolute: a path typed relative to where katana_cli ran is resolved
    /// when recorded, not against wherever the project is opened from.
    pub source: PathBuf,
    /// The same file relative to the project's folder, when it lies inside
    /// it ("data/terrain.asc"; empty otherwise): read when `source` is gone,
    /// so a project moved or copied with its data keeps its layers.
    pub in_project: PathBuf,
    pub visible: bool,
    /// A raster's.
    pub opacity: f64,
    pub role: RasterRole,
    pub display_style: RasterDisplayStyle,
    pub derivation: String,
    pub source_url: String,
    pub licence: String,
    pub attribution: String,
    /// The display copy's larger side, so it is read again at the size it
    /// had.
    pub max_pixels: i32,
    /// A point cloud's.
    pub color_mode: PointColorMode,
    pub point_size: f64,
    /// The points it held, as the import's budget, so it is read again at the
    /// density it had.
    pub budget: u64,
}

impl Default for ReferenceSource {
    fn default() -> Self {
        Self {
            kind: ReferenceSourceKind::Raster,
            name: String::new(),
            source: PathBuf::new(),
            in_project: PathBuf::new(),
            visible: true,
            opacity: 1.0,
            role: RasterRole::Imagery,
            display_style: RasterDisplayStyle::Plain,
            derivation: String::new(),
            source_url: String::new(),
            licence: String::new(),
            attribution: String::new(),
            max_pixels: 4096,
            color_mode: PointColorMode::Elevation,
            point_size: 1.0,
            budget: 2_000_000,
        }
    }
}

/// The C++ `sourceOf(const RasterOverlay&)`.
pub fn source_of_raster(raster: &RasterOverlay) -> ReferenceSource {
    ReferenceSource {
        kind: ReferenceSourceKind::Raster,
        name: raster.name.clone(),
        source: raster.source.clone(),
        visible: raster.visible,
        opacity: raster.opacity,
        role: raster.role,
        display_style: raster.display_style,
        derivation: raster.derivation.clone(),
        source_url: raster.source_url.clone(),
        licence: raster.licence.clone(),
        attribution: raster.attribution.clone(),
        max_pixels: raster.width.max(raster.height).max(1),
        ..ReferenceSource::default()
    }
}

/// The C++ `sourceOf(const PointCloudLayer&)`.
pub fn source_of_point_cloud(cloud: &PointCloudLayer) -> ReferenceSource {
    ReferenceSource {
        kind: ReferenceSourceKind::PointCloud,
        name: cloud.name.clone(),
        source: cloud.source.clone(),
        visible: cloud.visible,
        color_mode: cloud.color_mode,
        point_size: cloud.point_size,
        budget: (cloud.points.len() as u64).max(1),
        ..ReferenceSource::default()
    }
}

/// One line of JSON: {"version":1,"kind":"raster","name":...}.
pub fn to_record(source: &ReferenceSource) -> String {
    let mut record = OrderedJson::object_of([
        ("version", OrderedJson::from(RECORD_VERSION)),
        (
            "kind",
            OrderedJson::from(match source.kind {
                ReferenceSourceKind::Raster => "raster",
                ReferenceSourceKind::PointCloud => "pointcloud",
            }),
        ),
        ("name", OrderedJson::from(&source.name)),
        ("source", OrderedJson::from(utf8(&source.source))),
        ("visible", OrderedJson::from(source.visible)),
    ]);
    if !source.in_project.as_os_str().is_empty() {
        record["in_project"] = OrderedJson::from(utf8(&source.in_project));
    }
    if source.kind == ReferenceSourceKind::Raster {
        record["opacity"] = OrderedJson::from(source.opacity);
        record["role"] = OrderedJson::from(to_string(source.role));
        record["display"] = OrderedJson::from(to_word(source.display_style));
        record["derivation"] = OrderedJson::from(&source.derivation);
        record["url"] = OrderedJson::from(&source.source_url);
        record["licence"] = OrderedJson::from(&source.licence);
        record["attribution"] = OrderedJson::from(&source.attribution);
        record["max_pixels"] = OrderedJson::from(source.max_pixels);
    } else {
        record["color"] = OrderedJson::from(to_word(source.color_mode));
        record["point_size"] = OrderedJson::from(source.point_size);
        record["budget"] = OrderedJson::from(source.budget);
    }
    // Compact, so a record is one line; a line break inside a value is
    // escaped by the JSON itself.
    record.dump_with(-1, ' ', false)
}

// What parse_reference_record reads, nlohmann's exceptions apart: Err is the
// type_error value() throws for a member of another type than its default's
// ("version": "one"), which the C++ lets out of parseReferenceRecord
// uncaught.
fn read_record(text: &str) -> JsonResult<Result<ReferenceSource>> {
    let refused = |why: &str| -> Result<ReferenceSource> {
        Err(make_error(
            ErrorCode::InvalidArgument,
            format!("a reference layer's record {why}"),
            String::from_utf8_lossy(&text.as_bytes()[..text.len().min(200)]),
        ))
    };
    let record = match Json::parse(text) {
        Ok(record) if record.is_object() => record,
        _ => return Ok(refused("is not JSON")),
    };
    let version = record.value::<i32>("version", 0)?;
    if version < 1 {
        return Ok(refused("has no version"));
    }
    if version > RECORD_VERSION {
        return Ok(refused(&format!(
            "was written by a newer Katana (version {version})"
        )));
    }
    let mut source = ReferenceSource::default();
    let kind = record.value("kind", String::new())?;
    if kind == "raster" {
        source.kind = ReferenceSourceKind::Raster;
    } else if kind == "pointcloud" {
        source.kind = ReferenceSourceKind::PointCloud;
    } else {
        return Ok(refused("names no kind of layer this build reads"));
    }
    source.name = record.value("name", String::new())?;
    source.source = PathBuf::from(record.value("source", String::new())?);
    if source.source.as_os_str().is_empty() {
        return Ok(refused("names no source"));
    }
    source.in_project = PathBuf::from(record.value("in_project", String::new())?);
    source.visible = record.value("visible", true)?;
    source.opacity = cpp::clamp(record.value("opacity", 1.0)?, 0.0, 1.0);
    source.role = raster_role_from_word(&record.value("role", String::from("imagery"))?)
        .unwrap_or(RasterRole::Imagery);
    source.display_style =
        display_style_from_word(&record.value("display", String::from("plain"))?)
            .unwrap_or(RasterDisplayStyle::Plain);
    source.derivation = record.value("derivation", String::new())?;
    source.source_url = record.value("url", String::new())?;
    source.licence = record.value("licence", String::new())?;
    source.attribution = record.value("attribution", String::new())?;
    source.max_pixels = record.value::<i32>("max_pixels", 4096)?.max(1);
    source.color_mode =
        point_color_mode_from_word(&record.value("color", String::from("elevation"))?)
            .unwrap_or(PointColorMode::Elevation);
    source.point_size = record.value("point_size", 1.0)?;
    source.budget = record.value::<u64>("budget", 2_000_000)?.max(1);
    Ok(Ok(source))
}

/// InvalidArgument for a record that is not one, and for one of a version
/// newer than this build reads, naming it.
///
/// A member of another JSON type than its default's (`"version": "one"`) is
/// nlohmann's type_error, which the C++ lets out of parseReferenceRecord
/// uncaught (a defect, docs/rust.md "Not done"). Ported as it is, as
/// online_catalogue's parse_catalogue has it: a panic carrying the
/// exception's what().
pub fn parse_reference_record(record: &str) -> Result<ReferenceSource> {
    match read_record(record) {
        Ok(answer) => answer,
        Err(error) => panic!("{}", error.what),
    }
}

/// Every layer's record, the rasters then the clouds, each in the order
/// added, then the records kept as missing: what a save puts in the project
/// metadata's reference layers. `project` is the project's folder, when it
/// has one: a source inside it is also recorded relative to it
/// (ReferenceSource::in_project).
pub fn reference_records(reference: &ReferenceData, project: Option<&Path>) -> Vec<String> {
    // A source as the record keeps it: absolute (a path typed relative to
    // the folder katana_cli ran in means nothing to a later opening from
    // another), and relative to the project too when it lies inside it.
    let root = project.map(|project| {
        std::path::absolute(project)
            .map(|absolute| lexically_normal(&absolute))
            .unwrap_or_default()
    });
    let located = |mut source: ReferenceSource| {
        if !on_disk(&source.source) {
            return source;
        }
        if let Ok(absolute) = std::path::absolute(&source.source) {
            source.source = lexically_normal(&absolute);
        }
        if let Some(root) = &root
            && source.source.is_absolute()
        {
            let inside = lexically_relative(&source.source, root);
            if !inside.as_os_str().is_empty()
                && inside.components().next() != Some(Component::ParentDir)
            {
                source.in_project = inside;
            }
        }
        source
    };
    let mut records = Vec::new();
    for raster in reference.rasters() {
        records.push(to_record(&located(source_of_raster(raster))));
    }
    for cloud in reference.point_clouds() {
        records.push(to_record(&located(source_of_point_cloud(cloud))));
    }
    records.extend(reference.missing().iter().cloned());
    records
}

/// A layer read again from its source, with its recorded name and display
/// settings: what a project's opening restores. NotFound when the source is
/// gone - which a caller reports and carries on from - and the reader's own
/// failure otherwise. Pure: reads the file, touches no ReferenceData.
/// `project` is the folder the project was opened from: a source that is
/// gone is looked for there by its in_project path, and a relative source (a
/// record of an older build) is looked for there before the working folder.
pub fn read_reference(
    recorded: &ReferenceSource,
    project: Option<&Path>,
) -> Result<ReferenceLayer> {
    // Where the file is now: where it was recorded; else inside the project
    // as the record places it there (the project moved with its data); a
    // relative source - an older build recorded it as typed - inside the
    // project, then in the working folder as before.
    let mut source = recorded.clone();
    if on_disk(&source.source)
        && let Some(project) = project
        && !exists(&source.source)
    {
        let candidates = [
            if source.in_project.as_os_str().is_empty() {
                PathBuf::new()
            } else {
                project.join(&source.in_project)
            },
            if source.source.is_relative() {
                project.join(&source.source)
            } else {
                PathBuf::new()
            },
        ];
        for candidate in candidates {
            if !candidate.as_os_str().is_empty() && exists(&candidate) {
                source.source = candidate;
                break;
            }
        }
    }
    let name = utf8(&source.source);
    if on_disk(&source.source) && !exists(&source.source) {
        return Err(make_error(
            ErrorCode::NotFound,
            format!("the source of reference layer \"{}\" is gone", source.name),
            name,
        ));
    }
    if source.kind == ReferenceSourceKind::Raster {
        let options = RasterImportOptions {
            max_pixels: source.max_pixels,
            name: source.name.clone(),
            ..RasterImportOptions::default()
        };
        let mut raster = import_raster(&source.source, &options)?;
        raster.name = source.name;
        raster.visible = source.visible;
        raster.opacity = source.opacity;
        raster.role = source.role;
        raster.display_style = source.display_style;
        raster.derivation = source.derivation;
        raster.source_url = source.source_url;
        raster.licence = source.licence;
        raster.attribution = source.attribution;
        return Ok(ReferenceLayer::Raster(raster));
    }
    let mut cloud = if kind_for_path(&source.source) == SourceKind::Archive12d {
        // A cloud that came in with an archive: the archive read again, and
        // its cloud of this name taken. The rest of it is the drawing's,
        // which the project already holds.
        let archive = import_archive12d(&source.source, &Archive12dImportOptions::default())?;
        let Some(found) = archive
            .clouds
            .into_iter()
            .find(|each| each.name == source.name)
        else {
            // The vendor's name in a message, kept as the C++ has it until
            // the C++ line changes with it (CLAUDE.md section 9).
            return Err(make_error(
                ErrorCode::NotFound,
                format!("the archive holds no point cloud \"{}\" now", source.name),
                name,
            ));
        };
        found
    } else {
        let options = PointCloudImportOptions {
            budget: source.budget,
            name: source.name.clone(),
            ..PointCloudImportOptions::default()
        };
        import_point_cloud(&source.source, &options)?
    };
    cloud.name = source.name;
    cloud.visible = source.visible;
    cloud.color_mode = source.color_mode;
    cloud.point_size = source.point_size;
    Ok(ReferenceLayer::PointCloud(cloud))
}
