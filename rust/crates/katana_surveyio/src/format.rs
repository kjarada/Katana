//! Survey field-data formats: what each one is, what it can carry, and the
//! registry every parser puts itself into.
//!
//! surveyio turns manufacturer field data - Leica, Trimble, Topcon, LandXML, a
//! coordinate CSV - into katana_survey values and nothing else. It does not
//! transform coordinates and it cannot: see DeclaredCoordinateSystem for why
//! recording what a file DECLARES is the only honest thing to do with it.
//!
//! Conventions inherited from katana_survey and not restated per field: metres,
//! radians, azimuths clockwise in [0, 2*pi). A parser reading degrees or gons
//! converts on the way in and records what it converted from in DeclaredUnits.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, OnceLock};

use katana_core::{Error, ErrorCode, Result, Status};
use katana_survey::{ObservationPrecision, SurveyProject};

/// The spelling of an enumerator of the survey file readers: what the C++'s
/// `toString` overloads return, one implementation per enumeration.
pub trait Named: Copy {
    fn name(self) -> &'static str;
}

/// The C++'s `toString`, for every enumeration of the survey file readers.
pub fn to_string<T: Named>(value: T) -> &'static str {
    value.name()
}

/// Who defined the format. This list is deliberately longer than the formats
/// that exist today: adding a value is an edit to this line, and several
/// parsers landing at once must not all need it. A vendor that is not here and
/// is wanted by only one format is `Other`, spelled out in
/// FormatDescriptor::human_name.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Manufacturer {
    /// No vendor at all: a coordinate CSV, plain delimited text.
    #[default]
    Generic,
    /// A published specification with no owner: LandXML and the like.
    OpenStandard,
    Leica,
    Trimble,
    Topcon,
    Sokkia,
    Nikon,
    Geomax,
    Carlson,
    Other,
}

impl Named for Manufacturer {
    fn name(self) -> &'static str {
        match self {
            Manufacturer::Generic => "Generic",
            Manufacturer::OpenStandard => "Open standard",
            Manufacturer::Leica => "Leica",
            Manufacturer::Trimble => "Trimble",
            Manufacturer::Topcon => "Topcon",
            Manufacturer::Sokkia => "Sokkia",
            Manufacturer::Nikon => "Nikon",
            Manufacturer::Geomax => "GeoMax",
            Manufacturer::Carlson => "Carlson",
            Manufacturer::Other => "Other",
        }
    }
}

/// What a format can carry.
///
/// Stated per format so that the application can say what will be LOST before
/// an import runs - "this format carries points only; the raw observations in
/// the job will not come across" - instead of dropping half a file quietly.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FormatContent {
    pub points: bool,
    pub observations: bool,
    /// Instrument setups with their backsights.
    pub stations: bool,
    /// Coded strings.
    pub features: bool,
    /// The file declares one (declares, never transforms).
    pub coordinate_system: bool,
    /// InstrumentSettings per setup.
    pub instrument_settings: bool,
    /// GNSS positions, vectors or sessions.
    pub gnss: bool,
}

/// One format, as the user interface should describe it.
///
/// The point of this type is that the application can say "Leica GSI-16,
/// import: yes, export: no, parser: 1.0" rather than "all Leica files
/// supported", which is a promise no parser keeps. `parser_version` is the
/// version of THIS code, not of the format: when a file imports differently
/// after an upgrade, that string is what says so, and it is the string that
/// lands in SourceRecord.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FormatDescriptor {
    /// Stable identity: lower case letters, digits and '-' only. It appears in
    /// saved settings, in SourceRecord and in this library's own error
    /// messages, so it never changes and is never translated. e.g.
    /// "leica-gsi16".
    pub id: String,
    /// "Leica GSI-16"
    pub human_name: String,
    pub manufacturer: Manufacturer,
    /// What an import of this format can produce.
    pub reads: FormatContent,
    pub can_import: bool,
    pub can_export: bool,
    /// This parser's version, e.g. "1.0".
    pub parser_version: String,
    /// Lower case, no dot, e.g. {"gsi"}.
    pub extensions: Vec<String>,
}

