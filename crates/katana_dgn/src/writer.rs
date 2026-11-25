//! Entities and their layers -> a MicroStation V7 design file, built on a seed
//! design, with no third-party library (docs/dgn.md).
//!
//! WHY A SEED. A V7 design opens with a 768-word design header that holds the
//! design's views and every active setting MicroStation starts with, most of
//! which no open document describes. A writer of its own header would guess
//! them. So the header comes from a seed file, as MicroStation and GDAL's
//! writer make every new design (dgnwrite.cpp:290 DGNCreate): a person's own
//! seed, or GDAL's seed_2d.dgn or seed_3d.dgn. Of the seed, the header and
//! the two control elements after it (digitizer setup, level symbology) are
//! copied, as DGNCreate copies them; with Katana's units, the header's
//! working units and global origin are set, at the offsets DGNCreate sets
//! them (dgnwrite.cpp:331-358).
//!
//! WHAT IS WRITTEN.
//!
//! ```text
//!   Point                  a Line of no length, as dgnlib writes a point.
//!   Line                   a Line.
//!   Polyline               a Line String, or a Shape when closed; one of
//!                          more than 101 vertices a Complex Chain or Shape of
//!                          line strings sharing their ends.
//!   Arc, Circle, Ellipse   an Arc or an Ellipse element: true curves.
//!   CurvePolyline          a Complex Chain or Shape of line strings and arcs.
//!   Text                   a Text a line, each the left end of its baseline.
//!   Spline                 a Line String of chords, counted: V7 B-splines
//!                          are not written.
//!   Labels, dimensions,    what the front end drew of them for this file,
//!   leaders                as the DXF writer writes them; skipped, counted,
//!                          when nothing was drawn.
//! ```
//!
//! A layer is a level. V7 has 63 numbered levels and no names: a layer named
//! "level N" goes back to level N, every other layer takes the lowest free
//! level in the order its entities are first met, and the levels after the
//! 63rd share level 63, counted. The reply says which layer each level holds
//! (DgnExport::levels).
//!
//! Colours: a colour table element goes into every file, MicroStation's
//! default table with each colour of the drawing that is not in it put in an
//! unused slot from 254 down, so that the colours come back exact and 0-15
//! keep their usual meaning. Black, the background MicroStation draws on, is
//! written as 0, the foreground, as the DXF writer writes it as 7.
//!
//! Heights: a 3D seed's Z is each vertex's height. An entity heighted at only
//! some vertices, or an arc whose two ends differ, is written in plan (Z 0),
//! counted, since the format has no "no height" and a Z of 0 would be a false
//! level. A 2D seed holds no heights at all, which is said.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use katana_core::error::{Error, ErrorCode, Result, make_error};
use katana_core::path_text::path_to_utf8;
use katana_core::text::format_exact_real;
use katana_core::text_encoding::{encode_windows1252, utf8_or_windows1252};
use katana_entity::annotation::annotation_model_size;
use katana_entity::entity::{ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, heights_of};
use katana_entity::text_block::{
    estimated_text_corners, estimated_width, line_origin, text_block_extent, text_lines,
};
use katana_entity::{Color, Entity, EntityId, Geometry, Model, PropertyValue, TextGeometry};
use katana_geometry::curves2d::arc_from_bulge;
use katana_geometry::{Circle2, CurvePolyline2, Ellipse2, Point2, Polyline2, Spline2};
use katana_math::numerics::{RAD_TO_DEG, TWO_PI, normalize_angle, tolerance};

use crate::codes::*;
use crate::plane::{Xyz, rotation_quaternion};
use crate::reader::{
    Content, META_FONT, META_JUSTIFICATION, META_STYLE, META_WEIGHT, WorkingUnits, identify,
    level_of_layer_name, split_elements,
};

// ---- the public interface -------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct ExportOptions<'a> {
    /// The scale paper-sized text is written at: a 2.5 mm text is 2.5 x
    /// scale / 1000 model units in the file, as the DXF writer writes it.
    pub annotation_scale: f64,
    /// Annotation the caller has drawn out for this export, by entity, as the
    /// DXF writer takes it (katana_dxf's ExportOptions::drawn): an entity found
    /// here is written as these shapes, on its layer and in its colour. A
    /// label, dimension or leader not found here is skipped and counted.
    pub drawn: Option<&'a BTreeMap<EntityId, Vec<Geometry>>>,
    /// Largest distance a chord may stand off a spline, which has no V7
    /// element here.
    pub curve_tolerance: f64,
    /// Keep the seed's working units and global origin - a person's seed,
    /// set as their office sets every design - rather than Katana's: metres
    /// and millimetres at the finest resolution that holds the drawing, the
    /// global origin at its middle.
    pub keep_seed_units: bool,
}

impl Default for ExportOptions<'_> {
    fn default() -> Self {
        Self {
            annotation_scale: 1000.0,
            drawn: None,
            curve_tolerance: 0.001,
            keep_seed_units: false,
        }
    }
}

/// A level and the layer it holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LevelUse {
    pub level: u8,
    pub layer: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DgnExport {
    /// The file (write_dgn); empty from write_dgn_file.
    pub bytes: Vec<u8>,
    /// Katana entities written.
    pub entities_written: usize,
    pub entities_skipped: usize,
    /// Elements, the design header and control elements included.
    pub elements_written: usize,
    pub three_d: bool,
    pub units: WorkingUnits,
    /// The levels written, lowest first, and the layer each holds.
    pub levels: Vec<LevelUse>,
    pub bytes_written: usize,
    pub warnings: Vec<String>,
}

impl DgnExport {
    pub fn format(&self) -> &'static str {
        if self.three_d { "DGN V7 3D" } else { "DGN V7 2D" }
    }

    /// The step a coordinate was written to, in master units.
    pub fn resolution(&self) -> f64 {
        let per_master = self.units.per_master();
        if per_master > 0.0 { 1.0 / per_master } else { 1.0 }
    }
}

/// What a seed is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeedInfo {
    pub three_d: bool,
    pub units: WorkingUnits,
    pub global_origin: [f64; 3],
}

