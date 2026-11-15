//! The RASTER SHADE verb (docs/terrain.md, "Shading"), the port of
//! src/katana_app/geo/shade_verbs.cpp: an elevation source drawn as a
//! picture - hillshade, colour relief, relief over hillshade, slope shading,
//! or the band as it is - kept as a derived reference raster the plan view and
//! the sheet painter draw, the same on katana_cli, katana_mcp and the window's
//! command line, where Terrain > Analysis > Terrain Shading builds these
//! lines.
//!
//! ```text
//! RASTER SHADE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
//!              [style=hillshade|relief|relief+hillshade|slope|plain]
//!              [azimuth=315] [altitude=45] [z=1] [variant=regular|combined|multidirectional|igor]
//!              [ramp=terrain|earth|diverging|slope|grey|spectral|<file>] [range=<min>,<max>]
//!              [NAME <n>] [save=<file.tif>] [OVERWRITE] [PREVIEW]
//! ```
//!
//! The picture is made by GDAL's own algorithms, step by step through a
//! RasterChain: hillshade, color-map (the ramp spread over the range, written
//! as GDAL's colour-map text), blend --operator hsv-value (relief over
//! hillshade: the relief's hue and saturation, the hillshade's value) and
//! slope. It is made at display resolution: a raster longer than the display
//! copy a reference raster keeps (4096 pixels) is averaged down to it first,
//! since a finer picture would only be decimated again to be drawn. Every
//! style ends as an RGBA picture, so it is drawn exactly as rendered - a
//! single grey band would be stretched over its own range by the reader.
//!
//! Its `z=` is read by terrain_verbs' positive_option ("z is greater than
//! zero"), the one the C++ call resolves to: shade_verbs.cpp does not see
//! dem_support.hpp.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Keep the C++'s negated comparisons, which refuse NaN."
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use katana_core::error::make_error;
use katana_core::text::{parse_finite_double, parse_integer};
use katana_core::{Error, ErrorCode, Result};
use katana_gis::processing::{ArgValue, DatasetValue, RunOutputs};
use katana_interop::geo::LegendEntry;
use katana_interop::geo::colour_ramps::{
    built_in_ramp, built_in_ramp_names, colour_map_text, read_colour_map, spread,
};
use katana_interop::geo::drawing_dataset::ResultOptions;
use katana_interop::{RasterDisplayStyle, RasterImportOptions, ReferenceId};
use katana_math::cpp;

use super::bindings::{ApplyRequest, Target, TargetKind, Tokens, apply_outputs};
use super::geo_verbs::{
    Context, Prepared, Progress, make_apply, make_work, system_message, trimmed_text,
};
use super::replies::{fixed3, parse_records, value, warning_record};
use super::terrain_verbs::{
    RasterChain, answered_with, bind_terrain_source, gdal_number, joined_records, key_value,
    number_option, positive_option, source_name, value_range,
};
use crate::import_records::{path_from_text, path_text};

const USAGE: &str = "RASTER SHADE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path> \
                     [style=hillshade|relief|relief+hillshade|slope|plain] [azimuth=315] \
                     [altitude=45] [z=1] [variant=regular|combined|multidirectional|igor] \
                     [ramp=terrain|earth|diverging|slope|grey|spectral|<file>] \
                     [range=<min>,<max>] [NAME <n>] [save=<file.tif>] [OVERWRITE] [PREVIEW]";

fn refusal(why: impl Into<String>, word: impl Into<String>) -> Error {
    make_error(ErrorCode::InvalidArgument, why, word)
}

