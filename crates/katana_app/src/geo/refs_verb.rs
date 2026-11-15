//! REFS (docs/interop.md, "Reference layers"), the port of
//! src/katana_app/geo/refs_verb.cpp: the reference layers - rasters and point
//! clouds drawn under the drawing - listed, shown and hidden, restyled,
//! renamed, removed, given overviews, and read again from the sources a
//! project records.
//!
//! ```text
//! REFS [LIST] [JSON]
//! REFS SHOW|HIDE|REMOVE|INFO <ref>
//! REFS OPACITY <ref> <0..1>
//! REFS COLOR <ref> elevation|intensity|classification|rgb|flat
//! REFS RENAME <ref> <name>
//! REFS OVERVIEWS <ref> [levels=2,4,8] CONFIRM
//! REFS RESTORE
//! <ref> := a layer's id, or its name
//! ```
//!
//! A change to a layer is session data: not undoable, as reference data never
//! was (interop's reference_data). What answers from what the session holds
//! answers at once; OVERVIEWS writes beside the raster's file and RESTORE
//! reads every source again, so they are work, run as jobs in the window.

use std::path::PathBuf;

use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::text::{equals_ignoring_case, parse_finite_double, parse_integer};
use katana_core::{Error, ErrorCode, Result};
use katana_gis::gdal_adapter::describe_crs;
use katana_gis::processing::{self as gp, ArgValue, DatasetPath, DatasetValue, RunRequest};
use katana_interop::dataset_info::{DescribeOptions, describe_source, is_virtual_path};
use katana_interop::reference_data::{
    parse_reference_record, point_color_mode_from_word, read_reference,
};
use katana_interop::{
    PointCloudLayer, RasterOverlay, ReferenceData, ReferenceId, ReferenceLayer, ReferenceSourceKind,
};

use super::bindings::Tokens;
use super::geo_verbs::{Context, Prepared, make_apply, make_work, stop_requested, trimmed_text};
use super::gis_records::{
    record_json, reference_record_of_point_cloud, reference_record_of_raster,
};
use super::replies::{joined, parse_records, value, warning_record};
use super::terrain_verbs::answered_with;
use crate::import_records::path_text;

const USAGE: &str = "usage: REFS [LIST] [JSON] | REFS SHOW|HIDE|REMOVE|INFO <ref> | REFS OPACITY \
                     <ref> <0..1> | REFS COLOR <ref> elevation|intensity|classification|rgb|flat \
                     | REFS RENAME <ref> <name> | REFS OVERVIEWS <ref> [levels=2,4,8] CONFIRM | \
                     REFS RESTORE";

/// One record a line, as the C++ joins them: an empty record still takes its
/// place.
fn lines(records: &[String]) -> String {
    joined(records, "\n")
}

/// A missing record as REFS lists it: the layer a project names whose source
/// could not be read when it opened.
fn missing_record(record: &str) -> String {
    match parse_reference_record(record) {
        Err(error) => format!("missing name= kind= file= reason={}", value(&error.message)),
        Ok(source) => format!(
            "missing name={} kind={} file={}",
            value(&source.name),
            if source.kind == ReferenceSourceKind::Raster {
                "raster"
            } else {
                "pointcloud"
            },
            value(&path_text(&source.source))
        ),
    }
}

fn listed(reference: &ReferenceData) -> Vec<String> {
    let mut records = Vec::new();
    for raster in reference.rasters() {
        records.push(reference_record_of_raster(raster));
    }
    for cloud in reference.point_clouds() {
        records.push(reference_record_of_point_cloud(cloud));
    }
    for record in reference.missing() {
        records.push(missing_record(record));
    }
    // Said when there are none too: an empty reply reads as a line that did
    // nothing.
    records.push(format!(
        "references rasters={} clouds={} missing={}",
        reference.rasters().len(),
        reference.point_clouds().len(),
        reference.missing().len()
    ));
    records
}

// ---- which layer ----------------------------------------------------------------------------

/// The layer a word names, by its id: the C++'s pointer to the raster or the
/// cloud, here the id the reference data finds it by again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found {
    Raster(ReferenceId),
    Cloud(ReferenceId),
}

