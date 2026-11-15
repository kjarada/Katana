//! The working state of one reduce_and_adjust call, shared by the modules that
//! implement it: reduction.rs (pairing, corrections, orientation, radiation),
//! reduction_gnss.rs (GNSS vectors), reduction_adjust.rs (traverse, network and
//! resection). Private to the crate.
//!
//! Ids are held as views into the raw project, the settings and the context,
//! which all outlive the call: at 100 000 observations a copied id per
//! observation is the largest cost of the whole reduction.

use std::collections::{HashMap, HashSet};

use crate::data_model::{Face, SourceRecord, SurveyPoint, SurveyProject, SurveyStation};
use crate::reduction::ReductionContext;
use crate::reduction_report::{ComputationMethod, ReductionReport, ReportMessage};
use crate::reduction_settings::ReductionSettings;

/// No setup: what Position::from_setup holds for a point no setup radiated.
pub(crate) const NO_SETUP: usize = usize::MAX;

/// Where a position came from, in the order of authority.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum PositionOrigin {
    /// A ControlSelection: held.
    Control,
    /// The file's point, keyed in or published (CoordinateSource::Entered).
    Entered,
    /// A GNSS position converted to grid.
    Gnss,
    /// Radiated or adjusted here.
    #[default]
    Computed,
    /// The file's own coordinates, used only because nothing better exists.
    FileOnly,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Position {
    pub northing: f64,
    pub easting: f64,
    pub height: Option<f64>,
    pub origin: PositionOrigin,
    pub method: ComputationMethod,
    pub held_horizontal: bool,
    pub sigma_northing: Option<f64>,
    pub sigma_easting: Option<f64>,
    pub sigma_height: Option<f64>,
    /// The setup it was radiated from, for "side shot" bookkeeping.
    pub from_setup: usize,
}

impl Default for Position {
    fn default() -> Self {
        Self {
            northing: 0.0,
            easting: 0.0,
            height: None,
            origin: PositionOrigin::Computed,
            method: ComputationMethod::Radiation,
            held_horizontal: false,
            sigma_northing: None,
            sigma_easting: None,
            sigma_height: None,
            from_setup: NO_SETUP,
        }
    }
}

/// One reduced pointing: a face-left / face-right mean, or a single face.
///
/// The C++ also keeps the setup's index, whether it is a pair, the face-right
/// pointing's index and every raw row it came from; it writes them and reads
/// none, so they are not carried here.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ReducedPointing<'a> {
    pub target: &'a str,
    /// Left for a pair.
    pub face: Face,
    pub left_index: usize,

    // FL-equivalent values after the face mean.
    /// Circle reading.
    pub direction: Option<f64>,
    pub zenith: Option<f64>,
    /// Corrected slope distance.
    pub slope: Option<f64>,
    /// After slope to horizontal and curvature: ground, at the instrument.
    pub horizontal: Option<f64>,
    /// Mark to mark, after heights and curvature.
    pub height_difference: Option<f64>,
    /// X = S cos z, for the line's mean height.
    pub measured_vertical: f64,
    pub target_height: f64,
    /// The file gave a horizontal distance.
    pub recorded_horizontal: bool,

    // After phase B.
    pub grid_distance: Option<f64>,
    pub azimuth: Option<f64>,

    // A-priori standard deviations of the reduced values.
    /// Pointing only, radians.
    pub sigma_direction: f64,
    pub sigma_zenith: f64,
    /// EDM part, metres.
    pub sigma_distance: f64,

    // Report rows (indices into ReductionReport::observations) whose chains
    // continue with this pointing's later corrections.
    pub direction_rows: [usize; 2],
    pub direction_row_count: usize,
    pub distance_rows: [usize; 2],
    pub distance_row_count: usize,
    pub height_row: Option<usize>,

    pub rejected: bool,
    /// The first raw observation's record.
    pub source: Option<&'a SourceRecord>,
}