/// `seed` checked as a V7 design with its header whole; InvalidArgument
/// saying what it is otherwise.
pub fn seed_info(seed: &[u8]) -> Result<SeedInfo> {
    let what = match identify(seed) {
        Content::Design => None,
        Content::CompoundDocument => Some("a V8 design; a seed for a V7 file is a V7 design"),
        Content::CellLibrary => Some("a cell library, not a design"),
        Content::Empty => Some("empty"),
        Content::Dxf | Content::Dwg | Content::Other => Some("not a DGN design file"),
    };
    if let Some(what) = what {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("the seed file is {what}"),
        ));
    }
    if seed.len() < HEADER_BYTES {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the seed file ends inside its design header",
        ));
    }
    let header = &seed[..HEADER_BYTES];
    let mut global_origin = [0.0; 3];
    for (axis, origin) in global_origin.iter_mut().enumerate() {
        *origin = vax_double(header, HEADER_GLOBAL_ORIGIN + 8 * axis).unwrap_or(0.0);
    }
    let name = |at: usize| {
        let pair = &header[at..at + 2];
        let end = pair.iter().rposition(|&byte| byte != 0).map_or(0, |last| last + 1);
        utf8_or_windows1252(&pair[..end]).trim().to_owned()
    };
    Ok(SeedInfo {
        three_d: header[HEADER_DIMENSION] & HEADER_THREE_D != 0,
        units: WorkingUnits {
            master: name(HEADER_MASTER_NAME),
            sub: name(HEADER_SUB_NAME),
            subunits_per_master: int32(header, HEADER_SUBUNITS_PER_MASTER).unwrap_or(0),
            units_per_subunit: int32(header, HEADER_UNITS_PER_SUBUNIT).unwrap_or(0),
        },
        global_origin,
    })
}

/// Whether any entity of `model` holds a height: what makes a 3D seed the
/// one to write with.
pub fn has_heights(model: &Model) -> bool {
    model.entities.iter().any(|entity| {
        entity.properties.contains_key(ELEVATION_PROPERTY)
            || entity.properties.contains_key(ELEVATIONS_PROPERTY)
            || matches!(&entity.geometry, Geometry::CurvePolyline(polyline) if polyline.has_heights())
    })
}

pub fn write_dgn(model: &Model, seed: &[u8], options: &ExportOptions<'_>) -> Result<DgnExport> {
    if !(options.curve_tolerance.is_finite() && options.curve_tolerance > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the curve tolerance must be positive",
        ));
    }
    if !(options.annotation_scale.is_finite() && options.annotation_scale > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the annotation scale must be positive",
        ));
    }
    let info = seed_info(seed)?;
    let mut collector = Collector::new(model, options, info.three_d);
    collector.collect();
    let (units, origin) = if options.keep_seed_units {
        if info.units.per_master() <= 0.0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the seed gives no working units to keep",
            ));
        }
        (info.units.clone(), info.global_origin)
    } else {
        katana_units(&collector.bounds, info.three_d)?
    };
    let mut encoder = Encoder {
        three_d: info.three_d,
        per_master: units.per_master(),
        origin,
        out: Vec::with_capacity(4096 + 64 * collector.items.len()),
        elements: 0,
        chains_split: 0,
    };
    // The seed's header, with Katana's units where they are written, then
    // its digitizer setup and level symbology (DGNCreate copies elements 1
    // and 2, dgnwrite.cpp:390).
    let mut header = seed[..HEADER_BYTES].to_vec();
    if !options.keep_seed_units {
        set_units(&mut header, &units, origin)?;
    }
    encoder.out.extend_from_slice(&header);
    encoder.elements += 1;
    let mut ignored = Vec::new();
    for element in split_elements(seed, &mut ignored).iter().skip(1).take(2) {
        if !element.deleted
            && matches!(element.kind, TYPE_DIGITIZER_SETUP | TYPE_LEVEL_SYMBOLOGY)
        {
            encoder.out.extend_from_slice(element.bytes);
            encoder.elements += 1;
        }
    }
    encoder.colour_table(&collector.palette.table);
    for item in &collector.items {
        encoder.item(item)?;
    }
    encoder.out.extend_from_slice(&END_OF_DESIGN);

    let mut result = DgnExport {
        entities_written: collector.written,
        entities_skipped: collector.skipped,
        elements_written: encoder.elements,
        three_d: info.three_d,
        units,
        levels: collector.levels.used(),
        warnings: collector.warnings(),
        ..DgnExport::default()
    };
    if encoder.chains_split != 0 {
        result.warnings.push(format!(
            "{} polylines too long for one complex chain (65 535 words) were written as \
             several chains",
            encoder.chains_split
        ));
    }
    result.bytes_written = encoder.out.len();
    result.bytes = encoder.out;
    Ok(result)
}

/// Writes to `path`, replacing it. Fails with FileExportFailure when the file
/// cannot be written; a failed write leaves no half-written file behind it.
pub fn write_dgn_file(
    model: &Model,
    seed: &[u8],
    path: &Path,
    options: &ExportOptions<'_>,
) -> Result<DgnExport> {
    let mut written = write_dgn(model, seed, options)?;
    let context = || utf8_or_windows1252(path_to_utf8(path));
    // Beside the target first, then over it: a write that fails part way
    // leaves the previous file, not half of a new one.
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".partial");
    let temporary = PathBuf::from(temporary);
    if std::fs::write(&temporary, &written.bytes).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file could not be written",
            context(),
        ));
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            format!("the file could not be replaced: {error}"),
            context(),
        ));
    }
    written.bytes = Vec::new();
    Ok(written)
}

// ---- units ----------------------------------------------------------------------

/// The largest coordinate of the integer plane.
const PLANE_LIMIT: f64 = 2_147_483_647.0;

/// Katana's working units for a drawing within `bounds`: master metres, the
/// finest of 0.1 mm, 1 mm, 1 cm, 1 dm and 1 m whose plane holds the drawing
/// about its middle (the plane is 2^32 steps across: at 0.1 mm, 429 km), and
/// the global origin at that middle, a whole number of metres so that a
/// coordinate written on a decimal step reads back as the nearest double to
/// it.
fn katana_units(bounds: &[Xyz; 2], three_d: bool) -> Result<(WorkingUnits, [f64; 3])> {
    const STEPS: [(i32, i32, &str); 5] = [
        (1000, 10, "mm"),
        (1000, 1, "mm"),
        (100, 1, "cm"),
        (10, 1, "dm"),
        (1, 1, "m"),
    ];
    let [low, high] = bounds;
    let empty = low.x > high.x;
    let middle = |a: f64, b: f64| if empty { 0.0 } else { (0.5 * (a + b)).round() };
    let centre = [
        middle(low.x, high.x),
        middle(low.y, high.y),
        if three_d { middle(low.z, high.z) } else { 0.0 },
    ];
    let reach = if empty {
        0.0
    } else {
        let mut reach: f64 = 0.0;
        for (axis, (a, b)) in [(low.x, high.x), (low.y, high.y), (low.z, high.z)]
            .into_iter()
            .enumerate()
        {
            if axis == 2 && !three_d {
                continue;
            }
            reach = reach.max((a - centre[axis]).abs()).max((b - centre[axis]).abs());
        }
        // A metre's margin: the ranges of arcs and texts are written
        // rounded outward.
        reach + 1.0
    };
    for (subunits, units, sub) in STEPS {
        let per_master = f64::from(subunits) * f64::from(units);
        if reach * per_master <= PLANE_LIMIT {
            let origin = centre.map(|value| -value * per_master);
            return Ok((
                WorkingUnits {
                    master: String::from("m"),
                    sub: String::from(sub),
                    subunits_per_master: subunits,
                    units_per_subunit: units,
                },
                origin,
            ));
        }
    }
    Err(Error::new(
        ErrorCode::InvalidArgument,
        format!(
            "the drawing reaches {} m from its middle, more than a V7 design plane holds even \
             at a metre's resolution",
            format_exact_real(reach)
        ),
    ))
}

