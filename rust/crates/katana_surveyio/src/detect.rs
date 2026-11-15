//! Which format is this file?
//!
//! Detection inspects the extension, the leading bytes, the record layout and
//! any known signature, and returns a RANKED list of candidates. It is
//! deliberately hard to misuse: the answer is not a format id, it is a
//! Detection whose `format()` hands back a descriptor only when one format
//! actually won. Every other outcome comes back as a Result the caller has to
//! look at. Parsing an unknown file with an incompatible parser does not
//! corrupt the file, it corrupts the survey that is built from it, which is
//! found out much later.
//!
//! The formats themselves are not here. Each parser gives its own probe - see
//! the contract in format.rs.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use katana_core::text::format_fixed;
use katana_core::{Error, ErrorCode, Result};
use katana_survey::data_model::source_file_name;

use crate::format::{
    FormatDescriptor, FormatRegistry, Named, ProbeInput, ProbeResult, format_registry, to_string,
};

/// How much of a file detection reads.
///
/// Policy, not a measurement. 64 KiB is enough for the header and first
/// entities of an XML document and for several hundred records of any
/// fixed-width instrument format, and small enough that probing a file on a
/// network share is not a pause a person notices. A probe that cannot decide
/// within it must say so with a middling confidence rather than ask for more.
pub const PROBE_BYTES: usize = 64 * 1024;

/// The bar a format has to clear before a caller may use it without asking:
/// the winner must be at least this sure of itself (and must beat the
/// runner-up by IDENTIFICATION_MARGIN). Policy numbers, stated once here so
/// that every format is judged by the same bar rather than by whatever each
/// probe thinks "confident" means.
pub const IDENTIFIED_CONFIDENCE: f64 = 0.7;
/// How far the winner must be ahead of the runner-up. Two formats that both
/// look 0.8 likely is precisely the case where guessing corrupts data, so that
/// is Ambiguous and the caller has to choose.
pub const IDENTIFICATION_MARGIN: f64 = 0.15;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DetectionOutcome {
    /// One format cleared the bar and left the next one behind.
    Identified,
    /// Two or more are credible and too close to separate.
    Ambiguous,
    /// Nothing was confident enough; the candidates say what was close.
    Uncertain,
    /// There were no bytes to look at.
    #[default]
    Empty,
}

impl Named for DetectionOutcome {
    fn name(self) -> &'static str {
        match self {
            DetectionOutcome::Identified => "identified",
            DetectionOutcome::Ambiguous => "ambiguous",
            DetectionOutcome::Uncertain => "uncertain",
            DetectionOutcome::Empty => "empty",
        }
    }
}

/// One format's answer, with the reason it gave.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormatCandidate {
    pub format_id: String,
    pub confidence: f64,
    pub evidence: String,
}

/// The result of looking at a file.
///
/// Built only through the factories, so the invariant holds: a descriptor
/// exists exactly when the outcome is Identified. There is no accessor that
/// returns a plausible-looking format id for an uncertain file, because that is
/// the mistake this type exists to prevent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Detection {
    outcome: DetectionOutcome,
    descriptor: Option<FormatDescriptor>,
    candidates: Vec<FormatCandidate>,
    summary: String,
}

impl Detection {
    // `ranked` is best first; detect_format() is what normally builds these.

    pub fn empty() -> Self {
        Self {
            outcome: DetectionOutcome::Empty,
            summary: String::from("the file is empty"),
            ..Default::default()
        }
    }

    pub fn uncertain(ranked: Vec<FormatCandidate>, summary: String) -> Self {
        Self {
            outcome: DetectionOutcome::Uncertain,
            descriptor: None,
            candidates: ranked,
            summary,
        }
    }

    pub fn ambiguous(ranked: Vec<FormatCandidate>, summary: String) -> Self {
        Self {
            outcome: DetectionOutcome::Ambiguous,
            descriptor: None,
            candidates: ranked,
            summary,
        }
    }

    pub fn identified(
        descriptor: FormatDescriptor,
        ranked: Vec<FormatCandidate>,
        summary: String,
    ) -> Self {
        Self {
            outcome: DetectionOutcome::Identified,
            descriptor: Some(descriptor),
            candidates: ranked,
            summary,
        }
    }

    pub fn outcome(&self) -> DetectionOutcome {
        self.outcome
    }

    /// The format to parse with - and the ONLY way to get one. NotFound for
    /// every outcome but Identified, with the summary as the message and the
    /// ranked candidates as the context, so a caller that just propagates the
    /// error still reports something a person can act on.
    pub fn format(&self) -> Result<FormatDescriptor> {
        if self.outcome == DetectionOutcome::Identified
            && let Some(descriptor) = &self.descriptor
        {
            return Ok(descriptor.clone());
        }
        let mut context = String::new();
        for candidate in &self.candidates {
            if !context.is_empty() {
                context.push_str(", ");
            }
            context.push_str(&candidate.format_id);
            context.push(' ');
            context.push_str(&two_decimals(candidate.confidence));
        }
        Err(Error::new(
            ErrorCode::NotFound,
            format!(
                "the format of this file was not identified ({}): {}",
                to_string(self.outcome),
                self.summary
            ),
        )
        .with_context(context))
    }

