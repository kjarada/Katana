//! The annotation tables: text styles, label styles and the auto-label rules
//! (docs/annotation.md).
//!
//! They are named tables like the dimension styles beside them (tables.rs,
//! NamedTable), kept in the Model and stored with the project. What the three
//! have in common is that every size in them is PAPER millimetres: an
//! annotation is written for the sheet, and is drawn in the model at
//! paper_mm x scale / 1000 for the scale of the view or sheet viewport
//! showing it (annotation_model_size).

use katana_core::error::{ErrorCode, Result, Status, make_error};
use katana_core::text::format_exact_real;
use katana_math::numerics::{DEG_TO_RAD, tolerance};

use crate::entity::{Color, EnumName, entity_type_from_string, same_name, to_string};
use crate::entity_geometry::Validate;
use crate::label_text::check_label_template;
use crate::named_table::{Named, NamedMap, NamedTable, NamedTablePolicy};
use crate::validation::validate_name;

// ---- the one conversion ------------------------------------------------------------

/// Model units for `paper_millimetres` on a sheet at 1 : `scale`: a model
/// unit is a metre, and a millimetre on the sheet is `scale` millimetres on
/// the ground.
pub const fn annotation_model_size(paper_millimetres: f64, scale: f64) -> f64 {
    paper_millimetres * scale / 1000.0
}

/// The other way: the paper millimetres `model_units` are on a sheet at
/// 1 : `scale`.
pub const fn annotation_paper_size(model_units: f64, scale: f64) -> f64 {
    model_units * 1000.0 / scale
}

/// The annotation scale a view uses when nothing has chosen one: 1 : 1000,
/// the scale at which a 2.5 mm paper text is 2.5 model units tall - exactly
/// the default height a TextGeometry has always had.
pub const DEFAULT_ANNOTATION_SCALE: f64 = 1000.0;

fn enum_from_string<T: EnumName>(name: &str, values: &[T], what: &str) -> Result<T> {
    for &value in values {
        if same_name(to_string(value), name) {
            return Ok(value);
        }
    }
    let mut known = String::new();
    for &value in values {
        if !known.is_empty() {
            known.push(' ');
        }
        known.push_str(to_string(value));
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        format!("unknown {what} ({known})"),
        name,
    ))
}

fn require_finite_at_least(value: f64, minimum: f64, what: &str, name: &str) -> Status {
    if !value.is_finite() || value < minimum {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "{what} must be finite and at least {}",
                format_exact_real(minimum)
            ),
            name,
        ));
    }
    Ok(())
}

// ---- text styles --------------------------------------------------------------------

pub const DEFAULT_TEXT_STYLE_NAME: &str = "Standard";

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub name: String,
    /// The face; empty for the application's plain-text face.
    pub font_family: String,
    /// Millimetres on paper. 0 leaves a text with no paper height of its own
    /// at its model height.
    pub paper_height: f64,
    /// Width of each character relative to the face's own (DXF STYLE group
    /// 41); 1 is the face as designed.
    pub width_factor: f64,
    /// Slant of the verticals, radians, positive leaning right (DXF group 50,
    /// there in degrees). Refused at 85 degrees or more.
    pub oblique: f64,
    pub bold: bool,
    pub italic: bool,
    /// Empty is ByLayer.
    pub color: Option<Color>,
    /// A background mask behind the text; `mask_margin` is how far it reaches
    /// past the text, millimetres on paper.
    pub mask: bool,
    pub mask_margin: f64,
    /// A text whose rotation would put it upside down is turned half a turn
    /// and its justification mirrored (AS 1100.101 and ISO 3098 both).
    pub readable: bool,
    /// Multiplies the standard pitch between baselines (text_block.rs).
    pub line_spacing: f64,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            name: String::from(DEFAULT_TEXT_STYLE_NAME),
            font_family: String::new(),
            paper_height: 2.5,
            width_factor: 1.0,
            oblique: 0.0,
            bold: false,
            italic: false,
            color: None,
            mask: false,
            mask_margin: 0.5,
            readable: true,
            line_spacing: 1.0,
        }
    }
}