/// The header's working units and global origin (dgnwrite.cpp:333-358).
fn set_units(header: &mut [u8], units: &WorkingUnits, origin: [f64; 3]) -> Result<()> {
    let name = |text: &str| {
        let mut pair = [0_u8; 2];
        for (slot, byte) in pair.iter_mut().zip(text.bytes()) {
            *slot = byte;
        }
        pair
    };
    header[HEADER_MASTER_NAME..HEADER_MASTER_NAME + 2].copy_from_slice(&name(&units.master));
    header[HEADER_SUB_NAME..HEADER_SUB_NAME + 2].copy_from_slice(&name(&units.sub));
    put_int32(units.subunits_per_master, header, HEADER_SUBUNITS_PER_MASTER);
    put_int32(units.units_per_subunit, header, HEADER_UNITS_PER_SUBUNIT);
    for (axis, value) in origin.iter().enumerate() {
        let at = HEADER_GLOBAL_ORIGIN + 8 * axis;
        header[at..at + 8].copy_from_slice(&real_bytes(*value)?);
    }
    Ok(())
}

fn real_bytes(value: f64) -> Result<[u8; 8]> {
    vax_bytes(value).ok_or_else(|| {
        make_error(
            ErrorCode::InvalidArgument,
            "a number the format cannot hold",
            format_exact_real(value),
        )
    })
}

// ---- what is written --------------------------------------------------------------

/// A piece of a complex chain or shape.
enum Member {
    Points(Vec<Xyz>),
    Arc {
        centre: Xyz,
        radius: f64,
        start: f64,
        sweep: f64,
    },
}

enum Shape {
    Line([Xyz; 2]),
    Points {
        points: Vec<Xyz>,
        closed: bool,
    },
    /// Radians: the primary axis's direction, and the parametric start and
    /// sweep from it.
    Arc {
        centre: Xyz,
        primary: f64,
        secondary: f64,
        rotation: f64,
        start: f64,
        sweep: f64,
    },
    Ellipse {
        centre: Xyz,
        primary: f64,
        secondary: f64,
        rotation: f64,
    },
    Text {
        origin: Xyz,
        bytes: Vec<u8>,
        height: f64,
        rotation: f64,
        font: u8,
        justification: u8,
    },
    Chain {
        closed: bool,
        members: Vec<Member>,
    },
}

/// What an element is drawn with: its level, colour, line style and weight.
#[derive(Clone, Copy)]
struct Look {
    level: u8,
    colour: u8,
    style: u8,
    weight: u8,
}

struct Item {
    shape: Shape,
    look: Look,
}

/// Layers -> levels (the module comment).
struct Levels {
    of_layer: HashMap<String, u8>,
    taken: [bool; 64],
    shared: usize,
}

impl Levels {
    /// Every layer of `order` (first met first) given its level.
    fn assign(order: &[String]) -> Self {
        let mut levels = Self {
            of_layer: HashMap::new(),
            taken: [false; 64],
            shared: 0,
        };
        for layer in order {
            if let Some(level) = level_of_layer_name(layer)
                && level >= 1
                && !levels.taken[usize::from(level)]
            {
                levels.taken[usize::from(level)] = true;
                levels.of_layer.insert(layer.clone(), level);
            }
        }
        let mut next = 1_usize;
        for layer in order {
            if levels.of_layer.contains_key(layer) {
                continue;
            }
            while next < 64 && levels.taken[next] {
                next += 1;
            }
            if next < 64 {
                levels.taken[next] = true;
                levels.of_layer.insert(layer.clone(), next as u8);
            } else {
                levels.of_layer.insert(layer.clone(), 63);
                levels.shared += 1;
            }
        }
        levels
    }

    fn of(&self, layer: &str) -> u8 {
        self.of_layer.get(layer).copied().unwrap_or(63)
    }

    fn used(&self) -> Vec<LevelUse> {
        let mut used: Vec<LevelUse> = self
            .of_layer
            .iter()
            .map(|(layer, &level)| LevelUse {
                level,
                layer: layer.clone(),
            })
            .collect();
        used.sort_by(|a, b| a.level.cmp(&b.level).then_with(|| a.layer.cmp(&b.layer)));
        used
    }
}

/// The colour table written (the module comment).
struct Palette {
    table: [[u8; 3]; 256],
    used: [bool; 256],
    found: HashMap<[u8; 3], u8>,
    next_free: usize,
    approximated: usize,
    black: usize,
}

impl Palette {
    fn new() -> Self {
        Self {
            table: DEFAULT_COLOURS,
            used: [false; 256],
            found: HashMap::new(),
            next_free: BACKGROUND_INDEX - 1,
            approximated: 0,
            black: 0,
        }
    }

    fn index_of(&mut self, colour: Color) -> u8 {
        let rgb = [colour.r, colour.g, colour.b];
        if rgb == [0, 0, 0] {
            self.black += 1;
            self.used[0] = true;
            return 0;
        }
        if let Some(&index) = self.found.get(&rgb) {
            return index;
        }
        let index = match self.table[..BACKGROUND_INDEX]
            .iter()
            .position(|entry| *entry == rgb)
        {
            Some(index) => index,
            None => {
                // An unused slot from 254 down to 16, so that 0-15 keep their
                // meaning; the nearest of the table when there is none.
                while self.next_free >= 16 && self.used[self.next_free] {
                    self.next_free -= 1;
                }
                if self.next_free < 16 {
                    self.approximated += 1;
                    let nearest = nearest_colour(&self.table, colour);
                    self.used[usize::from(nearest)] = true;
                    return nearest;
                }
                let slot = self.next_free;
                self.table[slot] = rgb;
                self.next_free -= 1;
                slot
            }
        };
        self.used[index] = true;
        self.found.insert(rgb, index as u8);
        index as u8
    }
}

