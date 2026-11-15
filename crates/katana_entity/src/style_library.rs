//! A library of linestyle and symbol definitions (PLAN.MD 20.3, slice 1).
//!
//! In a style library a symbol IS a linestyle - the file format's own
//! documentation says so where it explains vertex symbols: "There can be the
//! same symbol (defined as a linestyle) for every vertex". One grammar
//! describes both, and the only difference is where the strokes go: `mode
//! vertex` puts them at each vertex of a string, anything else runs them along
//! it. So there is one type here for both, and `at_vertices` is what separates
//! them.
//!
//! The definitions live in the entity layer rather than in the layer that
//! reads the file, because three layers need them and they cannot all see each
//! other: `commands` applies a survey code, `cad` turns a definition into
//! strokes to draw, the archive reader reads the file. It is also where the
//! names they are referenced BY already live - `Style::linetype` and
//! `Style::symbol` are both names resolved against a library.
//!
//! A library is NOT part of a Model and is not saved inside a project, which
//! is the usual arrangement: the libraries are a site-wide customisation shared
//! by every project, named by a project rather than copied into it. The
//! alternative - a table in the Model beside Layer and Linetype - was rejected
//! because one production customisation alone is 792 definitions and 35,000
//! strokes, which would be copied into every project file that used one of
//! them, and because two projects would then be able to disagree about what
//! "WATR Main" looks like.

use std::collections::BTreeSet;

use katana_core::error::{ErrorCode, Result, Status, make_error};
use katana_geometry::{Box2, Point2};

use crate::entity_geometry::Validate;
use crate::named_table::{Named, NamedTable, NamedTablePolicy};
use crate::validation::validate_name;

/// How a definition's coordinates are measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StyleUnits {
    /// `worldstyle`: model units. The mark keeps its size on the GROUND, so it
    /// grows on the page as you zoom in - a 6 m road marking is 6 m wide.
    #[default]
    World,
    /// `paperstyle`: millimetres on the plot. The mark keeps its size on the
    /// PAGE whatever the scale - a 1.5 mm tick prints 1.5 mm wide.
    Paper,
    /// `twoptstyle`: the strokes are stretched between two anchors, so the
    /// definition fits the span it is drawn across - a gate, a doorway.
    TwoPoint,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StrokeOp {
    /// pen up to the point
    #[default]
    Move,
    /// pen down to the point
    Draw,
    /// radius about the current point, from start_angle to end_angle
    Arc,
    /// radius about the current point
    Circle,
    /// a dot at the current point; radius 0 means the smallest drawable
    Dot,
    /// characters sitting at the current point
    Text,
    /// not a mark: the colour everything after it is drawn in
    Pen,
}

/// What a `text` command carries beyond the point it sits at. Kept beside the
/// strokes rather than in them because a library is overwhelmingly move and
/// draw - 17,317 draws against 514 texts in the two reference library files -
/// and two Strings on every one of those would cost far more than this
/// indirection does.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeText {
    pub text: String,
    pub angle: f64,  // degrees counter-clockwise
    pub height: f64, // in the definition's own units
    /// The file's own spelling, "middle-centre" or "top-left". Kept verbatim:
    /// the renderer maps it, and a spelling this does not know must still be
    /// written back to the file as it was read.
    pub justify: String,
    pub font: String,
    pub width_factor: f64,
    /// The three numbers a `text` command ends with. All 907 uses across the
    /// two libraries have 0 in the first; the other two take values like -0.3
    /// and 0.035, and no documentation available here names them. They are
    /// KEPT so that what was read can be written back, and deliberately NOT
    /// interpreted - guessing that they are an offset would move the text.
    pub unnamed: [f64; 3],
}

