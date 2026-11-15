//! How a TIN surface is drawn: a named DISPLAY STYLE saying what the plan view
//! and the 3D view show of it - its triangles, its outline, its points and
//! breaklines, its faces coloured flat or by elevation, slope or aspect, and
//! its contours - and in which colours (docs/surface_display.md;
//! include/katana/cad/terrain/surface_style.hpp).
//!
//! Styles are the drawing's: kept in the Document (Document::surface_styles),
//! saved with the project as versioned JSON, changed as one undo step, and
//! assigned to a surface by name (terrain's NamedSurface::style). A surface
//! with no style of its own is drawn in the set's default. Several surfaces
//! may share a style, and one surface may be shown in another style by
//! assigning it: "multiple styles" is several of these, not several fields on
//! a surface.
//!
//! The 3D view's shading (SurfaceStyle below) lives here, its one home; the
//! scene reads it from here.
//!
//! The colour ramps themselves are surface_ramps.cpp's (with
//! terrain/surface_colouring, wave G3), but for their names, which the checks
//! below need: the ramp table is katana_terrain's.

use std::sync::OnceLock;

use katana_core::error::make_error;
use katana_core::json::{Json, OrderedJson};
use katana_core::text::{equals_ignoring_case, trimmed};
use katana_core::text_encoding::is_valid_utf8;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::Color;
use katana_terrain::NamedSurface;
use katana_terrain::colour_ramps::colour_ramp_names;

/// How a surface is shaded in the 3D view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SurfaceStyle {
    Hidden,
    /// Triangle edges only.
    Wireframe,
    /// Filled triangles, lit.
    Shaded,
    /// Both; the edges fade out as the triangles shrink on screen.
    ShadedWithEdges,
    /// The default: ShadedWithEdges up to DENSE_SURFACE_TRIANGLES, Shaded
    /// above. A dense TIN's edges are all the eye can see at any useful zoom -
    /// they covered 73% of the pixels of a 229k-triangle survey, which drew as
    /// a black slab - and costing a line per edge they doubled the frame.
    #[default]
    Automatic,
}

/// Past this many triangles an Automatic surface is drawn Shaded.
pub const DENSE_SURFACE_TRIANGLES: usize = 50000;

/// The most bands a banded ramp may have, and the most stops a style may list
/// (a banded set of stops is a band a stop). The 3D view cuts each triangle at
/// every band edge it crosses, so its geometry grows by about 643 triangles a
/// band on the 21 182-triangle sample terrain (measured: 28 078 triangles at
/// 10 bands, 3 235 844 at 5 000), and a legend shows 16 rows however many
/// there are. 100 bands is a 0.14 m band on that terrain's 13.9 m of relief,
/// and is the Surface Styles form's own range - one number for the form, the
/// line and a project file.
pub const MAXIMUM_SURFACE_BANDS: i32 = 100;

/// What colours a surface's faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SurfaceFaces {
    /// No faces: lines only (the plan view's default).
    #[default]
    None,
    /// SurfaceDisplayStyle::flat_colour.
    Flat,
    /// The ramp, or the stops, over the heights.
    Elevation,
    /// The ramp, or the stops, over the slope in percent.
    Slope,
    /// By the direction a face looks, in eight sectors from north.
    Aspect,
}

/// A value and its colour: a stop of a smooth ramp, or the lower bound of a
/// band. In order of value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColourStop {
    pub value: f64,
    pub colour: Color,
}

/// Colours as their RGB; opaque.
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b, 255)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceContours {
    pub show: bool,
    /// The minor interval, in drawing units.
    pub interval: f64,
    /// Every Nth level is major; 0 for none.
    pub major_every: i32,
    /// The level the intervals count from.
    pub base: f64,
    pub minor_colour: Color,
    pub major_colour: Color,
    /// The level written along each major contour.
    pub label_major: bool,
}