/// The entities as items, in the model's order.
struct Collector<'m> {
    model: &'m Model,
    options: &'m ExportOptions<'m>,
    three_d: bool,
    levels: Levels,
    palette: Palette,
    items: Vec<Item>,
    bounds: [Xyz; 2],
    written: usize,
    skipped: usize,
    // What is said at the end, counted.
    annotation_skipped: usize,
    drawn_nothing: usize,
    partial_heights: usize,
    heights_dropped: usize,
    splines: usize,
    texts_cut: usize,
    characters_replaced: usize,
}

impl<'m> Collector<'m> {
    fn new(model: &'m Model, options: &'m ExportOptions<'m>, three_d: bool) -> Self {
        let mut order: Vec<String> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for entity in model.entities.iter() {
            if seen.insert(entity.layer.as_str()) {
                order.push(entity.layer.clone());
            }
        }
        Self {
            model,
            options,
            three_d,
            levels: Levels::assign(&order),
            palette: Palette::new(),
            items: Vec::new(),
            bounds: [
                Xyz::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                Xyz::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            ],
            written: 0,
            skipped: 0,
            annotation_skipped: 0,
            drawn_nothing: 0,
            partial_heights: 0,
            heights_dropped: 0,
            splines: 0,
            texts_cut: 0,
            characters_replaced: 0,
        }
    }

    fn collect(&mut self) {
        let model = self.model;
        for entity in model.entities.iter() {
            let before = self.items.len();
            if let Some(drawn) = self.options.drawn
                && let Some(shapes) = drawn.get(&entity.id)
            {
                // An annotation's shapes have no surveyed heights.
                if shapes.is_empty() {
                    self.drawn_nothing += 1;
                }
                for geometry in shapes {
                    self.add(entity, geometry, false);
                }
                self.written += 1;
                continue;
            }
            match &entity.geometry {
                Geometry::Dimension(_) | Geometry::Label(_) | Geometry::Leader(_) => {
                    self.annotation_skipped += 1;
                    self.skipped += 1;
                }
                geometry => {
                    self.add(entity, geometry, true);
                    if self.items.len() > before {
                        self.written += 1;
                    } else {
                        self.skipped += 1;
                    }
                }
            }
        }
    }

    /// The heights of `count` vertices, or None for every one in a 2D design.
    fn heights(&mut self, entity: &Entity, count: usize, own: bool) -> Vec<Option<f64>> {
        if !own || entity.properties.is_empty() {
            return vec![None; count];
        }
        let heights = heights_of(&entity.properties, count);
        if !self.three_d {
            if heights.iter().any(Option::is_some) {
                self.heights_dropped += 1;
            }
            return vec![None; count];
        }
        heights
    }

    /// Z for each vertex: the heights when every vertex has one, else 0, the
    /// partial ones counted.
    fn zs_of(&mut self, heights: &[Option<f64>]) -> Vec<f64> {
        if heights.iter().all(Option::is_some) {
            return heights.iter().map(|height| height.unwrap_or(0.0)).collect();
        }
        if heights.iter().any(Option::is_some) {
            self.partial_heights += 1;
        }
        vec![0.0; heights.len()]
    }

    fn push(&mut self, entity: &Entity, shape: Shape) {
        let colour = match entity.color {
            Some(colour) => colour,
            None => self
                .model
                .layers
                .find(&entity.layer)
                .map_or_else(Color::default, |layer| layer.color),
        };
        let small = |key: &str, limit: i64| match entity.metadata.get(key) {
            Some(PropertyValue::Integer(value)) if (0..=limit).contains(value) => *value as u8,
            _ => 0,
        };
        let item = Item {
            look: Look {
                level: self.levels.of(&entity.layer),
                colour: self.palette.index_of(colour),
                style: small(META_STYLE, 7),
                weight: small(META_WEIGHT, 31),
            },
            shape,
        };
        self.expand(&item.shape);
        self.items.push(item);
    }

    fn expand_point(&mut self, point: Xyz, reach: f64) {
        let [low, high] = &mut self.bounds;
        low.x = low.x.min(point.x - reach);
        low.y = low.y.min(point.y - reach);
        low.z = low.z.min(point.z);
        high.x = high.x.max(point.x + reach);
        high.y = high.y.max(point.y + reach);
        high.z = high.z.max(point.z);
    }

    fn expand(&mut self, shape: &Shape) {
        match shape {
            Shape::Line(points) => {
                for point in points {
                    self.expand_point(*point, 0.0);
                }
            }
            Shape::Points { points, .. } => {
                for point in points {
                    self.expand_point(*point, 0.0);
                }
            }
            Shape::Arc {
                centre,
                primary,
                secondary,
                ..
            }
            | Shape::Ellipse {
                centre,
                primary,
                secondary,
                ..
            } => self.expand_point(*centre, primary.max(*secondary)),
            Shape::Text {
                origin,
                bytes,
                height,
                ..
            } => {
                // The estimate the ranges use: a character a height wide.
                let reach = height * (bytes.len() as f64 + 1.0);
                self.expand_point(*origin, reach);
            }
            Shape::Chain { members, .. } => {
                let mut reaches = Vec::new();
                for member in members {
                    match member {
                        Member::Points(points) => {
                            reaches.extend(points.iter().map(|point| (*point, 0.0)));
                        }
                        Member::Arc { centre, radius, .. } => reaches.push((*centre, *radius)),
                    }
                }
                for (point, reach) in reaches {
                    self.expand_point(point, reach);
                }
            }
        }
    }