/// "Leica GSI-16, import: yes, export: no, parser: 1.0" - one phrasing, so every
/// place that lists formats lists them the same way.
pub fn describe_format(descriptor: &FormatDescriptor) -> String {
    let yes_no = |flag: bool| if flag { "yes" } else { "no" };
    let name = if descriptor.human_name.is_empty() {
        descriptor.id.as_str()
    } else {
        descriptor.human_name.as_str()
    };
    let parser = if descriptor.parser_version.is_empty() {
        "unknown"
    } else {
        descriptor.parser_version.as_str()
    };
    format!(
        "{name}, import: {}, export: {}, parser: {parser}",
        yes_no(descriptor.can_import),
        yes_no(descriptor.can_export)
    )
}

// ---- Detection probes ------------------------------------------------------------

/// What a probe is given, prepared once and shared by every probe so that
/// detection reads the file once however many formats are registered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeInput<'a> {
    /// The first PROBE_BYTES of the file (detect.rs), or all of it when it is
    /// shorter. A VIEW: whatever owns the bytes must outlive the ProbeInput.
    pub bytes: &'a [u8],
    /// File NAME only, never a path - built with source_file_name(), for the
    /// reason given there. A probe is never handed something it could open.
    pub file_name: String,
    /// Lower case, no dot; empty when the name has none.
    pub extension: String,
    /// The file is longer than `bytes`.
    pub truncated: bool,
}

/// A probe's answer about ONE format.
///
/// `confidence` is in [0, 1]: 0 rules the format out, 1 means a signature that
/// cannot belong to anything else. Anything outside that range is a bug in the
/// probe and the candidate is dropped and reported, never clamped - a clamp
/// would turn a broken probe into a confident one. `evidence` says WHY in a few
/// words, because a detection a person has to second-guess is one they cannot
/// check: "extension .gsi, every record 24 bytes, word 11 first".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormatSignature {
    pub confidence: f64,
    pub evidence: String,
}

/// Named for the call site: `return ruled_out();` reads better than
/// `return FormatSignature::default();` where the whole point of the line is
/// that this is not the format.
pub fn ruled_out() -> FormatSignature {
    FormatSignature::default()
}

/// A plain function pointer rather than a closure: a probe is a free function
/// in a parser's own module, it captures nothing, and the registry is then a
/// value that allocates nothing per format.
pub type FormatProbe = fn(&ProbeInput<'_>) -> FormatSignature;

// ---- Readers ---------------------------------------------------------------------
//
// A format that can be imported in one step registers a READER beside its
// probe: bytes in, a SurveyProject out. read_survey() (reader.rs) is how
// anything calls one; it adds the checks every reader would otherwise repeat
// (size caps, the sibling-file rule, validate_project) and stamps the result
// with the format's id and parser version.

/// Another file of the same job, by name and content.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SiblingFile {
    /// A file NAME, never a path.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Returns the bytes of the file called `name` IN THE SAME FOLDER as the file
/// being read. Some formats are more than one file: a Leica DBX job is a folder
/// of files that name each other; a RINEX observation file has its navigation
/// file beside it.
///
/// SECURITY. A reader passes a NAME it found in the file, and the name is
/// untrusted: the lookup refuses anything source_file_name() would change (a
/// directory, "..", a drive, an alternate data stream), and the implementations
/// in reader.rs read only regular files in the one folder, capped in size.
/// NotFound when there is no such file - a reader that can do without it goes
/// on with a warning; one that cannot fails, naming the file.
pub type SiblingLookup = Arc<dyn Fn(&str) -> Result<Vec<u8>> + Send + Sync>;

#[derive(Clone, Default)]
pub struct ReadOptions {
    /// None: no sibling can be read (a single file handed over on its own).
    pub siblings: Option<SiblingLookup>,
    /// Standard deviations for observations whose file states none: every
    /// observation must carry sigma > 0 (validate_observation). The reduction
    /// replaces them with the person's own (reduction_settings), so these only
    /// have to be sensible, not right.
    pub precision: ObservationPrecision,
}

// A lookup is a function and has nothing to show but whether it is there.
impl fmt::Debug for ReadOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadOptions")
            .field("siblings", &self.siblings.as_ref().map(|_| "a lookup"))
            .field("precision", &self.precision)
            .finish()
    }
}