impl Named for TextStyle {
    fn name(&self) -> &str {
        &self.name
    }
}

/// (The C++ also refused a font family that is not UTF-8, which a `String`
/// cannot be.)
impl Validate for TextStyle {
    fn validate(&self) -> Status {
        validate_name(&self.name, "text style")?;
        require_finite_at_least(self.paper_height, 0.0, "paper height", &self.name)?;
        // A width factor of zero draws nothing and a negative one mirrors: DXF
        // accepts neither for a STYLE, and neither is a text a person can read.
        if !self.width_factor.is_finite() || self.width_factor <= 0.0 || self.width_factor > 100.0 {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "width factor must be greater than zero and at most 100",
                self.name.as_str(),
            ));
        }
        // 85 degrees: the limit AutoCAD's STYLE command gives the obliquing angle.
        if !self.oblique.is_finite() || self.oblique.abs() >= 85.0 * DEG_TO_RAD {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the obliquing angle must be less than 85 degrees either way",
                self.name.as_str(),
            ));
        }
        require_finite_at_least(self.mask_margin, 0.0, "mask margin", &self.name)?;
        if !self.line_spacing.is_finite() || !(0.25..=4.0).contains(&self.line_spacing) {
            // AutoCAD's MTEXT accepts 0.25 to 4.0 for its line spacing factor.
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "line spacing must be between 0.25 and 4",
                self.name.as_str(),
            ));
        }
        Ok(())
    }
}

/// Always contains "Standard", which can be neither removed nor renamed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextStylePolicy;

impl NamedTablePolicy<TextStyle> for TextStylePolicy {
    const NOUN: &'static str = "text style";
    fn validate(item: &TextStyle) -> Status {
        item.validate()
    }
    fn is_protected(name: &str) -> bool {
        name == DEFAULT_TEXT_STYLE_NAME
    }
    fn seed(items: &mut NamedMap<TextStyle>) {
        let standard = TextStyle::default();
        items.insert(standard.name.clone(), standard);
    }
}

pub type TextStyleDatabase = NamedTable<TextStyle, TextStylePolicy>;

// ---- label styles ------------------------------------------------------------------

/// What a label style labels, which decides the values its template can use
/// (label_text.rs lists them) and the placements that make sense.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LabelKind {
    /// A point: its number, code, level, coordinates; spot levels.
    #[default]
    Point = 0,
    /// A line, or each segment of a polyline: bearing and distance.
    Segment = 1,
    /// An arc (or a circle): radius, length, chord, delta.
    Arc = 2,
    /// A closed polyline or a circle: area and perimeter, at its centroid.
    Area = 3,
    /// An alignment: chainages at an interval, with ticks.
    Chainage = 4,
}

impl EnumName for LabelKind {
    fn name(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Segment => "segment",
            Self::Arc => "arc",
            Self::Area => "area",
            Self::Chainage => "chainage",
        }
    }
}

pub fn label_kind_from_string(name: &str) -> Result<LabelKind> {
    enum_from_string(
        name,
        &[
            LabelKind::Point,
            LabelKind::Segment,
            LabelKind::Arc,
            LabelKind::Area,
            LabelKind::Chainage,
        ],
        "label kind",
    )
}

/// Where the placer tries first. Auto is the kind's natural place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LabelPlacement {
    #[default]
    Auto = 0,
    /// A line's left side seen along it, an arc's outside, above a point.
    Above = 1,
    /// The other side.
    Below = 2,
    /// Centred ON the line, a mask behind it.
    Along = 3,
    /// An area's centroid, a line's midpoint.
    Centroid = 4,
    /// Of a point.
    Right = 5,
    /// Of a point.
    Left = 6,
}

impl EnumName for LabelPlacement {
    fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Above => "above",
            Self::Below => "below",
            Self::Along => "along",
            Self::Centroid => "centroid",
            Self::Right => "right",
            Self::Left => "left",
        }
    }
}

