//! Road templates (include/katana/cad/road/road_template.hpp,
//! src/katana_cad/road/road_template.cpp; docs/road_design.md, "Road
//! templates"): the cross-section a corridor sweeps along an alignment and its
//! design profile.
//!
//! A template is a list of COMPONENTS out from the centreline on each side, in
//! order: traffic lanes, shoulders, kerbs, footpaths, verges, medians, table
//! drains, and last a batter that carries the edge to the ground. Each has a
//! width and a crossfall, the layer and colour its 3D objects are shown in,
//! and, where it is paved or covered, its PAVEMENT COURSES from the top down,
//! each a solid of its own in the road model (bim::bim_object).
//!
//! It is plain data. The project keeps the drawing's templates as versioned
//! JSON under the metadata key "road_templates" (Document::road_templates,
//! road_document.rs), every edit is one undo step through the ROADTEMPLATE
//! verbs, and nothing built from a template is stored: the corridor's
//! sections, strings, surfaces and solids are made from it again each time
//! (road_section.rs), so none can disagree with the template it came from.
//!
//! SIGNS. An offset is positive to the LEFT of the alignment looking along
//! increasing station, as SolvedAlignment::point_at_station_offset has it. A
//! crossfall is in PERCENT and is the change of level going OUTWARD from the
//! centreline: -3 falls 3 % away from it (a crowned lane), +2.5 rises (a
//! footpath rising to the boundary). A batter slope is horizontal run per unit
//! rise, as corridor::Assembly has it: 2 is 1V:2H.
//!
//! The old one-carriageway corridor::Assembly is a template like any other
//! (template_from_assembly), so there is one corridor engine.

use std::collections::BTreeSet;

use katana_core::error::make_error;
use katana_core::json::{Json, JsonError};
use katana_core::text::{format_exact_real, trimmed};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::{Color, EnumName};
use katana_math::cpp;

use crate::corridor::Assembly;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ComponentKind {
    #[default]
    Lane,
    Shoulder,
    Kerb,
    Footpath,
    Verge,
    Median,
    Drain,
    Batter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum KerbShape {
    #[default]
    Barrier,
    Mountable,
    KerbAndGutter,
    EdgeStrip,
}

/// Which way a kerb's face looks. INWARD, the default, is a kerb on the outer
/// edge of a carriageway: its lip is on the carriageway side, nearer the
/// centreline, and it rises going outward to its top - the face looks in, to
/// the traffic. OUTWARD is a kerb on the inner edge of a carriageway, a
/// median's or an island's: the kerb is highest where it starts, against the
/// median, and falls going outward to its lip, so the carriageway beyond it is
/// lower and the face looks out, to the traffic on that side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum KerbFacing {
    #[default]
    Inward,
    Outward,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Side {
    #[default]
    Left,
    Right,
}

const FORMAT: &str = "katana-road-templates";

// The words the verbs and the JSON use, read in any case.
const KINDS: [(ComponentKind, &str); 8] = [
    (ComponentKind::Lane, "lane"),
    (ComponentKind::Shoulder, "shoulder"),
    (ComponentKind::Kerb, "kerb"),
    (ComponentKind::Footpath, "footpath"),
    (ComponentKind::Verge, "verge"),
    (ComponentKind::Median, "median"),
    (ComponentKind::Drain, "drain"),
    (ComponentKind::Batter, "batter"),
];

const SHAPES: [(KerbShape, &str); 4] = [
    (KerbShape::Barrier, "barrier"),
    (KerbShape::Mountable, "mountable"),
    (KerbShape::KerbAndGutter, "kerbgutter"),
    (KerbShape::EdgeStrip, "edgestrip"),
];

const FACINGS: [(KerbFacing, &str); 2] = [
    (KerbFacing::Inward, "inward"),
    (KerbFacing::Outward, "outward"),
];

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b, 255)
}

// A barrier face battered 1 in 6 (horizontal over vertical): near enough
// vertical to read as one, and never vertical, which a TIN of the finished
// surface could not hold (kerb_shape).
const BARRIER_FACE_BATTER: f64 = 1.0 / 6.0;
// A mountable kerb's face runs 1.5 times its height out: a slope a wheel
// climbs, as the Green Book describes a sloping curb (AASHTO, A Policy on
// Geometric Design of Highways and Streets, 7th ed. 2018, section 4.7).
const MOUNTABLE_FACE_RUN: f64 = 1.5;

fn side_name(component: &Component) -> String {
    format!("{}:{}", component.side.name(), component.name)
}

// ---- words ------------------------------------------------------------------------------

/// "lane", "shoulder", "kerb", "footpath", "verge", "median", "drain",
/// "batter" (the C++ `toString(ComponentKind)`).
impl EnumName for ComponentKind {
    fn name(self) -> &'static str {
        KINDS
            .iter()
            .find(|(value, _)| *value == self)
            .map_or("lane", |&(_, word)| word)
    }
}

pub fn component_kind_from(text: &str) -> Option<ComponentKind> {
    KINDS
        .iter()
        .find(|(_, word)| word.eq_ignore_ascii_case(text))
        .map(|(value, _)| *value)
}

/// "barrier", "mountable", "kerbgutter", "edgestrip".
impl EnumName for KerbShape {
    fn name(self) -> &'static str {
        SHAPES
            .iter()
            .find(|(value, _)| *value == self)
            .map_or("barrier", |&(_, word)| word)
    }
}

pub fn kerb_shape_from(text: &str) -> Option<KerbShape> {
    SHAPES
        .iter()
        .find(|(_, word)| word.eq_ignore_ascii_case(text))
        .map(|(value, _)| *value)
}