impl Default for SurfaceContours {
    fn default() -> Self {
        Self {
            show: false,
            interval: 1.0,
            major_every: 5,
            base: 0.0,
            minor_colour: rgb(160, 120, 80),
            major_colour: rgb(110, 70, 40),
            label_major: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceDisplayStyle {
    pub name: String,
    pub description: String,

    // ---- the plan view ----
    pub show_triangles: bool,
    pub triangle_colour: Color,
    pub show_boundary: bool,
    pub boundary_colour: Color,
    pub show_points: bool,
    pub point_colour: Color,
    /// The constrained edges.
    pub show_breaklines: bool,
    pub breakline_colour: Color,
    pub plan_faces: SurfaceFaces,
    /// Of the faces over what is beneath, 0 to 1.
    pub plan_opacity: f64,
    pub contours: SurfaceContours,

    // ---- colouring by value, the plan and the 3D view alike ----
    /// A ramp by name (surface_ramp_names), stretched over the range; or, when
    /// `stops` is not empty, the stops, which override it.
    pub ramp: String,
    /// One colour per band instead of a smooth blend.
    pub banded: bool,
    /// When banded and there are no stops.
    pub bands: i32,
    pub stops: Vec<ColourStop>,
    /// The values the ramp spans. Absent: the surface's own heights (or, in
    /// the 3D view, every visible surface's, so one height is one colour on
    /// every surface - the scene's rule, docs/render.md) for Elevation, and 0
    /// to the steepest face for Slope.
    pub range_min: Option<f64>,
    pub range_max: Option<f64>,
    pub flat_colour: Color,

    // ---- the 3D view ----
    pub shading: SurfaceStyle,
    pub faces3d: SurfaceFaces,
}

impl Default for SurfaceDisplayStyle {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            show_triangles: false,
            triangle_colour: rgb(150, 150, 150),
            show_boundary: true,
            boundary_colour: rgb(60, 160, 60),
            show_points: false,
            point_colour: rgb(200, 60, 60),
            show_breaklines: false,
            breakline_colour: rgb(220, 120, 0),
            plan_faces: SurfaceFaces::None,
            plan_opacity: 0.6,
            contours: SurfaceContours::default(),
            ramp: String::from("terrain"),
            banded: false,
            bands: 10,
            stops: Vec::new(),
            range_min: None,
            range_max: None,
            flat_colour: rgb(170, 170, 160),
            shading: SurfaceStyle::Automatic,
            faces3d: SurfaceFaces::Elevation,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceStyleSet {
    pub styles: Vec<SurfaceDisplayStyle>,
    /// What a surface with no style of its own is drawn in.
    pub default_style: String,
}

impl Default for SurfaceStyleSet {
    fn default() -> Self {
        Self {
            styles: Vec::new(),
            default_style: String::from("Standard"),
        }
    }
}

impl SurfaceStyleSet {
    /// The JSON form's version (surface_styles_to_json); a newer one is
    /// refused with Unsupported rather than guessed at.
    pub const VERSION: i32 = 1;

    /// Case-insensitive; None when there is none of that name.
    pub fn find(&self, name: &str) -> Option<&SurfaceDisplayStyle> {
        self.styles
            .iter()
            .find(|style| equals_ignoring_case(style.name.as_bytes(), name.as_bytes()))
    }
}

// ---- the built-in set -------------------------------------------------------------------

/// The styles a drawing has before any is made: Standard (the default), and
/// styles for triangles, contours, elevation bands, slope bands, a cut/fill
/// map and an outline only. What Document::surface_styles answers for a
/// project that never saved any.
pub fn built_in_surface_styles() -> SurfaceStyleSet {
    let mut set = SurfaceStyleSet {
        default_style: String::from("Standard"),
        ..SurfaceStyleSet::default()
    };

    // Its outline in plan and its faces shaded by height in 3D: what every
    // surface was drawn as before there were styles, so a drawing that never
    // chose one looks as it did.
    set.styles.push(SurfaceDisplayStyle {
        name: String::from("Standard"),
        description: String::from("The outline in plan; shaded by elevation in 3D"),
        ..SurfaceDisplayStyle::default()
    });

    set.styles.push(SurfaceDisplayStyle {
        name: String::from("Triangles"),
        description: String::from(
            "Every triangle and the outline in plan; shaded with edges in 3D",
        ),
        show_triangles: true,
        shading: SurfaceStyle::ShadedWithEdges,
        ..SurfaceDisplayStyle::default()
    });

    set.styles.push(SurfaceDisplayStyle {
        name: String::from("Contours"),
        description: String::from(
            "Contours every 1 m, every fifth major and labelled, and the outline",
        ),
        contours: SurfaceContours {
            show: true,
            interval: 1.0,
            major_every: 5,
            label_major: true,
            ..SurfaceContours::default()
        },
        ..SurfaceDisplayStyle::default()
    });

    set.styles.push(SurfaceDisplayStyle {
        name: String::from("Elevation Bands"),
        description: String::from("Faces in ten bands of elevation over the terrain ramp"),
        plan_faces: SurfaceFaces::Elevation,
        ramp: String::from("terrain"),
        banded: true,
        bands: 10,
        ..SurfaceDisplayStyle::default()
    });

    // Grades a designer reads: 2% the least a surface drains at, 5% about the
    // steepest a footpath is laid at, 10% and 20% the steep road and the steep
    // batter, 33% (1 on 3) the steepest batter mown. The colours are
    // ColorBrewer 2.0's six-class RdYlGn (Brewer and Harrower, Penn State)
    // from green, flat, to red, steep.
    set.styles.push(SurfaceDisplayStyle {
        name: String::from("Slope Bands"),
        description: String::from("Faces banded by slope at 0, 2, 5, 10, 20 and 33 percent"),
        plan_faces: SurfaceFaces::Slope,
        faces3d: SurfaceFaces::Slope,
        ramp: String::from("slope"),
        banded: true,
        stops: vec![
            ColourStop {
                value: 0.0,
                colour: rgb(0x1A, 0x98, 0x50),
            },
            ColourStop {
                value: 2.0,
                colour: rgb(0x91, 0xCF, 0x60),
            },
            ColourStop {
                value: 5.0,
                colour: rgb(0xD9, 0xEF, 0x8B),
            },
            ColourStop {
                value: 10.0,
                colour: rgb(0xFE, 0xE0, 0x8B),
            },
            ColourStop {
                value: 20.0,
                colour: rgb(0xFC, 0x8D, 0x59),
            },
            ColourStop {
                value: 33.0,
                colour: rgb(0xD7, 0x30, 0x27),
            },
        ],
        ..SurfaceDisplayStyle::default()
    });

    // For a difference surface (top minus base): below 0 is cut, blue; above
    // is fill, red; 0, no change, white. ColorBrewer 2.0's seven-class RdBu,
    // its order turned so blue is low, on stops symmetric about 0.
    set.styles.push(SurfaceDisplayStyle {
        name: String::from("Cut Fill"),
        description: String::from("A difference surface: cut blue below 0, fill red above"),
        plan_faces: SurfaceFaces::Elevation,
        ramp: String::from("diverging"),
        stops: vec![
            ColourStop {
                value: -2.0,
                colour: rgb(0x21, 0x66, 0xAC),
            },
            ColourStop {
                value: -1.0,
                colour: rgb(0x67, 0xA9, 0xCF),
            },
            ColourStop {
                value: -0.5,
                colour: rgb(0xD1, 0xE5, 0xF0),
            },
            ColourStop {
                value: 0.0,
                colour: rgb(0xF7, 0xF7, 0xF7),
            },
            ColourStop {
                value: 0.5,
                colour: rgb(0xFD, 0xDB, 0xC7),
            },
            ColourStop {
                value: 1.0,
                colour: rgb(0xEF, 0x8A, 0x62),
            },
            ColourStop {
                value: 2.0,
                colour: rgb(0xB2, 0x18, 0x2B),
            },
        ],
        ..SurfaceDisplayStyle::default()
    });

    set.styles.push(SurfaceDisplayStyle {
        name: String::from("Outline"),
        description: String::from("The outline alone; a wireframe in 3D"),
        shading: SurfaceStyle::Wireframe,
        ..SurfaceDisplayStyle::default()
    });
    set
}

/// The style `surface` is drawn in: its own when the set has it, else the
/// set's default, else the built-in Standard. Never absent: a surface naming a
/// style since deleted is still drawn.
pub fn style_of<'a>(
    styles: &'a SurfaceStyleSet,
    surface: &NamedSurface,
) -> &'a SurfaceDisplayStyle {
    if !surface.style.is_empty()
        && let Some(own) = styles.find(&surface.style)
    {
        return own;
    }
    if let Some(fallback) = styles.find(&styles.default_style) {
        return fallback;
    }
    // A set with neither - made by hand, never by the Document, which keeps
    // its default among its styles - is drawn in the built-in Standard.
    static STANDARD: OnceLock<SurfaceDisplayStyle> = OnceLock::new();
    STANDARD.get_or_init(|| built_in_surface_styles().styles.swap_remove(0))
}

// ---- the checks -------------------------------------------------------------------------

fn refuse(style: &SurfaceDisplayStyle, field: &str, why: &str) -> Status {
    Err(make_error(
        ErrorCode::InvalidArgument,
        format!("style \"{}\": {field} {why}", style.name),
        format!("field={field}"),
    ))
}

fn is_ramp(name: &str) -> bool {
    surface_ramp_names()
        .iter()
        .any(|ramp| equals_ignoring_case(ramp.as_bytes(), name.as_bytes()))
}

fn holds_quote_or_break(text: &str) -> bool {
    text.contains(['"', '\r', '\n'])
}

/// InvalidArgument, naming the field, for a style that cannot be drawn: an
/// empty name, a contour interval that is not finite and positive, an opacity
/// outside 0 to 1, fewer than two or more than MAXIMUM_SURFACE_BANDS bands,
/// more than that many stops, stops out of order or not finite, a range that
/// is not finite or empty, an unknown ramp.
#[allow(
    clippy::manual_range_contains,
    reason = "The C++'s comparisons, kept as they are written: an opacity that is NaN is refused by the finiteness test before them."
)]
pub fn check_surface_style(style: &SurfaceDisplayStyle) -> Status {
    if trimmed(style.name.as_bytes()).is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a surface style needs a name",
            "field=name",
        ));
    }
    // Text that reaches the model is UTF-8 (core's text_encoding): the set is
    // written to the project as JSON. THE door for a style's text; every way
    // to a style (SURFACESTYLE NEW/SET/RENAME, the dialog, a project's own
    // JSON) ends in check_surface_style_set. Checked before anything echoes
    // the name, and the message does not: it is the bytes that are wrong. A
    // Rust String is UTF-8 already, so these hold of every style; they are
    // kept as the C++ has them, the door it is.
    if !is_valid_utf8(&style.name) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a surface style's name is not valid UTF-8",
            "field=name",
        ));
    }
    if !is_valid_utf8(&style.description) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a surface style's description is not valid UTF-8",
            "field=description",
        ));
    }
    // A line names a style in quotes, and has no way to type a quote inside
    // them; a line break would end the line.
    if holds_quote_or_break(&style.name) {
        return refuse(
            style,
            "name",
            "holds a double quote or a line break, which a command line cannot name",
        );
    }
    // The same for a description: SURFACESTYLE INFO prints it for the person
    // to type back, and the tokenizer reads a quote as the end of the word.
    if holds_quote_or_break(&style.description) {
        return refuse(
            style,
            "description",
            "holds a double quote or a line break, which a command line cannot carry",
        );
    }
    if style.name.as_bytes() != trimmed(style.name.as_bytes()) {
        return refuse(style, "name", "begins or ends with a blank");
    }
    let contours = &style.contours;
    if !contours.interval.is_finite() || contours.interval <= 0.0 {
        return refuse(
            style,
            "interval",
            "must be a finite length greater than zero",
        );
    }
    if !contours.base.is_finite() {
        return refuse(style, "base", "must be a finite level");
    }
    if contours.major_every < 0 {
        return refuse(style, "major", "must be 0 (none) or a count of intervals");
    }
    if !style.plan_opacity.is_finite() || style.plan_opacity < 0.0 || style.plan_opacity > 1.0 {
        return refuse(style, "opacity", "must be from 0 to 1");
    }
    if style.bands < 2 || style.bands > MAXIMUM_SURFACE_BANDS {
        return refuse(
            style,
            "bands",
            &format!("must be 2 to {MAXIMUM_SURFACE_BANDS}"),
        );
    }
    if style.stops.len() > MAXIMUM_SURFACE_BANDS as usize {
        return refuse(
            style,
            "stops",
            &format!("may be at most {MAXIMUM_SURFACE_BANDS}"),
        );
    }
    for (i, stop) in style.stops.iter().enumerate() {
        if !stop.value.is_finite() {
            return refuse(style, "stops", "must be finite values");
        }
        if i > 0 && !(stop.value > style.stops[i - 1].value) {
            return refuse(style, "stops", "must rise from one to the next");
        }
    }
    if style.range_min.is_some_and(|value| !value.is_finite())
        || style.range_max.is_some_and(|value| !value.is_finite())
    {
        return refuse(style, "min", "and max must be finite values");
    }
    if let (Some(low), Some(high)) = (style.range_min, style.range_max)
        && !(low < high)
    {
        return refuse(style, "min", "must be below max");
    }
    if !is_ramp(&style.ramp) {
        let names = surface_ramp_names().join(", ");
        return refuse(
            style,
            "ramp",
            &format!("\"{}\" is no ramp; the ramps are {names}", style.ramp),
        );
    }
    Ok(())
}