impl Found {
    fn id(self) -> ReferenceId {
        match self {
            Found::Raster(id) | Found::Cloud(id) => id,
        }
    }

    fn record(self, reference: &ReferenceData) -> String {
        match self {
            Found::Raster(id) => reference_record_of_raster(raster_of(reference, id)),
            Found::Cloud(id) => reference_record_of_point_cloud(cloud_of(reference, id)),
        }
    }
}

fn raster_of(reference: &ReferenceData, id: ReferenceId) -> &RasterOverlay {
    reference
        .find_raster(id)
        .expect("a raster found by its id is there")
}

fn cloud_of(reference: &ReferenceData, id: ReferenceId) -> &PointCloudLayer {
    reference
        .find_point_cloud(id)
        .expect("a point cloud found by its id is there")
}

fn raster_mut(reference: &mut ReferenceData, id: ReferenceId) -> &mut RasterOverlay {
    reference
        .find_raster_mut(id)
        .expect("a raster found by its id is there")
}

fn cloud_mut(reference: &mut ReferenceData, id: ReferenceId) -> &mut PointCloudLayer {
    reference
        .find_point_cloud_mut(id)
        .expect("a point cloud found by its id is there")
}

/// A layer by its id, or by its name in any case. NotFound for neither;
/// InvalidArgument for a name several layers have, naming their ids.
fn find(reference: &ReferenceData, word: &str) -> Result<Found> {
    let digits = !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit());
    let number = if digits { parse_integer(word) } else { None };
    if let Some(number) = number
        && number > 0
    {
        let id = number as ReferenceId;
        if reference.find_raster(id).is_some() {
            return Ok(Found::Raster(id));
        }
        if reference.find_point_cloud(id).is_some() {
            return Ok(Found::Cloud(id));
        }
    }
    let mut named = Vec::new();
    for raster in reference.rasters() {
        if equals_ignoring_case(raster.name.as_bytes(), word.as_bytes()) {
            named.push(Found::Raster(raster.id));
        }
    }
    for cloud in reference.point_clouds() {
        if equals_ignoring_case(cloud.name.as_bytes(), word.as_bytes()) {
            named.push(Found::Cloud(cloud.id));
        }
    }
    if named.len() == 1 {
        return Ok(named[0]);
    }
    if named.is_empty() {
        return Err(make_error(
            ErrorCode::NotFound,
            "no reference layer has that id or name; REFS lists them",
            word,
        ));
    }
    let mut ids = String::new();
    for each in &named {
        if !ids.is_empty() {
            ids += ", ";
        }
        ids += &format!("{}", each.id());
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        format!("several reference layers are called that; name one by its id: {ids}"),
        word,
    ))
}

/// The layer the line's third word names, after checking the line's length.
fn target(context: &Context<'_>, tokens: &Tokens, words: usize, usage: &str) -> Result<Found> {
    if tokens.size() != words {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("usage: {usage}"),
        ));
    }
    find(context.reference, &tokens[2])
}

fn changed(context: &Context<'_>) {
    if let Some(changed) = &context.front_end.changed {
        changed();
    }
}

// ---- the edits --------------------------------------------------------------------------------

fn show_or_hide(context: &mut Context<'_>, tokens: &Tokens, visible: bool) -> Result<Prepared> {
    let found = target(
        context,
        tokens,
        3,
        if visible {
            "REFS SHOW <ref>"
        } else {
            "REFS HIDE <ref>"
        },
    )?;
    match found {
        Found::Raster(id) => raster_mut(context.reference, id).visible = visible,
        Found::Cloud(id) => cloud_mut(context.reference, id).visible = visible,
    }
    changed(context);
    Ok(answered_with(
        if visible { "REFS SHOW" } else { "REFS HIDE" },
        found.record(context.reference),
    ))
}