/// "inward", "outward".
impl EnumName for KerbFacing {
    fn name(self) -> &'static str {
        FACINGS
            .iter()
            .find(|(value, _)| *value == self)
            .map_or("inward", |&(_, word)| word)
    }
}

pub fn kerb_facing_from(text: &str) -> Option<KerbFacing> {
    FACINGS
        .iter()
        .find(|(_, word)| word.eq_ignore_ascii_case(text))
        .map(|(value, _)| *value)
}

/// "left", "right".
impl EnumName for Side {
    fn name(self) -> &'static str {
        if self == Side::Left { "left" } else { "right" }
    }
}

pub fn side_from(text: &str) -> Option<Side> {
    if text.eq_ignore_ascii_case("left") {
        return Some(Side::Left);
    }
    if text.eq_ignore_ascii_case("right") {
        return Some(Side::Right);
    }
    None
}

// ---- the data ---------------------------------------------------------------------------

/// One course of a pavement, a footpath or a verge: a layer of material of a
/// constant thickness under the component's surface.
#[derive(Clone, Debug, PartialEq)]
pub struct PavementCourse {
    /// A word (letters, digits, '-', '_', '.'): it names the course in a verb
    /// and in its object's id, "corridor/Main/left/lane/AC14/1".
    pub name: String,
    /// Metres, measured vertically.
    pub thickness: f64,
    /// "asphalt", "crushed rock", "concrete", "topsoil" ...
    pub material: String,
    /// The IfcCourse predefined type the IFC export writes (IFC 4.3
    /// IfcCourseTypeEnum: ARMOUR, BALLASTBED, CORE, FILTER, PAVEMENT,
    /// PROTECTION, USERDEFINED, NOTDEFINED).
    pub ifc_type: String,
    pub colour: Color,
}