/// check_surface_style for every style, and the set as a whole: at least one
/// style, no two of one name compared without case (a line names a style in
/// any case), and a default that is one of them. InvalidArgument naming the
/// style. What Document::set_surface_styles and surface_styles_from_json
/// refuse.
pub fn check_surface_style_set(styles: &SurfaceStyleSet) -> Status {
    if styles.styles.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a drawing keeps at least one surface style",
        ));
    }
    for (i, style) in styles.styles.iter().enumerate() {
        check_surface_style(style)?;
        for earlier in &styles.styles[..i] {
            if equals_ignoring_case(earlier.name.as_bytes(), style.name.as_bytes()) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "two surface styles have one name",
                    format!("style={}", style.name),
                ));
            }
        }
    }
    if styles.find(&styles.default_style).is_none() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the default surface style is not one of the styles",
            format!("default={}", styles.default_style),
        ));
    }
    Ok(())
}

// ---- words ------------------------------------------------------------------------------

const FACES: [(SurfaceFaces, &str); 5] = [
    (SurfaceFaces::None, "none"),
    (SurfaceFaces::Flat, "flat"),
    (SurfaceFaces::Elevation, "elevation"),
    (SurfaceFaces::Slope, "slope"),
    (SurfaceFaces::Aspect, "aspect"),
];

