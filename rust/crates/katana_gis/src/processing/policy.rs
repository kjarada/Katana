//! Which GDAL algorithms may run without asking, and which words of a
//! command-line tail are refused (src/katana_io/geo/policy.cpp,
//! docs/geoprocessing.md, "Safety").
//!
//! An algorithm that changes or removes EXISTING data in place - a file
//! deleted, renamed, repacked, its overviews rebuilt, its pixels edited - is
//! Confirm: the line must say CONFIRM, the toolbox's Confirm box must be
//! ticked, an MCP call must pass confirm:true. Everything that only reads, or
//! writes a new output the line names, is Safe; replacing an existing output
//! is OVERWRITE's business, not the policy's.
//!
//! Two sources decide, and the stricter wins: the table below, one entry per
//! leaf GDAL 3.13 has, judged by hand; and a rule, that an input dataset GDAL
//! opens for update makes an algorithm Confirm whatever the table says, so a
//! leaf that gains such an input in a later GDAL is caught without anyone
//! reading its release notes. A leaf in neither - one a later GDAL adds - is
//! Confirm until someone judges it: "unclassified", which the contract test
//! lists by name.

use katana_core::error::make_error;
use katana_core::text::is_ascii_space;
use katana_core::{ErrorCode, Status};

use super::bridge::path_text;
use super::run_detail::{Algorithm, PipelineWord};
use super::types::{Policy, TailEffects};
use crate::ffi::gdal;

/// Leaves that change or remove what already exists.
const CONFIRM: [&str; 18] = [
    "dataset copy",
    "dataset delete",
    "dataset rename",
    "driver gpkg repack",
    "driver openfilegdb repack",
    // Writes _metadata INTO the partitioned dataset it reads.
    "driver parquet create-metadata-file",
    "raster edit",
    "raster overview add",
    "raster overview delete",
    "raster overview refresh",
    "raster update",
    "vector update",
    "vsi copy",
    "vsi delete",
    "vsi move",
    "vsi sozip create",
    "vsi sozip optimize",
    "vsi sync",
];

/// Leaves that read, or write only a new output the line names.
const SAFE: [&str; 103] = [
    "convert",
    "dataset check",
    "dataset identify",
    "driver cog validate",
    "driver gpkg validate",
    "driver gti create",
    "driver pdf list-layers",
    "driver rpftoc create",
    "info",
    "mdim convert",
    "mdim info",
    "mdim mosaic",
    "pipeline",
    "raster as-features",
    "raster aspect",
    "raster blend",
    "raster calc",
    "raster clean-collar",
    "raster clip",
    "raster color-map",
    "raster compare",
    "raster contour",
    "raster convert",
    "raster create",
    "raster fill-nodata",
    "raster footprint",
    "raster hillshade",
    "raster index",
    "raster info",
    "raster mosaic",
    "raster neighbors",
    "raster nodata-to-alpha",
    "raster pansharpen",
    "raster pipeline",
    "raster pixel-info",
    "raster polygonize",
    "raster proximity",
    "raster reclassify",
    "raster reproject",
    "raster resize",
    "raster rgb-to-palette",
    "raster roughness",
    "raster scale",
    "raster select",
    "raster set-type",
    "raster sieve",
    "raster slope",
    "raster stack",
    "raster tile",
    "raster tpi",
    "raster tri",
    "raster unscale",
    "raster viewshed",
    "raster zonal-stats",
    "vector buffer",
    "vector check-coverage",
    "vector check-geometry",
    "vector clean-coverage",
    "vector clip",
    "vector combine",
    "vector concat",
    "vector concave-hull",
    "vector convert",
    "vector convex-hull",
    "vector create",
    "vector dissolve",
    "vector edit",
    "vector explode-collections",
    "vector export-schema",
    "vector filter",
    "vector grid average",
    "vector grid average-distance",
    "vector grid average-distance-points",
    "vector grid count",
    "vector grid invdist",
    "vector grid invdistnn",
    "vector grid linear",
    "vector grid maximum",
    "vector grid minimum",
    "vector grid nearest",
    "vector grid range",
    "vector index",
    "vector info",
    "vector layer-algebra",
    "vector make-point",
    "vector make-valid",
    "vector partition",
    "vector pipeline",
    "vector rasterize",
    "vector rename-layer",
    "vector reproject",
    "vector segmentize",
    "vector select",
    "vector set-field-type",
    "vector set-geom-type",
    "vector simplify",
    "vector simplify-coverage",
    "vector sort",
    "vector sql",
    "vector swap-xy",
    "vsi list",
    "vsi sozip list",
    "vsi sozip validate",
];