impl Default for PavementCourse {
    fn default() -> Self {
        Self {
            name: String::new(),
            thickness: 0.0,
            material: String::new(),
            ifc_type: String::from("PAVEMENT"),
            colour: Color::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    /// A word, unique on its side of the template: "lane", "lane2", "kerb".
    pub name: String,
    pub kind: ComponentKind,
    pub side: Side,
    /// Metres across, horizontally. A kerb's is its top (the back of the kerb
    /// past the face); the whole kerb is wider (kerb_width). Not used by a
    /// batter, which runs to the ground.
    pub width: f64,
    /// Percent, outward (the module's comment). Not used by a kerb, whose
    /// shape is its own, nor by a batter.
    pub crossfall: f64,
    /// Lanes and shoulders by default: their crossfall is the corridor's
    /// superelevation table's for the station, where it has one
    /// (corridor_model.rs).
    pub superelevated: bool,
    /// The drawing layer its objects are shown on, and their colour.
    pub layer: String,
    pub colour: Color,
    /// Top down. None for a kerb (the kerb is its own solid) and a batter.
    pub courses: Vec<PavementCourse>,

    // ---- a kerb ---------------------------------------------------------------
    pub shape: KerbShape,
    /// Which way the face looks (KerbFacing). An edge strip has no face and
    /// takes only the default.
    pub faces: KerbFacing,
    /// The face's height above the lip (the pavement edge), metres. An edge
    /// strip is flush and has none.
    pub height: f64,
    /// A kerb: how far below the lip its base is. A table drain: how far its
    /// invert is below its inner edge. An edge strip: its thickness.
    pub depth: f64,
    /// Kerb and gutter: the channel's width and its fall towards the kerb
    /// face, percent.
    pub gutter_width: f64,
    pub gutter_fall: f64,
    /// A kerb's; a course has its own.
    pub material: String,

    // ---- a batter -------------------------------------------------------------
    /// Run per rise in cut (the batter climbing to the ground) and in fill
    /// (falling to it); the farthest out it may reach before the section is
    /// given up as not meeting the ground; and benches: a flat of bench_width
    /// every bench_height of rise or fall (0 for none).
    pub cut_slope: f64,
    pub fill_slope: f64,
    pub maximum_width: f64,
    pub bench_height: f64,
    pub bench_width: f64,
}

impl Default for Component {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: ComponentKind::Lane,
            side: Side::Left,
            width: 3.5,
            crossfall: -3.0,
            superelevated: false,
            layer: String::new(),
            colour: Color::default(),
            courses: Vec::new(),
            shape: KerbShape::Barrier,
            faces: KerbFacing::Inward,
            height: 0.150,
            depth: 0.150,
            gutter_width: 0.300,
            gutter_fall: 6.0,
            material: String::new(),
            cut_slope: 2.0,
            fill_slope: 4.0,
            maximum_width: 50.0,
            bench_height: 0.0,
            bench_width: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadTemplate {
    /// Any text but a '/', a '"' or a line break; found ignoring case.
    pub name: String,
    pub description: String,
    /// Every component of both sides, each side's in order out from the
    /// centreline: the n-th left component is the n-th one whose side is
    /// Left.
    pub components: Vec<Component>,
}

impl RoadTemplate {
    /// The components of one side, in order outward.
    pub fn side(&self, which: Side) -> Vec<&Component> {
        self.components
            .iter()
            .filter(|component| component.side == which)
            .collect()
    }

    /// Its index in `components`, or None: name compared ignoring case.
    pub fn find(&self, which: Side, component_name: &str) -> Option<usize> {
        self.components.iter().position(|component| {
            component.side == which && component.name.eq_ignore_ascii_case(component_name)
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadTemplateSet {
    /// In the order they were made.
    pub templates: Vec<RoadTemplate>,
}

impl RoadTemplateSet {
    pub fn find(&self, name: &str) -> Option<&RoadTemplate> {
        self.templates
            .iter()
            .find(|each| each.name.eq_ignore_ascii_case(name))
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut RoadTemplate> {
        self.templates
            .iter_mut()
            .find(|each| each.name.eq_ignore_ascii_case(name))
    }
}

// ---- the defaults -------------------------------------------------------------------

/// A course as COURSE ADD makes one: its colour from its material (asphalt
/// dark grey, concrete pale grey, crushed rock and gravel tan, sand pale tan,
/// topsoil brown; else mid grey). The C++ defaults `ifc_type` to "PAVEMENT".
pub fn make_course(name: &str, thickness: f64, material: &str, ifc_type: &str) -> PavementCourse {
    let word = material.to_ascii_lowercase();
    // Colours a person reads a section by: dark asphalt over tan granular
    // courses, pale concrete, brown topsoil.
    let colour = if word.contains("asphalt") || word.contains("seal") {
        rgb(60, 60, 60)
    } else if word.contains("concrete") {
        rgb(200, 200, 190)
    } else if word.contains("rock") || word.contains("gravel") || word.contains("granular") {
        rgb(170, 145, 105)
    } else if word.contains("sand") {
        rgb(215, 195, 150)
    } else if word.contains("topsoil") || word.contains("soil") {
        rgb(115, 90, 60)
    } else {
        rgb(150, 150, 150)
    };
    PavementCourse {
        name: String::from(name),
        thickness,
        material: String::from(material),
        ifc_type: String::from(ifc_type),
        colour,
    }
}

/// A component of `kind` as ROADTEMPLATE ADD makes one before its options:
/// its width, crossfall, layer, colour and courses, named after its kind.
///
/// The numbers are a starting point a designer edits, each of the order the
/// guide named gives (docs/road_design.md, "Defaults, and where they come
/// from"): a 3.5 m lane at 3 % crossfall (Austroads, Guide to Road Design Part
/// 3, the common rural two-lane cross-section), a path no steeper across than 1
/// in 40 (AS 1428.1, a continuous accessible path of travel), kerbs as the
/// Green Book's curbs (AASHTO 2018, section 4.7: a vertical curb 150 to 200 mm
/// high, a sloping one 100 mm or less, a gutter 0.3 to 1.8 m wide at 5 to 8 %),
/// a fill batter of 1V:4H, the slope the Roadside Design Guide calls
/// recoverable (AASHTO 2011, chapter 3), and a 1V:2H cut. The courses are a
/// typical flexible pavement's, not a design.
pub fn default_component(kind: ComponentKind, side: Side) -> Component {
    let mut component = Component {
        kind,
        side,
        name: String::from(kind.name()),
        ..Component::default()
    };
    match kind {
        ComponentKind::Lane => {
            component.width = 3.5;
            component.crossfall = -3.0;
            component.superelevated = true;
            component.layer = String::from("road/pavement");
            component.colour = rgb(80, 80, 80);
            component.courses = vec![
                make_course("AC14", 0.050, "asphalt", "PAVEMENT"),
                make_course("base", 0.200, "crushed rock", "PAVEMENT"),
                make_course("subbase", 0.250, "crushed rock", "PAVEMENT"),
            ];
        }
        ComponentKind::Shoulder => {
            component.width = 1.5;
            component.crossfall = -3.0;
            component.superelevated = true;
            component.layer = String::from("road/shoulder");
            component.colour = rgb(110, 110, 110);
            component.courses = vec![
                make_course("seal", 0.020, "sprayed seal", "PAVEMENT"),
                make_course("base", 0.200, "crushed rock", "PAVEMENT"),
            ];
        }
        ComponentKind::Kerb => {
            component.width = 0.150;
            component.crossfall = 0.0;
            component.layer = String::from("road/kerb");
            component.colour = rgb(200, 200, 190);
            component.material = String::from("concrete");
            component.shape = KerbShape::Barrier;
            component.height = 0.150;
            component.depth = 0.150;
        }
        ComponentKind::Footpath => {
            component.width = 1.5;
            component.crossfall = 2.5;
            component.layer = String::from("road/footpath");
            component.colour = rgb(180, 180, 170);
            component.courses = vec![
                make_course("concrete", 0.100, "concrete", "PAVEMENT"),
                make_course("bedding", 0.050, "sand", "PAVEMENT"),
            ];
        }
        ComponentKind::Verge => {
            component.width = 2.0;
            component.crossfall = 2.5;
            component.layer = String::from("road/verge");
            component.colour = rgb(90, 140, 70);
            component.courses = vec![make_course("topsoil", 0.100, "topsoil", "NOTDEFINED")];
        }
        ComponentKind::Median => {
            component.width = 2.0;
            component.crossfall = 0.0;
            component.layer = String::from("road/median");
            component.colour = rgb(90, 140, 70);
            component.courses = vec![make_course("topsoil", 0.100, "topsoil", "NOTDEFINED")];
        }
        ComponentKind::Drain => {
            component.width = 2.0;
            component.crossfall = 0.0;
            component.depth = 0.5;
            component.layer = String::from("road/drain");
            component.colour = rgb(140, 115, 80);
        }
        ComponentKind::Batter => {
            component.width = 0.0;
            component.crossfall = 0.0;
            component.layer = String::from("road/batter");
            component.colour = rgb(160, 140, 100);
            component.cut_slope = 2.0;
            component.fill_slope = 4.0;
            component.maximum_width = 50.0;
        }
    }
    component
}

// Both sides, mirror images: `left` lists the left side's components.
fn mirrored(name: &str, description: &str, left: Vec<Component>) -> RoadTemplate {
    let mut made = RoadTemplate {
        name: String::from(name),
        description: String::from(description),
        components: Vec::new(),
    };
    for component in &left {
        made.components.push(component.clone());
    }
    for mut component in left {
        component.side = Side::Right;
        made.components.push(component);
    }
    made
}

fn named(mut component: Component, name: &str) -> Component {
    component.name = String::from(name);
    component
}

/// What ROADTEMPLATE DEFAULTS adds: a two-lane rural road with sealed
/// shoulders and batters; an urban street with kerb and gutter, footpaths and
/// verges; a divided road with a median, mountable kerbs and batters.
pub fn standard_templates() -> Vec<RoadTemplate> {
    let mut made = Vec::new();
    {
        let mut shoulder = default_component(ComponentKind::Shoulder, Side::Left);
        shoulder.width = 2.0;
        made.push(mirrored(
            "rural two-lane",
            "Two 3.5 m lanes at 3 %, 2 m sealed shoulders, table drains and batters",
            vec![
                default_component(ComponentKind::Lane, Side::Left),
                shoulder,
                default_component(ComponentKind::Drain, Side::Left),
                default_component(ComponentKind::Batter, Side::Left),
            ],
        ));
    }
    {
        let mut kerb = default_component(ComponentKind::Kerb, Side::Left);
        kerb.shape = KerbShape::KerbAndGutter;
        let mut lane = default_component(ComponentKind::Lane, Side::Left);
        lane.width = 3.3;
        let mut parking = named(
            default_component(ComponentKind::Lane, Side::Left),
            "parking",
        );
        parking.width = 2.3;
        let mut verge = default_component(ComponentKind::Verge, Side::Left);
        verge.width = 1.0;
        made.push(mirrored(
            "urban street",
            "A 3.3 m lane and 2.3 m parking each way, kerb and gutter, 1.5 m footpaths and verges",
            vec![
                lane,
                parking,
                kerb,
                default_component(ComponentKind::Footpath, Side::Left),
                verge,
                default_component(ComponentKind::Batter, Side::Left),
            ],
        ));
    }
    {
        // A raised median: the kerb is highest against the median, whose top
        // is the profile, and falls to its lip at the carriageway, so the lanes
        // start 100 mm under the profile and the face looks out to the traffic.
        let mut kerb = default_component(ComponentKind::Kerb, Side::Left);
        kerb.shape = KerbShape::Mountable;
        kerb.height = 0.100;
        kerb.faces = KerbFacing::Outward;
        let inner = named(default_component(ComponentKind::Lane, Side::Left), "lane1");
        let outer = named(default_component(ComponentKind::Lane, Side::Left), "lane2");
        let mut shoulder = default_component(ComponentKind::Shoulder, Side::Left);
        shoulder.width = 2.5;
        made.push(mirrored(
            "divided road",
            "A 4 m median (the profile is its top) with mountable kerbs, two 3.5 m lanes each way, \
             2.5 m shoulders and batters",
            vec![
                default_component(ComponentKind::Median, Side::Left),
                kerb,
                inner,
                outer,
                shoulder,
                default_component(ComponentKind::Batter, Side::Left),
            ],
        ));
    }
    made
}

/// The old one-carriageway assembly as a template: a lane of its half width
/// each side at its crossfall (Assembly's positive-falls-away made outward
/// percent), then a batter with its slopes and maximum width. No courses, so
/// its formation is its finished surface, as the assembly's was.
pub fn template_from_assembly(assembly: &Assembly) -> RoadTemplate {
    let mut lane = default_component(ComponentKind::Lane, Side::Left);
    lane.name = String::from("carriageway");
    lane.width = assembly.half_width;
    // The assembly's crossfall is a ratio falling AWAY from the centreline; a
    // template's is outward percent, rising positive.
    lane.crossfall = -assembly.crossfall * 100.0;
    lane.superelevated = false;
    lane.courses.clear();
    let mut batter = default_component(ComponentKind::Batter, Side::Left);
    batter.cut_slope = assembly.cut_batter;
    batter.fill_slope = assembly.fill_batter;
    batter.maximum_width = assembly.maximum_batter_width;
    mirrored(
        "assembly",
        "The one-carriageway assembly of cad::Assembly",
        vec![lane, batter],
    )
}

// ---- limits -----------------------------------------------------------------------------

/// The most a template's length may be - a width, a kerb's or drain's depth, a
/// course's thickness, a batter's maximum width - in metres. A sanity bound,
/// not a design rule, with no standard behind the number: the road a template
/// describes is metres to tens of metres across, and a section is sampled
/// every half metre across its whole span, so a width typed in the wrong unit
/// would cost time linear in it and, past about 4.5e15 m (where adding 0.5 no
/// longer moves a double), never end. 1 km is far beyond any real component
/// and a few thousand samples at most.
pub const MAXIMUM_COMPONENT_LENGTH: f64 = 1000.0;
/// The most benches a batter may make across its maximum width. A batter with
/// a bench every 6 to 10 m of rise has a handful; each bench is two named
/// points a section, and the cut and fill areas cost more than linearly in a
/// section's points, so a bench every millimetre - a typo - is refused instead
/// of taking the session with it (the same reason as a corridor's most
/// sections, road_section.rs).
pub const MAXIMUM_BENCHES: usize = 100;

// ---- checks ---------------------------------------------------------------------------

/// A template name, a corridor name: not empty, no '/', '"' or control
/// character. InvalidArgument naming `what`.
pub fn check_name(what: &str, name: &str) -> Status {
    if trimmed(name.as_bytes()).is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{what} needs a name"),
        ));
    }
    for &c in name.as_bytes() {
        if c == b'/' || c == b'"' || c < 0x20 {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{what} names may not hold a '/', a '\"' or a line break"),
                name,
            ));
        }
    }
    Ok(())
}

/// A component or course name: a word of letters, digits, '-', '_' and '.'.
pub fn check_word(what: &str, name: &str) -> Status {
    if name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{what} needs a name"),
        ));
    }
    for &c in name.as_bytes() {
        let ok = c.is_ascii_lowercase()
            || c.is_ascii_uppercase()
            || c.is_ascii_digit()
            || c == b'-'
            || c == b'_'
            || c == b'.';
        if !ok {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{what} names are one word of letters, digits, '-', '_' and '.'"),
                name,
            ));
        }
    }
    Ok(())
}