    /// `geometry` of `entity` as items; `own` when it is the entity's own
    /// geometry, whose heights are its properties.
    fn add(&mut self, entity: &Entity, geometry: &Geometry, own: bool) {
        let at = |point: Point2, z: f64| Xyz::new(point.x, point.y, z);
        match geometry {
            Geometry::Point(point) => {
                let heights = self.heights(entity, 1, own);
                let z = self.zs_of(&heights)[0];
                let p = at(point.position, z);
                self.push(entity, Shape::Line([p, p]));
            }
            Geometry::Line(line) => {
                let heights = self.heights(entity, 2, own);
                let z = self.zs_of(&heights);
                self.push(
                    entity,
                    Shape::Line([at(line.start, z[0]), at(line.end, z[1])]),
                );
            }
            Geometry::Polyline(polyline) => self.add_polyline(entity, polyline, own),
            Geometry::Arc(arc) => {
                if arc.sweep.abs() >= TWO_PI - tolerance::ANGULAR {
                    let circle = Circle2::new(arc.center, arc.radius);
                    self.add(entity, &Geometry::Circle(circle), own);
                    return;
                }
                let heights = self.heights(entity, 2, own);
                // One Z holds both ends only when they are the same.
                let z = match (heights[0], heights[1]) {
                    (Some(start), Some(end)) if start == end => start,
                    (None, None) => 0.0,
                    _ => {
                        self.partial_heights += 1;
                        0.0
                    }
                };
                self.push(
                    entity,
                    Shape::Arc {
                        centre: at(arc.center, z),
                        primary: arc.radius,
                        secondary: arc.radius,
                        rotation: 0.0,
                        start: arc.start_angle,
                        sweep: arc.sweep,
                    },
                );
            }
            Geometry::Circle(circle) => {
                let heights = self.heights(entity, 1, own);
                let z = self.zs_of(&heights)[0];
                self.push(
                    entity,
                    Shape::Ellipse {
                        centre: at(circle.center, z),
                        primary: circle.radius,
                        secondary: circle.radius,
                        rotation: 0.0,
                    },
                );
            }
            Geometry::Ellipse(ellipse) => self.add_ellipse(entity, ellipse, own),
            Geometry::CurvePolyline(polyline) => self.add_curve_polyline(entity, polyline, own),
            Geometry::Spline(spline) => self.add_spline(entity, spline),
            Geometry::Text(text) => self.add_text(entity, text, own),
            Geometry::Dimension(_) | Geometry::Label(_) | Geometry::Leader(_) => {
                // A drawn shape is never one of these; an entity's own is
                // skipped in collect.
            }
        }
    }

    fn add_polyline(&mut self, entity: &Entity, polyline: &Polyline2, own: bool) {
        let heights = self.heights(entity, polyline.vertices.len(), own);
        let z = self.zs_of(&heights);
        let points: Vec<Xyz> = polyline
            .vertices
            .iter()
            .zip(&z)
            .map(|(vertex, z)| Xyz::new(vertex.x, vertex.y, *z))
            .collect();
        if points.len() < 2 {
            return;
        }
        self.push(
            entity,
            Shape::Points {
                points,
                closed: polyline.closed,
            },
        );
    }

    fn add_ellipse(&mut self, entity: &Entity, ellipse: &Ellipse2, own: bool) {
        let heights = self.heights(entity, 1, own);
        let z = self.zs_of(&heights)[0];
        let centre = Xyz::new(ellipse.center.x, ellipse.center.y, z);
        let primary = ellipse.major_radius();
        let secondary = ellipse.minor_radius();
        let rotation = ellipse.major_axis.angle();
        let shape = if ellipse.is_full() {
            Shape::Ellipse {
                centre,
                primary,
                secondary,
                rotation,
            }
        } else {
            Shape::Arc {
                centre,
                primary,
                secondary,
                rotation,
                start: ellipse.start_parameter,
                sweep: ellipse.sweep,
            }
        };
        self.push(entity, shape);
    }

    /// A polyline with arcs: a complex chain or shape of line strings and
    /// arcs. Its heights are its vertices'; an arc that climbs puts the whole
    /// of it in plan, since an arc element holds one Z.
    fn add_curve_polyline(&mut self, entity: &Entity, polyline: &CurvePolyline2, own: bool) {
        if polyline.vertices.len() < 2 {
            return;
        }
        let mut heights: Vec<Option<f64>> = if own && self.three_d {
            polyline.heights()
        } else {
            if own && polyline.has_heights() {
                self.heights_dropped += 1;
            }
            vec![None; polyline.vertices.len()]
        };
        let climbs = (0..polyline.segment_count()).any(|index| {
            polyline.is_arc(index)
                && heights[index] != heights[polyline.segment_end(index)]
        });
        if climbs {
            self.partial_heights += 1;
            heights = vec![None; polyline.vertices.len()];
        }
        let z = self.zs_of(&heights);
        if !polyline.has_arcs() {
            let points: Vec<Xyz> = polyline
                .vertices
                .iter()
                .zip(&z)
                .map(|(vertex, z)| Xyz::new(vertex.position.x, vertex.position.y, *z))
                .collect();
            self.push(
                entity,
                Shape::Points {
                    points,
                    closed: polyline.closed,
                },
            );
            return;
        }
        let vertex = |index: usize| {
            let position = polyline.vertices[index].position;
            Xyz::new(position.x, position.y, z[index])
        };
        let mut members = Vec::new();
        let mut run = vec![vertex(0)];
        // Each segment from its first vertex and that vertex's height, in order.
        let segments = polyline
            .vertices
            .iter()
            .zip(z.iter())
            .enumerate()
            .take(polyline.segment_count());
        for (index, (from, &height)) in segments {
            let end = polyline.segment_end(index);
            let to = polyline.vertices[end];
            match arc_from_bulge(from.position, to.position, from.bulge) {
                Some(arc) => {
                    if run.len() >= 2 {
                        members.push(Member::Points(std::mem::take(&mut run)));
                    }
                    members.push(Member::Arc {
                        centre: Xyz::new(arc.center.x, arc.center.y, height),
                        radius: arc.radius,
                        start: arc.start_angle,
                        sweep: arc.sweep,
                    });
                    run = vec![vertex(end)];
                }
                None => run.push(vertex(end)),
            }
        }
        if run.len() >= 2 {
            members.push(Member::Points(run));
        }
        self.push(
            entity,
            Shape::Chain {
                closed: polyline.closed,
                members,
            },
        );
    }

    fn add_spline(&mut self, entity: &Entity, spline: &Spline2) {
        let mut points = spline.tessellate(self.options.curve_tolerance);
        let closed = spline.is_closed_shape();
        if closed && points.len() > 2 && points.first() == points.last() {
            points.pop();
        }
        if points.len() < 2 {
            return;
        }
        self.splines += 1;
        let points = points
            .into_iter()
            .map(|point| Xyz::new(point.x, point.y, 0.0))
            .collect();
        self.push(entity, Shape::Points { points, closed });
    }

    /// A text's height in the file: its paper height, or its style's, at the
    /// export's annotation scale; else its model height (the DXF writer's
    /// rule).
    fn text_height_of(&self, text: &TextGeometry) -> f64 {
        let mut paper = text.paper_height;
        if paper <= 0.0
            && !text.style.is_empty()
            && let Some(style) = self.model.text_styles.find(&text.style)
        {
            paper = style.paper_height;
        }
        if paper > 0.0 {
            annotation_model_size(paper, self.options.annotation_scale)
        } else {
            text.height
        }
    }

