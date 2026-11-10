//! The reply records of the geoprocessing verbs (docs/geoprocessing.md,
//! "Replies"), the port of src/katana_app/geo/replies.hpp/.cpp: one record per
//! line, a word saying what it is and then key=value fields in a fixed order, a
//! value quoted when it holds a blank, a quote or an '=' (cad's record_value).
//! One definition, so HELP, LIST, a run's reply and the MCP tools that read
//! them back cannot come to differ.

use katana_core::text::{format_exact_real, format_fixed, parse_integer, split_lines};
use katana_gis::processing::{
    self as gp, AlgorithmInfo, ArgSpec, ArgType, DatasetKind, Versions, dataset_kinds_text,
};

use crate::import_records::record_text;

/// Items joined with `separator` as the C++ joins them,
/// `text += (text.empty() ? "" : separator) + item`: a separator only after
/// some text, so an empty item at the start leaves none behind it.
pub(crate) fn joined<S: AsRef<str>>(items: &[S], separator: &str) -> String {
    let mut text = String::new();
    for item in items {
        if !text.is_empty() {
            text += separator;
        }
        text += item.as_ref();
    }
    text
}

fn yes_no(flag: bool) -> &'static str {
    if flag { "yes" } else { "no" }
}

/// cad's record_value: plain, or quoted and escaped; nothing for an empty
/// value. Nothing at all for nothing ("aliases="), so an absent value is not a
/// pair of quotes a reader has to recognise - the rule import_records'
/// record_text keeps, so it is that one function.
pub fn value(text: &str) -> String {
    record_text(text)
}

/// A length or a time to 3 decimals, locale-independent: "0.017".
pub fn fixed3(number: f64) -> String {
    // The one formatter (core's format_fixed): any finite double, and no sign
    // on a value that rounds to nothing - a difference of -1e-7 m printed
    // zmin=-0.000, a sign a reader of the record would take as meant.
    format_fixed(number, 3)
}

/// The words a line reads as its own where a name may stand, compared lower
/// case: a line reads its words in any case.
const LINE_KEYWORDS: [&str; 38] = [
    // Clauses and flags.
    "to",
    "from",
    "preview",
    "confirm",
    "overwrite",
    "replace",
    "append",
    "convex",
    "holes",
    "observers",
    "only",
    "extents",
    "drawn",
    // Words that take a value.
    "name",
    "save",
    "format",
    "layer",
    "layers",
    "with",
    "by",
    "minus",
    "at",
    "as",
    "observer",
    "target",
    "cell",
    "where",
    // Sources, targets and scopes.
    "file",
    "raster",
    "surface",
    "reference",
    "report",
    "selection",
    "sel",
    "drawing",
    "all",
    "view",
    "area",
];

/// Whether a line reads `word` - in any case, unquoted - as one of its own
/// words where a name may stand: the clause words (TO, FROM, PREVIEW,
/// CONFIRM, OVERWRITE), the flags (REPLACE ...), the words that take a value
/// (NAME, FORMAT, LAYER, WITH, BY, MINUS, AT ...) and the source and scope
/// words (FILE, RASTER, SURFACE, REFERENCE, DRAWING, WHERE ...).
pub fn line_keyword(word: &str) -> bool {
    let folded = word.to_ascii_lowercase();
    LINE_KEYWORDS.contains(&folded.as_str())
}

/// A name or a path as a word of a line: as it is, or quoted when it holds a
/// blank, is empty, or is a line_keyword - a quoted word is always a value, so
/// TO LAYER "preview" draws on the layer `preview` where TO LAYER preview was
/// refused and TO REFERENCE preview previewed. None for a double quote or a
/// line break, which a line has no way to carry. The ONE quoting rule of the
/// window's dialogs and the MCP tools that write lines.
pub fn line_word(text: &str) -> Option<String> {
    if text.contains(['"', '\r', '\n']) {
        return None;
    }
    let quote = text.is_empty() || text.contains([' ', '\t']) || line_keyword(text);
    Some(if quote {
        format!("\"{text}\"")
    } else {
        String::from(text)
    })
}

