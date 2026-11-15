//! FORMATS (docs/interop.md, "Formats"), the port of
//! src/katana_app/geo/formats_verbs.hpp/.cpp: the formats this build of GDAL
//! reads and writes, from its own registry (gis's formats), and the options
//! each driver takes. The pieces the verb, the katana_formats tool and the
//! katana://formats resource share, so the three cannot come to differ. It
//! changes nothing, so it answers at prepare, as GDAL LIST does.
//!
//! ```text
//! FORMATS [RASTER|VECTOR] [READ|WRITE] [<text>...] [JSON]
//! FORMATS OPTIONS <driver> [JSON]
//! ```

use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_gis::formats::{find_format, format_options, formats};
use katana_gis::processing::versions;
use katana_gis::{DataKind, Format, FormatOption, FormatOptions};

use super::bindings::Tokens;
use super::geo_verbs::{Context, Prepared};
use super::replies::{joined, value};

const USAGE: &str =
    "FORMATS [RASTER|VECTOR] [READ|WRITE] [<text>...] [JSON] | FORMATS OPTIONS <driver> [JSON]";

/// What a format must do (the C++'s `FormatsQuery::Capability`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FormatsQueryCapability {
    #[default]
    Any,
    Read,
    Write,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormatsQuery {
    /// RASTER or VECTOR; both when absent.
    pub kind: Option<DataKind>,
    pub capability: FormatsQueryCapability,
    /// Words each of which the driver's name, description or an extension
    /// holds, ignoring case.
    pub text: String,
}

fn kinds(raster: bool, vector: bool) -> Vec<&'static str> {
    let mut out = Vec::new();
    if raster {
        out.push("raster");
    }
    if vector {
        out.push("vector");
    }
    out
}

/// "raster,vector", or "no" where it does neither: a record never has a
/// value a reader must know means nothing.
fn kinds_or_no(raster: bool, vector: bool) -> String {
    let text = joined(&kinds(raster, vector), ",");
    if text.is_empty() {
        String::from("no")
    } else {
        text
    }
}

fn capability_word(capability: FormatsQueryCapability) -> &'static str {
    match capability {
        FormatsQueryCapability::Read => "read",
        FormatsQueryCapability::Write => "write",
        FormatsQueryCapability::Any => "any",
    }
}

fn answered(reply: String) -> Prepared {
    Prepared {
        title: String::from("FORMATS"),
        reply: Some(reply),
        work: None,
    }
}

fn words_json(items: &[&str]) -> Json {
    Json::array_of(items.iter().map(|item| Json::from(*item)))
}

fn strings_json(items: &[String]) -> Json {
    Json::array_of(items.iter().map(Json::from))
}

fn options(tokens: &Tokens, at: usize) -> Result<Prepared> {
    let mut json = false;
    let mut driver = String::new();
    for (i, word) in tokens.words.iter().enumerate().skip(at) {
        if tokens.is(i, "JSON") {
            json = true;
        } else if driver.is_empty() {
            driver = word.clone();
        } else {
            // "ESRI Shapefile": a driver name of two words, unquoted.
            driver.push(' ');
            driver += word;
        }
    }
    if driver.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("FORMATS OPTIONS names a driver: {USAGE}"),
        ));
    }
    let Some(format) = find_format(&driver) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "this GDAL build has no format of that name (FORMATS lists them)",
            driver,
        ));
    };
    let found = format_options(&format.driver)?;
    if json {
        return Ok(answered(format_options_json(format, &found).dump()));
    }
    Ok(answered(format!(
        "{}\nlisted driver={} open={} creation={} layer_creation={}",
        options_records(format, &found),
        value(&format.driver),
        found.open.len(),
        found.creation.len(),
        found.layer_creation.len()
    )))
}

/// The formats a query takes, in the registry's order (by driver name).
pub fn choose_formats(query: &FormatsQuery) -> Vec<&'static Format> {
    let words: Vec<String> = query
        .text
        .to_ascii_lowercase()
        .split([' ', '\t'])
        .filter(|word| !word.is_empty())
        .map(String::from)
        .collect();
    let raster = query.kind.is_none_or(|kind| kind == DataKind::Raster);
    let vector = query.kind.is_none_or(|kind| kind == DataKind::Vector);
    let mut chosen = Vec::new();
    for format in formats() {
        let take = match query.capability {
            FormatsQueryCapability::Any => (raster && format.raster) || (vector && format.vector),
            FormatsQueryCapability::Read => {
                (raster && format.read_raster) || (vector && format.read_vector)
            }
            FormatsQueryCapability::Write => {
                (raster && format.write_raster) || (vector && format.write_vector)
            }
        };
        if !take {
            continue;
        }
        let haystack = format!(
            "{} {} {}",
            format.driver,
            format.description,
            joined(&format.extensions, ",")
        )
        .to_ascii_lowercase();
        if words.iter().all(|word| haystack.contains(word.as_str())) {
            chosen.push(format);
        }
    }
    chosen
}

/// ```text
/// format driver=GPKG kind=raster,vector read=raster,vector write=raster,vector
///        extensions=gpkg,gpkg.zip vsi=yes description=GeoPackage
/// ```
pub fn format_record(format: &Format) -> String {
    format!(
        "format driver={} kind={} read={} write={} extensions={} vsi={} description={}",
        value(&format.driver),
        kinds_or_no(format.raster, format.vector),
        kinds_or_no(format.read_raster, format.read_vector),
        kinds_or_no(format.write_raster, format.write_vector),
        value(&joined(&format.extensions, ",")),
        if format.virtual_io { "yes" } else { "no" },
        value(&format.description)
    )
}