    /// Best first; equal confidences broken by id so the order never depends
    /// on registration order. Formats that ruled themselves out are not
    /// listed.
    pub fn candidates(&self) -> &[FormatCandidate] {
        &self.candidates
    }

    /// One sentence for a person: what was decided and why.
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// printf's "%.2f": format_fixed's digits, and a negative value that rounds to
// zero keeps its sign as printf keeps it ("-0.00" for a probe that reported
// -0.001), where format_fixed drops it.
fn two_decimals(value: f64) -> String {
    let text = format_fixed(value, 2);
    if value.is_sign_negative() && !text.starts_with('-') {
        format!("-{text}")
    } else {
        text
    }
}

struct Ranking {
    /// Best first.
    candidates: Vec<FormatCandidate>,
    /// Probes that misbehaved, if any.
    notes: String,
}

fn rank(results: Vec<ProbeResult>) -> Ranking {
    let mut ranking = Ranking {
        candidates: Vec::new(),
        notes: String::new(),
    };
    for result in results {
        let confidence = result.signature.confidence;
        // NaN and the infinities are outside the range too.
        if !(0.0..=1.0).contains(&confidence) {
            // Not clamped. A clamp would turn a broken probe into a confident
            // one and the wrong parser would run; saying so is the only honest
            // handling.
            if !ranking.notes.is_empty() {
                ranking.notes.push_str("; ");
            }
            ranking.notes.push_str(&format!(
                "format '{}' reported a confidence of {}, which is outside [0, 1]; it was ignored",
                result.format_id,
                two_decimals(confidence)
            ));
            continue;
        }
        if confidence <= 0.0 {
            continue; // the probe ruled its format out
        }
        ranking.candidates.push(FormatCandidate {
            format_id: result.format_id,
            confidence,
            evidence: result.signature.evidence,
        });
    }
    // STABLE, and on confidence alone: probe_all walks the registry, which is
    // ordered by id, so equal confidences keep that order and the ranking never
    // depends on which parser registered first.
    ranking
        .candidates
        .sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    ranking
}

fn with_notes(summary: String, notes: &str) -> String {
    if notes.is_empty() {
        return summary;
    }
    format!("{summary} ({notes})")
}

// ---- Detecting -------------------------------------------------------------------

/// Builds what every probe is given. `file_name` may be anything - a name, a
/// path, something a file supplied - and only its name part survives
/// (source_file_name). The extension is lower-cased here so that no probe has
/// to do it and none of them disagree about how.
///
/// `bytes` is not copied: it must outlive the ProbeInput.
pub fn probe_of<'a>(bytes: &'a [u8], file_name: &str, truncated: bool) -> ProbeInput<'a> {
    let file_name = source_file_name(file_name);
    // A name that BEGINS with a dot is a hidden file, not an extension.
    let extension = match file_name.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < file_name.len() => {
            file_name[dot + 1..].to_ascii_lowercase()
        }
        _ => String::new(),
    };
    ProbeInput {
        bytes,
        file_name,
        extension,
        truncated,
    }
}

/// Runs every probe of the program's registry and ranks what comes back.
pub fn detect_format(input: &ProbeInput<'_>) -> Detection {
    detect_format_with(input, format_registry())
}

/// Runs every probe of `registry` and ranks what comes back.
pub fn detect_format_with(input: &ProbeInput<'_>, registry: &FormatRegistry) -> Detection {
    // Before the registry is consulted: an empty file is empty whatever is
    // registered, and no probe should have to handle zero bytes.
    if input.bytes.is_empty() {
        return Detection::empty();
    }
    if registry.empty() {
        return Detection::uncertain(Vec::new(), String::from("no survey format is registered"));
    }

    let Ranking { candidates, notes } = rank(registry.probe_all(input));
    let Some(best) = candidates.first().cloned() else {
        return Detection::uncertain(
            Vec::new(),
            with_notes(
                String::from("no registered format recognises this file"),
                &notes,
            ),
        );
    };
    let runner_up = candidates.get(1).map_or(0.0, |second| second.confidence);
    let runner_up_id = candidates
        .get(1)
        .map(|second| second.format_id.clone())
        .unwrap_or_default();

    if best.confidence < IDENTIFIED_CONFIDENCE {
        let summary = with_notes(
            format!(
                "no format is confident enough about this file; the closest is {} at {} ({})",
                best.format_id,
                two_decimals(best.confidence),
                best.evidence
            ),
            &notes,
        );
        return Detection::uncertain(candidates, summary);
    }
    if best.confidence - runner_up < IDENTIFICATION_MARGIN {
        let summary = with_notes(
            format!(
                "{} and {runner_up_id} both fit this file ({} and {}); choose one",
                best.format_id,
                two_decimals(best.confidence),
                two_decimals(runner_up)
            ),
            &notes,
        );
        return Detection::ambiguous(candidates, summary);
    }

    match registry.find(&best.format_id) {
        // The registry produced this id a moment ago, so it can only be gone if
        // the registry was changed underneath us. Reported as uncertain rather
        // than asserted away, because an uncertain answer is a safe one.
        Err(_) => {
            let summary = format!(
                "format '{}' answered the probe but is no longer registered",
                best.format_id
            );
            Detection::uncertain(candidates, summary)
        }
        Ok(descriptor) => {
            let summary = with_notes(
                format!(
                    "{} ({}) at {}: {}",
                    descriptor.human_name,
                    best.format_id,
                    two_decimals(best.confidence),
                    best.evidence
                ),
                &notes,
            );
            Detection::identified(descriptor, candidates, summary)
        }
    }
}

