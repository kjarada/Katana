//! The ONLINE verbs (include/katana/interop/online_verbs.hpp,
//! src/katana_interop/online_verbs.cpp; docs/gis_online.md, "Verbs"): what an
//! agent, a script or a person types to do everything GIS > Online Data does.
//!
//! ```text
//! ONLINE PROVIDERS [filter]
//! ONLINE LAYERS <provider> [search words, for a catalogue provider]
//! ONLINE INFO <provider> <layer>
//! ONLINE IMPORT <provider> <layer> area=view|drawing|selection|x0,y0,x1,y1|lonlat:w,s,e,n
//!               [res=<units per pixel>] [layer=<target layer>] [from=YYYY-MM-DD]
//!               [to=YYYY-MM-DD] [cloud=<percent>] [tag=<key[=value]>] [time=<date>]
//!               [crs=EPSG:<code>] [timeout=<seconds>]
//! ONLINE CUSTOM <url>
//! ONLINE KEY <name> <value>      (ONLINE KEY <name> alone removes it)
//! ```
//!
//! The grammar and the replies live here, beside the fetcher and away from
//! the window, so that they are tested as text and so that every front end
//! speaks them the same way.
//!
//! Replies are for machines first: one record per line, a word saying what
//! the record is, then key=value fields in a fixed order, a value quoted with
//! "" when it holds a space, a quote or an equals sign (the Logger's rule,
//! docs/architecture.md "Logging"), so `split on spaces outside quotes` reads
//! every line.

use katana_core::error::make_error;
use katana_core::text::{parse_finite_double, trimmed};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::EnumName;
use katana_gis::reproject::CrsBox;
use katana_gis::web_access::redact_url;
use katana_math::cpp;

/// The C++ `toString` overload set: `to_string(OnlineVerb::Import)`.
pub use katana_entity::entity::to_string;

use crate::online_catalogue::{LonLatBox, OnlineLayer, OnlineProvider, OnlineServiceType};
use crate::online_fetch::OnlineImport;
use crate::online_requests::{CkanResource, format_number};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OnlineVerb {
    #[default]
    Providers,
    Layers,
    Info,
    Import,
    Custom,
    Key,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OnlineAreaKind {
    #[default]
    View,
    Drawing,
    Selection,
    Box,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineCommand {
    pub verb: OnlineVerb,
    /// PROVIDERS; LAYERS' search words.
    pub filter: String,
    /// LAYERS, INFO, IMPORT.
    pub provider: String,
    /// INFO, IMPORT.
    pub layer: String,
    /// CUSTOM.
    pub url: String,
    /// KEY.
    pub key_name: String,
    /// KEY; empty removes.
    pub key_value: String,
    // IMPORT
    pub area: OnlineAreaKind,
    /// Box: the typed corners.
    pub r#box: CrsBox,
    /// lonlat: prefix - WGS 84 degrees, not the project's CRS.
    pub box_is_lon_lat: bool,
    pub resolution: Option<f64>,
    pub target_layer: String,
    pub from_date: String,
    pub to_date: String,
    pub max_cloud: Option<f64>,
    pub tag: String,
    pub time: String,
    pub crs: String,
    pub timeout_seconds: Option<i32>,
}

/// The C++ `toString(OnlineVerb)`.
impl EnumName for OnlineVerb {
    fn name(self) -> &'static str {
        match self {
            OnlineVerb::Providers => "PROVIDERS",
            OnlineVerb::Layers => "LAYERS",
            OnlineVerb::Info => "INFO",
            OnlineVerb::Import => "IMPORT",
            OnlineVerb::Custom => "CUSTOM",
            OnlineVerb::Key => "KEY",
        }
    }
}

/// Words separated by whitespace, a pair of double quotes keeping spaces in
/// one word ("layer=\"my layer\"" and "\"a b\"" alike) and removed.
fn words(line: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for character in line.chars() {
        if character == '"' {
            quoted = !quoted;
            any = true;
            continue;
        }
        if !quoted && (character == ' ' || character == '\t') {
            if any {
                out.push(word.clone());
            }
            word.clear();
            any = false;
            continue;
        }
        word.push(character);
        any = true;
    }
    if quoted {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a quoted value is never closed",
        ));
    }
    if any {
        out.push(word);
    }
    Ok(out)
}