fn remove(context: &mut Context<'_>, tokens: &Tokens) -> Result<Prepared> {
    if tokens.size() != 3 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "usage: REFS REMOVE <ref>",
        ));
    }
    let found = match find(context.reference, &tokens[2]) {
        Ok(found) => found,
        Err(error) => {
            // A layer the project names that could not be read is dropped
            // from the record by its name.
            if error.code == ErrorCode::NotFound && context.reference.forget_missing(&tokens[2]) {
                return Ok(answered_with(
                    "REFS REMOVE",
                    format!("removed id= kind= name={} missing=yes", value(&tokens[2])),
                ));
            }
            return Err(error);
        }
    };
    let (kind, name) = match found {
        Found::Raster(id) => ("raster", raster_of(context.reference, id).name.clone()),
        Found::Cloud(id) => ("pointcloud", cloud_of(context.reference, id).name.clone()),
    };
    let record = format!(
        "removed id={} kind={kind} name={}",
        found.id(),
        value(&name)
    );
    context.reference.remove(found.id());
    changed(context);
    Ok(answered_with("REFS REMOVE", record))
}

/// A layer's coordinate system as a record says it; nothing for none.
fn crs_of(wkt: &str) -> String {
    if wkt.is_empty() {
        String::new()
    } else {
        describe_crs(wkt)
    }
}

fn info(context: &Context<'_>, tokens: &Tokens) -> Result<Prepared> {
    let found = target(context, tokens, 3, "REFS INFO <ref>")?;
    let mut records = vec![found.record(context.reference)];
    match found {
        Found::Raster(id) => {
            let raster = raster_of(context.reference, id);
            records.push(format!(
                "source file={} url={} licence={} attribution={} crs={}",
                value(&path_text(&raster.source)),
                value(&raster.source_url),
                value(&raster.licence),
                value(&raster.attribution),
                value(&crs_of(&raster.projection_wkt))
            ));
            if !raster.derivation.is_empty() {
                // The line that made it: run again, it makes it again.
                records.push(format!("derivation line={}", value(&raster.derivation)));
            }
        }
        Found::Cloud(id) => {
            let cloud = cloud_of(context.reference, id);
            records.push(format!(
                "source file={} decimation={} crs={}",
                value(&path_text(&cloud.source)),
                cloud.decimation_step,
                value(&crs_of(&cloud.projection_wkt))
            ));
        }
    }
    Ok(answered_with("REFS INFO", lines(&records)))
}

fn opacity(context: &mut Context<'_>, tokens: &Tokens) -> Result<Prepared> {
    let found = target(context, tokens, 4, "REFS OPACITY <ref> <0..1>")?;
    let Found::Raster(id) = found else {
        return Err(make_error(
            ErrorCode::Unsupported,
            "a point cloud is drawn opaque; REFS COLOR chooses how it is coloured",
            tokens[2].clone(),
        ));
    };
    // parse_finite_double refuses NaN, so the range is the C++'s
    // `amount < 0 || amount > 1` refused.
    let amount = match parse_finite_double(&tokens[3]) {
        Some(amount) if (0.0..=1.0).contains(&amount) => amount,
        _ => {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "opacity is a number from 0 (clear) to 1 (opaque)",
                tokens[3].clone(),
            ));
        }
    };
    raster_mut(context.reference, id).opacity = amount;
    changed(context);
    Ok(answered_with(
        "REFS OPACITY",
        found.record(context.reference),
    ))
}

fn colour(context: &mut Context<'_>, tokens: &Tokens) -> Result<Prepared> {
    let found = target(
        context,
        tokens,
        4,
        "REFS COLOR <ref> elevation|intensity|classification|rgb|flat",
    )?;
    let Found::Cloud(id) = found else {
        return Err(make_error(
            ErrorCode::Unsupported,
            "a raster is drawn in its own colours; REFS OPACITY sets how much of it shows",
            tokens[2].clone(),
        ));
    };
    let Some(mode) = point_color_mode_from_word(&tokens[3]) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a point cloud is coloured by elevation, intensity, classification, rgb or flat",
            tokens[3].clone(),
        ));
    };
    cloud_mut(context.reference, id).color_mode = mode;
    changed(context);
    Ok(answered_with("REFS COLOR", found.record(context.reference)))
}

fn rename(context: &mut Context<'_>, tokens: &Tokens) -> Result<Prepared> {
    let found = target(context, tokens, 4, "REFS RENAME <ref> <name>")?;
    let name = String::from(trimmed_text(&tokens[3]));
    if name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a reference layer's name is not empty",
        ));
    }
    match found {
        Found::Raster(id) => raster_mut(context.reference, id).name = name,
        Found::Cloud(id) => cloud_mut(context.reference, id).name = name,
    }
    changed(context);
    Ok(answered_with(
        "REFS RENAME",
        found.record(context.reference),
    ))
}