/// An algorithm's path, as the bridge takes it.
fn algorithm(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

/// The built-in ramps as a phrase for a refusal - "terrain, earth, diverging,
/// slope, grey or spectral" - read from the one table, so a ramp added to it
/// is named here without anyone remembering to.
fn ramp_words() -> String {
    let names = built_in_ramp_names();
    let mut words = String::new();
    for (i, name) in names.iter().enumerate() {
        words += name;
        if i + 2 < names.len() {
            words += ", ";
        } else if i + 2 == names.len() {
            words += " or ";
        }
    }
    words
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Style {
    #[default]
    Hillshade,
    Relief,
    ReliefHillshade,
    Slope,
    Plain,
}

struct StyleName {
    style: Style,
    word: &'static str,
    display: RasterDisplayStyle,
    /// The ramp it colours with unless ramp= says; "" for none.
    ramp: &'static str,
}

static STYLES: [StyleName; 5] = [
    StyleName {
        style: Style::Hillshade,
        word: "hillshade",
        display: RasterDisplayStyle::Hillshade,
        ramp: "",
    },
    StyleName {
        style: Style::Relief,
        word: "relief",
        display: RasterDisplayStyle::Relief,
        ramp: "terrain",
    },
    StyleName {
        style: Style::ReliefHillshade,
        word: "relief+hillshade",
        display: RasterDisplayStyle::ReliefHillshade,
        ramp: "terrain",
    },
    StyleName {
        style: Style::Slope,
        word: "slope",
        display: RasterDisplayStyle::Slope,
        ramp: "slope",
    },
    StyleName {
        style: Style::Plain,
        word: "plain",
        display: RasterDisplayStyle::Plain,
        ramp: "grey",
    },
];

fn style_name(style: Style) -> &'static StyleName {
    STYLES
        .iter()
        .find(|name| name.style == style)
        .unwrap_or(&STYLES[0])
}

fn lit(style: Style) -> bool {
    style == Style::Hillshade || style == Style::ReliefHillshade
}

#[derive(Clone, Debug, Default)]
struct ShadeWords {
    style: Style,
    azimuth: Option<f64>,
    altitude: Option<f64>,
    z_factor: Option<f64>,
    variant: Option<String>,
    ramp: Option<String>,
    range: Option<(f64, f64)>,
    name: Option<String>,
    save: Option<String>,
    overwrite: bool,
    preview: bool,
}

/// What a save= path's extension is, as std::filesystem's extension() gives
/// it: the dot and what follows, lower-cased; nothing for none.
fn extension_of(text: &str) -> String {
    path_from_text(text)
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()).to_ascii_lowercase())
        .unwrap_or_default()
}

fn shade_words(tokens: &Tokens, mut at: usize) -> Result<ShadeWords> {
    let mut words = ShadeWords::default();
    let takes = "RASTER SHADE takes style=, azimuth=, altitude=, z=, variant=, ramp=, range=, \
                 NAME, save=, OVERWRITE and PREVIEW";
    while at < tokens.size() {
        if tokens.is(at, "PREVIEW") {
            words.preview = true;
            at += 1;
            continue;
        }
        if tokens.is(at, "OVERWRITE") {
            words.overwrite = true;
            at += 1;
            continue;
        }
        if tokens.is(at, "NAME") {
            if at + 1 >= tokens.size() || tokens[at + 1].is_empty() {
                return Err(refusal("NAME needs the reference raster's name", ""));
            }
            words.name = Some(tokens[at + 1].clone());
            at += 2;
            continue;
        }
        let Some((key, text)) = key_value(tokens, at) else {
            return Err(refusal(takes, tokens[at].clone()));
        };
        match key.as_str() {
            "style" => {
                let Some(found) = STYLES
                    .iter()
                    .find(|name| name.word.eq_ignore_ascii_case(&text))
                else {
                    return Err(refusal(
                        "style is hillshade, relief, relief+hillshade, slope or plain",
                        text,
                    ));
                };
                words.style = found.style;
            }
            "azimuth" => {
                let azimuth = number_option("azimuth", &text)
                    .ok()
                    .filter(|azimuth| (0.0..=360.0).contains(azimuth));
                let Some(azimuth) = azimuth else {
                    return Err(refusal(
                        "azimuth is the light's direction, 0 to 360 degrees clockwise from north",
                        text,
                    ));
                };
                words.azimuth = Some(azimuth);
            }
            "altitude" => {
                let altitude = number_option("altitude", &text)
                    .ok()
                    .filter(|altitude| (0.0..=90.0).contains(altitude));
                let Some(altitude) = altitude else {
                    return Err(refusal(
                        "altitude is the light's height, 0 to 90 degrees",
                        text,
                    ));
                };
                words.altitude = Some(altitude);
            }
            "z" => {
                words.z_factor = Some(positive_option("z", &text)?);
            }
            "variant" => {
                let variant = text.to_ascii_lowercase();
                if variant != "regular"
                    && variant != "combined"
                    && variant != "multidirectional"
                    && variant != "igor"
                {
                    return Err(refusal(
                        "variant is regular, combined, multidirectional or igor",
                        text,
                    ));
                }
                words.variant = Some(variant);
            }
            "ramp" => {
                if text.is_empty() {
                    return Err(refusal(
                        format!("ramp is {} or a colour-map file", ramp_words()),
                        "",
                    ));
                }
                words.ramp = Some(text);
            }
            "range" => {
                let comma = text.find(',');
                let low = parse_finite_double(&text[..comma.unwrap_or(text.len())]);
                let high = comma.and_then(|comma| parse_finite_double(&text[comma + 1..]));
                match (low, high) {
                    (Some(low), Some(high)) if high > low => words.range = Some((low, high)),
                    _ => {
                        return Err(refusal(
                            "range is the values the ramp spans, low before high: range=100,140",
                            text,
                        ));
                    }
                }
            }
            "save" => {
                let extension = extension_of(&text);
                if extension != ".tif" && extension != ".tiff" {
                    return Err(refusal("save= writes a GeoTIFF: a .tif file", text));
                }
                words.save = Some(text);
            }
            _ => return Err(refusal(takes, tokens[at].clone())),
        }
        at += 1;
    }
    if !lit(words.style)
        && (words.azimuth.is_some()
            || words.altitude.is_some()
            || words.z_factor.is_some()
            || words.variant.is_some())
    {
        return Err(refusal(
            format!(
                "azimuth=, altitude=, z= and variant= light a hillshade; style={} has none",
                style_name(words.style).word
            ),
            "",
        ));
    }
    if words.style == Style::Hillshade && (words.ramp.is_some() || words.range.is_some()) {
        return Err(refusal(
            "a hillshade is grey; ramp= and range= colour relief, slope and plain",
            "",
        ));
    }
    Ok(words)
}

