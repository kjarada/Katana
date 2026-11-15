//! Corridors (include/katana/cad/road/corridor_model.hpp,
//! src/katana_cad/road/corridor_model.cpp; docs/road_design.md,
//! "Corridors"): a road template swept along an alignment and its design
//! profile.
//!
//! A corridor names what it is made from - the alignment, the template, the
//! target surface its batters daylight on - and holds only what is its own:
//! the station range, the interval it is sectioned at, the REGIONS where
//! another template applies, and the SUPERELEVATION table. Nothing built from
//! it is kept: the sections, the strings, the surfaces and the 3D objects are
//! made from it again (road_section.rs), so moving a PI or editing a template
//! is seen at once.
//!
//! Kept, as the templates are, as versioned JSON under the metadata key
//! "corridors" (Document::corridors), every edit one undo step through the
//! CORRIDOR verbs.

use std::collections::BTreeSet;

use katana_core::error::make_error;
use katana_core::json::{Json, JsonError};
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};

use crate::road::road_template::check_name;

/// From `start` to `end` (stations, start < end) the corridor is built with
/// `template_name` instead of its own template.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TemplateRegion {
    pub start: f64,
    pub end: f64,
    pub template_name: String,
}

/// At `station`, the superelevated components (lanes and shoulders by
/// default) of each side fall or rise at these crossfalls - outward percent,
/// as a template's: (+6, -6) is full superelevation falling to the right, for
/// a curve to the right. Linear between two entries; outside the table the
/// template's own crossfalls apply. A table is entered with its normal crown
/// at both ends of each transition, as a superelevation diagram is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SuperelevationPoint {
    pub station: f64,
    pub left: f64,
    pub right: f64,
}

/// The relative error a corridor allows in a course's plan area and volume on
/// a curve when it does not say: a thousandth, 0.1 %. A section is lofted to
/// the next with flat quads, so over a step that turns through theta radians
/// the area and the volume come out short by 1 - sin(theta) / theta, about
/// theta^2 / 6, and a kerb's length by 1 - sin(theta / 2) / (theta / 2), about
/// theta^2 / 24, a quarter as much. The corridor puts sections inside a curve
/// until no step turns through more than sqrt(6 x tolerance), which holds the
/// area and the volume to the tolerance (road_section::section_stations_along).
/// The number is a choice and not a standard's: it makes the steps 3.9 m on
/// R = 50 m and 15.5 m on R = 200 m, so a tight curve is sectioned finer and a
/// gentle one by its interval.
pub const DEFAULT_CURVE_TOLERANCE: f64 = 0.001;
/// The largest a corridor may ask: a tenth. Beyond it the "tolerance" is no
/// longer a small error (the step would turn through 0.77 radians), and 1 would
/// allow steps of 2.4 radians, which a flat quad does not approximate at all.
pub const LARGEST_CURVE_TOLERANCE: f64 = 0.1;

#[derive(Clone, Debug, PartialEq)]
pub struct Corridor {
    pub name: String,
    /// An alignment of the drawing, with a design profile.
    pub alignment: String,
    /// A template of the drawing's road templates.
    pub template_name: String,
    /// The surface the batters daylight on and the earthworks are measured
    /// against (Document::surfaces); empty: none, and the corridor is its
    /// pavement alone.
    pub surface: String,
    /// Metres between sections. The alignment's and the profile's key
    /// stations, the regions' ends and the superelevation stations are
    /// sectioned as well.
    pub interval: f64,
    /// The relative error of a course's area and volume on a curve, at most:
    /// sections are added inside the curves until the chords between them are
    /// that close (DEFAULT_CURVE_TOLERANCE says why a thousandth). 0 adds
    /// none, which is every section the interval and the key stations give and
    /// the chord error that leaves (road_section::section_stations_along).
    pub curve_tolerance: f64,
    /// The station range; each absent is that end of the alignment and the
    /// profile's overlap.
    pub start: Option<f64>,
    pub end: Option<f64>,
    /// Ascending, not overlapping.
    pub regions: Vec<TemplateRegion>,
    /// Ascending station.
    pub superelevation: Vec<SuperelevationPoint>,
}