fn too_long(what: &str) -> String {
    format!(
        "{what} is at most {} m",
        format_exact_real(MAXIMUM_COMPONENT_LENGTH)
    )
}

/// Everything a section needs to be built: its name; component names words,
/// unique on each side; finite widths and crossfalls - a width positive, a
/// crossfall within +-100 % - kerb dimensions positive (a flush edge strip's
/// height excepted), a drain's depth not negative; every length at most
/// MAXIMUM_COMPONENT_LENGTH; at most one batter a side and nothing outside it;
/// batter slopes and its maximum width positive, benches both given or
/// neither and no more than MAXIMUM_BENCHES across the maximum width; course
/// names words unique in their component, thicknesses positive, and no courses
/// on a kerb or a batter. InvalidArgument naming the component (left:lane) and
/// what is wrong.
pub fn check_template(road_template: &RoadTemplate) -> Status {
    check_name("a road template", &road_template.name)?;
    let refuse = |component: &Component, what: &str| -> Status {
        Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "template {}, {}: {what}",
                road_template.name,
                side_name(component)
            ),
        ))
    };
    for side in [Side::Left, Side::Right] {
        let mut names: BTreeSet<String> = BTreeSet::new();
        let mut batter_seen = false;
        for component in road_template.side(side) {
            if let Err(error) = check_word("a component", &component.name) {
                return refuse(component, &error.message);
            }
            // The corridor's earthworks objects are corridor/<c>/<side>/earthworks/
            // cut|fill/<run>: a component of that name with a course called cut or
            // fill would make the same id, and the IFC export keeps the first
            // element of an id and skips the later one.
            if component.name.eq_ignore_ascii_case("earthworks") {
                return refuse(
                    component,
                    "earthworks is the name of the corridor's cut and fill solids; a component \
                     cannot take it",
                );
            }
            if !names.insert(component.name.to_ascii_lowercase()) {
                return refuse(component, "a second component of that name on this side");
            }
            if batter_seen {
                return refuse(
                    component,
                    "nothing can be outside the batter, which runs to the ground",
                );
            }
            if !component.width.is_finite()
                || !component.crossfall.is_finite()
                || !component.height.is_finite()
                || !component.depth.is_finite()
                || !component.gutter_width.is_finite()
                || !component.gutter_fall.is_finite()
                || !component.cut_slope.is_finite()
                || !component.fill_slope.is_finite()
                || !component.maximum_width.is_finite()
                || !component.bench_height.is_finite()
                || !component.bench_width.is_finite()
            {
                return refuse(component, "a value is not a finite number");
            }
            if component.crossfall.abs() > 100.0 {
                return refuse(component, "a crossfall is a percent within -100 to 100");
            }
            match component.kind {
                ComponentKind::Batter => {
                    batter_seen = true;
                    if !(component.cut_slope > 0.0) || !(component.fill_slope > 0.0) {
                        return refuse(
                            component,
                            "batter slopes must be positive (horizontal run per unit rise)",
                        );
                    }
                    if !(component.maximum_width > 0.0) {
                        return refuse(component, "the maximum batter width must be positive");
                    }
                    if component.maximum_width > MAXIMUM_COMPONENT_LENGTH {
                        return refuse(component, &too_long("the maximum batter width"));
                    }
                    if component.bench_height < 0.0
                        || component.bench_width < 0.0
                        || ((component.bench_height > 0.0) != (component.bench_width > 0.0))
                    {
                        return refuse(
                            component,
                            "a bench needs both its height and its width, or neither",
                        );
                    }
                    if component.bench_height > 0.0 {
                        // The most benches the batter can make: one every period,
                        // the run being the flatter slope's (the most benches).
                        // An overflow to infinity is over the limit as well.
                        let period = component.bench_height
                            * cpp::min(component.cut_slope, component.fill_slope)
                            + component.bench_width;
                        if !(component.maximum_width / period <= MAXIMUM_BENCHES as f64) {
                            return refuse(
                                component,
                                &format!(
                                    "a bench of {} m every {} m of rise would make more than \
                                     {MAXIMUM_BENCHES} benches across the batter's maximum width \
                                     of {} m: give a bench height and width nearer a real \
                                     batter's",
                                    format_exact_real(component.bench_width),
                                    format_exact_real(component.bench_height),
                                    format_exact_real(component.maximum_width)
                                ),
                            );
                        }
                    }
                }
                ComponentKind::Kerb => {
                    if !(component.width > 0.0) || !(component.depth > 0.0) {
                        return refuse(component, "a kerb's width and depth must be positive");
                    }
                    if component.width > MAXIMUM_COMPONENT_LENGTH
                        || component.height > MAXIMUM_COMPONENT_LENGTH
                        || component.depth > MAXIMUM_COMPONENT_LENGTH
                        || component.gutter_width > MAXIMUM_COMPONENT_LENGTH
                    {
                        return refuse(
                            component,
                            &too_long("a kerb's width, height, depth and gutter"),
                        );
                    }
                    if component.shape != KerbShape::EdgeStrip && !(component.height > 0.0) {
                        return refuse(component, "a kerb's face height must be positive");
                    }
                    if component.shape == KerbShape::EdgeStrip
                        && component.faces != KerbFacing::Inward
                    {
                        return refuse(
                            component,
                            "an edge strip is flush: it has no face to turn outward",
                        );
                    }
                    if component.superelevated && component.shape != KerbShape::EdgeStrip {
                        return refuse(
                            component,
                            &format!(
                                "a {} kerb has its own shape and no crossfall to superelevate; \
                                 only an edge strip has",
                                component.shape.name()
                            ),
                        );
                    }
                    if component.shape == KerbShape::KerbAndGutter
                        && (!(component.gutter_width > 0.0) || component.gutter_fall.abs() > 100.0)
                    {
                        return refuse(
                            component,
                            "a gutter's width must be positive and its fall a percent within -100 \
                             to 100",
                        );
                    }
                }
                ComponentKind::Drain => {
                    if !(component.width > 0.0) || component.depth < 0.0 {
                        return refuse(
                            component,
                            "a drain's width must be positive and its depth not negative",
                        );
                    }
                    if component.width > MAXIMUM_COMPONENT_LENGTH
                        || component.depth > MAXIMUM_COMPONENT_LENGTH
                    {
                        return refuse(component, &too_long("a drain's width and depth"));
                    }
                }
                _ => {
                    if !(component.width > 0.0) {
                        return refuse(component, "the width must be positive");
                    }
                    if component.width > MAXIMUM_COMPONENT_LENGTH {
                        return refuse(component, &too_long("the width"));
                    }
                }
            }
            let coursed =
                component.kind != ComponentKind::Kerb && component.kind != ComponentKind::Batter;
            if !coursed && !component.courses.is_empty() {
                return refuse(component, "a kerb or a batter has no courses");
            }
            let mut course_names: BTreeSet<String> = BTreeSet::new();
            for course in &component.courses {
                if let Err(error) = check_word("a course", &course.name) {
                    return refuse(component, &error.message);
                }
                if !course_names.insert(course.name.to_ascii_lowercase()) {
                    return refuse(
                        component,
                        &format!("a second course called {}", course.name),
                    );
                }
                if !course.thickness.is_finite() || !(course.thickness > 0.0) {
                    return refuse(
                        component,
                        &format!("course {}: the thickness must be positive", course.name),
                    );
                }
                if course.thickness > MAXIMUM_COMPONENT_LENGTH {
                    return refuse(
                        component,
                        &format!("course {}: {}", course.name, too_long("the thickness")),
                    );
                }
            }
        }
    }
    Ok(())
}