/// The picture, made; what the apply keeps and says.
#[derive(Clone, Debug, Default)]
struct Rendered {
    file: PathBuf,
    /// resampled, shade, ramp, saved.
    records: Vec<String>,
    legend: Vec<LegendEntry>,
    warnings: Vec<String>,
}

/// The range a ramp spans: the one range= gave, else the values'. The
/// diverging ramp is made symmetric about zero, so zero is its white, as a
/// difference's cut and fill need; a range of one value is widened a unit
/// about it, so that value takes the ramp's middle colour.
fn span_of(words: &ShadeWords, ramp: &str, mut values: (f64, f64)) -> (f64, f64) {
    if let Some(range) = words.range {
        return range;
    }
    if ramp.eq_ignore_ascii_case("diverging") {
        let reach = cpp::max(values.0.abs(), values.1.abs());
        values = (-reach, reach);
    }
    if !(values.1 > values.0) {
        values = (values.0 - 0.5, values.0 + 0.5);
    }
    values
}

/// The current raster coloured by the ramp over its range: color-map, no-data
/// transparent. The legend it was painted with.
fn colour(
    chain: &mut RasterChain,
    words: &ShadeWords,
    default_ramp: &str,
    records: &mut Vec<String>,
    stop: Option<&AtomicBool>,
    progress: Progress<'_>,
) -> Result<Vec<LegendEntry>> {
    let Some(values) = value_range(chain.current(), 0)? else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "every cell of the raster is no-data: there is nothing to colour",
        ));
    };
    let ramp = words
        .ramp
        .clone()
        .unwrap_or_else(|| String::from(default_ramp));
    let (low, high) = span_of(words, &ramp, values);
    let facts = chain.info()?;
    let legend;
    let map;
    if let Some(built_in) = built_in_ramp(&ramp) {
        legend = spread(built_in, low, high);
        map = chain.write(
            "ramp.txt",
            &colour_map_text(&legend, facts.no_data_value.is_some()),
        )?;
    } else {
        // A person's colour-map file, as GDAL reads it; its lines are the
        // legend.
        map = path_from_text(&ramp);
        legend = read_colour_map(&map, low, high)?;
    }
    let name = match built_in_ramp(&ramp) {
        Some(built_in) => built_in.name.clone(),
        None => map
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    records.push(format!(
        "ramp name={} min={} max={} from={}",
        value(&name),
        fixed3(low),
        fixed3(high),
        if words.range.is_some() {
            "range"
        } else {
            "data"
        }
    ));
    chain.step(
        &algorithm(&["raster", "color-map"]),
        vec![
            format!("--color-map={}", path_text(&map)),
            String::from("--add-alpha"),
        ],
        stop,
        progress,
        Vec::new(),
    )?;
    Ok(legend)
}

