//! A RINEX broadcast navigation file, summarised: which systems it has orbits
//! for and how many, whether it carries ionosphere parameters and the leap
//! seconds. The orbits themselves are not kept - nothing in the survey model
//! uses them until a positioning step does (see rinex.rs for why there is none
//! yet) - but the file is read through, so a navigation file that is not one,
//! or is damaged, is said to be so.
//!
//! Specifications: "RINEX: The Receiver Independent Exchange Format, Version
//! 2.11" (W. Gurtner, L. Estey, 2007), tables A3 and A4 (GPS navigation), A10
//! and A11 (GLONASS), A15 and A16 (GEO); "RINEX Version 3.05" (IGS/RTCM RINEX
//! WG, I. Romero ed., 1 December 2020), tables A5 to A18; "RINEX Version 4.00"
//! (IGS/RTCM RINEX WG, 1 December 2021), section 5.4 and tables A7 on: from
//! version 4 every navigation record begins with a "> EPH", "> STO", "> EOP" or
//! "> ION" line.

use katana_core::{Error, ErrorCode, Result};

use crate::format::ReadWarning;
use crate::rinex_internal::{
    LineCursor, NavigationSummary, Packing, clean_text, header_content, header_label, is_digit,
    packing_of, packing_refusal, system_name, tokens, version_record,
};

// A damaged navigation file can produce a warning per line; past this many
// the rest are counted in one closing warning instead of listed.
const MAX_NAVIGATION_WARNINGS: usize = 50;

fn is_digit_or_blank(c: u8) -> bool {
    is_digit(c) || c == b' '
}

// The system a RINEX 2 navigation file holds all its records for, from its
// file type: one file per system in version 2.
fn version_two_system(file_type: u8) -> Option<&'static str> {
    match file_type {
        b'N' => Some("GPS"),
        b'G' => Some("GLONASS"),
        b'H' => Some("SBAS"),
        b'E' | b'L' => Some("Galileo"),
        _ => None,
    }
}

fn warn(summary: &mut NavigationSummary, suppressed: &mut usize, record: usize, message: String) {
    if summary.warnings.len() < MAX_NAVIGATION_WARNINGS {
        summary.warnings.push(ReadWarning {
            file_name: summary.file_name.clone(),
            record,
            message,
        });
    } else {
        *suppressed += 1;
    }
}

fn count(summary: &mut NavigationSummary, system: &str) {
    *summary.ephemerides.entry(String::from(system)).or_insert(0) += 1;
}

/// FileImportFailure (naming the file) when `bytes` is not a RINEX navigation
/// file at all; otherwise the summary, with a warning per unreadable record.
pub fn summarise_navigation(bytes: &[u8], file_name: &str) -> Result<NavigationSummary> {
    let name = String::from(file_name);
    let packing = packing_of(bytes);
    if packing != Packing::Plain {
        return Err(Error::new(
            ErrorCode::FileImportFailure,
            packing_refusal(packing, file_name),
        ));
    }
    let mut cursor = LineCursor::new(bytes);
    let Some(line) = cursor.next() else {
        return Err(Error::new(
            ErrorCode::FileImportFailure,
            format!("{name} is empty"),
        ));
    };
    let Some(version) = version_record(line) else {
        return Err(Error::new(
            ErrorCode::FileImportFailure,
            format!(
                "{name} is not a RINEX file: its first line is not a RINEX VERSION / TYPE record"
            ),
        ));
    };
    let version_two = version.major <= 2;
    let single_system = if version_two {
        version_two_system(version.file_type)
    } else {
        None
    };
    let not_navigation = if version_two {
        single_system.is_none()
    } else {
        version.file_type != b'N'
    };
    if not_navigation {
        return Err(Error::new(
            ErrorCode::FileImportFailure,
            format!(
                "{name} is a RINEX {} '{}' file, not a navigation file",
                version.version_text, version.type_text
            ),
        ));
    }

    let mut summary = NavigationSummary {
        file_name: name.clone(),
        version_text: version.version_text.clone(),
        ..Default::default()
    };
    let mut suppressed = 0usize;

    let mut header_ended = false;
    while let Some(line) = cursor.next() {
        let label = header_label(line);
        if label == b"END OF HEADER" {
            header_ended = true;
            break;
        }
        if label == b"ION ALPHA" || label == b"ION BETA" || label == b"IONOSPHERIC CORR" {
            summary.ionosphere_parameters = true;
        } else if label == b"LEAP SECONDS" {
            summary.leap_seconds = clean_text(header_content(line), None);
        }
    }
    if !header_ended {
        return Err(Error::new(
            ErrorCode::FileImportFailure,
            format!(
                "{name} has no END OF HEADER record: the navigation file is cut short or is not \
                 RINEX"
            ),
        ));
    }

    while let Some(line) = cursor.next() {
        let number = cursor.line_number();
        if line.iter().all(|&c| c == b' ') {
            continue; // a blank line at the end of a file is common and harmless
        }
        if version.major >= 4 {
            if line[0] != b'>' {
                continue; // an orbit line of the record above
            }
            let fields = tokens(&line[1..]);
            if fields.len() >= 2
                && fields[0] == b"EPH"
                && let Some(system) = fields[1].first().copied().and_then(system_name)
            {
                count(&mut summary, system);
            } else if !fields.is_empty()
                && (fields[0] == b"STO" || fields[0] == b"EOP" || fields[0] == b"ION")
            {
                summary.other_messages += 1;
                if fields[0] == b"ION" {
                    summary.ionosphere_parameters = true;
                }
            } else {
                let shown = clean_text(&line[..line.len().min(40)], None);
                warn(
                    &mut summary,
                    &mut suppressed,
                    number,
                    format!("not a RINEX 4 navigation record: '{shown}'"),
                );
            }
            continue;
        }
        if let Some(system) = single_system {
            // Table A4: a record starts with the PRN in columns 1-2; its
            // broadcast-orbit lines start with three blanks.
            if line.starts_with(b"   ") {
                continue;
            }
            if line.len() >= 2 && is_digit_or_blank(line[0]) && is_digit(line[1]) {
                count(&mut summary, system);
            } else {
                warn(
                    &mut summary,
                    &mut suppressed,
                    number,
                    String::from("neither the start of an ephemeris nor one of its orbit lines"),
                );
            }
            continue;
        }
        // Version 3: a record starts with the satellite (A1,I2.2); its orbit
        // lines start with four blanks.
        if line.starts_with(b"    ") {
            continue;
        }
        let system = if line.len() >= 3 && is_digit_or_blank(line[1]) && is_digit(line[2]) {
            system_name(line[0])
        } else {
            None
        };
        match system {
            Some(system) => count(&mut summary, system),
            None => warn(
                &mut summary,
                &mut suppressed,
                number,
                String::from("neither the start of an ephemeris nor one of its orbit lines"),
            ),
        }
    }
    summary.lines = cursor.line_number();
    if suppressed > 0 {
        summary.warnings.push(ReadWarning {
            file_name: name,
            record: 0,
            message: format!("{suppressed} more problems like these are not listed"),
        });
    }
    Ok(summary)
}