/// `gdal algorithm="raster hillshade" policy=safe seconds=0.017 cancelled=no`
pub fn gdal_record(info: &AlgorithmInfo, seconds: f64) -> String {
    format!(
        "gdal algorithm={} policy={} seconds={} cancelled=no",
        value(&gp::path_text(&info.path)),
        gp::to_string(info.policy),
        fixed3(seconds)
    )
}

/// `algorithm path="raster hillshade" policy=safe aliases= description="..."`
pub fn algorithm_record(info: &AlgorithmInfo) -> String {
    format!(
        "algorithm path={} policy={} aliases={} description={}",
        value(&gp::path_text(&info.path)),
        gp::to_string(info.policy),
        value(&joined(&info.aliases, ",")),
        value(&info.description)
    )
}

/// `group path="raster" algorithms=47`
pub fn group_record(info: &AlgorithmInfo, algorithms: usize) -> String {
    format!(
        "group path={} algorithms={algorithms} description={}",
        value(&gp::path_text(&info.path)),
        value(&info.description)
    )
}

/// The ways a dataset argument may be given: "name", "object", both.
fn accepts(arg: &ArgSpec) -> Vec<&'static str> {
    let mut ways = Vec::new();
    if arg.accepts_name {
        ways.push("name");
    }
    if arg.accepts_object {
        ways.push("object");
    }
    ways
}

/// ```text
/// arg name=zfactor short=z aliases= type=real required=no positional=no
///     category=Base default=1 min=0 min_inclusive=no max= max_inclusive=
///     choices= count=0..1 dataset= accepts= description="..."
/// ```
pub fn arg_record(arg: &ArgSpec) -> String {
    let default = arg
        .default_value
        .as_ref()
        .map(gp::to_string)
        .unwrap_or_default();
    let mut record = format!(
        "arg name={} short={} aliases={} type={} required={} positional={} category={} default={}",
        value(&arg.name),
        value(&arg.short_name),
        value(&joined(&arg.aliases, ",")),
        gp::to_string(arg.r#type),
        yes_no(arg.required),
        yes_no(arg.positional),
        value(&arg.category),
        value(&default)
    );
    record += &format!(
        " min={} min_inclusive={}",
        arg.min
            .map(|bound| format_exact_real(bound.value))
            .unwrap_or_default(),
        arg.min.map(|bound| yes_no(bound.inclusive)).unwrap_or("")
    );
    record += &format!(
        " max={} max_inclusive={}",
        arg.max
            .map(|bound| format_exact_real(bound.value))
            .unwrap_or_default(),
        arg.max.map(|bound| yes_no(bound.inclusive)).unwrap_or("")
    );
    record += &format!(" choices={}", value(&joined(&arg.choices, ",")));
    record += &format!(
        " count={}..{}",
        arg.min_count,
        if arg.max_count < 0 {
            String::from("*")
        } else {
            format!("{}", arg.max_count)
        }
    );
    record += &format!(
        " dataset={} accepts={} input={} output={} description={}",
        dataset_kinds_text(arg.dataset_kinds),
        joined(&accepts(arg), ","),
        yes_no(arg.is_input),
        yes_no(arg.is_output),
        value(&arg.description)
    );
    record
}

/// The Katana sources a dataset argument can be bound from: drawing (a
/// vector), raster and surface (a raster), and file (either).
pub fn sources_for(arg: &ArgSpec) -> String {
    let mut sources = Vec::new();
    if (arg.dataset_kinds & DatasetKind::Vector as u32) != 0 {
        sources.push("drawing");
    }
    if (arg.dataset_kinds & DatasetKind::Raster as u32) != 0 {
        sources.push("raster");
        sources.push("surface");
    }
    sources.push("file");
    joined(&sources, ",")
}

/// `binding arg=input kinds=raster accepts=name,object sources=raster,surface,file`
pub fn binding_record(arg: &ArgSpec) -> String {
    format!(
        "binding arg={} kinds={} accepts={} sources={} list={} required={}",
        value(&arg.name),
        dataset_kinds_text(arg.dataset_kinds),
        joined(&accepts(arg), ","),
        sources_for(arg),
        yes_no(arg.r#type == ArgType::DatasetList),
        yes_no(arg.required)
    )
}

/// ```text
/// gdal version=3.13.2 release="Iowa City" proj=9.7.1 geos=3.14.1
///      raster_drivers=148 vector_drivers=82 algorithms=121
/// ```
pub fn version_record(versions: &Versions) -> String {
    format!(
        "gdal version={} release={} proj={} geos={} raster_drivers={} vector_drivers={} algorithms={}",
        value(&versions.gdal),
        value(&versions.release),
        value(&versions.proj),
        value(&versions.geos),
        versions.raster_drivers,
        versions.vector_drivers,
        versions.algorithms
    )
}

/// `text lines=<n>`, then the n lines verbatim.
pub fn text_record(text: &str) -> String {
    let body = text.trim_end_matches(['\n', '\r']);
    let lines = split_lines(body.as_bytes());
    let mut record = format!("text lines={}", lines.len());
    for line in lines {
        record.push('\n');
        // Every cut is at a line break, an ASCII byte of UTF-8 text.
        record += &String::from_utf8_lossy(line);
    }
    record
}

/// `warning text="..."`
pub fn warning_record(text: &str) -> String {
    format!("warning text={}", value(text))
}

/// A record read back: what katana_gdal_run hands an agent as structured
/// data, from the very text the command line prints.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub kind: String,
    pub fields: Vec<(String, String)>,
    /// A text record's lines.
    pub body: Vec<String>,
}

impl Record {
    /// The first field called `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, text)| text.as_str())
    }
}