/// A horizontal angle or azimuth the file recorded as such, kept for the
/// network and for radiation.
///
/// The C++ also keeps the setup's index, written and never read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RecordedAngle<'a> {
    pub at: &'a str,
    /// Empty for an azimuth.
    pub from: &'a str,
    pub to: &'a str,
    pub value: f64,
    pub sigma: f64,
    pub row: usize,
}

/// A GNSS vector in grid terms: a grid baseline as the file gave it, or a
/// geocentric one converted through the context (reduction_gnss.rs).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GridVector<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub delta_northing: f64,
    pub delta_easting: f64,
    /// Mark to mark; absent when an antenna height could not be reduced.
    pub delta_height: Option<f64>,
    pub sigma_northing: f64,
    pub sigma_easting: f64,
    pub sigma_height: f64,
    /// The vector's report row.
    pub row: usize,
    /// Its height difference row.
    pub height_row: Option<usize>,
    pub source: Option<&'a SourceRecord>,
    /// Placed its far end, or checked it.
    pub radiated: bool,
}

/// A GNSS position of a point converted to grid and taken down to the mark,
/// one per position observation: a point occupied twice has two, and each is
/// checked against the first (radiation) or enters the network with its own
/// value.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GlobalGridPosition<'a> {
    pub point: &'a str,
    pub northing: f64,
    pub easting: f64,
    /// Of the mark; absent when the antenna could not be reduced.
    pub height: Option<f64>,
    pub sigma_horizontal: f64,
    pub sigma_vertical: f64,
    pub row: usize,
    pub source: Option<&'a SourceRecord>,
}

/// Per setup, after phase B.
#[derive(Clone, Debug, Default)]
pub(crate) struct SetupState<'a> {
    pub positioned: bool,
    /// Added to a circle reading to give an azimuth.
    pub orientation: Option<f64>,
    /// From a set circle, not from coordinates.
    pub orientation_assumed: bool,
    /// Of those, the ones oriented on the azimuth the file states for the
    /// backsight (SurveyStation::stated_backsight_azimuth) rather than the
    /// circle.
    pub orientation_stated: bool,
    /// Whether a circle set on a NAMED backsight that has no position may
    /// stand in for its grid azimuth, or the azimuth the file states for it
    /// be used. Off until nothing else can place the backsight: a controller
    /// records the circle it set (often 0 00 00) whether or not it is a grid
    /// azimuth, so taking it at once would orient the setup wrongly and leave
    /// the right orientation, which a later setup may give by radiating the
    /// backsight, unused; and coordinates, where they come, are the
    /// reduction's own azimuth, so a stated one waits for them as well.
    pub accept_circle_as_set: bool,
    // Notices already given for this setup: orient_and_radiate runs again
    // after the backsight is placed and after an adjustment, and the report
    // should say each thing once.
    pub warned_height: bool,
    pub warned_geoid: bool,
    pub warned_scale: bool,
    pub report_index: usize,
    /// Indices into Engine::pointings.
    pub pointings: Vec<usize>,
    /// Set when resect_setup positioned the station. Such a setup is oriented
    /// by its resection, not its backsight: on the weighted mean of azimuth
    /// less reading over these pointings, each with the standard deviation its
    /// direction had in the resection (the least-squares orientation for the
    /// station where it stands, so it follows the station when an adjustment
    /// moves it). The pointings its resection used are not checks when it
    /// radiates (their residuals are the resection's) but a later pointing
    /// to one of the same points (a controller's check after its block) is.
    pub resected: bool,
    pub resection_directions: Vec<(usize, f64)>,
    pub resection_pointings: HashSet<usize>,
    pub resection_targets: HashSet<&'a str>,
    /// The record that ends the observations the file's own resection was
    /// computed from (RESECTION_END_METADATA), when the file says: only the
    /// pointings before it may enter the reduction's resection.
    pub resection_block_end: Option<usize>,
}