const SHADING: [(SurfaceStyle, &str); 5] = [
    (SurfaceStyle::Automatic, "automatic"),
    (SurfaceStyle::Shaded, "shaded"),
    (SurfaceStyle::ShadedWithEdges, "edges"),
    (SurfaceStyle::Wireframe, "wireframe"),
    (SurfaceStyle::Hidden, "hidden"),
];

/// The words a line (SURFACESTYLE ... faces= shading=) and the JSON form say
/// these in: none, flat, elevation, slope, aspect; and automatic, shaded,
/// edges (ShadedWithEdges), wireframe, hidden. Read without case; None for
/// any other word.
pub fn surface_faces_word(faces: SurfaceFaces) -> &'static str {
    FACES
        .iter()
        .find(|(value, _)| *value == faces)
        .map_or("none", |(_, word)| word)
}

pub fn parse_surface_faces(word: &str) -> Option<SurfaceFaces> {
    FACES
        .iter()
        .find(|(_, name)| equals_ignoring_case(name.as_bytes(), word.as_bytes()))
        .map(|(value, _)| *value)
}

pub fn surface_shading_word(shading: SurfaceStyle) -> &'static str {
    SHADING
        .iter()
        .find(|(value, _)| *value == shading)
        .map_or("automatic", |(_, word)| word)
}