// ---- the keys ------------------------------------------------------------------------------

/// The keys ROADTEMPLATE ADD and SET take for a component of `kind`: what its
/// JSON keeps (name, layer, colour for all; width, crossfall and
/// superelevated for all but a batter; a kerb's shape, faces, height, depth,
/// gutter, gutterfall and material; a drain's depth; a batter's cut, fill,
/// maxwidth, benchheight and benchwidth). `color` and `thickness` are read as
/// `colour` and `depth` and not listed. A key a kind does not keep changes
/// nothing, so the verbs refuse it rather than reply "updated".
pub fn component_keys(kind: ComponentKind) -> Vec<&'static str> {
    let mut keys = vec!["name", "layer", "colour"];
    if kind != ComponentKind::Batter {
        keys.extend(["width", "crossfall", "superelevated"]);
    }
    match kind {
        ComponentKind::Kerb => {
            keys.extend([
                "shape",
                "faces",
                "height",
                "depth",
                "gutter",
                "gutterfall",
                "material",
            ]);
        }
        ComponentKind::Drain => keys.push("depth"),
        ComponentKind::Batter => {
            keys.extend(["cut", "fill", "maxwidth", "benchheight", "benchwidth"]);
        }
        _ => {}
    }
    keys
}

// ---- a kerb's shape ------------------------------------------------------------------