    /// A text element a line, each at the left end of its baseline as the
    /// entity layer lays the lines out (text_block.rs), with estimated widths
    /// for a justification other than the bottom left.
    fn add_text(&mut self, entity: &Entity, text: &TextGeometry, own: bool) {
        let heights = self.heights(entity, 1, own);
        let z = self.zs_of(&heights)[0];
        let height = self.text_height_of(text);
        let lines = text_lines(&text.text);
        let widest = lines
            .iter()
            .map(|line| estimated_width(line) * height)
            .fold(0.0, f64::max);
        let block = text_block_extent(widest, lines.len(), height, 1.0, text.justify);
        let font = match entity.metadata.get(META_FONT) {
            Some(PropertyValue::Integer(value)) if (0..=255).contains(value) => *value as u8,
            _ => DEFAULT_FONT,
        };
        // A justification read from a file goes back with the one line it
        // was read with; any other text is written from its baseline's left.
        let justification = match entity.metadata.get(META_JUSTIFICATION) {
            Some(PropertyValue::Integer(value))
                if (0..=14).contains(value)
                    && lines.len() == 1
                    && text.justify == katana_entity::TextJustify::BottomLeft =>
            {
                *value as u8
            }
            _ => JUSTIFY_LEFT_BOTTOM,
        };
        for (index, line) in lines.iter().copied().enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            let local = line_origin(
                &block,
                index,
                estimated_width(line) * height,
                height,
                1.0,
                text.justify,
            );
            let position = text.position + local.rotated(text.rotation);
            let (mut bytes, replaced) = encode_windows1252(line);
            self.characters_replaced += replaced;
            if bytes.len() > MAXIMUM_TEXT_BYTES {
                bytes.truncate(MAXIMUM_TEXT_BYTES);
                self.texts_cut += 1;
            }
            self.push(
                entity,
                Shape::Text {
                    origin: Xyz::new(position.x, position.y, z),
                    bytes,
                    height,
                    rotation: text.rotation,
                    font,
                    justification,
                },
            );
        }
    }

    fn warnings(&self) -> Vec<String> {
        let mut said = Vec::new();
        if self.annotation_skipped != 0 {
            said.push(format!(
                "{} labels, dimensions and leaders were not written: nothing was drawn of them \
                 for this file",
                self.annotation_skipped
            ));
        }
        if self.drawn_nothing != 0 {
            said.push(format!(
                "{} labels had no room at 1:{} and were not written, as the plan view leaves \
                 them out at that scale",
                self.drawn_nothing,
                format_exact_real(self.options.annotation_scale)
            ));
        }
        if self.partial_heights != 0 {
            said.push(format!(
                "{} entities with heights the format cannot hold - at only some vertices, or \
                 different at the two ends of an arc - were written in plan (Z 0), since the \
                 format has no \"no height\" and a Z of 0 is a level",
                self.partial_heights
            ));
        }
        if self.heights_dropped != 0 {
            said.push(format!(
                "{} entities hold heights, and the seed is a 2D design, which holds none: give \
                 a 3D seed to write them",
                self.heights_dropped
            ));
        }
        if self.splines != 0 {
            said.push(format!(
                "{} splines were written as line strings of chords within {}",
                self.splines,
                format_exact_real(self.options.curve_tolerance)
            ));
        }
        if self.texts_cut != 0 {
            said.push(format!(
                "{} text lines were longer than a text element's {MAXIMUM_TEXT_BYTES} \
                 characters and were cut there",
                self.texts_cut
            ));
        }
        if self.characters_replaced != 0 {
            said.push(format!(
                "{} characters have no place in Windows-1252, the code page V7 text is in, and \
                 were written as ?",
                self.characters_replaced
            ));
        }
        if self.levels.shared != 0 {
            said.push(format!(
                "{} layers went beyond the 63 levels a V7 design has and share level 63",
                self.levels.shared
            ));
        }
        if self.palette.approximated != 0 {
            said.push(format!(
                "{} colours found no free slot in the colour table and were written as the \
                 nearest there",
                self.palette.approximated
            ));
        }
        if self.palette.black != 0 {
            said.push(format!(
                "{} black entities were written in colour 0, the foreground, as black is \
                 MicroStation's background",
                self.palette.black
            ));
        }
        said
    }
}

// ---- the bytes ------------------------------------------------------------------

/// The angle `radians` as the format's integer of 1/360000 degree, in a turn.
fn angle_steps(radians: f64) -> i32 {
    let steps = (normalize_angle(radians) * RAD_TO_DEG * ANGLE_STEPS_PER_DEGREE).round();
    // normalize_angle is in [0, 2 pi): a turn's worth of steps rounds to 0.
    (steps as i64 % 129_600_000) as i32
}

/// An arc's sweep: a sign and a magnitude, the sign in the high word's top
/// bit (dgnwrite.cpp:1036-1049).
fn sweep_bytes(radians: f64) -> [u8; 4] {
    let magnitude = (radians.abs() * RAD_TO_DEG * ANGLE_STEPS_PER_DEGREE)
        .round()
        .min(129_600_000.0) as i32;
    let mut bytes = int32_bytes(magnitude);
    if radians < 0.0 {
        bytes[1] |= 0x80;
    }
    bytes
}

struct Encoder {
    three_d: bool,
    per_master: f64,
    origin: [f64; 3],
    out: Vec<u8>,
    elements: usize,
    chains_split: usize,
}

impl Encoder {
    /// A coordinate on the integer plane; InvalidArgument for one off it.
    fn integer(&self, value: f64, axis: usize) -> Result<i32> {
        on_plane((value * self.per_master + self.origin[axis]).round(), value)
    }