pub fn parse_surface_shading(word: &str) -> Option<SurfaceStyle> {
    SHADING
        .iter()
        .find(|(_, name)| equals_ignoring_case(name.as_bytes(), word.as_bytes()))
        .map(|(value, _)| *value)
}

// ---- the JSON form ----------------------------------------------------------------------
//
// Every field of every style is written, so a reader never supplies a default
// for what a writer meant: a later build may change a default without a new
// version. A key a reader does not know is skipped, so a field added later
// within version 1 reads in an older build as the style without it.

const FORMAT: &str = "katana-surface-styles";

fn colour_json(colour: Color) -> OrderedJson {
    OrderedJson::from(colour.to_hex())
}

fn shown_json(show: bool, colour: Color) -> OrderedJson {
    OrderedJson::object_of([
        ("show", OrderedJson::from(show)),
        ("colour", colour_json(colour)),
    ])
}

fn optional_json(value: Option<f64>) -> OrderedJson {
    value.map_or(OrderedJson::Null, OrderedJson::from)
}

fn style_json(style: &SurfaceDisplayStyle) -> OrderedJson {
    let stops = OrderedJson::array_of(style.stops.iter().map(|stop| {
        OrderedJson::object_of([
            ("value", OrderedJson::from(stop.value)),
            ("colour", colour_json(stop.colour)),
        ])
    }));
    let c = &style.contours;
    OrderedJson::object_of([
        ("name", OrderedJson::from(style.name.as_str())),
        ("description", OrderedJson::from(style.description.as_str())),
        (
            "triangles",
            shown_json(style.show_triangles, style.triangle_colour),
        ),
        (
            "boundary",
            shown_json(style.show_boundary, style.boundary_colour),
        ),
        ("points", shown_json(style.show_points, style.point_colour)),
        (
            "breaklines",
            shown_json(style.show_breaklines, style.breakline_colour),
        ),
        (
            "faces",
            OrderedJson::from(surface_faces_word(style.plan_faces)),
        ),
        ("opacity", OrderedJson::from(style.plan_opacity)),
        (
            "contours",
            OrderedJson::object_of([
                ("show", OrderedJson::from(c.show)),
                ("interval", OrderedJson::from(c.interval)),
                ("major", OrderedJson::from(c.major_every)),
                ("base", OrderedJson::from(c.base)),
                ("minor_colour", colour_json(c.minor_colour)),
                ("major_colour", colour_json(c.major_colour)),
                ("labels", OrderedJson::from(c.label_major)),
            ]),
        ),
        ("ramp", OrderedJson::from(style.ramp.as_str())),
        ("banded", OrderedJson::from(style.banded)),
        ("bands", OrderedJson::from(style.bands)),
        ("stops", stops),
        ("min", optional_json(style.range_min)),
        ("max", optional_json(style.range_max)),
        ("flat_colour", colour_json(style.flat_colour)),
        (
            "shading",
            OrderedJson::from(surface_shading_word(style.shading)),
        ),
        (
            "faces3d",
            OrderedJson::from(surface_faces_word(style.faces3d)),
        ),
    ])
}