/// One vertical of a solid's cross-section: its distance out from where it
/// starts, and its top and bottom levels relative to that start.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShapeColumn {
    pub out: f64,
    pub top: f64,
    pub bottom: f64,
}

// The kerb as seen from its other end: the columns in reverse, `out` measured
// from the far end and every level from the far end's top, so a kerb that rose
// going outward falls going outward, from its back to its lip.
fn reversed_kerb(columns: &[ShapeColumn]) -> Vec<ShapeColumn> {
    let Some(last) = columns.last() else {
        return Vec::new();
    };
    let width = last.out;
    let top = last.top;
    columns
        .iter()
        .rev()
        .map(|column| ShapeColumn {
            out: width - column.out,
            top: column.top - top,
            bottom: column.bottom - top,
        })
        .collect()
}

/// The kerb's cross-section from where the previous component ends (out 0,
/// level 0) outward, a column at each change of its top. For a kerb facing
/// INWARD that is its lip (the pavement's edge): the lip, the gutter's invert,
/// the top of the face, the back. For one facing OUTWARD it is the same kerb
/// mirrored: the back, the top of the face, the gutter's invert, the lip - its
/// levels measured from the back, so the kerb falls to its lip. The base is
/// `depth` below the lip (an edge strip: below its top), level under a barrier
/// or a mountable kerb and following the channel under a gutter. The last
/// column is where the next component starts: the back of the kerb, or its
/// lip.
///
/// ```text
/// barrier      face `height` high, battered 1 in 6, then `width` of top
/// mountable    face rising `height` over 1.5 x height out, then `width`
/// kerbgutter   the gutter, `gutter_width` falling `gutter_fall` %, then a
///              barrier face and top
/// edgestrip    flush: `width` at the component's crossfall, `depth` thick;
///              it has no face and takes no facing
/// ```
///
/// The face is never vertical: a vertical face gives the finished surface two
/// levels at one point, which a TIN cannot hold.
pub fn kerb_shape(kerb: &Component) -> Vec<ShapeColumn> {
    let base = -kerb.depth;
    let inward = match kerb.shape {
        KerbShape::Barrier => {
            let face = kerb.height * BARRIER_FACE_BATTER;
            vec![
                ShapeColumn {
                    out: 0.0,
                    top: 0.0,
                    bottom: base,
                },
                ShapeColumn {
                    out: face,
                    top: kerb.height,
                    bottom: base,
                },
                ShapeColumn {
                    out: face + kerb.width,
                    top: kerb.height,
                    bottom: base,
                },
            ]
        }
        KerbShape::Mountable => {
            let face = kerb.height * MOUNTABLE_FACE_RUN;
            vec![
                ShapeColumn {
                    out: 0.0,
                    top: 0.0,
                    bottom: base,
                },
                ShapeColumn {
                    out: face,
                    top: kerb.height,
                    bottom: base,
                },
                ShapeColumn {
                    out: face + kerb.width,
                    top: kerb.height,
                    bottom: base,
                },
            ]
        }
        KerbShape::KerbAndGutter => {
            // The channel falls outward to the face, where the water runs; its
            // base follows it down, so the gutter is `depth` thick throughout.
            let invert = -kerb.gutter_width * kerb.gutter_fall / 100.0;
            let face = kerb.height * BARRIER_FACE_BATTER;
            let bottom = invert - kerb.depth;
            vec![
                ShapeColumn {
                    out: 0.0,
                    top: 0.0,
                    bottom: -kerb.depth,
                },
                ShapeColumn {
                    out: kerb.gutter_width,
                    top: invert,
                    bottom,
                },
                ShapeColumn {
                    out: kerb.gutter_width + face,
                    top: invert + kerb.height,
                    bottom,
                },
                ShapeColumn {
                    out: kerb.gutter_width + face + kerb.width,
                    top: invert + kerb.height,
                    bottom,
                },
            ]
        }
        KerbShape::EdgeStrip => {
            // Flush, with no face to turn.
            let rise = kerb.width * kerb.crossfall / 100.0;
            return vec![
                ShapeColumn {
                    out: 0.0,
                    top: 0.0,
                    bottom: -kerb.depth,
                },
                ShapeColumn {
                    out: kerb.width,
                    top: rise,
                    bottom: rise - kerb.depth,
                },
            ];
        }
    };
    if kerb.faces == KerbFacing::Outward {
        reversed_kerb(&inward)
    } else {
        inward
    }
}

