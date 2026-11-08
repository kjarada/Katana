//! Leica DBX jobs: recognised and refused, with the export to make instead.
//!
//! DBX is the job database of Leica's field software (System 1200, Viva,
//! Captivate): a folder holding an index file, <job>.xcf, and data files
//! <job>.x01, <job>.x02 ... Leica publishes no specification of it. The
//! open-source Total Open Station project records it as "a binary, undocumented
//! file format" (github.com/totalopenstation/totalopenstation issue 38, which is
//! also where the .xcf index / .x01, .x02 file set is described), and Leica's
//! own route out of a job is an export: GSI, LandXML, or Leica's XML (HeXML).
//!
//! So nothing here reads a DBX value. A reader that guessed at an undocumented
//! binary layout would produce coordinates that look right and are not, which
//! is worse than no reader. What it does is recognise the job - its files by
//! name, confirmed through the sibling lookup, since a job is several files -
//! and say plainly what to export and import instead. It is registered WITH a
//! reader, one that always refuses, so that the import wizard (which offers the
//! column layout page to any format without a reader) gives this sentence rather
//! than a page of columns for a binary file.

use katana_core::{Error, ErrorCode, Result};

use crate::format::{
    FormatContent, FormatDescriptor, FormatRegistration, FormatSignature, Manufacturer, ProbeInput,
    ReadOptions, ReadResult, ruled_out,
};
use crate::leica::LEICA_DBX_FORMAT_ID;

const PARSER_VERSION: &str = "1.0";

const REFUSAL: &str = "Leica DBX is Leica's internal job database and has no published format; export the job \
     from Captivate/Infinity as GSI-16 or LandXML (or HeXML) and import that.";

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

// "x01" .. "x99": a DBX job's data files.
fn is_data_extension(extension: &str) -> bool {
    let bytes = extension.as_bytes();
    bytes.len() == 3
        && bytes[0] == b'x'
        && is_digit(bytes[1])
        && is_digit(bytes[2])
        && extension != "x00"
}

// Binary content: a NUL, or a large share of bytes that no text file holds.
fn looks_binary(bytes: &[u8]) -> bool {
    let mut control = 0usize;
    let sample = &bytes[..bytes.len().min(4096)];
    for &byte in sample {
        if byte == 0 {
            return true;
        }
        if byte < 0x09 || (byte > 0x0D && byte < 0x20) {
            control += 1;
        }
    }
    !sample.is_empty() && control * 10 > sample.len()
}

fn dbx_format() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(LEICA_DBX_FORMAT_ID),
        // The name says what happens, because describe_format() will say
        // "import: yes" - the registry lets only an importable format carry
        // the reader that gives the refusal.
        human_name: String::from("Leica DBX job (recognised; export GSI or LandXML to import)"),
        manufacturer: Manufacturer::Leica,
        // nothing: see the module comment
        reads: FormatContent::default(),
        can_import: true,
        can_export: false,
        parser_version: String::from(PARSER_VERSION),
        extensions: vec![String::from("xcf")],
    }
}

fn probe_dbx(input: &ProbeInput<'_>) -> FormatSignature {
    let index = input.extension == "xcf";
    let data = is_data_extension(&input.extension);
    if !index && !data {
        return ruled_out();
    }
    // GIMP's image format shares the extension and names itself.
    if input.bytes.starts_with(b"gimp xcf") {
        return ruled_out();
    }
    if !looks_binary(input.bytes) {
        // A text file called .xcf / .x01 is not a DBX job file, but the name
        // alone is some evidence.
        return FormatSignature {
            confidence: 0.3,
            evidence: format!(
                "extension .{}, but the content is text, which a DBX job file is not",
                input.extension
            ),
        };
    }
    if index {
        return FormatSignature {
            confidence: 0.9,
            evidence: String::from(
                "extension .xcf, binary, not a GIMP image: the index file of a Leica DBX job",
            ),
        };
    }
    FormatSignature {
        confidence: 0.75,
        evidence: format!(
            "extension .{}, binary: a data file of a Leica DBX job",
            input.extension
        ),
    }
}

// The job's other files, by name: the index names the job, the data files
// share its stem. Only whether they are there is looked at.
fn read_dbx(_bytes: &[u8], file_name: &str, options: &ReadOptions) -> Result<ReadResult> {
    let dot = file_name.rfind('.');
    let stem = match dot {
        Some(dot) => &file_name[..dot],
        None => file_name,
    };
    let mut found = String::new();
    if let (Some(siblings), Some(dot)) = (&options.siblings, dot) {
        let name = file_name.as_bytes();
        let is_index = name.len() - dot == 4
            && (name[dot + 1] == b'x' || name[dot + 1] == b'X')
            && (name[dot + 2] == b'c' || name[dot + 2] == b'C');
        let other = format!("{stem}{}", if is_index { ".x01" } else { ".xcf" });
        if siblings(other.as_str()).is_ok() {
            found = format!("{file_name} with {other} beside it");
        }
    }
    let mut message = if found.is_empty() {
        format!("{file_name} looks like a file of a Leica DBX job. ")
    } else {
        format!("This is a Leica DBX job ({found}). ")
    };
    message.push_str(REFUSAL);
    Err(Error::new(ErrorCode::Unsupported, message).with_context(file_name))
}

pub(crate) fn registration() -> FormatRegistration {
    FormatRegistration::new(dbx_format(), probe_dbx, Some(read_dbx))
}