pub fn label_placement_from_string(name: &str) -> Result<LabelPlacement> {
    enum_from_string(
        name,
        &[
            LabelPlacement::Auto,
            LabelPlacement::Above,
            LabelPlacement::Below,
            LabelPlacement::Along,
            LabelPlacement::Centroid,
            LabelPlacement::Right,
            LabelPlacement::Left,
        ],
        "label placement",
    )
}

/// Horizontal on the sheet, or turned to the line it labels (and kept
/// readable).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LabelOrientation {
    #[default]
    Aligned = 0,
    Horizontal = 1,
}

impl EnumName for LabelOrientation {
    fn name(self) -> &'static str {
        if self == Self::Horizontal {
            "horizontal"
        } else {
            "aligned"
        }
    }
}

pub fn label_orientation_from_string(name: &str) -> Result<LabelOrientation> {
    enum_from_string(
        name,
        &[LabelOrientation::Aligned, LabelOrientation::Horizontal],
        "label orientation",
    )
}

/// A mark at a point label's point: a spot level's cross.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LabelMarker {
    #[default]
    None = 0,
    Cross = 1,
    Dot = 2,
    Circle = 3,
}

impl EnumName for LabelMarker {
    fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Cross => "cross",
            Self::Dot => "dot",
            Self::Circle => "circle",
        }
    }
}

pub fn label_marker_from_string(name: &str) -> Result<LabelMarker> {
    enum_from_string(
        name,
        &[
            LabelMarker::None,
            LabelMarker::Cross,
            LabelMarker::Dot,
            LabelMarker::Circle,
        ],
        "label marker",
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelStyle {
    pub name: String,
    pub kind: LabelKind,
    /// The template: literal text with {value:format} fields, lines separated
    /// by '\n' (label_text.rs).
    pub text: String,
    /// The text style the label is set in; empty for Standard.
    pub text_style: String,
    /// Millimetres on paper; 0 takes the text style's paper height.
    pub paper_height: f64,
    pub placement: LabelPlacement,
    pub orientation: LabelOrientation,
    /// The gap between what is labelled and the text, millimetres on paper.
    pub offset: f64,
    /// A leader from the label back to what it labels when it was moved away
    /// from its first place.
    pub leader: bool,
    /// The placer may move the label to its other candidate places when the
    /// first is taken; off, a label that does not fit is suppressed.
    pub displace: bool,
    /// Placed before styles of lower priority.
    pub priority: i32,
    /// Empty takes the text style's colour, and that ByLayer.
    pub color: Option<Color>,
    pub marker: LabelMarker,
    /// Millimetres on paper, across.
    pub marker_size: f64,
    /// Segment labels: a piece shorter than this many millimetres on the
    /// sheet is not labelled.
    pub minimum_length: f64,
    /// Chainage labels: a label every `interval` model units of chainage and a
    /// tick every `tick_interval` (0: at the labels only), `tick_length`
    /// millimetres either side of the line.
    pub interval: f64,
    pub tick_interval: f64,
    pub tick_length: f64,
}

impl Default for LabelStyle {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: LabelKind::Point,
            text: String::from("{point}"),
            text_style: String::new(),
            paper_height: 0.0,
            placement: LabelPlacement::Auto,
            orientation: LabelOrientation::Aligned,
            offset: 1.0,
            leader: true,
            displace: true,
            priority: 0,
            color: None,
            marker: LabelMarker::None,
            marker_size: 1.0,
            minimum_length: 0.0,
            interval: 20.0,
            tick_interval: 10.0,
            tick_length: 1.5,
        }
    }
}

impl Named for LabelStyle {
    fn name(&self) -> &str {
        &self.name
    }
}