fn valid_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if [0, 1, 2, 3, 5, 6, 8, 9]
        .iter()
        .any(|&i| !bytes[i].is_ascii_digit())
    {
        return false;
    }
    let two = |at: usize| u32::from(bytes[at] - b'0') * 10 + u32::from(bytes[at + 1] - b'0');
    let month = two(5);
    let day = two(8);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn number(text: &str) -> Option<f64> {
    parse_finite_double(trimmed(text.as_bytes()))
}

fn parse_box(text: &str) -> Result<CrsBox> {
    let not_four = || {
        make_error(
            ErrorCode::InvalidArgument,
            "the area is four numbers, x0,y0,x1,y1",
            text,
        )
    };
    let mut values: Vec<f64> = Vec::new();
    let mut start = 0;
    while start <= text.len() {
        let end = text[start..]
            .find(',')
            .map_or(text.len(), |comma| comma + start);
        let Some(value) = number(&text[start..end]) else {
            return Err(not_four());
        };
        values.push(value);
        start = end + 1;
        if end == text.len() {
            break;
        }
    }
    if values.len() != 4 {
        return Err(not_four());
    }
    // Corners in either order: x0,y0 and x1,y1 are two corners of the box.
    let corners = CrsBox {
        min_x: cpp::min(values[0], values[2]),
        min_y: cpp::min(values[1], values[3]),
        max_x: cpp::max(values[0], values[2]),
        max_y: cpp::max(values[1], values[3]),
    };
    if !corners.valid() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the area has no width or no height",
            text,
        ));
    }
    Ok(corners)
}

fn usage_of(verb: OnlineVerb) -> &'static str {
    match verb {
        OnlineVerb::Providers => "ONLINE PROVIDERS [filter]",
        OnlineVerb::Layers => "ONLINE LAYERS <provider> [search words]",
        OnlineVerb::Info => "ONLINE INFO <provider> <layer>",
        OnlineVerb::Import => {
            "ONLINE IMPORT <provider> <layer> area=view|drawing|selection|x0,y0,x1,y1|\
             lonlat:w,s,e,n [res=] [layer=] [from=YYYY-MM-DD] [to=YYYY-MM-DD] [cloud=] [tag=] \
             [time=] [crs=EPSG:<code>] [timeout=<seconds>]"
        }
        OnlineVerb::Custom => "ONLINE CUSTOM <url>",
        OnlineVerb::Key => "ONLINE KEY <name> [value]",
    }
}

fn field(name: &str, value: &str) -> String {
    format!(" {name}={}", reply_value(value))
}

fn coverage_text(lon_lat: &LonLatBox) -> String {
    format!(
        "{},{},{},{}",
        format_number(lon_lat[0]),
        format_number(lon_lat[1]),
        format_number(lon_lat[2]),
        format_number(lon_lat[3])
    )
}

/// The usage text, every verb.
pub fn online_usage() -> String {
    let mut text = String::new();
    for verb in [
        OnlineVerb::Providers,
        OnlineVerb::Layers,
        OnlineVerb::Info,
        OnlineVerb::Import,
        OnlineVerb::Custom,
        OnlineVerb::Key,
    ] {
        text.push_str("usage: ");
        text.push_str(usage_of(verb));
        text.push('\n');
    }
    text
}

/// A value as a reply field: quoted when it holds whitespace, '"', '=' or
/// '\', with '"' and '\' escaped.
pub fn reply_value(value: &str) -> String {
    let plain = !value.is_empty() && !value.contains([' ', '\t', '"', '=', '\\', '\n', '\r']);
    if plain {
        return String::from(value);
    }
    let mut out = String::from("\"");
    for character in value.chars() {
        if character == '"' || character == '\\' {
            out.push('\\');
        }
        out.push(if character == '\n' || character == '\r' {
            ' '
        } else {
            character
        });
    }
    out.push('"');
    out
}