/// Reads at most PROBE_BYTES from `path` and detects on those with the
/// program's registry. FileImportFailure when the file cannot be opened or
/// read; an existing but empty file is not a failure, it is
/// DetectionOutcome::Empty.
pub fn detect_file(path: &Path) -> Result<Detection> {
    detect_file_with(path, format_registry())
}

/// detect_file() against `registry`.
pub fn detect_file_with(path: &Path, registry: &FormatRegistry) -> Result<Detection> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        return Err(Error::new(
            ErrorCode::FileImportFailure,
            format!("'{name}' is not a readable file"),
        ));
    }
    let file = File::open(path).map_err(|_| {
        Error::new(
            ErrorCode::FileImportFailure,
            format!("cannot open '{name}'"),
        )
    })?;
    // One byte past the budget, purely to learn whether there is more to come.
    let mut limited = file.take(PROBE_BYTES as u64 + 1);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).map_err(|_| {
        Error::new(
            ErrorCode::FileImportFailure,
            format!("cannot read '{name}'"),
        )
    })?;
    let truncated = bytes.len() > PROBE_BYTES;
    bytes.truncate(PROBE_BYTES);
    Ok(detect_format_with(
        &probe_of(&bytes, &name, truncated),
        registry,
    ))
}

// ---- Helpers for probes ----------------------------------------------------------
//
// Here rather than in each parser so that five probes do not answer the same
// question five ways, and so that adding a parser stays a one-file change.

/// `bytes` with any UTF-8, UTF-16 or UTF-32 byte order mark stepped over. A
/// file saved by a Windows editor carries one, and without this every signature
/// test would have to know that "<LandXML" might not start at byte 0.
///
/// Not the archive layer's text decoder: that is a full decoder in a layer
/// surveyio may not see, and a probe must not decode a file merely to look at
/// it.
pub fn without_byte_order_mark(bytes: &[u8]) -> &[u8] {
    // UTF-32LE first: its mark begins with the UTF-16LE one, so testing the
    // shorter mark first would leave two NULs at the front of every UTF-32 file.
    const MARKS: [&[u8]; 5] = [
        b"\xFF\xFE\x00\x00",
        b"\x00\x00\xFE\xFF",
        b"\xEF\xBB\xBF",
        b"\xFF\xFE",
        b"\xFE\xFF",
    ];
    for mark in MARKS {
        if let Some(rest) = bytes.strip_prefix(mark) {
            return rest;
        }
    }
    bytes
}

/// At most `count` lines of the probe's bytes, without their terminators. CR,
/// LF and CRLF all end a line, because instrument files come off every
/// operating system there is. When the input is truncated the last line is
/// dropped - half a record is not a record, and a probe that measured its
/// length would be measuring where PROBE_BYTES fell.
pub fn probe_lines<'a>(input: &ProbeInput<'a>, count: usize) -> Vec<&'a [u8]> {
    let mut lines = Vec::new();
    if count == 0 {
        return lines;
    }
    lines.reserve(count.min(64));
    let mut rest = without_byte_order_mark(input.bytes);
    while !rest.is_empty() && lines.len() < count {
        let Some(end) = rest.iter().position(|&c| c == b'\r' || c == b'\n') else {
            // No terminator: a real last line, unless the bytes were cut short,
            // in which case its length says where PROBE_BYTES fell and nothing
            // about the file.
            if !input.truncated {
                lines.push(rest);
            }
            break;
        };
        lines.push(&rest[..end]);
        let mut next = end + 1;
        if rest[end] == b'\r' && next < rest.len() && rest[next] == b'\n' {
            next += 1;
        }
        rest = &rest[next..];
    }
    lines
}

/// True when the bytes are plausibly the text an instrument format is written
/// in: no NUL, and control bytes other than tab, CR and LF no more than one
/// byte in a hundred. UTF-16 fails this test - it is full of NULs - and that is
/// deliberate: no format read here is UTF-16, and a probe that wants to accept
/// one can look for the byte order mark itself. Empty bytes are not text: there
/// is nothing to judge, and a probe must not treat "I saw nothing" as "I saw
/// text".
pub fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false; // nothing to judge
    }
    let mut control = 0usize;
    for &byte in bytes {
        if byte == 0x00 {
            return false;
        }
        if (byte < 0x20 && byte != b'\t' && byte != b'\r' && byte != b'\n') || byte == 0x7F {
            control += 1;
        }
    }
    // One byte in a hundred. Policy, not a measurement: a form feed or an
    // escape in a header is ordinary in instrument output, a page of them is
    // not text.
    control * 100 <= bytes.len()
}