/// A warning with the place it is about, so a person can open the file at the
/// record. `record` is 1-based: a line for a text format, a record or block for
/// a binary one; 0 when the warning is about the file as a whole.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReadWarning {
    /// The file read, or the sibling the warning is about.
    pub file_name: String,
    pub record: usize,
    pub message: String,
}

/// "job.gsi record 12: target height 99.999 read as unset" - one phrasing, so
/// seven readers do not invent seven.
pub fn describe(warning: &ReadWarning) -> String {
    let mut text = warning.file_name.clone();
    if warning.record != 0 {
        let separator = if text.is_empty() { "" } else { " " };
        text = format!("{text}{separator}record {}", warning.record);
    }
    if !text.is_empty() {
        text.push_str(": ");
    }
    text.push_str(&warning.message);
    text
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadResult {
    pub project: SurveyProject,
    /// A record that cannot be read is a WARNING naming it, never a silently
    /// dropped value; an unreadable FILE is an error in the Result instead.
    pub warnings: Vec<ReadWarning>,
    pub records_read: usize,
    /// Records the format defines that this reader does not handle, counted
    /// (see ImportResult::records_skipped).
    pub records_skipped: usize,
    /// What this FILE did not carry that an import usually needs, in words:
    /// "no instrument heights", "no atmospheric settings - the atmospheric
    /// correction state is unknown". Shown before the import and in the report.
    pub not_carried: Vec<String>,

    // Filled by read_survey(), not by the reader:
    pub format_id: String,
    pub parser_version: String,
    /// Every sibling the reader fetched, in fetch order.
    pub siblings_read: Vec<SiblingFile>,
}

/// A reader: the file's bytes, its NAME (source_file_name already applied) and
/// the options. A plain function pointer for the reason given beside
/// FormatProbe. It must not open anything itself; other files come through
/// ReadOptions::siblings.
pub type FormatReader = fn(&[u8], &str, &ReadOptions) -> Result<ReadResult>;

// ---- The registry ----------------------------------------------------------------

/// Every registered probe's answer to one input (FormatRegistry::probe_all).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeResult {
    pub format_id: String,
    pub signature: FormatSignature,
}

#[derive(Clone, Debug)]
struct Entry {
    descriptor: FormatDescriptor,
    probe: FormatProbe,
    reader: Option<FormatReader>,
}

/// The formats one program knows about.
///
/// A value, not a singleton. The process-wide instance is format_registry()
/// below, but a test (or a caller that wants to detect against a restricted
/// set) builds its own, which is why registration is a method and not a free
/// function.
///
/// Ordered by id, never hashed, so that iteration - and therefore the ranking
/// of two equally confident candidates - is the same on every run.
#[derive(Clone, Debug, Default)]
pub struct FormatRegistry {
    formats: BTreeMap<String, Entry>,
}

// ASCII only, deliberately: a format id or an extension must mean the same on
// every machine, whatever its locale.
const fn lowered(c: u8) -> u8 {
    c.to_ascii_lowercase()
}

// Empty when the id is usable.
//
// The id is written into saved settings and into every SourceRecord an import
// produces, and is compared byte for byte. Restricting it to a lower case slug
// means two formats can never differ only by case or by some character a
// person cannot see in a diff.
fn id_problem(id: &str) -> String {
    let Some(&first) = id.as_bytes().first() else {
        return String::from("a format descriptor has an empty id");
    };
    if !first.is_ascii_lowercase() {
        return format!("format id '{id}' must start with a lower case letter");
    }
    for c in id.bytes() {
        let allowed = c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-';
        if !allowed {
            return format!("format id '{id}' may contain only lower case letters, digits and '-'");
        }
    }
    String::new()
}