// ---- OVERVIEWS ------------------------------------------------------------------------------

fn overviews(context: &Context<'_>, tokens: &Tokens) -> Result<Prepared> {
    let mut confirm = false;
    let mut levels = String::new();
    if tokens.size() < 3 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "usage: REFS OVERVIEWS <ref> [levels=2,4,8] CONFIRM",
        ));
    }
    let mut at = 3;
    while at < tokens.size() {
        let word = &tokens[at];
        if tokens.is(at, "CONFIRM") {
            confirm = true;
        } else if !tokens.quoted[at] && word.to_ascii_lowercase().starts_with("levels=") {
            levels = String::from(&word["levels=".len()..]);
        } else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "REFS OVERVIEWS takes levels=2,4,8 and CONFIRM",
                word.clone(),
            ));
        }
        at += 1;
    }
    let found = find(context.reference, &tokens[2])?;
    let Found::Raster(id) = found else {
        return Err(make_error(
            ErrorCode::Unsupported,
            "overviews are a raster's",
            tokens[2].clone(),
        ));
    };
    let raster = raster_of(context.reference, id);
    let source = raster.source.clone();
    if source.as_os_str().is_empty() || is_virtual_path(&source) {
        return Err(make_error(
            ErrorCode::Unsupported,
            "overviews are written beside a raster's file, and this layer has none on the disk",
            raster.name.clone(),
        ));
    }
    let mut ovr = source.clone().into_os_string();
    ovr.push(".ovr");
    let ovr = PathBuf::from(ovr);
    // Writing beside a person's file is what Katana otherwise never does
    // (docs/geoprocessing.md, "Safety"): asked for by name.
    if !confirm {
        return Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "REFS OVERVIEWS writes {} beside the raster's file; add CONFIRM to write it",
                path_text(&ovr)
            ),
            raster.name.clone(),
        ));
    }
    let title = format!("REFS OVERVIEWS {}", raster.name);
    let work = make_work(move |stop, progress| {
        let mut tokens = vec![
            // External: an .ovr beside the file, never the file itself
            // changed.
            String::from("--external"),
            String::from("--resampling=average"),
        ];
        if !levels.is_empty() {
            tokens.push(format!("--levels={levels}"));
        }
        let request = RunRequest {
            path: vec![
                String::from("raster"),
                String::from("overview"),
                String::from("add"),
            ],
            values: vec![(
                String::from("input"),
                ArgValue::Dataset(DatasetValue::Path(DatasetPath {
                    path: path_text(&source),
                    ..DatasetPath::default()
                })),
            )],
            tokens,
            ..RunRequest::default()
        };
        gp::run(&request, stop, progress)?;
        // What was made, as GDAL now reads the file.
        let described = describe_source(&source, &DescribeOptions::default());
        let mut records = Vec::new();
        let mut made = 0;
        if let Ok(described) = &described
            && let Some(raster) = &described.raster
            && let Some(band) = raster.bands.first()
        {
            made = band.overviews.len();
            for (k, (width, height)) in band.overviews.iter().enumerate() {
                records.push(format!(
                    "overview band=1 index={} width={width} height={height}",
                    k + 1
                ));
            }
        }
        let head = format!(
            "overviews id={id} file={} count={made} levels={}",
            value(&path_text(&ovr)),
            value(&levels)
        );
        records.insert(0, head);
        let text = lines(&records);
        Ok(make_apply(move |_context| Ok(text)))
    });
    Ok(Prepared {
        title,
        reply: None,
        work: Some(work),
    })
}

// ---- RESTORE --------------------------------------------------------------------------------

/// What RESTORE read of one recorded layer.
struct Restored {
    record: String,
    layer: Option<ReferenceLayer>,
    /// When it could not be read.
    why: String,
}