fn listed(table: &[&str], path: &str) -> bool {
    table.contains(&path)
}

/// The steps GDAL 3.13's three pipelines offer (their usage's
/// pipeline_algorithms), judged by hand as the leaves are. Only `update`
/// writes into a dataset that already exists - "Update the destination raster
/// with the content of the input one", the vector one likewise - so it is
/// Confirm, as `raster update` and `vector update` are. `edit` and `overview`
/// are Confirm as leaves, which change the dataset they open, but as steps
/// they change the piped dataset only, which the pipeline then writes where
/// it is told (a new output, or OVERWRITE's business). `external` is refused
/// outright (check_tokens). A step in neither list is Confirm until someone
/// judges it; the contract test lists every one GDAL offers.
const CONFIRM_STEPS: [&str; 1] = ["update"];
const SAFE_STEPS: [&str; 70] = [
    "as-features",
    "aspect",
    "blend",
    "buffer",
    "calc",
    "check-coverage",
    "check-geometry",
    "clean-coverage",
    "clip",
    "color-map",
    "combine",
    "compare",
    "concat",
    "concave-hull",
    "contour",
    "convex-hull",
    "create",
    "dissolve",
    "edit",
    "explode-collections",
    "export-schema",
    "fill-nodata",
    "filter",
    "footprint",
    "grid",
    "hillshade",
    "info",
    "limit",
    "make-point",
    "make-valid",
    "materialize",
    "mosaic",
    "neighbors",
    "nodata-to-alpha",
    "overview",
    "pansharpen",
    "partition",
    "pixel-info",
    "polygonize",
    "proximity",
    "rasterize",
    "read",
    "reclassify",
    "rename-layer",
    "reproject",
    "resize",
    "rgb-to-palette",
    "roughness",
    "scale",
    "segmentize",
    "select",
    "set-field-type",
    "set-geom-type",
    "set-type",
    "sieve",
    "simplify",
    "simplify-coverage",
    "slope",
    "sort",
    "sql",
    "stack",
    "swap-xy",
    "tee",
    "tile",
    "tpi",
    "tri",
    "unscale",
    "viewshed",
    "write",
    "zonal-stats",
];

/// Options that let GDAL change a dataset that is already there rather than
/// write a new one: replace it (--overwrite, --overwrite-layer), add to it
/// (--append, --upsert, --update; rasterize's --add burns into the raster the
/// output names, measured: its checksum changed with no --update; tile's
/// --resume writes the missing tiles into a tile set already there). They
/// need OVERWRITE on the line. Found by listing every boolean argument of
/// every GDAL 3.13 algorithm and step (gdal --json-usage) and reading the
/// description of each whose name suggested it.
const CHANGES_EXISTING: [&str; 7] = [
    "--overwrite",
    "--overwrite-layer",
    "--append",
    "--update",
    "--upsert",
    "--add",
    "--resume",
];

/// Words that would print instead of run, or reach past the algorithm:
/// --config is how GDAL_ENABLE_EXTERNAL (programs run by a pipeline) and
/// SPATIALITE_SECURITY (Spatialite's file functions) would be switched on.
const REFUSED_WORDS: [&str; 8] = [
    "--config",
    "--help",
    "-h",
    "--help-doc",
    "--json-usage",
    "--progress",
    "--quiet",
    "-q",
];

// ---- gdal_run_detail.hpp's functions -----------------------------------------------------------