/// The reader's refusals, as ParseFailure naming where (the C++'s exception
/// of its own, thrown inside the reader and caught at its top).
struct Malformed(String);

type Read<T> = std::result::Result<T, Malformed>;

/// The C++ `read<T>`: the member read as T when it is there, a Malformed when
/// it is not a T, `out` left as it was when there is none.
fn read<T: katana_core::json::FromJson>(
    object: &Json,
    key: &str,
    out: &mut T,
    place: &str,
) -> Read<()> {
    let Some(value) = object.find(key) else {
        return Ok(());
    };
    match value.get::<T>() {
        Ok(read) => {
            *out = read;
            Ok(())
        }
        Err(_) => Err(Malformed(format!(
            "{place}: {key} is not what it should be"
        ))),
    }
}

fn read_colour(object: &Json, key: &str, out: &mut Color, place: &str) -> Read<()> {
    let mut text = String::new();
    read(object, key, &mut text, place)?;
    if text.is_empty() {
        return Ok(());
    }
    match Color::from_hex(&text) {
        Ok(colour) => {
            *out = colour;
            Ok(())
        }
        Err(_) => Err(Malformed(format!("{place}: {key} is not #RRGGBB"))),
    }
}

/// {"show": ..., "colour": ...} under `key`.
fn read_shown(
    object: &Json,
    key: &str,
    show: &mut bool,
    colour: &mut Color,
    place: &str,
) -> Read<()> {
    let Some(part) = object.find(key) else {
        return Ok(());
    };
    if !part.is_object() {
        return Err(Malformed(format!("{place}: {key} is not an object")));
    }
    let inner = format!("{place}.{key}");
    read(part, "show", show, &inner)?;
    read_colour(part, "colour", colour, &inner)
}