fn restore(context: &Context<'_>, tokens: &Tokens) -> Result<Prepared> {
    if tokens.size() != 2 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "usage: REFS RESTORE",
        ));
    }
    let records = context.document.metadata().reference_layers.clone();
    let project = context.document.project_directory();
    let work = make_work(move |stop, progress| {
        let mut read = Vec::new();
        for (k, record) in records.iter().enumerate() {
            if stop_requested(stop) {
                return Err(Error::new(ErrorCode::InvalidState, "cancelled"));
            }
            let mut one = Restored {
                record: record.clone(),
                layer: None,
                why: String::new(),
            };
            match parse_reference_record(record) {
                Err(error) => one.why = error.describe(),
                Ok(source) => match read_reference(&source, project.as_deref()) {
                    Ok(layer) => one.layer = Some(layer),
                    Err(error) => one.why = error.describe(),
                },
            }
            read.push(one);
            if let Some(progress) = progress {
                progress((k + 1) as f64 / records.len() as f64);
            }
        }
        Ok(make_apply(move |context| {
            // The layers become those the project records: what was held is
            // let go first, so a RESTORE typed twice does not double them.
            context.reference.clear();
            let total = read.len();
            let mut reply = Vec::new();
            let mut restored = 0;
            for one in read {
                let Some(layer) = one.layer else {
                    // Warned of, not fatal: the drawing opens without it, and
                    // the record is kept, so a save does not drop the layer
                    // because its drive was not there today.
                    context.reference.keep_missing(one.record);
                    reply.push(warning_record(&one.why));
                    continue;
                };
                restored += 1;
                match layer {
                    ReferenceLayer::Raster(raster) => {
                        let id = context.reference.add(raster);
                        reply.push(reference_record_of_raster(raster_of(context.reference, id)));
                    }
                    ReferenceLayer::PointCloud(cloud) => {
                        let id = context.reference.add(cloud);
                        reply.push(reference_record_of_point_cloud(cloud_of(
                            context.reference,
                            id,
                        )));
                    }
                }
            }
            reply.push(format!(
                "restored layers={restored} missing={}",
                total - restored
            ));
            if let Some(changed) = &context.front_end.changed {
                changed();
            }
            Ok(lines(&reply))
        }))
    });
    Ok(Prepared {
        title: String::from("REFS RESTORE"),
        reply: None,
        work: Some(work),
    })
}

/// A REFS line (verb_table.rs, the I0 block's REFS row).
pub fn prepare_refs(context: &mut Context<'_>, tokens: &Tokens, _line: &str) -> Result<Prepared> {
    if tokens.size() == 1 || tokens.is(1, "LIST") || tokens.is(1, "JSON") {
        let mut json = false;
        let mut at = 1;
        while at < tokens.size() {
            if tokens.is(at, "JSON") {
                json = true;
            } else if !(at == 1 && tokens.is(at, "LIST")) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    USAGE,
                    tokens[at].clone(),
                ));
            }
            at += 1;
        }
        let records = listed(context.reference);
        if !json {
            return Ok(answered_with("REFS", lines(&records)));
        }
        let mut layers = Vec::new();
        let mut missing = Vec::new();
        for record in parse_records(&lines(&records)) {
            if record.kind == "reference" {
                layers.push(record_json(&record));
            } else if record.kind == "missing" {
                missing.push(record_json(&record));
            }
        }
        return Ok(answered_with(
            "REFS",
            Json::object_of([
                ("references", Json::array_of(layers)),
                ("missing", Json::array_of(missing)),
            ])
            .dump(),
        ));
    }
    if tokens.is(1, "SHOW") || tokens.is(1, "HIDE") {
        let visible = tokens.is(1, "SHOW");
        return show_or_hide(context, tokens, visible);
    }
    if tokens.is(1, "REMOVE") {
        return remove(context, tokens);
    }
    if tokens.is(1, "INFO") {
        return info(context, tokens);
    }
    if tokens.is(1, "OPACITY") {
        return opacity(context, tokens);
    }
    if tokens.is(1, "COLOR") || tokens.is(1, "COLOUR") {
        return colour(context, tokens);
    }
    if tokens.is(1, "RENAME") {
        return rename(context, tokens);
    }
    if tokens.is(1, "OVERVIEWS") {
        return overviews(context, tokens);
    }
    if tokens.is(1, "RESTORE") {
        return restore(context, tokens);
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        USAGE,
        tokens[1].clone(),
    ))
}