/// The policy of the leaf at `path`, whose instance is `algorithm`, and why
/// (AlgorithmInfo::policy_reason).
pub(crate) fn classify(path: &[String], algorithm: &Algorithm, reason: &mut String) -> Policy {
    for arg in algorithm.args() {
        if arg.is_dataset()
            && arg.is_input()
            && !arg.is_output()
            && (arg.dataset_type() as u32 & gdal::GDAL_OF_UPDATE) != 0
        {
            *reason = String::from("update");
            return Policy::Confirm;
        }
    }
    let text = path_text(path);
    if listed(&CONFIRM, &text) {
        *reason = String::from("listed");
        return Policy::Confirm;
    }
    if listed(&SAFE, &text) {
        *reason = String::from("listed");
        return Policy::Safe;
    }
    *reason = String::from("unclassified");
    Policy::Confirm
}

/// One word of a pipeline per PipelineWord, in order (run_detail.rs says what
/// a step and `first` are).
pub(crate) fn pipeline_words(pipeline: &str) -> Vec<PipelineWord> {
    // A step begins the text, and follows each '!' and each '[' that opens a
    // nested pipeline; its first word is its name.
    const NAMED: &[u8] = b"--pipeline=";
    let bytes = pipeline.as_bytes();
    let mut words = Vec::new();
    let mut step_start = true;
    let mut first = true;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'!' || c == b'[' {
            step_start = true;
            first = false;
            i += 1;
            continue;
        }
        // std::isspace in the C locale, as the C++ reads it.
        if is_ascii_space(c) || c == b']' {
            i += 1;
            continue;
        }
        let end = bytes[i..]
            .iter()
            .position(|byte| b" \t\r\n![]".contains(byte))
            .map_or(bytes.len(), |offset| i + offset);
        // Each end is an ASCII delimiter or the text's end, so the word is
        // whole characters.
        let mut word = &pipeline[i..end];
        i = end;
        // The pipeline given by name, as one word: --pipeline=read x ! ...
        let head = &word.as_bytes()[..word.len().min(NAMED.len())];
        if head.eq_ignore_ascii_case(NAMED) {
            word = &word[NAMED.len()..];
            if word.is_empty() {
                continue;
            }
        }
        words.push(PipelineWord {
            text: String::from(word),
            step: step_start && !word.starts_with('-'),
            first,
        });
        step_start = step_start && word.starts_with('-');
    }
    words
}

/// Whether a pipeline, as GDAL's pipeline argument takes it ("read ! slope !
/// write", nested steps in [ ]), has a step called `external`, which runs a
/// program of the caller's choosing.
pub(crate) fn has_external_step(pipeline: &str) -> bool {
    pipeline_words(pipeline)
        .iter()
        .any(|word| word.step && word.text.eq_ignore_ascii_case("external"))
}

/// Whether the path is one of the three pipeline algorithms.
pub(crate) fn is_pipeline(path: &[String]) -> bool {
    path.last().is_some_and(|last| last == "pipeline")
}

// ---- processing.hpp's functions -----------------------------------------------------------------

/// A pipeline step's policy, judged as a leaf's is: Confirm for `update`,
/// which writes into a dataset already there, and for a step no one has
/// judged ("unclassified"); `external` is "refused" (check_tokens). `reason`
/// is "listed", "unclassified" or "refused".
pub fn step_policy(step: &str, reason: &mut String) -> Policy {
    let name = step.to_ascii_lowercase();
    if name == "external" {
        *reason = String::from("refused");
        return Policy::Confirm;
    }
    if listed(&CONFIRM_STEPS, &name) {
        *reason = String::from("listed");
        return Policy::Confirm;
    }
    if listed(&SAFE_STEPS, &name) {
        *reason = String::from("listed");
        return Policy::Safe;
    }
    *reason = String::from("unclassified");
    Policy::Confirm
}

/// The tail as one text, the way GDAL's pipeline argument reads it: words
/// given one by one, or quoted as one.
fn pipeline_text(tokens: &[String]) -> String {
    let mut joined = String::new();
    for token in tokens {
        joined.push_str(token);
        joined.push(' ');
    }
    joined
}

/// A word's option name, before any '=', in lower case.
fn option_name(word: &str) -> String {
    word[..word.find('=').unwrap_or(word.len())].to_ascii_lowercase()
}

fn changes_existing(word: &str) -> bool {
    listed(&CHANGES_EXISTING, &option_name(word))
}