/// Parses a line that starts with ONLINE (case-insensitive). InvalidArgument
/// with the usage of the verb for anything malformed: an unknown option, a
/// box that is not four numbers with min below max, a date that is not
/// YYYY-MM-DD, a cloud ceiling outside 0 to 100, a non-positive resolution.
/// Values may be quoted ("layer=\"my layer\"").
pub fn parse_online_command(line: &str) -> Result<OnlineCommand> {
    let parts = words(line)?;
    if parts.is_empty() || !parts[0].eq_ignore_ascii_case("ONLINE") {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "not an ONLINE command",
            line,
        ));
    }
    if parts.len() < 2 {
        return Err(Error::new(ErrorCode::InvalidArgument, online_usage()));
    }
    let verb = parts[1].to_ascii_uppercase();
    let rest = &parts[2..];
    let mut command = OnlineCommand::default();
    let refuse = |why: &str, which: OnlineVerb| {
        make_error(
            ErrorCode::InvalidArgument,
            why,
            format!("usage: {}", usage_of(which)),
        )
    };
    if verb == "PROVIDERS" {
        command.verb = OnlineVerb::Providers;
        command.filter = rest.join(" ");
        return Ok(command);
    }
    if verb == "LAYERS" {
        command.verb = OnlineVerb::Layers;
        let Some((provider, words)) = rest.split_first() else {
            return Err(refuse("which provider's layers?", command.verb));
        };
        command.provider.clone_from(provider);
        command.filter = words.join(" ");
        return Ok(command);
    }
    if verb == "INFO" {
        command.verb = OnlineVerb::Info;
        if rest.len() != 2 {
            return Err(refuse("INFO names a provider and a layer", command.verb));
        }
        command.provider.clone_from(&rest[0]);
        command.layer.clone_from(&rest[1]);
        return Ok(command);
    }
    if verb == "CUSTOM" {
        command.verb = OnlineVerb::Custom;
        if rest.len() != 1 {
            return Err(refuse("CUSTOM takes one URL", command.verb));
        }
        command.url.clone_from(&rest[0]);
        return Ok(command);
    }
    if verb == "KEY" {
        command.verb = OnlineVerb::Key;
        if rest.is_empty() || rest.len() > 2 {
            return Err(refuse("KEY takes a name and a value", command.verb));
        }
        command.key_name.clone_from(&rest[0]);
        command.key_value = rest.get(1).cloned().unwrap_or_default();
        return Ok(command);
    }
    if verb != "IMPORT" {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("unknown ONLINE verb {verb}"),
            online_usage(),
        ));
    }
    command.verb = OnlineVerb::Import;
    let mut positional: Vec<&String> = Vec::new();
    let mut saw_area = false;
    for word in rest {
        let Some(equals) = word.find('=') else {
            positional.push(word);
            continue;
        };
        let key = word[..equals].to_ascii_lowercase();
        let value = &word[equals + 1..];
        match key.as_str() {
            "area" => {
                saw_area = true;
                let folded = value.to_ascii_lowercase();
                if folded == "view" {
                    command.area = OnlineAreaKind::View;
                } else if folded == "drawing" || folded == "extents" {
                    command.area = OnlineAreaKind::Drawing;
                } else if folded == "selection" {
                    command.area = OnlineAreaKind::Selection;
                } else {
                    let mut corners = value;
                    for prefix in ["lonlat:", "wgs84:"] {
                        if folded.starts_with(prefix) {
                            command.box_is_lon_lat = true;
                            corners = &corners[prefix.len()..];
                        }
                    }
                    let parsed = parse_box(corners)?;
                    command.area = OnlineAreaKind::Box;
                    command.r#box = parsed;
                    if command.box_is_lon_lat
                        && (parsed.min_x < -180.0
                            || parsed.max_x > 180.0
                            || parsed.min_y < -90.0
                            || parsed.max_y > 90.0)
                    {
                        return Err(make_error(
                            ErrorCode::InvalidArgument,
                            "a lonlat: area is longitudes within -180..180 and latitudes within \
                             -90..90",
                            value,
                        ));
                    }
                }
            }
            "res" | "resolution" => {
                let Some(parsed) = number(value).filter(|&parsed| parsed > 0.0) else {
                    return Err(refuse(
                        "res is a positive number of project units per pixel",
                        command.verb,
                    ));
                };
                command.resolution = Some(parsed);
            }
            "layer" => {
                if value.is_empty() {
                    return Err(refuse("layer= needs a name", command.verb));
                }
                command.target_layer = String::from(value);
            }
            "from" | "to" => {
                if !valid_date(value) {
                    return Err(refuse(
                        &format!("{key} is a date, YYYY-MM-DD"),
                        command.verb,
                    ));
                }
                if key == "from" {
                    command.from_date = String::from(value);
                } else {
                    command.to_date = String::from(value);
                }
            }
            "cloud" => {
                let Some(parsed) = number(value).filter(|parsed| (0.0..=100.0).contains(parsed))
                else {
                    return Err(refuse("cloud is a percentage, 0 to 100", command.verb));
                };
                command.max_cloud = Some(parsed);
            }
            "tag" => command.tag = String::from(value),
            "time" => command.time = String::from(value),
            "crs" => command.crs = String::from(value),
            "timeout" => {
                let Some(parsed) = number(value).filter(|parsed| (1.0..=86400.0).contains(parsed))
                else {
                    return Err(refuse("timeout is seconds, 1 to 86400", command.verb));
                };
                command.timeout_seconds = Some(parsed as i32);
            }
            _ => {
                return Err(refuse(&format!("unknown option {key}="), command.verb));
            }
        }
    }
    if positional.len() != 2 {
        return Err(refuse("IMPORT names a provider and a layer", command.verb));
    }
    if !saw_area {
        return Err(refuse("IMPORT needs area=", command.verb));
    }
    if !command.from_date.is_empty()
        && !command.to_date.is_empty()
        && command.from_date > command.to_date
    {
        return Err(refuse("from= is after to=", command.verb));
    }
    command.provider.clone_from(positional[0]);
    command.layer.clone_from(positional[1]);
    Ok(command)
}