impl FormatRegistry {
    /// InvalidArgument when the descriptor is not usable: an empty or non-slug
    /// id, an empty human name or parser version, no probe, a format that can
    /// neither import nor export, or a reader for a format that cannot import.
    /// AlreadyExists when the id is taken - two parsers answering to one name
    /// is a programming error, not a preference. `reader` may be None: a format
    /// can be detected without being readable in one step (see reader.rs for
    /// the delimited-points format, which is read through its column layout).
    pub fn add(
        &mut self,
        descriptor: FormatDescriptor,
        probe: Option<FormatProbe>,
        reader: Option<FormatReader>,
    ) -> Status {
        let reason = id_problem(&descriptor.id);
        if !reason.is_empty() {
            return Err(
                Error::new(ErrorCode::InvalidArgument, reason).with_context(descriptor.human_name)
            );
        }
        let Some(probe) = probe else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("format '{}' has no detection probe", descriptor.id),
            ));
        };
        if descriptor.human_name.is_empty() {
            // What the application shows in a file dialog and in an import
            // report.
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("format '{}' has no human readable name", descriptor.id),
            ));
        }
        if descriptor.parser_version.is_empty() {
            // Without it an import cannot say which version of this code
            // produced a value, which is the whole point of
            // SourceRecord::format_version.
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("format '{}' has no parser version", descriptor.id),
            ));
        }
        if !descriptor.can_import && !descriptor.can_export {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("format '{}' can neither import nor export", descriptor.id),
            ));
        }
        if reader.is_some() && !descriptor.can_import {
            // A reader for a format that says it cannot be imported: one of the
            // two is wrong, and the application would disagree with itself
            // about it.
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "format '{}' has a reader but says it cannot import",
                    descriptor.id
                ),
            ));
        }
        for extension in &descriptor.extensions {
            let usable = !extension.is_empty()
                && !extension.starts_with('.')
                && extension.bytes().all(|c| c == lowered(c));
            if !usable {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "format '{}': extension '{extension}' must be lower case and without a leading dot",
                        descriptor.id
                    ),
                )
                .with_context("ProbeInput::extension is normalised that way"));
            }
        }
        if self.formats.contains_key(&descriptor.id) {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                format!(
                    "a survey format with id '{}' is already registered",
                    descriptor.id
                ),
            )
            .with_context(descriptor.human_name));
        }
        self.formats.insert(
            descriptor.id.clone(),
            Entry {
                descriptor,
                probe,
                reader,
            },
        );
        Ok(())
    }

    pub fn formats(&self) -> Vec<FormatDescriptor> {
        self.formats
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect()
    }

    pub fn find(&self, id: &str) -> Result<FormatDescriptor> {
        match self.formats.get(id) {
            Some(entry) => Ok(entry.descriptor.clone()),
            None => Err(Error::new(
                ErrorCode::NotFound,
                format!("no survey format is registered with id '{id}'"),
            )),
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.formats.contains_key(id)
    }

    /// The reader registered for `id`, or None when there is none (or no such
    /// format).
    pub fn reader(&self, id: &str) -> Option<FormatReader> {
        self.formats.get(id).and_then(|entry| entry.reader)
    }

    pub fn size(&self) -> usize {
        self.formats.len()
    }

    pub fn empty(&self) -> bool {
        self.formats.is_empty()
    }

    /// Every registered probe run against one input, in id order. detect.rs
    /// turns these into a ranked Detection; this is the raw material and is
    /// exposed so that the ranking can be tested apart from the probing.
    pub fn probe_all(&self, input: &ProbeInput<'_>) -> Vec<ProbeResult> {
        self.formats
            .iter()
            .map(|(id, entry)| ProbeResult {
                format_id: id.clone(),
                signature: (entry.probe)(input),
            })
            .collect()
    }
}