/// What a run's own words change of data already there, beyond its
/// algorithm's policy (docs/geoprocessing.md, "Safety"). A pipeline is read
/// word by word however it was quoted - one quoted text, words one by one, or
/// --pipeline=... - nested [ ] steps included.
pub fn tail_effects(path: &[String], tokens: &[String]) -> TailEffects {
    let mut effects = TailEffects::default();
    if !is_pipeline(path) {
        if let Some(found) = tokens.iter().find(|token| changes_existing(token)) {
            effects.overwrite_word = found.clone();
        }
        return effects;
    }
    // Every word of a pipeline, however it was quoted: a pipeline given as
    // one quoted text is still steps and options to GDAL.
    for word in pipeline_words(&pipeline_text(tokens)) {
        if effects.overwrite_word.is_empty() && changes_existing(&word.text) {
            effects.overwrite_word = word.text.clone();
        }
        if !word.step || !effects.confirm_step.is_empty() {
            continue;
        }
        let mut reason = String::new();
        let policy = step_policy(&word.text, &mut reason);
        // The text's first word may be a value of an option before the
        // pipeline (--output-format GTiff read ...), which GDAL refuses by
        // itself if it is no step; only a step after '!' or '[' that no one
        // has judged is taken as one GDAL may have added.
        if policy == Policy::Confirm && (reason != "unclassified" || !word.first) {
            effects.confirm_step = word.text.to_ascii_lowercase();
        }
    }
    effects
}

/// Whether the algorithm, with this tail, reads the Z of the features it is
/// given: a value-producing vector grid without --zfield, rasterize --3d, or
/// a pipeline with such a step. Such a run is given only features with a
/// height at every vertex, since GDAL reads a missing one as 0.
pub fn reads_heights(path: &[String], tokens: &[String]) -> bool {
    let has = |words: &[String], option: &str| words.iter().any(|word| option_name(word) == option);
    // The grids that interpolate or summarise a value read it from Z unless
    // --zfield names a field; count and the two average-distance grids read
    // positions only. Rasterize reads Z with --3d.
    let grid_reads_z = |method: &str, words: &[String]| {
        let positions_only = method == "count"
            || method == "average-distance"
            || method == "average-distance-points";
        !positions_only && !has(words, "--zfield")
    };
    if path.len() == 3 && path[0] == "vector" && path[1] == "grid" {
        return grid_reads_z(&path[2], tokens);
    }
    if path == ["vector", "rasterize"] {
        return has(tokens, "--3d");
    }
    if !is_pipeline(path) {
        return false;
    }
    // A pipeline reads heights when one of its steps does: each step's words
    // are judged on their own.
    let words = pipeline_words(&pipeline_text(tokens));
    for (i, word) in words.iter().enumerate() {
        if !word.step {
            continue;
        }
        let step = word.text.to_ascii_lowercase();
        let own: Vec<String> = words[i + 1..]
            .iter()
            .take_while(|next| !next.step)
            .map(|next| next.text.clone())
            .collect();
        if step == "grid"
            && let Some(method) = own.first()
            && grid_reads_z(&method.to_ascii_lowercase(), &own)
        {
            return true;
        }
        if step == "rasterize" && has(&own, "--3d") {
            return true;
        }
    }
    false
}

/// The words of a tail refused whatever the algorithm: --config (it could
/// switch on GDAL_ENABLE_EXTERNAL or SPATIALITE_SECURITY), --help,
/// --help-doc, --json-usage, --progress and --quiet, which would print
/// instead of run, and a pipeline step `external`, which runs a program.
/// InvalidArgument naming it.
pub fn check_tokens(path: &[String], tokens: &[String]) -> Status {
    for token in tokens {
        let word = option_name(token);
        if REFUSED_WORDS.contains(&word.as_str()) {
            let message = if word == "--config" {
                "--config is refused: it could switch on what runs programs or reads files beyond \
                 the algorithm's own"
            } else {
                "is refused: it prints instead of running; GDAL HELP <algorithm> describes the \
                 arguments"
            };
            return Err(make_error(
                ErrorCode::InvalidArgument,
                message,
                token.clone(),
            ));
        }
    }
    if is_pipeline(path) && has_external_step(&pipeline_text(tokens)) {
        return Err(katana_core::Error::new(
            ErrorCode::InvalidArgument,
            "a pipeline step 'external' runs a program and is refused",
        ));
    }
    Ok(())
}