/// `std::string::npos`, for the C++'s finds.
const NPOS: usize = usize::MAX;

/// The first `byte` in `line` at or after `from`; NPOS for none.
fn find_byte(line: &[u8], byte: u8, from: usize) -> usize {
    line.get(from..)
        .and_then(|rest| rest.iter().position(|&each| each == byte))
        .map_or(NPOS, |at| from + at)
}

/// Bytes of a line, cut at ASCII bytes, as text.
fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The records of a reply, a text record with the lines it says it has. A
/// record's kind is every word before its first key= ("ifc exported"); lines
/// that are not records (words with no fields) are records of that kind with
/// no fields.
pub fn parse_records(text: &str) -> Vec<Record> {
    let lines = split_lines(text.as_bytes());
    let mut records = Vec::new();
    let mut n = 0;
    while n < lines.len() {
        let line = lines[n];
        let mut record = Record::default();
        // The kind is every word before the first key=: IFC's records have
        // two ("ifc exported file=..."), and reading one word made the second
        // part of the first key ("exported file"). A bare word after the
        // fields is a field with no value, not glued to the next key.
        let mut i = 0;
        let mut fields_begun = false;
        while i < line.len() {
            while i < line.len() && line[i] == b' ' {
                i += 1;
            }
            if i >= line.len() {
                break;
            }
            let equals = find_byte(line, b'=', i);
            let space = find_byte(line, b' ', i);
            if equals == NPOS || (space != NPOS && space < equals) {
                let end = if space == NPOS { line.len() } else { space };
                let word = text_of(&line[i..end]);
                if fields_begun {
                    record.fields.push((word, String::new()));
                } else {
                    if !record.kind.is_empty() {
                        record.kind.push(' ');
                    }
                    record.kind += &word;
                }
                i = space;
                continue;
            }
            fields_begun = true;
            let key = text_of(&line[i..equals]);
            let mut field = Vec::new();
            i = equals + 1;
            if i < line.len() && line[i] == b'"' {
                // record_value's quoting: a quote and a backslash escaped with
                // a backslash.
                i += 1;
                while i < line.len() && line[i] != b'"' {
                    if line[i] == b'\\' && i + 1 < line.len() {
                        i += 1;
                    }
                    field.push(line[i]);
                    i += 1;
                }
                i += 1;
            } else {
                let end = find_byte(line, b' ', i);
                field.extend_from_slice(&line[i..if end == NPOS { line.len() } else { end }]);
                i = end;
            }
            record.fields.push((key, text_of(&field)));
        }
        if record.kind == "text" {
            let count = parse_integer(record.get("lines").unwrap_or("0"));
            if let Some(count) = count {
                let mut k = 0;
                while k < count && n + 1 < lines.len() {
                    n += 1;
                    record.body.push(text_of(lines[n]));
                    k += 1;
                }
            }
        }
        records.push(record);
        n += 1;
    }
    records
}