// ---- How a parser adds its format ------------------------------------------------
//
// THIS IS THE CONTRACT. The C++ adds each format from the parser's own
// translation unit at static initialisation. Rust has no life before main, so
// the program's formats are one explicit list, REGISTRATIONS below, and a
// parser adds itself with ONE line there:
//
//     // src/leica_gsi.rs
//     fn probe_gsi(input: &ProbeInput<'_>) -> FormatSignature {
//         if input.extension != "gsi" {
//             return ruled_out();
//         }
//         ...
//         FormatSignature { confidence: 0.95, evidence: String::from("extension .gsi and every record 24 bytes") }
//     }
//
//     pub fn leica_gsi_format() -> FormatDescriptor { ... }
//
//     fn read_gsi(bytes: &[u8], file_name: &str, options: &ReadOptions) -> Result<ReadResult> {
//         ... a bad record is a ReadWarning naming it, never a dropped value ...
//     }
//
//     pub(crate) fn registration() -> FormatRegistration {
//         FormatRegistration::new(leica_gsi_format(), probe_gsi, Some(read_gsi))
//     }
//
// and in REGISTRATIONS, `crate::leica_gsi::registration,`.
//
// What the mechanism depends on:
//   * The id is yours alone. A clash is a programming error, and building the
//     registry panics rather than let two formats answer to one name;
//     FormatRegistry::add is the same check as a Status, for callers building
//     their own registry.
//   * A probe must be cheap and must not open anything. It gets the first
//     PROBE_BYTES of the file and the file's NAME, and that is all it may look
//     at.
//   * The order of the list is irrelevant. The registry is ordered by id and
//     the ranking never depends on who registered first.

/// Every format this program registers, one line each.
const REGISTRATIONS: &[fn() -> FormatRegistration] = &[
    crate::delimited_points::registration,
    crate::opcode_field_file::registration,
    crate::trimble_jobxml::registration,
    crate::trimble_dc::registration,
    crate::leica_gsi::registration,
    crate::leica_dbx::registration,
    crate::rinex::registration,
    crate::sokkia_sdr::registration,
    crate::topcon_gts::gts6_registration,
    crate::topcon_gts::gts7_registration,
    crate::topcon_rw5::registration,
];

/// One format as a parser hands it to the registry: the descriptor, the probe
/// and the reader, if the format has one.
#[derive(Clone, Debug)]
pub struct FormatRegistration {
    pub descriptor: FormatDescriptor,
    pub probe: FormatProbe,
    pub reader: Option<FormatReader>,
}

impl FormatRegistration {
    pub fn new(
        descriptor: FormatDescriptor,
        probe: FormatProbe,
        reader: Option<FormatReader>,
    ) -> Self {
        Self {
            descriptor,
            probe,
            reader,
        }
    }

    /// Adds the format to `registry`, and PANICS with the reason when the
    /// registry rejects it - the C++ constructor's std::logic_error. The
    /// program's registry is built where there is no caller to hand a Status
    /// to, and a format that silently failed to register would show up as
    /// "this file is not recognised" months later. Failing at first use with
    /// the reason is the loud alternative.
    pub fn register(self, registry: &mut FormatRegistry) {
        let id = self.descriptor.id.clone();
        if let Err(error) = registry.add(self.descriptor, Some(self.probe), self.reader) {
            panic!(
                "surveyio: format '{id}' could not be registered: {}",
                error.describe()
            );
        }
    }
}

/// The registry the whole program shares, built from REGISTRATIONS on first
/// use.
pub fn format_registry() -> &'static FormatRegistry {
    static REGISTRY: OnceLock<FormatRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = FormatRegistry::default();
        for registration in REGISTRATIONS {
            registration().register(&mut registry);
        }
        registry
    })
}

// ---- What a parser returns -------------------------------------------------------

/// One shape for every importer, so that five of them do not return five.
///
/// A warning is a sentence a person can act on ("record 412: target height
/// 99.999 read as unset"). It is never how a failure is reported: a failure is
/// an error in the Result the importer returns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportResult {
    pub project: SurveyProject,
    /// The FormatDescriptor::id that read it.
    pub format_id: String,
    pub warnings: Vec<String>,
    pub records_read: usize,
    /// Records the format defines and this parser does not yet handle. Counted
    /// rather than ignored, so "it imported fine" and "it imported the third of
    /// the file I understand" are distinguishable.
    pub records_skipped: usize,
}