fn read_faces(object: &Json, key: &str, out: &mut SurfaceFaces, place: &str) -> Read<()> {
    let mut word = String::new();
    read(object, key, &mut word, place)?;
    if word.is_empty() {
        return Ok(());
    }
    match parse_surface_faces(&word) {
        Some(faces) => {
            *out = faces;
            Ok(())
        }
        None => Err(Malformed(format!(
            "{place}: {key} \"{word}\" is no kind of faces"
        ))),
    }
}

fn read_optional(object: &Json, key: &str, out: &mut Option<f64>, place: &str) -> Read<()> {
    let value = match object.find(key) {
        Some(value) if !value.is_null() => value,
        _ => {
            *out = None;
            return Ok(());
        }
    };
    if !value.is_number() {
        return Err(Malformed(format!("{place}: {key} is not a number")));
    }
    match value.get::<f64>() {
        Ok(number) => {
            *out = Some(number);
            Ok(())
        }
        Err(_) => Err(Malformed(format!("{place}: {key} is not a number"))),
    }
}

fn read_style(item: &Json, index: usize) -> Read<SurfaceDisplayStyle> {
    let mut place = format!("style {}", index + 1);
    if !item.is_object() {
        return Err(Malformed(format!("{place} is not an object")));
    }
    let mut style = SurfaceDisplayStyle::default();
    read(item, "name", &mut style.name, &place)?;
    place = format!("style \"{}\"", style.name);
    read(item, "description", &mut style.description, &place)?;
    read_shown(
        item,
        "triangles",
        &mut style.show_triangles,
        &mut style.triangle_colour,
        &place,
    )?;
    read_shown(
        item,
        "boundary",
        &mut style.show_boundary,
        &mut style.boundary_colour,
        &place,
    )?;
    read_shown(
        item,
        "points",
        &mut style.show_points,
        &mut style.point_colour,
        &place,
    )?;
    read_shown(
        item,
        "breaklines",
        &mut style.show_breaklines,
        &mut style.breakline_colour,
        &place,
    )?;
    read_faces(item, "faces", &mut style.plan_faces, &place)?;
    read(item, "opacity", &mut style.plan_opacity, &place)?;
    if let Some(contours) = item.find("contours") {
        if !contours.is_object() {
            return Err(Malformed(format!("{place}: contours is not an object")));
        }
        let inner = format!("{place}.contours");
        let c = &mut style.contours;
        read(contours, "show", &mut c.show, &inner)?;
        read(contours, "interval", &mut c.interval, &inner)?;
        read(contours, "major", &mut c.major_every, &inner)?;
        read(contours, "base", &mut c.base, &inner)?;
        read_colour(contours, "minor_colour", &mut c.minor_colour, &inner)?;
        read_colour(contours, "major_colour", &mut c.major_colour, &inner)?;
        read(contours, "labels", &mut c.label_major, &inner)?;
    }
    read(item, "ramp", &mut style.ramp, &place)?;
    read(item, "banded", &mut style.banded, &place)?;
    read(item, "bands", &mut style.bands, &place)?;
    if let Some(stops) = item.find("stops") {
        if !stops.is_array() {
            return Err(Malformed(format!("{place}: stops is not a list")));
        }
        let inner = format!("{place}.stops");
        for each in stops.iter() {
            if !each.is_object() {
                return Err(Malformed(format!("{place}: a stop is not an object")));
            }
            let mut stop = ColourStop::default();
            read(each, "value", &mut stop.value, &inner)?;
            read_colour(each, "colour", &mut stop.colour, &inner)?;
            style.stops.push(stop);
        }
    }
    read_optional(item, "min", &mut style.range_min, &place)?;
    read_optional(item, "max", &mut style.range_max, &place)?;
    read_colour(item, "flat_colour", &mut style.flat_colour, &place)?;
    let mut shading = String::new();
    read(item, "shading", &mut shading, &place)?;
    if !shading.is_empty() {
        match parse_surface_shading(&shading) {
            Some(parsed) => style.shading = parsed,
            None => {
                return Err(Malformed(format!(
                    "{place}: shading \"{shading}\" is no kind of shading"
                )));
            }
        }
    }
    read_faces(item, "faces3d", &mut style.faces3d, &place)?;
    Ok(style)
}