impl Default for Corridor {
    fn default() -> Self {
        Self {
            name: String::new(),
            alignment: String::new(),
            template_name: String::new(),
            surface: String::new(),
            interval: 10.0,
            curve_tolerance: DEFAULT_CURVE_TOLERANCE,
            start: None,
            end: None,
            regions: Vec::new(),
            superelevation: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CorridorSet {
    /// In the order they were made.
    pub corridors: Vec<Corridor>,
}

impl CorridorSet {
    pub fn find(&self, name: &str) -> Option<&Corridor> {
        self.corridors
            .iter()
            .find(|each| each.name.eq_ignore_ascii_case(name))
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Corridor> {
        self.corridors
            .iter_mut()
            .find(|each| each.name.eq_ignore_ascii_case(name))
    }
}

const FORMAT: &str = "katana-corridors";

fn refuse(corridor: &Corridor, what: &str) -> Status {
    Err(Error::new(
        ErrorCode::InvalidArgument,
        format!("corridor {}: {what}", corridor.name),
    ))
}

fn number(value: f64) -> String {
    format_exact_real(value)
}

/// What a corridor must be on its own, without the drawing: a name, an
/// alignment and a template named, a positive finite interval, a range with
/// start before end, regions each start before end, not overlapping and
/// naming a template, the table's stations distinct and every value finite
/// with crossfalls within +-100 %. InvalidArgument naming the corridor and
/// what is wrong. Regions and the table are sorted by the verbs that add to
/// them; a corridor read with them out of order is refused.
pub fn check_corridor(corridor: &Corridor) -> Status {
    check_name("a corridor", &corridor.name)?;
    if corridor.alignment.is_empty() {
        return refuse(corridor, "it needs an alignment");
    }
    if corridor.template_name.is_empty() {
        return refuse(corridor, "it needs a road template");
    }
    if !corridor.interval.is_finite() || !(corridor.interval > 0.0) {
        return refuse(corridor, "the interval must be positive");
    }
    if !corridor.curve_tolerance.is_finite()
        || corridor.curve_tolerance < 0.0
        || corridor.curve_tolerance > LARGEST_CURVE_TOLERANCE
    {
        return refuse(
            corridor,
            &format!(
                "the curve tolerance is 0 (none) or a relative error of at most {}, not {}",
                number(LARGEST_CURVE_TOLERANCE),
                number(corridor.curve_tolerance)
            ),
        );
    }
    if corridor.start.is_some_and(|start| !start.is_finite())
        || corridor.end.is_some_and(|end| !end.is_finite())
    {
        return refuse(corridor, "a station is not a finite number");
    }
    if let (Some(start), Some(end)) = (corridor.start, corridor.end)
        && !(start < end)
    {
        return refuse(corridor, "the start station must be before the end station");
    }
    let mut previous: Option<&TemplateRegion> = None;
    for region in &corridor.regions {
        if !region.start.is_finite() || !region.end.is_finite() || !(region.start < region.end) {
            return refuse(
                corridor,
                &format!(
                    "a region runs from a station to a later one: {} to {}",
                    number(region.start),
                    number(region.end)
                ),
            );
        }
        if region.template_name.is_empty() {
            return refuse(corridor, "a region names the template it uses");
        }
        if let Some(previous) = previous
            && region.start < previous.end
        {
            return refuse(
                corridor,
                &format!(
                    "regions may not overlap: {} to {} and {} to {}",
                    number(previous.start),
                    number(previous.end),
                    number(region.start),
                    number(region.end)
                ),
            );
        }
        previous = Some(region);
    }
    let mut before: Option<&SuperelevationPoint> = None;
    for point in &corridor.superelevation {
        if !point.station.is_finite() || !point.left.is_finite() || !point.right.is_finite() {
            return refuse(corridor, "a superelevation value is not a finite number");
        }
        if point.left.abs() > 100.0 || point.right.abs() > 100.0 {
            return refuse(
                corridor,
                "a superelevation crossfall is a percent within -100 to 100",
            );
        }
        if let Some(before) = before
            && !(point.station > before.station)
        {
            return refuse(
                corridor,
                &format!(
                    "the superelevation table has two entries at station {} or is out of order",
                    number(point.station)
                ),
            );
        }
        before = Some(point);
    }
    Ok(())
}

/// The superelevation table at a station, in outward percent each side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Crossfalls {
    pub left: f64,
    pub right: f64,
}

/// The superelevation table at `station`: None outside it (the template's own
/// crossfalls then apply), the entry's values at an entry, linear between two.
pub fn superelevation_at(corridor: &Corridor, station: f64) -> Option<Crossfalls> {
    let table = &corridor.superelevation;
    let (first, last) = (table.first()?, table.last()?);
    if station < first.station || station > last.station {
        return None;
    }
    for pair in table.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if station >= a.station && station <= b.station {
            let t = (station - a.station) / (b.station - a.station);
            return Some(Crossfalls {
                left: a.left + (b.left - a.left) * t,
                right: a.right + (b.right - a.right) * t,
            });
        }
    }
    Some(Crossfalls {
        left: last.left,
        right: last.right,
    })
}

/// The template that applies at a station: a region's, or the corridor's.
pub fn template_at(corridor: &Corridor, station: f64) -> &str {
    for region in &corridor.regions {
        if station >= region.start && station <= region.end {
            return &region.template_name;
        }
    }
    &corridor.template_name
}

/// The corridors' JSON form's version; a newer one is refused.
pub const CORRIDORS_VERSION: i32 = 1;

/// {"format":"katana-corridors","version":1,"corridors":[...]}, doubles
/// exact. The C++ fails with InvalidArgument for text that is not UTF-8,
/// which a String cannot hold.
pub fn corridors_to_json(set: &CorridorSet) -> Result<String> {
    let corridors = Json::array_of(set.corridors.iter().map(|corridor| {
        let regions = Json::array_of(corridor.regions.iter().map(|region| {
            Json::object_of([
                ("start", Json::from(region.start)),
                ("end", Json::from(region.end)),
                ("template", Json::from(region.template_name.as_str())),
            ])
        }));
        let table = Json::array_of(corridor.superelevation.iter().map(|point| {
            Json::object_of([
                ("station", Json::from(point.station)),
                ("left", Json::from(point.left)),
                ("right", Json::from(point.right)),
            ])
        }));
        let mut out = Json::object_of([
            ("name", Json::from(corridor.name.as_str())),
            ("alignment", Json::from(corridor.alignment.as_str())),
            ("template", Json::from(corridor.template_name.as_str())),
            ("surface", Json::from(corridor.surface.as_str())),
            ("interval", Json::from(corridor.interval)),
            ("curveTolerance", Json::from(corridor.curve_tolerance)),
            ("regions", regions),
            ("superelevation", table),
        ]);
        // Absent stays absent: no key, not a zero.
        if let Some(start) = corridor.start {
            out["start"] = Json::from(start);
        }
        if let Some(end) = corridor.end {
            out["end"] = Json::from(end);
        }
        out
    }));
    let root = Json::object_of([
        ("format", Json::from(FORMAT)),
        ("version", Json::from(CORRIDORS_VERSION)),
        ("corridors", corridors),
    ]);
    Ok(root.dump())
}

// What the C++ reader's try block gives: an answer of its own, or the what()
// text of the JSON library's exception it catches.
enum Thrown {
    Refused(Error),
    What(String),
}

impl From<JsonError> for Thrown {
    fn from(error: JsonError) -> Self {
        Thrown::What(error.what)
    }
}

fn read_corridors(text: &str) -> std::result::Result<CorridorSet, Thrown> {
    let mut set = CorridorSet::default();
    let root = Json::parse(text)?;
    if !root.is_object() || root.value("format", String::new())? != FORMAT {
        return Err(Thrown::Refused(Error::new(
            ErrorCode::ParseFailure,
            "this is not a set of corridors",
        )));
    }
    let version = root.at("version")?.get::<i32>()?;
    if version > CORRIDORS_VERSION {
        return Err(Thrown::Refused(make_error(
            ErrorCode::Unsupported,
            "the corridors were written by a newer version of Katana",
            format!("version {version}"),
        )));
    }
    // `value(key, json::array())`: the member when it is there, an empty
    // array when it is not.
    let empty = Json::array();
    for value in root.at("corridors")?.iter() {
        let mut corridor = Corridor {
            name: value.at("name")?.get::<String>()?,
            alignment: value.at("alignment")?.get::<String>()?,
            template_name: value.at("template")?.get::<String>()?,
            ..Corridor::default()
        };
        corridor.surface = value.value("surface", String::new())?;
        corridor.interval = value.at("interval")?.get::<f64>()?;
        // A corridor saved before the curve tolerance existed has none
        // written, and reads as the default: its quantities on a curve come
        // out as the tolerance says now, not as the chords did.
        corridor.curve_tolerance = value.value("curveTolerance", DEFAULT_CURVE_TOLERANCE)?;
        if value.contains("start") {
            corridor.start = Some(value.at("start")?.get::<f64>()?);
        }
        if value.contains("end") {
            corridor.end = Some(value.at("end")?.get::<f64>()?);
        }
        for region in value.find("regions").unwrap_or(&empty).iter() {
            corridor.regions.push(TemplateRegion {
                start: region.at("start")?.get::<f64>()?,
                end: region.at("end")?.get::<f64>()?,
                template_name: region.at("template")?.get::<String>()?,
            });
        }
        for point in value.find("superelevation").unwrap_or(&empty).iter() {
            corridor.superelevation.push(SuperelevationPoint {
                station: point.at("station")?.get::<f64>()?,
                left: point.at("left")?.get::<f64>()?,
                right: point.at("right")?.get::<f64>()?,
            });
        }
        set.corridors.push(corridor);
    }
    Ok(set)
}

/// ParseFailure for text that is not one, Unsupported for a newer version,
/// check_corridor's refusal for a corridor that is not sensible.
pub fn corridors_from_json(text: &str) -> Result<CorridorSet> {
    let set = match read_corridors(text) {
        Ok(set) => set,
        Err(Thrown::Refused(error)) => return Err(error),
        Err(Thrown::What(what)) => {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "the corridors cannot be read",
                what,
            ));
        }
    };
    let mut names: BTreeSet<String> = BTreeSet::new();
    for corridor in &set.corridors {
        check_corridor(corridor)?;
        if !names.insert(corridor.name.to_ascii_lowercase()) {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "two corridors have one name",
                corridor.name.as_str(),
            ));
        }
    }
    Ok(set)
}