    fn point(&self, point: Xyz) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(12);
        bytes.extend_from_slice(&int32_bytes(self.integer(point.x, 0)?));
        bytes.extend_from_slice(&int32_bytes(self.integer(point.y, 1)?));
        if self.three_d {
            bytes.extend_from_slice(&int32_bytes(self.integer(point.z, 2)?));
        }
        Ok(bytes)
    }

    /// A point of reals, in units of resolution (an arc's origin).
    fn real_point(&self, point: Xyz) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(24);
        bytes.extend_from_slice(&real_bytes(point.x * self.per_master + self.origin[0])?);
        bytes.extend_from_slice(&real_bytes(point.y * self.per_master + self.origin[1])?);
        if self.three_d {
            bytes.extend_from_slice(&real_bytes(point.z * self.per_master + self.origin[2])?);
        }
        Ok(bytes)
    }

    fn length(&self, value: f64) -> Result<[u8; 8]> {
        real_bytes(value * self.per_master)
    }

    /// The range block: the corners rounded outward, in binary offset
    /// (dgnwrite.cpp:786-803). A 2D design's Z is 0.
    fn range(&self, low: Xyz, high: Xyz) -> Result<[u8; 24]> {
        let corner = |value: f64, axis: usize, up: bool| -> Result<i32> {
            let scaled = value * self.per_master + self.origin[axis];
            on_plane(if up { scaled.ceil() } else { scaled.floor() }, value)
        };
        let values = [
            corner(low.x, 0, false)?,
            corner(low.y, 1, false)?,
            if self.three_d { corner(low.z, 2, false)? } else { 0 },
            corner(high.x, 0, true)?,
            corner(high.y, 1, true)?,
            if self.three_d { corner(high.z, 2, true)? } else { 0 },
        ];
        let mut bytes = [0_u8; 24];
        for (index, value) in values.into_iter().enumerate() {
            bytes[4 * index..4 * index + 4].copy_from_slice(&range_bytes(value));
        }
        Ok(bytes)
    }

    /// One element: its header and display header, then `data` from byte 36,
    /// then `linkage` as attribute data (dgnwrite.cpp:715-765).
    fn element(
        look: Look,
        kind: u8,
        complex: bool,
        data: &[u8],
        range: [u8; 24],
        linkage: &[u8],
    ) -> Vec<u8> {
        let mut bytes = vec![0_u8; 36];
        bytes.extend_from_slice(data);
        if !bytes.len().is_multiple_of(2) {
            bytes.push(0);
        }
        let attribute_index = (bytes.len() - 32) / 2;
        bytes.extend_from_slice(linkage);
        let words = bytes.len() / 2 - 2;
        let component = if complex { 0x80 } else { 0 };
        bytes[0] = look.level | component;
        bytes[1] = kind;
        put_word(words as u16, &mut bytes, 2);
        bytes[4..28].copy_from_slice(&range);
        put_word(attribute_index as u16, &mut bytes, 30);
        let properties = if linkage.is_empty() {
            0
        } else {
            PROPERTY_ATTRIBUTES
        };
        put_word(properties, &mut bytes, 32);
        bytes[34] = (look.style & 0x07) | (look.weight << 3);
        bytes[35] = look.colour;
        bytes
    }

    fn emit(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
        self.elements += 1;
    }

    /// The colour table element: group data on level 1, the background first,
    /// then colours 0-254 (dgnwrite.cpp:1508-1548).
    fn colour_table(&mut self, table: &[[u8; 3]; 256]) {
        let mut data = vec![0_u8; 2]; // the screen it is for: 0
        data.extend_from_slice(&table[BACKGROUND_INDEX]);
        for entry in &table[..BACKGROUND_INDEX] {
            data.extend_from_slice(entry);
        }
        let look = Look {
            level: LEVEL_COLOUR_TABLE,
            colour: 0,
            style: 0,
            weight: 0,
        };
        // dgnlib writes the table's range as it allocates it: zeros.
        let bytes = Self::element(look, TYPE_GROUP_DATA, false, &data, [0; 24], &[]);
        self.emit(&bytes);
    }

    fn item(&mut self, item: &Item) -> Result<()> {
        match &item.shape {
            Shape::Line([a, b]) => {
                let mut data = self.point(*a)?;
                data.extend(self.point(*b)?);
                let range = self.range(min_of(&[*a, *b]), max_of(&[*a, *b]))?;
                let bytes = Self::element(item.look, TYPE_LINE, false, &data, range, &[]);
                self.emit(&bytes);
            }
            Shape::Points { points, closed } => {
                let mut points = points.clone();
                if *closed {
                    points.push(points[0]);
                }
                if points.len() <= MAXIMUM_VERTICES {
                    let kind = if *closed { TYPE_SHAPE } else { TYPE_LINE_STRING };
                    let bytes = self.points_element(item, kind, false, &points)?;
                    self.emit(&bytes);
                } else {
                    let members = vec![Member::Points(points)];
                    self.chain(item, *closed, &members)?;
                }
            }
            Shape::Arc { .. } => {
                let bytes = self.arc_element(item, &item.shape, false)?;
                self.emit(&bytes);
            }
            Shape::Ellipse {
                centre,
                primary,
                secondary,
                rotation,
            } => {
                let mut data = Vec::with_capacity(56);
                data.extend_from_slice(&self.length(*primary)?);
                data.extend_from_slice(&self.length(*secondary)?);
                self.orientation(&mut data, *rotation);
                data.extend(self.real_point(*centre)?);
                let reach = primary.max(*secondary);
                let range = self.range(
                    Xyz::new(centre.x - reach, centre.y - reach, centre.z),
                    Xyz::new(centre.x + reach, centre.y + reach, centre.z),
                )?;
                let bytes = Self::element(item.look, TYPE_ELLIPSE, false, &data, range, &[]);
                self.emit(&bytes);
            }
            Shape::Text {
                origin,
                bytes: text,
                height,
                rotation,
                font,
                justification,
            } => {
                let size = self.text_size(*height)?;
                let mut data = vec![*font, *justification];
                data.extend_from_slice(&int32_bytes(size));
                data.extend_from_slice(&int32_bytes(size));
                self.orientation(&mut data, *rotation);
                data.extend(self.point(*origin)?);
                data.push(text.len() as u8);
                data.push(0); // no enter-data fields
                data.extend_from_slice(text);
                let corners = estimated_text_corners(&TextGeometry::new(
                    Point2::new(origin.x, origin.y),
                    utf8_or_windows1252(text),
                    *height,
                    *rotation,
                ));
                let low = corners.iter().fold(
                    Xyz::new(f64::INFINITY, f64::INFINITY, origin.z),
                    |low, corner| Xyz::new(low.x.min(corner.x), low.y.min(corner.y), low.z),
                );
                let high = corners.iter().fold(
                    Xyz::new(f64::NEG_INFINITY, f64::NEG_INFINITY, origin.z),
                    |high, corner| Xyz::new(high.x.max(corner.x), high.y.max(corner.y), high.z),
                );
                let range = self.range(low, high)?;
                let bytes = Self::element(item.look, TYPE_TEXT, false, &data, range, &[]);
                self.emit(&bytes);
            }
            Shape::Chain { closed, members } => self.chain(item, *closed, members)?,
        }
        Ok(())
    }

    /// A 2D element's rotation, or a 3D element's quaternion.
    fn orientation(&self, data: &mut Vec<u8>, radians: f64) {
        if self.three_d {
            for value in rotation_quaternion(radians) {
                data.extend_from_slice(&int32_bytes(value));
            }
        } else {
            data.extend_from_slice(&int32_bytes(angle_steps(radians)));
        }
    }

    /// A text's height or width, an integer of 6/1000 of a unit of resolution.
    fn text_size(&self, size: f64) -> Result<i32> {
        let steps = (size * self.per_master / TEXT_SIZE_STEP).round();
        if !(steps.is_finite() && steps.abs() <= PLANE_LIMIT) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a text too high for the format's text sizes at this resolution",
                format_exact_real(size),
            ));
        }
        Ok(steps as i32)
    }

    fn points_element(
        &self,
        item: &Item,
        kind: u8,
        complex: bool,
        points: &[Xyz],
    ) -> Result<Vec<u8>> {
        let mut data = Vec::with_capacity(2 + points.len() * 12);
        data.extend_from_slice(&(points.len() as u16).to_le_bytes());
        for point in points {
            data.extend(self.point(*point)?);
        }
        let range = self.range(min_of(points), max_of(points))?;
        Ok(Self::element(item.look, kind, complex, &data, range, &[]))
    }

    fn arc_element(&self, item: &Item, shape: &Shape, complex: bool) -> Result<Vec<u8>> {
        let Shape::Arc {
            centre,
            primary,
            secondary,
            rotation,
            start,
            sweep,
        } = shape
        else {
            return Ok(Vec::new());
        };
        let mut data = Vec::with_capacity(64);
        data.extend_from_slice(&int32_bytes(angle_steps(*start)));
        data.extend_from_slice(&sweep_bytes(*sweep));
        data.extend_from_slice(&self.length(*primary)?);
        data.extend_from_slice(&self.length(*secondary)?);
        self.orientation(&mut data, *rotation);
        data.extend(self.real_point(*centre)?);
        let reach = primary.max(*secondary);
        let range = self.range(
            Xyz::new(centre.x - reach, centre.y - reach, centre.z),
            Xyz::new(centre.x + reach, centre.y + reach, centre.z),
        )?;
        Ok(Self::element(item.look, TYPE_ARC, complex, &data, range, &[]))
    }

    /// A complex chain or shape: its members' elements, then as many headers
    /// as their length needs - a header says its group's length in one word
    /// (MAXIMUM_GROUP_WORDS), and a group longer than that is written as
    /// several open chains, counted. The header carries eight bytes of null
    /// linkage, as dgnlib writes it, since an element is at least 48 bytes
    /// (dgnwrite.cpp:1621-1625).
    fn chain(&mut self, item: &Item, closed: bool, members: &[Member]) -> Result<()> {
        let mut parts: Vec<(Vec<u8>, Xyz, Xyz)> = Vec::new();
        for member in members {
            match member {
                Member::Points(points) => {
                    // Pieces of at most 101 vertices, each starting where
                    // the last ended (ogrdgnlayer.cpp:1039-1050).
                    let mut start = 0;
                    while start + 1 < points.len() {
                        let end = (start + MAXIMUM_VERTICES).min(points.len());
                        let piece = &points[start..end];
                        let bytes = self.points_element(item, TYPE_LINE_STRING, true, piece)?;
                        parts.push((bytes, min_of(piece), max_of(piece)));
                        start = end - 1;
                    }
                }
                Member::Arc {
                    centre,
                    radius,
                    start,
                    sweep,
                } => {
                    let shape = Shape::Arc {
                        centre: *centre,
                        primary: *radius,
                        secondary: *radius,
                        rotation: 0.0,
                        start: *start,
                        sweep: *sweep,
                    };
                    let bytes = self.arc_element(item, &shape, true)?;
                    parts.push((
                        bytes,
                        Xyz::new(centre.x - radius, centre.y - radius, centre.z),
                        Xyz::new(centre.x + radius, centre.y + radius, centre.z),
                    ));
                }
            }
        }
        // The header's own words after its length word: five (dgnwrite.cpp:1672).
        let mut groups: Vec<Vec<(Vec<u8>, Xyz, Xyz)>> = vec![Vec::new()];
        let mut words = 5;
        for part in parts {
            let part_words = part.0.len() / 2;
            if words + part_words > MAXIMUM_GROUP_WORDS
                && groups.last().is_some_and(|group| !group.is_empty())
            {
                groups.push(Vec::new());
                words = 5;
            }
            words += part_words;
            if let Some(group) = groups.last_mut() {
                group.push(part);
            }
        }
        let several = groups.len() > 1;
        if several {
            self.chains_split += 1;
        }
        let kind = if closed && !several {
            TYPE_COMPLEX_SHAPE
        } else {
            TYPE_COMPLEX_CHAIN
        };
        for group in groups {
            if group.is_empty() {
                continue;
            }
            let total = 5 + group.iter().map(|part| part.0.len() / 2).sum::<usize>();
            let mut data = Vec::with_capacity(4);
            data.extend_from_slice(&(total as u16).to_le_bytes());
            data.extend_from_slice(&(group.len() as u16).to_le_bytes());
            let low = group.iter().fold(group[0].1, |low, part| min_xyz(low, part.1));
            let high = group.iter().fold(group[0].2, |high, part| max_xyz(high, part.2));
            let range = self.range(low, high)?;
            let header = Self::element(item.look, kind, true, &data, range, &[0; 8]);
            self.emit(&header);
            for (bytes, _, _) in group {
                self.emit(&bytes);
            }
        }
        Ok(())
    }
}