/// How far out the kerb reaches from its lip: its last column's `out`.
pub fn kerb_width(kerb: &Component) -> f64 {
    kerb_shape(kerb).last().map_or(0.0, |column| column.out)
}

// ---- JSON -----------------------------------------------------------------------------------

/// The set's JSON form's version; a newer one is refused.
pub const ROAD_TEMPLATES_VERSION: i32 = 1;

// What the C++ reader throws and catches at its edge: the JSON library's
// exceptions and its own BadValue, both reported with their what() text.
enum Thrown {
    // An early answer of the reader's own, given as it is.
    Refused(Error),
    // A what() text, of nlohmann's exception or of a BadValue.
    What(String),
}

impl From<JsonError> for Thrown {
    fn from(error: JsonError) -> Self {
        Thrown::What(error.what)
    }
}

type Read<T> = std::result::Result<T, Thrown>;

fn bad<T>(what: String) -> Read<T> {
    Err(Thrown::What(what))
}

fn colour_json(colour: Color) -> Json {
    Json::from(colour.to_hex())
}

fn colour_from(value: &Json) -> Read<Color> {
    let text = value.get::<String>()?;
    match Color::from_hex(&text) {
        Ok(colour) => Ok(colour),
        Err(_) => bad(format!(
            "a colour is \"#RRGGBB\": {}",
            value.get::<String>()?
        )),
    }
}

fn course_json(course: &PavementCourse) -> Json {
    Json::object_of([
        ("name", Json::from(course.name.as_str())),
        ("thickness", Json::from(course.thickness)),
        ("material", Json::from(course.material.as_str())),
        ("ifc", Json::from(course.ifc_type.as_str())),
        ("colour", colour_json(course.colour)),
    ])
}

fn course_from(value: &Json) -> Read<PavementCourse> {
    let mut course = PavementCourse {
        name: value.at("name")?.get::<String>()?,
        thickness: value.at("thickness")?.get::<f64>()?,
        ..PavementCourse::default()
    };
    course.material = value.value("material", String::new())?;
    course.ifc_type = value.value("ifc", String::from("PAVEMENT"))?;
    if value.contains("colour") {
        course.colour = colour_from(value.at("colour")?)?;
    }
    Ok(course)
}

// Only what the kind uses is written: a lane says nothing of batters.
fn component_json(component: &Component) -> Json {
    let mut out = Json::object_of([
        ("name", Json::from(component.name.as_str())),
        ("kind", Json::from(component.kind.name())),
        ("side", Json::from(component.side.name())),
        ("layer", Json::from(component.layer.as_str())),
        ("colour", colour_json(component.colour)),
    ]);
    if component.kind != ComponentKind::Batter {
        out["width"] = Json::from(component.width);
        out["crossfall"] = Json::from(component.crossfall);
        out["superelevated"] = Json::from(component.superelevated);
    }
    if component.kind == ComponentKind::Kerb {
        out["shape"] = Json::from(component.shape.name());
        out["faces"] = Json::from(component.faces.name());
        out["height"] = Json::from(component.height);
        out["depth"] = Json::from(component.depth);
        out["gutterWidth"] = Json::from(component.gutter_width);
        out["gutterFall"] = Json::from(component.gutter_fall);
        out["material"] = Json::from(component.material.as_str());
    }
    if component.kind == ComponentKind::Drain {
        out["depth"] = Json::from(component.depth);
    }
    if component.kind == ComponentKind::Batter {
        out["cutSlope"] = Json::from(component.cut_slope);
        out["fillSlope"] = Json::from(component.fill_slope);
        out["maximumWidth"] = Json::from(component.maximum_width);
        out["benchHeight"] = Json::from(component.bench_height);
        out["benchWidth"] = Json::from(component.bench_width);
    }
    if !component.courses.is_empty() {
        out["courses"] = Json::array_of(component.courses.iter().map(course_json));
    }
    out
}