/// The versioned JSON form, as the Document keeps it: written with the keys in
/// a fixed order, so the same set is the same text. Never fails: nlohmann's
/// `replace` handler is what kept a set built by hand with bytes that are not
/// UTF-8 from throwing in the C++, and a String holds none (the stored value is
/// guarded apart, in the surface document's surface_styles_metadata_value).
pub fn surface_styles_to_json(styles: &SurfaceStyleSet) -> String {
    let list = OrderedJson::array_of(styles.styles.iter().map(style_json));
    let root = OrderedJson::object_of([
        ("format", OrderedJson::from(FORMAT)),
        ("version", OrderedJson::from(SurfaceStyleSet::VERSION)),
        ("default", OrderedJson::from(styles.default_style.as_str())),
        ("styles", list),
    ]);
    root.dump()
}

/// ParseFailure for text that is not the form; Unsupported for a newer
/// version; InvalidArgument (check_surface_style) for a style that cannot be
/// drawn. The text is bytes, as the C++'s string_view is: the parser is what
/// refuses one that is not UTF-8.
pub fn surface_styles_from_json(text: impl AsRef<[u8]>) -> Result<SurfaceStyleSet> {
    let Ok(root) = Json::parse(text) else {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "the surface styles are not JSON",
        ));
    };
    // The type is checked before the value is read: nlohmann's value() THROWS
    // type_error.302 on a "format" that is a number, an object or null, and
    // this is called from Document::open, which promises the built-ins in
    // place of text it cannot read - not an exception out of the open.
    let format = if root.is_object() {
        root.find("format")
    } else {
        None
    };
    if format.and_then(Json::as_str) != Some(FORMAT) {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "not a set of surface styles",
        ));
    }
    let version = match root.find("version") {
        Some(version) if version.is_number_integer() => version.get::<i64>().unwrap_or_default(),
        _ => {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "the surface styles have no version",
            ));
        }
    };
    if version > i64::from(SurfaceStyleSet::VERSION) {
        return Err(make_error(
            ErrorCode::Unsupported,
            "the surface styles were written by a newer version of Katana",
            format!("version {version}"),
        ));
    }
    let mut set = SurfaceStyleSet::default();
    let read_set = |set: &mut SurfaceStyleSet| -> Read<()> {
        read(&root, "default", &mut set.default_style, "the set")?;
        let Some(styles) = root.find("styles").filter(|styles| styles.is_array()) else {
            return Err(Malformed(String::from("the set has no list of styles")));
        };
        for (index, item) in styles.iter().enumerate() {
            set.styles.push(read_style(item, index)?);
        }
        Ok(())
    };
    if let Err(Malformed(what)) = read_set(&mut set) {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            format!("the surface styles are not in the expected form: {what}"),
        ));
    }
    check_surface_style_set(&set)?;
    Ok(set)
}

/// The named colour ramps a style may use, the default first: terrain, earth,
/// diverging (cut blue to fill red), slope (green to red), grey, spectral.
/// The ramp table is katana_terrain's (colour_ramps), the same one RASTER
/// SHADE and RASTER SLOPE paint with.
pub fn surface_ramp_names() -> &'static [&'static str] {
    colour_ramp_names()
}