/// `listed formats=12 kind=vector capability=write filter=`
pub fn formats_summary(query: &FormatsQuery, count: usize) -> String {
    let kind = match query.kind {
        None => "any",
        Some(DataKind::Raster) => "raster",
        Some(DataKind::Vector) => "vector",
    };
    format!(
        "listed formats={count} kind={kind} capability={} filter={} gdal={}",
        capability_word(query.capability),
        value(&query.text),
        value(&versions().gdal)
    )
}

/// `{driver, description, kinds:[...], read:[...], write:[...], extensions,
/// vsi, connection_prefix, help_url}`
pub fn format_json(format: &Format) -> Json {
    let text = |text: &str| {
        if text.is_empty() {
            Json::Null
        } else {
            Json::from(text)
        }
    };
    Json::object_of([
        ("driver", Json::from(&format.driver)),
        ("description", Json::from(&format.description)),
        ("kinds", words_json(&kinds(format.raster, format.vector))),
        (
            "read",
            words_json(&kinds(format.read_raster, format.read_vector)),
        ),
        (
            "write",
            words_json(&kinds(format.write_raster, format.write_vector)),
        ),
        ("extensions", strings_json(&format.extensions)),
        ("vsi", Json::from(format.virtual_io)),
        ("connection_prefix", text(&format.connection_prefix)),
        ("help_url", text(&format.help_url)),
    ])
}

/// `{name, type, description, default, scope, choices, min, max}`; absent
/// members are null.
pub fn format_option_json(option: &FormatOption) -> Json {
    let text = |text: &str| {
        if text.is_empty() {
            Json::Null
        } else {
            Json::from(text)
        }
    };
    let number = |number: Option<f64>| number.map_or(Json::Null, Json::from);
    Json::object_of([
        ("name", Json::from(&option.name)),
        ("type", text(&option.r#type)),
        ("description", text(&option.description)),
        ("default", text(&option.default_value)),
        ("scope", text(&option.scope)),
        ("choices", strings_json(&option.choices)),
        ("min", number(option.min)),
        ("max", number(option.max)),
    ])
}

/// ```text
/// option driver=GPKG list=open name=LIST_ALL_TABLES type=string-select
///        default=AUTO scope=vector choices=AUTO,YES,NO min= max= description="..."
/// ```
pub fn option_record(format: &Format, list: &str, option: &FormatOption) -> String {
    let bound = |number: Option<f64>| number.map(format_exact_real).unwrap_or_default();
    format!(
        "option driver={} list={list} name={} type={} default={} scope={} choices={} min={} \
         max={} description={}",
        value(&format.driver),
        value(&option.name),
        value(&option.r#type),
        value(&option.default_value),
        value(&option.scope),
        value(&joined(&option.choices, ",")),
        bound(option.min),
        bound(option.max),
        value(&option.description)
    )
}

/// The format's record, then its options' records: open, creation, layer
/// creation - FORMATS OPTIONS <driver> without its summary.
pub fn options_records(format: &Format, options: &FormatOptions) -> String {
    let mut text = format_record(format);
    for (list, entries) in [
        ("open", &options.open),
        ("creation", &options.creation),
        ("layer_creation", &options.layer_creation),
    ] {
        for option in entries {
            text.push('\n');
            text += &option_record(format, list, option);
        }
    }
    text
}

/// `{driver, open_options:[...], creation_options:[...],
/// layer_creation_options:[...]}`
pub fn format_options_json(format: &Format, options: &FormatOptions) -> Json {
    let list = |entries: &[FormatOption]| Json::array_of(entries.iter().map(format_option_json));
    Json::object_of([
        ("format", format_json(format)),
        ("open_options", list(&options.open)),
        ("creation_options", list(&options.creation)),
        ("layer_creation_options", list(&options.layer_creation)),
    ])
}

/// The records a line each, as the C++ joins them.
fn lines(records: &[String]) -> String {
    joined(records, "\n")
}

// ---- I2: the verb ---------------------------------------------------------------------------

/// The verb, its row in the verb table's I2 block (verb_table.rs).
pub fn prepare_formats(
    _context: &mut Context<'_>,
    tokens: &Tokens,
    _line: &str,
) -> Result<Prepared> {
    if tokens.size() > 1 && tokens.is(1, "OPTIONS") {
        return options(tokens, 2);
    }
    let mut query = FormatsQuery::default();
    let mut json = false;
    let mut text: Vec<String> = Vec::new();
    for (at, word) in tokens.words.iter().enumerate().skip(1) {
        if tokens.is(at, "JSON") {
            json = true;
        } else if tokens.is(at, "RASTER") || tokens.is(at, "VECTOR") {
            let kind = if tokens.is(at, "RASTER") {
                DataKind::Raster
            } else {
                DataKind::Vector
            };
            if query.kind.is_some_and(|given| given != kind) {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "FORMATS takes RASTER or VECTOR, not both; neither lists both: {USAGE}"
                    ),
                ));
            }
            query.kind = Some(kind);
        } else if tokens.is(at, "READ") || tokens.is(at, "WRITE") {
            let capability = if tokens.is(at, "READ") {
                FormatsQueryCapability::Read
            } else {
                FormatsQueryCapability::Write
            };
            if query.capability != FormatsQueryCapability::Any && query.capability != capability {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!("FORMATS takes READ or WRITE, not both: {USAGE}"),
                ));
            }
            query.capability = capability;
        } else {
            text.push(word.clone());
        }
    }
    query.text = joined(&text, " ");
    let chosen = choose_formats(&query);
    if json {
        let array = Json::array_of(chosen.iter().copied().map(format_json));
        return Ok(answered(array.dump()));
    }
    let mut records: Vec<String> = chosen.iter().copied().map(format_record).collect();
    records.push(formats_summary(&query, chosen.len()));
    Ok(answered(lines(&records)))
}