/// (The C++ also refused an option past its last enumerator and text that is
/// not UTF-8, which the enumerations and a `String` cannot be.)
impl Validate for LabelStyle {
    fn validate(&self) -> Status {
        validate_name(&self.name, "label style")?;
        if self.text.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a label style needs a template",
                self.name.as_str(),
            ));
        }
        // A template that could never print is refused on the way in, naming
        // what is wrong, rather than stored to draw a label with a hole in it.
        if let Err(error) = check_label_template(&self.text, self.kind) {
            return Err(make_error(
                error.code,
                error.message,
                format!("{}: {}", self.name, error.context),
            ));
        }
        for (what, value) in [
            ("paper height", self.paper_height),
            ("offset", self.offset),
            ("marker size", self.marker_size),
            ("minimum length", self.minimum_length),
            ("tick interval", self.tick_interval),
            ("tick length", self.tick_length),
        ] {
            require_finite_at_least(value, 0.0, what, &self.name)?;
        }
        if self.kind == LabelKind::Chainage
            && !(self.interval.is_finite() && self.interval > tolerance::GEOMETRIC)
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a chainage label's interval must be positive",
                self.name.as_str(),
            ));
        }
        Ok(())
    }
}

/// No built-in entry: LABELSTYLE DEFAULTS adds the standard set as one step,
/// so a project written before them opens with none and looks as it did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LabelStylePolicy;

impl NamedTablePolicy<LabelStyle> for LabelStylePolicy {
    const NOUN: &'static str = "label style";
    fn validate(item: &LabelStyle) -> Status {
        item.validate()
    }
}

pub type LabelStyleDatabase = NamedTable<LabelStyle, LabelStylePolicy>;

// ---- auto-label rules ------------------------------------------------------------------

/// "Label every X with style S": the rules AUTOLABEL RUN applies. An entity
/// matches when it passes every filter that is not empty; the patterns are
/// globs (* any run, ? any one character), matched without regard to ASCII
/// case, since a survey code is typed in whatever case the field crew used.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelRule {
    pub name: String,
    pub label_style: String,
    /// The entity's layer, or any ancestor of it.
    pub layer: String,
    /// The entity's survey code.
    pub code: String,
    /// An EntityType's name, or empty for every type the style's kind labels.
    pub entity_type: String,
    /// The layer the labels are put on; empty puts each on its target's.
    pub label_layer: String,
    pub enabled: bool,
}

impl Default for LabelRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            label_style: String::new(),
            layer: String::new(),
            code: String::new(),
            entity_type: String::new(),
            label_layer: String::new(),
            enabled: true,
        }
    }
}

impl Named for LabelRule {
    fn name(&self) -> &str {
        &self.name
    }
}

/// (The C++ also refused rule text that is not UTF-8, which a `String` cannot
/// be.)
impl Validate for LabelRule {
    fn validate(&self) -> Status {
        validate_name(&self.name, "label rule")?;
        if self.label_style.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a label rule needs a label style",
                self.name.as_str(),
            ));
        }
        if !self.entity_type.is_empty() && entity_type_from_string(&self.entity_type).is_err() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "unknown entity type in a label rule",
                format!("{} type={}", self.name, self.entity_type),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LabelRulePolicy;

impl NamedTablePolicy<LabelRule> for LabelRulePolicy {
    const NOUN: &'static str = "label rule";
    fn validate(item: &LabelRule) -> Status {
        item.validate()
    }
}

pub type LabelRuleDatabase = NamedTable<LabelRule, LabelRulePolicy>;

const fn folded(character: u8) -> u8 {
    character.to_ascii_lowercase()
}

/// `pattern` (* and ?) matches all of `text`, ASCII case folded. An empty
/// pattern matches everything.
pub fn glob_matches(pattern: &str, text: &str) -> bool {
    // The iterative matcher with one backtrack point: on a mismatch after a
    // '*', the star swallows one more character and the match resumes. Linear
    // in practice, and no recursion for a pattern of many stars.
    if pattern.is_empty() {
        return true; // no filter
    }
    let (pattern, text) = (pattern.as_bytes(), text.as_bytes());
    let mut p = 0;
    let mut t = 0;
    let mut star: Option<usize> = None;
    let mut resume = 0;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || folded(pattern[p]) == folded(text[t])) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            resume = t;
        } else if let Some(at) = star {
            p = at + 1;
            resume += 1;
            t = resume;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}