fn light_tokens(words: &ShadeWords) -> Vec<String> {
    let mut tokens = Vec::new();
    if let Some(azimuth) = words.azimuth {
        tokens.push(format!("--azimuth={}", gdal_number(azimuth)));
    }
    if let Some(altitude) = words.altitude {
        tokens.push(format!("--altitude={}", gdal_number(altitude)));
    }
    if let Some(factor) = words.z_factor {
        tokens.push(format!("--zfactor={}", gdal_number(factor)));
    }
    if let Some(variant) = &words.variant {
        tokens.push(format!("--variant={variant}"));
    }
    tokens
}

/// A hillshade as a picture: its greys, 1 to 255, drawn as they are, and its
/// no-data 0 transparent.
const HILLSHADE_GREYS: &str = "1 1 1 1 255\n255 255 255 255 255\nnv 0 0 0 0\n";

fn render(
    input: DatasetValue,
    words: &ShadeWords,
    scratch: &Path,
    stop: Option<&AtomicBool>,
    progress: Progress<'_>,
) -> Result<Rendered> {
    let mut chain = RasterChain::new(input, scratch);
    let mut rendered = Rendered::default();
    let facts = chain.info()?;
    // At display resolution: the picture is drawn from a copy of at most
    // this many pixels a side, so a finer one only costs time.
    let display = RasterImportOptions::default().max_pixels;
    let longest = facts.width.max(facts.height);
    if longest > display {
        let step = (longest + display - 1) / display;
        let width = (facts.width + step - 1) / step;
        let height = (facts.height + step - 1) / step;
        chain.step(
            &algorithm(&["raster", "resize"]),
            vec![
                format!("--size={width},{height}"),
                String::from("--resampling=average"),
            ],
            stop,
            progress,
            Vec::new(),
        )?;
        rendered.records.push(format!(
            "resampled from={}x{} to={width}x{height}",
            facts.width, facts.height
        ));
    }
    let style = style_name(words.style);
    let mut shade = format!("shade style={}", style.word);
    if lit(words.style) {
        shade += &format!(
            " azimuth={} altitude={} z={} variant={}",
            gdal_number(words.azimuth.unwrap_or(315.0)),
            gdal_number(words.altitude.unwrap_or(45.0)),
            gdal_number(words.z_factor.unwrap_or(1.0)),
            words.variant.as_deref().unwrap_or("regular")
        );
    } else if words.style == Style::Slope {
        // Degrees: a picture's legend reads naturally in them, and they are
        // bounded (0 to 90) where percent is not.
        shade += " unit=degree";
    }
    rendered.records.push(shade);

    match words.style {
        Style::Hillshade => {
            chain.step(
                &algorithm(&["raster", "hillshade"]),
                light_tokens(words),
                stop,
                progress,
                Vec::new(),
            )?;
            let map = chain.write("greys.txt", HILLSHADE_GREYS)?;
            chain.step(
                &algorithm(&["raster", "color-map"]),
                vec![
                    format!("--color-map={}", path_text(&map)),
                    String::from("--add-alpha"),
                ],
                stop,
                progress,
                Vec::new(),
            )?;
        }
        Style::ReliefHillshade => {
            let mut light = RasterChain::new(chain.current().clone(), scratch);
            light.step(
                &algorithm(&["raster", "hillshade"]),
                light_tokens(words),
                stop,
                progress,
                Vec::new(),
            )?;
            rendered.legend = colour(
                &mut chain,
                words,
                style.ramp,
                &mut rendered.records,
                stop,
                progress,
            )?;
            chain.step(
                &algorithm(&["raster", "blend"]),
                vec![String::from("--operator=hsv-value")],
                stop,
                progress,
                vec![(
                    String::from("overlay"),
                    ArgValue::Dataset(light.current().clone()),
                )],
            )?;
            for warning in &light.warnings {
                rendered.warnings.push(warning.message.clone());
            }
        }
        Style::Slope | Style::Relief | Style::Plain => {
            if words.style == Style::Slope {
                chain.step(
                    &algorithm(&["raster", "slope"]),
                    vec![String::from("--unit=degree")],
                    stop,
                    progress,
                    Vec::new(),
                )?;
            }
            rendered.legend = colour(
                &mut chain,
                words,
                style.ramp,
                &mut rendered.records,
                stop,
                progress,
            )?;
        }
    }
    for warning in &chain.warnings {
        rendered.warnings.push(warning.message.clone());
    }
    if let Some(save_text) = &words.save {
        // The picture as rendered, a tiled DEFLATE GeoTIFF with the source's
        // georeferencing: for delivery, beside the reference raster kept.
        let DatasetValue::Path(file) = chain.current() else {
            return Err(Error::new(
                ErrorCode::Internal,
                "the picture was never written to a file",
            ));
        };
        let save = path_from_text(save_text);
        if let Some(parent) = save.parent()
            && !parent.as_os_str().is_empty()
        {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::copy(path_from_text(&file.path), &save) {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                format!("could not write the picture: {}", system_message(&error)),
                save_text.clone(),
            ));
        }
        rendered.records.push(format!(
            "saved file={} driver=GTiff",
            value(&path_text(&save))
        ));
    }
    static MADE: AtomicU64 = AtomicU64::new(0);
    let made = MADE.fetch_add(1, Ordering::Relaxed) + 1;
    rendered.file = chain.keep(&scratch.join(format!("katana-shade-{made}.tif")))?;
    Ok(rendered)
}

