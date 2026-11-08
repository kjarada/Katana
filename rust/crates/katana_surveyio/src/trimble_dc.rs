//! Trimble Survey Controller DC (.dc): recognised, and REFUSED with a way out.
//!
//! The port of src/katana_surveyio/trimble_dc.cpp. The format has no public
//! interface of its own: it is reached through the registry (`registration`),
//! as the C++ reaches it through its static registration.

// Why there is no reader. The DC format is Trimble's own. Its records are one
// per line, each opening with a two-character record type ("00", "10", "77",
// "D9", "E0", ...) and a two-character derivation code ("NM", "F1", ...), the
// same shape as the Sokkia SDR format it grew out of but with Trimble's own
// record numbers and fields. Trimble has not published the record layouts:
// the Survey Controller Reference Manual (version 7.7, part number
// 32968-70-ENG, January 2001, section "File formats") says only that
// "information about the .dc file format is available from the Trimble web
// site ... contact your local Trimble dealer", and no such document is
// available there now. What is public are third parties' partial notes - a
// list of record names without layouts (Traverse PC's "Trimble DC10.70"
// help page) and seven example lines (the Novapoint wiki's "Trimble DC file
// format" page) - which is not enough to know what a field means, only where
// it might be.
//
// A reader built from example lines would be a guess at which 16-character
// field is the zenith angle and which the slope distance, and at what a
// one-character flag between them says. A guess that is wrong reads a
// plausible survey that is wrong, which is worse than no survey. So this
// format is detected - so the person is not told "unrecognised file" - and
// refused with what to do instead: Trimble Access and Trimble Business Center
// both export the same job as JobXML, whose schema IS published and which
// trimble_jobxml.rs reads.
//
// The refusal lists the record types the file holds, counted, so a person or
// a support request can see what the file contained.
//
// A Sokkia SDR file (a header "00", a derivation code, then "SDR") is NOT
// claimed: a controller can write SDR33 into a file named .dc, and the Sokkia
// SDR reader reads it. The derivation code is not only NM - Sokkia's SDR33
// layout allows ED as well - so this probe steps aside for any.

use std::collections::BTreeMap;

use katana_core::{Error, ErrorCode, Result};

use crate::detect::{looks_like_text, probe_lines};
use crate::format::{
    FormatContent, FormatDescriptor, FormatRegistration, FormatSignature, Manufacturer, ProbeInput,
    ReadOptions, ReadResult, ruled_out,
};

const FORMAT_ID: &str = "trimble-dc";

fn is_record_type_char(c: u8) -> bool {
    c.is_ascii_digit() || (b'A'..=b'F').contains(&c)
}

fn is_derivation_char(c: u8) -> bool {
    c.is_ascii_digit() || c.is_ascii_uppercase()
}

// Two hexadecimal-looking characters and two upper-case ones: the shape every
// DC record opens with (the Novapoint notes' "00NM", "D9F1", "E0NM").
fn has_record_shape(line: &[u8]) -> bool {
    line.len() >= 4
        && is_record_type_char(line[0])
        && is_record_type_char(line[1])
        && is_derivation_char(line[2])
        && is_derivation_char(line[3])
}

fn probe_dc(input: &ProbeInput<'_>) -> FormatSignature {
    let dc_extension = input.extension == "dc";
    if !looks_like_text(input.bytes) {
        return ruled_out();
    }
    let lines = probe_lines(input, 40);
    let Some(&header) = lines.first() else {
        return ruled_out();
    };
    if header.starts_with(b"00") && header.len() >= 7 && header[4..].starts_with(b"SDR") {
        return ruled_out(); // Sokkia SDR, even when named .dc
    }
    let mut shaped = 0usize;
    let mut non_empty = 0usize;
    for line in &lines {
        if line.is_empty() {
            continue;
        }
        non_empty += 1;
        if has_record_shape(line) {
            shaped += 1;
        }
    }
    let all_shaped = non_empty > 0 && shaped == non_empty;
    let controller_header = header.starts_with(b"00NM") && header[4..].starts_with(b"SC V");
    if controller_header && all_shaped {
        return FormatSignature {
            confidence: if dc_extension { 0.97 } else { 0.92 },
            evidence: String::from(if dc_extension {
                "extension .dc, header 00NM naming Survey Controller, every record opens with a \
                 record type and derivation code"
            } else {
                "header 00NM naming Survey Controller, every record opens with a record type and \
                 derivation code"
            }),
        };
    }
    if header.starts_with(b"00NM") && all_shaped && dc_extension {
        return FormatSignature {
            confidence: 0.8,
            evidence: String::from(
                "extension .dc, header record 00NM, every record opens with a record type",
            ),
        };
    }
    if dc_extension && all_shaped {
        return FormatSignature {
            confidence: 0.5,
            evidence: String::from(
                "extension .dc and every record opens with a record type, but no 00NM header",
            ),
        };
    }
    if dc_extension {
        return FormatSignature {
            confidence: 0.1,
            evidence: String::from("extension .dc only"),
        };
    }
    ruled_out()
}

fn read_dc(bytes: &[u8], file_name: &str, _options: &ReadOptions) -> Result<ReadResult> {
    // Count record types, so the refusal says what the file holds.
    let mut types: BTreeMap<String, usize> = BTreeMap::new();
    let mut lines = 0usize;
    let mut start = 0;
    while start < bytes.len() {
        let end = bytes[start..]
            .iter()
            .position(|&c| c == b'\r' || c == b'\n')
            .map_or(bytes.len(), |at| start + at);
        let line = &bytes[start..end];
        if !line.is_empty() {
            lines += 1;
            // A record type is two ASCII characters, so it is text as it is.
            let kind = if has_record_shape(line) {
                String::from_utf8_lossy(&line[..2]).into_owned()
            } else {
                String::from("??")
            };
            *types.entry(kind).or_insert(0) += 1;
        }
        start = end + 1;
    }
    let mut context = format!("{lines} record(s):");
    // At most 48 types, as the C++'s `++listed > 48` lists them.
    for (listed, (kind, count)) in types.iter().enumerate() {
        if listed >= 48 {
            context.push_str(" ...");
            break;
        }
        context.push_str(&format!(" {kind} x{count}"));
    }
    Err(Error::new(
        ErrorCode::Unsupported,
        format!(
            "{file_name} is a Trimble Survey Controller DC file. Trimble has not published the \
             layout of its records, so Katana does not guess at them. Export the job as JobXML \
             (.jxl) from Trimble Access or Trimble Business Center and import that file instead."
        ),
    )
    .with_context(context))
}

fn trimble_dc_format() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(FORMAT_ID),
        human_name: String::from("Trimble Survey Controller DC (recognised only: export JobXML)"),
        manufacturer: Manufacturer::Trimble,
        // Nothing: the reader refuses every file, and says so before an import
        // is attempted through this empty list.
        reads: FormatContent::default(),
        // The registry has no "recognised but refused" state: a format must
        // import or export to be registered, and one that imports must have a
        // reader to refuse through. So can_import is true and the reader is
        // the refusal.
        can_import: true,
        can_export: false,
        parser_version: String::from("1.0"),
        extensions: vec![String::from("dc")],
    }
}

/// This format's line in the program's registry (format.rs).
pub(crate) fn registration() -> FormatRegistration {
    FormatRegistration::new(trimble_dc_format(), probe_dc, Some(read_dc))
}