fn component_from(value: &Json) -> Read<Component> {
    let kind_text = value.at("kind")?.get::<String>()?;
    let Some(kind) = component_kind_from(&kind_text) else {
        return bad(format!("unknown component kind \"{kind_text}\""));
    };
    let side_text = value.at("side")?.get::<String>()?;
    let Some(side) = side_from(&side_text) else {
        return bad(format!("a side is left or right: \"{side_text}\""));
    };
    // The defaults first, so a field a kind does not write keeps its value.
    let mut component = default_component(kind, side);
    component.courses.clear();
    component.name = value.at("name")?.get::<String>()?;
    component.layer = value.value("layer", component.layer.clone())?;
    if value.contains("colour") {
        component.colour = colour_from(value.at("colour")?)?;
    }
    component.width = value.value("width", component.width)?;
    component.crossfall = value.value("crossfall", component.crossfall)?;
    component.superelevated = value.value("superelevated", component.superelevated)?;
    if value.contains("shape") {
        let shape_text = value.at("shape")?.get::<String>()?;
        let Some(shape) = kerb_shape_from(&shape_text) else {
            return bad(format!("unknown kerb shape \"{shape_text}\""));
        };
        component.shape = shape;
    }
    if value.contains("faces") {
        let faces_text = value.at("faces")?.get::<String>()?;
        let Some(facing) = kerb_facing_from(&faces_text) else {
            return bad(format!("a kerb faces inward or outward: \"{faces_text}\""));
        };
        component.faces = facing;
    }
    component.height = value.value("height", component.height)?;
    component.depth = value.value("depth", component.depth)?;
    component.gutter_width = value.value("gutterWidth", component.gutter_width)?;
    component.gutter_fall = value.value("gutterFall", component.gutter_fall)?;
    component.material = value.value("material", component.material.clone())?;
    component.cut_slope = value.value("cutSlope", component.cut_slope)?;
    component.fill_slope = value.value("fillSlope", component.fill_slope)?;
    component.maximum_width = value.value("maximumWidth", component.maximum_width)?;
    component.bench_height = value.value("benchHeight", component.bench_height)?;
    component.bench_width = value.value("benchWidth", component.bench_width)?;
    if value.contains("courses") {
        for course in value.at("courses")?.iter() {
            component.courses.push(course_from(course)?);
        }
    }
    Ok(component)
}

/// The set as the project stores it: {"format":"katana-road-templates",
/// "version":1,"templates":[...]} with every double exact. The C++ fails with
/// InvalidArgument for text that is not UTF-8, which a String cannot hold.
pub fn road_templates_to_json(set: &RoadTemplateSet) -> Result<String> {
    let templates = Json::array_of(set.templates.iter().map(|road_template| {
        Json::object_of([
            ("name", Json::from(road_template.name.as_str())),
            (
                "description",
                Json::from(road_template.description.as_str()),
            ),
            (
                "components",
                Json::array_of(road_template.components.iter().map(component_json)),
            ),
        ])
    }));
    let root = Json::object_of([
        ("format", Json::from(FORMAT)),
        ("version", Json::from(ROAD_TEMPLATES_VERSION)),
        ("templates", templates),
    ]);
    Ok(root.dump())
}

fn read_templates(text: &str) -> Read<RoadTemplateSet> {
    let mut set = RoadTemplateSet::default();
    let root = Json::parse(text)?;
    if !root.is_object() || root.value("format", String::new())? != FORMAT {
        return Err(Thrown::Refused(Error::new(
            ErrorCode::ParseFailure,
            "this is not a set of road templates",
        )));
    }
    let version = root.at("version")?.get::<i32>()?;
    if version > ROAD_TEMPLATES_VERSION {
        return Err(Thrown::Refused(make_error(
            ErrorCode::Unsupported,
            "the road templates were written by a newer version of Katana",
            format!("version {version}"),
        )));
    }
    for value in root.at("templates")?.iter() {
        let mut road_template = RoadTemplate {
            name: value.at("name")?.get::<String>()?,
            ..RoadTemplate::default()
        };
        road_template.description = value.value("description", String::new())?;
        for component in value.at("components")?.iter() {
            road_template.components.push(component_from(component)?);
        }
        set.templates.push(road_template);
    }
    Ok(set)
}

/// Read back: a set equal to the one written. ParseFailure for text that is
/// not one (naming what), Unsupported for a newer version, and
/// check_template's refusal for a template that cannot be built.
pub fn road_templates_from_json(text: &str) -> Result<RoadTemplateSet> {
    let set = match read_templates(text) {
        Ok(set) => set,
        Err(Thrown::Refused(error)) => return Err(error),
        Err(Thrown::What(what)) => {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "the road templates cannot be read",
                what,
            ));
        }
    };
    let mut names: BTreeSet<String> = BTreeSet::new();
    for road_template in &set.templates {
        check_template(road_template)?;
        if !names.insert(road_template.name.to_ascii_lowercase()) {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "two road templates have one name",
                road_template.name.as_str(),
            ));
        }
    }
    Ok(set)
}