pub(crate) struct Engine<'a> {
    pub raw: &'a SurveyProject,
    pub settings: &'a ReductionSettings,
    pub context: &'a ReductionContext,
    pub report: ReductionReport,

    pub pointings: Vec<ReducedPointing<'a>>,
    pub angles: Vec<RecordedAngle<'a>>,
    pub setups: Vec<SetupState<'a>>,
    pub vectors: Vec<GridVector<'a>>,
    /// Indexed like raw.observations: the converted GNSS position of each
    /// position observation, empty for every other kind and for one that
    /// could not be converted.
    pub global_positions: Vec<Option<GlobalGridPosition<'a>>>,

    // Looked up, never iterated in an order the output shows: the order of
    // the output is position_order's.
    pub positions: HashMap<&'a str, Position>,
    /// The order points were first positioned in: the order of the output.
    pub position_order: Vec<&'a str>,
    /// The file's own coordinates, by id. (The C++ also keeps the setups
    /// occupying each point, written and never read.)
    pub file_points: HashMap<&'a str, &'a SurveyPoint>,

    setup_notices: Vec<(String, Vec<&'a SurveyStation>)>,
    setup_notice_slot: HashMap<String, usize>,
}

impl<'a> Engine<'a> {
    pub fn new(
        project: &'a SurveyProject,
        settings: &'a ReductionSettings,
        context: &'a ReductionContext,
    ) -> Self {
        Self {
            raw: project,
            settings,
            context,
            report: ReductionReport::default(),
            pointings: Vec::new(),
            angles: Vec::new(),
            setups: Vec::new(),
            vectors: Vec::new(),
            global_positions: Vec::new(),
            positions: HashMap::new(),
            position_order: Vec::new(),
            file_points: HashMap::new(),
            setup_notices: Vec::new(),
            setup_notice_slot: HashMap::new(),
        }
    }

    pub fn warn(&mut self, text: impl Into<String>, source: SourceRecord) {
        self.report.warnings.push(ReportMessage {
            text: text.into(),
            source,
        });
    }

    /// A notice that is the same sentence for many setups ("the file does not
    /// say whether the prism constant is in the distances") is collected and
    /// written once, naming the setups: a thousand identical lines would bury
    /// the one warning that matters.
    pub fn warn_setup(&mut self, station: &'a SurveyStation, text: impl Into<String>) {
        let text = text.into();
        let slot = match self.setup_notice_slot.get(&text) {
            Some(slot) => *slot,
            None => {
                let slot = self.setup_notices.len();
                self.setup_notice_slot.insert(text.clone(), slot);
                self.setup_notices.push((text, Vec::new()));
                slot
            }
        };
        self.setup_notices[slot].1.push(station);
    }

    pub fn flush_setup_notices(&mut self) {
        let notices = std::mem::take(&mut self.setup_notices);
        for (text, stations) in notices {
            let who = if stations.len() == 1 {
                format!("Setup {}", stations[0].setup.id)
            } else {
                let mut who = format!("{} setups (", stations.len());
                for (i, station) in stations.iter().take(5).enumerate() {
                    if i != 0 {
                        who.push_str(", ");
                    }
                    who.push_str(&station.setup.id);
                }
                who.push_str(if stations.len() > 5 { ", ...)" } else { ")" });
                who
            };
            self.warn(format!("{who}: {text}"), stations[0].source.clone());
        }
        self.setup_notice_slot.clear();
    }

    pub fn find(&self, id: &str) -> Option<&Position> {
        self.positions.get(id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Position> {
        self.positions.get_mut(id)
    }

    /// Sets a position; a first positioning also records the output order.
    pub fn place(&mut self, id: &'a str, position: Position) {
        match self.positions.get_mut(id) {
            Some(existing) => *existing = position,
            None => {
                self.positions.insert(id, position);
                self.position_order.push(id);
            }
        }
    }
}