pub fn prepare_shade(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    let mut at = 2;
    let source = bind_terrain_source(context, tokens, &mut at, "RASTER SHADE")?;
    let words = shade_words(tokens, at)?;
    if let Some(save) = &words.save
        && !words.overwrite
        && path_from_text(save).exists()
    {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "the file exists; add OVERWRITE to replace it",
            save.clone(),
        ));
    }
    if let Some(ramp) = &words.ramp
        && built_in_ramp(ramp).is_none()
        && !path_from_text(ramp).exists()
    {
        return Err(make_error(
            ErrorCode::NotFound,
            format!(
                "ramp is {} or a colour-map file, and no file is there",
                ramp_words()
            ),
            ramp.clone(),
        ));
    }
    let style = style_name(words.style);
    let name = match &words.name {
        Some(name) => name.clone(),
        None => format!(
            "{}-{}",
            source_name(context, &source.source),
            if words.style == Style::ReliefHillshade {
                "relief-hillshade"
            } else {
                style.word
            }
        ),
    };
    let mut records = vec![source.record.clone()];
    if words.preview {
        records.push(format!("shade style={} name={}", style.word, value(&name)));
        records.push(String::from("preview valid=yes changed=no"));
        return Ok(answered_with("RASTER SHADE", joined_records(&records)));
    }

    let options = words;
    let dataset = Arc::clone(&source.dataset);
    let scratch = context.scratch.to_path_buf();
    let command = String::from(trimmed_text(line));
    let display = style.display;
    let work = make_work(move |stop, progress| {
        let input = dataset()?;
        let rendered = render(input, &options, &scratch, stop, progress)?;
        Ok(make_apply(move |ctx| {
            let request = ApplyRequest {
                target: Target {
                    kind: TargetKind::Reference,
                    name: name.clone(),
                    ..Target::default()
                },
                default_name: name.clone(),
                result: ResultOptions {
                    command_name: command,
                    ..ResultOptions::default()
                },
                ..ApplyRequest::default()
            };
            let outputs = RunOutputs {
                file: Some(path_text(&rendered.file)),
                ..RunOutputs::default()
            };
            let applied = apply_outputs(ctx, &request, outputs)?;
            // The reference raster is drawn as what it is: a shading.
            for record in parse_records(&applied) {
                let id = if record.kind == "output" {
                    record.get("id")
                } else {
                    None
                };
                let Some(id) = id else {
                    continue;
                };
                if let Some(number) = parse_integer(id)
                    && let Some(raster) = ctx.reference.find_raster_mut(number as ReferenceId)
                {
                    raster.display_style = display;
                }
            }
            let mut reply = records;
            reply.extend(rendered.records.iter().cloned());
            reply.push(applied);
            for entry in &rendered.legend {
                reply.push(format!(
                    "legend value={} r={} g={} b={}",
                    fixed3(entry.value),
                    entry.r,
                    entry.g,
                    entry.b
                ));
            }
            for warning in &rendered.warnings {
                reply.push(warning_record(warning));
            }
            Ok(joined_records(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("Terrain Shading"),
        reply: None,
        work: Some(work),
    })
}

/// The verb's usage on one line (terrain_verbs.hpp's shadeUsage). Nothing
/// calls it, in the C++ either: the verb table carries the usage HELP shows.
#[allow(
    dead_code,
    reason = "Ported as the C++ declares it; the verb table holds its own usage text, so nothing calls this one."
)]
pub fn shade_usage() -> String {
    String::from(USAGE)
}