/// `scaled`, a whole number of units of resolution, as the plane's integer;
/// InvalidArgument naming `value`, the coordinate it came from, when it is
/// off the plane.
fn on_plane(scaled: f64, value: f64) -> Result<i32> {
    if scaled.is_nan() || scaled.abs() > PLANE_LIMIT {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the drawing does not fit the design plane of the seed's working units and global \
             origin: write without keeping the seed's units, or use a seed whose origin is near \
             the drawing",
            format_exact_real(value),
        ));
    }
    Ok(scaled as i32)
}

fn min_xyz(a: Xyz, b: Xyz) -> Xyz {
    Xyz::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
}

fn max_xyz(a: Xyz, b: Xyz) -> Xyz {
    Xyz::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
}

fn min_of(points: &[Xyz]) -> Xyz {
    points.iter().copied().reduce(min_xyz).unwrap_or_default()
}

fn max_of(points: &[Xyz]) -> Xyz {
    points.iter().copied().reduce(max_xyz).unwrap_or_default()
}

/// For the tests: the writer's units for a drawing within `low`..`high`.
pub fn units_for(low: Point2, high: Point2, three_d: bool) -> Result<(WorkingUnits, [f64; 3])> {
    katana_units(
        &[Xyz::new(low.x, low.y, 0.0), Xyz::new(high.x, high.y, 0.0)],
        three_d,
    )
}

/// The writer's colour table for colours met in `colours`' order: what each
/// becomes, and the table.
pub fn palette_for(colours: &[Color]) -> (Vec<u8>, [[u8; 3]; 256]) {
    let mut palette = Palette::new();
    let indices = colours.iter().map(|colour| palette.index_of(*colour)).collect();
    (indices, palette.table)
}