impl Default for StrokeText {
    fn default() -> Self {
        Self {
            text: String::new(),
            angle: 0.0,
            height: 0.0,
            justify: String::new(),
            font: String::new(),
            width_factor: 1.0,
            unnamed: [0.0; 3],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub op: StrokeOp,
    pub point: Point2,    // Move, Draw
    pub radius: f64,      // Arc, Circle, Dot
    pub start_angle: f64, // Arc, degrees
    pub end_angle: f64,   // Arc, degrees
    pub text: usize,      // Text: an index into LineStyle::texts
    pub pen: String,      // Pen: a colour name; "view_colour" is the entity's own
}

impl Stroke {
    /// No text: a stroke that is not a Text stroke.
    pub const NO_TEXT: usize = usize::MAX;
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            op: StrokeOp::Move,
            point: Point2::default(),
            radius: 0.0,
            start_angle: 0.0,
            end_angle: 0.0,
            text: Self::NO_TEXT,
            pen: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineStyle {
    pub name: String,
    /// A `/`-separated path, "Survey/WATR" - the tree a library browser
    /// shows, the same shape as a layer path and for the same reason.
    pub group: String,
    pub units: StyleUnits,
    /// `mode vertex`: drawn at each vertex of a string rather than along it.
    /// One sign that a definition is a symbol, and not the only one: see
    /// `symbol` below.
    pub at_vertices: bool,
    /// The period of the pattern along a line, in the definition's units. 0
    /// means the file said nothing, and the span of the strokes themselves is
    /// used instead (cad::linestyleStrokes).
    pub length: f64,
    pub factor: f64, // a scale applied to every coordinate
    pub origin: Point2,
    /// TwoPoint only: where the two anchors sit in the definition's own
    /// coordinates, so the strokes can be mapped onto the two points the
    /// style is drawn between.
    pub anchor1: Point2,
    pub anchor2: Point2,
    /// TwoPoint only, and the file's own numbers kept as read: the files use
    /// 1 and 2, and no documentation available here says what they mean, so
    /// they are preserved rather than acted on.
    pub stretch_mode: i32,
    pub cycle_mode: i32,
    pub strokes: Vec<Stroke>,
    pub texts: Vec<StrokeText>,
    /// Where this definition came from - a NAME only, never a path, so a
    /// library carries no trace of where on someone's disk it came from: the
    /// name of its customisation (customisation.rs), or of the file when a
    /// style library (.4d) was read. Empty for a definition made in code or in
    /// a session. A browser groups and filters by it and a project records it.
    /// It says nothing of which KIND a definition is - `symbol` below does,
    /// and the catalogue and the lint read that.
    pub source: String,
    /// Its customisation LISTS it as a symbol: in the Katana customisation
    /// format, the definition sits in the "symbols" array rather than in
    /// "linestyles". That is a statement by the customisation's author, and
    /// `at_vertices` cannot stand in for it: most symbols the reference survey
    /// codes place are not `mode vertex`.
    ///
    /// It is a field of its own and NOT derived from `source`, which a
    /// file-name rule would read (a file name containing "symbol"). One
    /// customisation holds both kinds under ONE name, so where a definition
    /// came from no longer says which kind it is - a customisation called
    /// "Symbols and lines" would make everything a symbol, and one called
    /// "NSW" nothing. The style library reader, which does still read one
    /// kind a file, sets this by that file-name rule.
    pub symbol: bool,
}

impl Default for LineStyle {
    fn default() -> Self {
        Self {
            name: String::new(),
            group: String::new(),
            units: StyleUnits::World,
            at_vertices: false,
            length: 0.0,
            factor: 1.0,
            origin: Point2::default(),
            anchor1: Point2::default(),
            anchor2: Point2::default(),
            stretch_mode: 0,
            cycle_mode: 0,
            strokes: Vec::new(),
            texts: Vec::new(),
            source: String::new(),
            symbol: false,
        }
    }
}

impl LineStyle {
    /// The box the Move and Draw strokes cover, with `factor` applied. Text,
    /// arcs and circles are included by their point; a text's extent needs a
    /// font and is not known here. Empty for a definition with no strokes.
    pub fn bounds(&self) -> Box2 {
        let mut bounds = Box2::default();
        // A circle, an arc, a dot and a text are all drawn ABOUT THE CURRENT
        // POINT and carry no point of their own, so the pen has to be followed
        // to know where they are. Getting this wrong put every circle at the
        // origin.
        let mut pen = Point2::default();
        for stroke in &self.strokes {
            if stroke.op == StrokeOp::Pen {
                continue; // a colour, not a mark
            }
            if stroke.op == StrokeOp::Move || stroke.op == StrokeOp::Draw {
                pen = stroke.point;
            }
            let at = Point2::new(pen.x * self.factor, pen.y * self.factor);
            bounds.expand(at);
            if stroke.op == StrokeOp::Circle || stroke.op == StrokeOp::Arc {
                // The radius is in the same units as the coordinates, so it is
                // scaled the same way. An arc is bounded by its full circle
                // rather than its sweep: cheap, never too small, and a
                // symbol's box is used to lay a preview out, not to clip.
                let radius = stroke.radius.abs() * self.factor;
                bounds.expand(Point2::new(at.x - radius, at.y - radius));
                bounds.expand(Point2::new(at.x + radius, at.y + radius));
            }
        }
        bounds
    }
}

impl Named for LineStyle {
    fn name(&self) -> &str {
        &self.name
    }
}

/// Fails with InvalidArgument, naming the rule broken. (The C++ also refused
/// a group, a pen or a text that is not UTF-8; a String cannot be one.)
impl Validate for LineStyle {
    fn validate(&self) -> Status {
        validate_name(&self.name, "linestyle")?;
        if !(self.length.is_finite() && self.length >= 0.0) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "linestyle length must be finite and not negative",
                self.name.as_str(),
            ));
        }
        if !(self.factor.is_finite() && self.factor > 0.0) {
            // A factor of 0 would collapse every stroke onto the origin, which
            // reads as "the definition is empty" rather than as the mistake it
            // is.
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "linestyle factor must be finite and greater than zero",
                self.name.as_str(),
            ));
        }
        for point in [self.origin, self.anchor1, self.anchor2] {
            if !(point.x.is_finite() && point.y.is_finite()) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "linestyle origin is not finite",
                    self.name.as_str(),
                ));
            }
        }
        for stroke in &self.strokes {
            if !(stroke.point.x.is_finite()
                && stroke.point.y.is_finite()
                && stroke.radius.is_finite()
                && stroke.start_angle.is_finite()
                && stroke.end_angle.is_finite())
            {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a stroke of this linestyle is not finite",
                    self.name.as_str(),
                ));
            }
            if stroke.op == StrokeOp::Text && stroke.text >= self.texts.len() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a text stroke of this linestyle names no text",
                    self.name.as_str(),
                ));
            }
        }
        for text in &self.texts {
            if !(text.angle.is_finite() && text.height.is_finite() && text.width_factor.is_finite())
            {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "linestyle text is not finite",
                    self.name.as_str(),
                ));
            }
            if text.height < 0.0 {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "linestyle text height is negative",
                    self.name.as_str(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LineStylePolicy;

impl NamedTablePolicy<LineStyle> for LineStylePolicy {
    const NOUN: &'static str = "linestyle";
    fn validate(item: &LineStyle) -> Status {
        item.validate()
    }
}

/// The same container as every other table of named things. Names compare
/// with regard to CASE, as they do in the rest of the model, although the
/// files' own comparisons are case-insensitive ("Bypass" and "BYPASS" are one
/// linestyle there). That difference is deliberate and measured rather than
/// overlooked: of the 372 references the reference survey code file makes
/// into its two libraries, every single one matches exactly and none needs
/// case folding. A name that does not resolve is reported by the caller rather
/// than guessed at, so if another customisation ever does spell one
/// differently it will say so instead of drawing the wrong mark.
pub type StyleLibrary = NamedTable<LineStyle, LineStylePolicy>;

/// Later wins: a customisation is loaded in layers, and four of the reference
/// customisation's definitions ("BUIL Doorway" among them) appear in both
/// files, so "already exists" is the normal case rather than an error. Returns
/// whether a definition of that name was replaced.
pub fn add_or_replace(library: &mut StyleLibrary, style: LineStyle) -> Result<bool> {
    let replaced = library.contains(&style.name);
    if replaced {
        library.update(&style)?;
    } else {
        library.add(style)?;
    }
    Ok(replaced)
}

/// Every distinct group, in name order, without the empty one.
pub fn style_groups(library: &StyleLibrary) -> Vec<String> {
    let mut distinct = BTreeSet::new();
    for name in library.names() {
        if let Some(style) = library.find(&name)
            && !style.group.is_empty()
        {
            distinct.insert(style.group.clone());
        }
    }
    distinct.into_iter().collect()
}

/// The definitions drawn at vertices - the symbols - in name order. Named for
/// what it returns rather than `symbol_names`, which already means the sixteen
/// shapes Katana draws without a library.
pub fn vertex_style_names(library: &StyleLibrary) -> Vec<String> {
    let mut names = Vec::new();
    for name in library.names() {
        if library.find(&name).is_some_and(|style| style.at_vertices) {
            names.push(name);
        }
    }
    names // names() is already ascending
}