// ---- replies ------------------------------------------------------------------------------------

/// "provider id=... group=... layers=N title=..." per provider, after a
/// "providers count=N" line.
pub fn format_providers(providers: &[&OnlineProvider]) -> String {
    let mut text = format!("providers count={}\n", providers.len());
    for provider in providers {
        text.push_str("provider");
        text.push_str(&field("id", &provider.id));
        text.push_str(&field("group", &provider.group));
        text.push_str(&field("layers", &format!("{}", provider.layers().len())));
        text.push_str(&field(
            "custom",
            if provider.user_defined { "yes" } else { "no" },
        ));
        text.push_str(&field("title", &provider.title));
        text.push('\n');
    }
    text
}

/// "layer provider=... id=... kind=... service=... key=yes|no
/// verified=DATE|no title=..." per layer, after "layers provider=...
/// count=N".
pub fn format_layers(provider: &OnlineProvider) -> String {
    let layers = provider.layers();
    let mut text = format!(
        "layers{}{}\n",
        field("provider", &provider.id),
        field("count", &format!("{}", layers.len()))
    );
    for layer in layers {
        text.push_str("layer");
        text.push_str(&field("provider", &provider.id));
        text.push_str(&field("id", &layer.id));
        text.push_str(&field("kind", to_string(layer.kind)));
        text.push_str(&field("service", to_string(layer.r#type)));
        text.push_str(&field("key", if layer.key_required { "yes" } else { "no" }));
        text.push_str(&field(
            "verified",
            if layer.verified.is_empty() {
                "no"
            } else {
                &layer.verified
            },
        ));
        text.push_str(&field("licence", &layer.licence));
        text.push_str(&field("title", &layer.title));
        text.push('\n');
    }
    text
}

/// "info provider=... layer=..." then one "field name=value" line per fact,
/// in a fixed order: title, kind, service, endpoint (key redacted), layer,
/// crs, coverage, resolution, limits, licence, attribution, key, verified,
/// evidence.
pub fn format_info(layer: &OnlineLayer) -> String {
    let mut text = format!(
        "info{}{}\n",
        field("provider", &layer.provider_id),
        field("layer", &layer.id)
    );
    let mut line = |name: &str, value: &str| {
        if !value.is_empty() {
            text.push_str("field");
            text.push_str(&field("name", name));
            text.push_str(&field("value", value));
            text.push('\n');
        }
    };
    line("title", &layer.title);
    line("provider_title", &layer.provider_title);
    line("group", &layer.group);
    line("kind", to_string(layer.kind));
    line("service", to_string(layer.r#type));
    line("endpoint", &redact_url(&layer.endpoint));
    line("service_layer", &layer.layer_name);
    line("version", &layer.version);
    line("crs", &layer.crs);
    line("coverage", &coverage_text(&layer.coverage));
    line(
        "resolution",
        &layer.resolution.map_or_else(String::new, format_number),
    );
    match layer.r#type {
        OnlineServiceType::XyzTiles | OnlineServiceType::Wmts => {
            line("max_zoom", &format!("{}", layer.max_zoom));
            line("max_tiles", &format!("{}", layer.max_tiles));
        }
        OnlineServiceType::ArcgisQuery
        | OnlineServiceType::Wfs
        | OnlineServiceType::OgcApiFeatures => {
            line("page_size", &format!("{}", layer.page_size));
        }
        OnlineServiceType::ArcgisExport
        | OnlineServiceType::ArcgisExportImage
        | OnlineServiceType::Wms
        | OnlineServiceType::Wcs => {
            line(
                "max_request_pixels",
                &format!("{}", layer.max_request_pixels),
            );
        }
        OnlineServiceType::Overpass => {
            line("max_area_km2", &format_number(layer.max_area_km2));
            line("default_tag", &layer.layer_name);
        }
        OnlineServiceType::Stac => {
            line("asset", &layer.asset);
            line("max_cloud", &format_number(layer.max_cloud));
            line("days", &format!("{}", layer.days));
        }
        _ => {}
    }
    line("cache_days", &format!("{}", layer.cache_days));
    line("licence", &layer.licence);
    line("attribution", &layer.attribution);
    line(
        "key",
        &if layer.key_required {
            format!("yes ({})", layer.key_name)
        } else {
            String::from("no")
        },
    );
    line(
        "verified",
        if layer.verified.is_empty() {
            "no"
        } else {
            &layer.verified
        },
    );
    line("evidence", &layer.evidence);
    line("homepage", &layer.homepage);
    text
}

/// The outcome of an import: "imported provider=... layer=... kind=..." with
/// what landed (entities and target layer, or raster size and resolution),
/// the requests, cache hits, pages and repeats dropped, and the licence,
/// attribution and source; each warning on its own "warning text=..." line.
pub fn format_import(
    layer: &OnlineLayer,
    result: &OnlineImport,
    entities_added: usize,
    target_layer: &str,
) -> String {
    let mut text = format!(
        "imported{}{}{}",
        field("provider", &layer.provider_id),
        field("layer", &layer.id),
        field("kind", to_string(result.kind))
    );
    if let Some(raster) = &result.raster {
        text.push_str(&field("width", &format!("{}", raster.width)));
        text.push_str(&field("height", &format!("{}", raster.height)));
        text.push_str(&field("resolution", &format_number(result.resolution)));
        text.push_str(&field("reference", &format!("{}", raster.id)));
        text.push_str(&field("name", &raster.name));
    } else {
        text.push_str(&field("entities", &format!("{entities_added}")));
        text.push_str(&field("target", target_layer));
    }
    let stats = &result.stats;
    text.push_str(&field("requests", &format!("{}", stats.requests)));
    text.push_str(&field("cache_hits", &format!("{}", stats.cache_hits)));
    text.push_str(&field(
        "remote_sources",
        &format!("{}", stats.remote_sources),
    ));
    text.push_str(&field("pages", &format!("{}", stats.pages)));
    text.push_str(&field("duplicates", &format!("{}", stats.duplicates)));
    text.push_str(&field(
        "cached_result",
        if stats.product_from_cache {
            "yes"
        } else {
            "no"
        },
    ));
    text.push_str(&field("retrieved", &result.retrieved));
    text.push_str(&field("licence", &result.licence));
    text.push_str(&field("attribution", &result.attribution));
    text.push_str(&field("source", &result.source_url));
    text.push('\n');
    for warning in &result.warnings {
        text.push_str("warning");
        text.push_str(&field("text", warning));
        text.push('\n');
    }
    text
}

/// "error verb=... code=... message=... context=...", the one shape every
/// refusal takes.
pub fn format_error(verb: OnlineVerb, error: &Error) -> String {
    let mut text = format!(
        "error{}{}{}",
        field("verb", to_string(verb)),
        field("code", katana_core::error::to_string(error.code)),
        field("message", &error.message)
    );
    if !error.context.is_empty() {
        text.push_str(&field("context", &error.context));
    }
    text.push('\n');
    text
}

/// Catalogue search results (ONLINE LAYERS data-gov-au <words>).
pub fn format_catalogue_search(resources: &[CkanResource]) -> String {
    let mut text = format!("services count={}\n", resources.len());
    for resource in resources {
        text.push_str("service");
        text.push_str(&field("format", &resource.format));
        text.push_str(&field("url", &resource.url));
        text.push_str(&field("licence", &resource.licence));
        text.push_str(&field("dataset", &resource.dataset));
        text.push_str(&field("title", &resource.title));
        text.push('\n');
    }
    text
}
