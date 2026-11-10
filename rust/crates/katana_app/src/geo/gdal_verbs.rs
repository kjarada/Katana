//! The GDAL verb (docs/geoprocessing.md, "The GDAL verb"), the port of
//! src/katana_app/geo/gdal_verbs.cpp: any algorithm of GDAL's, in GDAL's own
//! argument vocabulary, with Katana's FROM and TO clauses binding drawing
//! data, rasters, surfaces and files.
//!
//! ```text
//! GDAL VERSION
//! GDAL LIST [raster|vector|mdim|dataset|vsi|pipeline|driver|convert|info|<text>] [JSON]
//! GDAL HELP <algorithm> [JSON]
//! GDAL [RUN] <algorithm> [<gdal word>...] [FROM [<arg>] <source>]... [TO [<arg>] <target>]
//!      [CONFIRM] [OVERWRITE] [PREVIEW]
//! ```
//!
//! The line is Katana's, not GDAL's serialised command: GDAL cannot write
//! back a dataset it was handed as an object ("<input:unserialisable>"), and
//! the line a person typed is what the history, the undo list and a script
//! keep.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_gis::processing::{
    self as gp, AlgorithmInfo, AlgorithmSpec, ArgSpec, ArgType, ArgValue, DatasetKind,
    DatasetValue, OutputTo, Policy, RunRequest,
};
use katana_interop::geo::{DrawingDatasetOptions, ResultOptions};

use super::bindings::{
    ApplyRequest, DeferredDataset, Source, SourceKind, Target, TargetKind, Tokens, apply_outputs,
    bind_drawing, bind_raster, derived_folder, input_record, is_source_keyword, parse_source,
    parse_target, project_crs, scope_record,
};
use super::geo_verbs::{Context, Prepared, make_apply, make_work, trimmed_text};
use super::replies::{
    algorithm_record, arg_record, binding_record, gdal_record, group_record, joined, sources_for,
    value, version_record, warning_record,
};
use super::schema::{algorithm_json, describe_json};
use super::seams;
use super::staged_files::StagedFiles;
use crate::import_records::{path_from_text, path_text};

const USAGE: &str = "GDAL VERSION | GDAL LIST [<group>|<text>] [JSON] | GDAL HELP <algorithm> \
                     [JSON] | GDAL [RUN] <algorithm> [<gdal word>...] [FROM [<arg>] <source>]... \
                     [TO [<arg>] <target>] [CONFIRM] [OVERWRITE] [PREVIEW]";

fn clause(tokens: &Tokens, i: usize) -> bool {
    ["FROM", "TO", "CONFIRM", "OVERWRITE", "PREVIEW"]
        .iter()
        .any(|word| tokens.is(i, word))
}

fn info_of(path: &[String]) -> Option<&'static AlgorithmInfo> {
    gp::catalogue().iter().find(|info| info.path == path)
}

fn under(path: &[String], group: &[String]) -> bool {
    path.len() > group.len() && path.starts_with(group)
}

fn leaves_under(group: &[String]) -> usize {
    gp::catalogue()
        .iter()
        .filter(|info| !info.container && under(&info.path, group))
        .count()
}

/// The records a line each, an empty one kept in its place: gdal_verbs.cpp's
/// own joinedRecords, which differs from terrain_verbs' (that drops an empty
/// record).
fn joined_records(records: &[String]) -> String {
    joined(records, "\n")
}

fn answered(title: impl Into<String>, reply: String) -> Prepared {
    Prepared {
        title: title.into(),
        reply: Some(reply),
        work: None,
    }
}

// ---- LIST ------------------------------------------------------------------------------------

fn list(tokens: &Tokens, at: usize) -> Result<Prepared> {
    let mut json = false;
    let mut words: Vec<String> = Vec::new();
    for (i, word) in tokens.words.iter().enumerate().skip(at) {
        if tokens.is(i, "JSON") {
            json = true;
        } else {
            words.push(word.to_ascii_lowercase());
        }
    }
    // A group named ("raster", "vector grid") lists what is in it; any other
    // text is looked for in the names and the descriptions.
    let mut group: Vec<String> = Vec::new();
    let mut is_group = false;
    if !words.is_empty() {
        let mut consumed = 0;
        let resolved = gp::resolve(&words, &mut consumed);
        let mut named = None;
        if let Err(error) = &resolved
            && error.code == ErrorCode::InvalidArgument
        {
            group = words[..consumed].to_vec();
            named = info_of(&group);
        }
        is_group = named.is_some_and(|named| named.container) && consumed == words.len();
    }
    let text = joined(&words, " ");
    let chosen: Vec<&AlgorithmInfo> = gp::catalogue()
        .iter()
        .filter(|info| {
            words.is_empty()
                || if is_group {
                    under(&info.path, &group)
                } else {
                    gp::path_text(&info.path)
                        .to_ascii_lowercase()
                        .contains(&text)
                        || info.description.to_ascii_lowercase().contains(&text)
                }
        })
        .collect();
    if json {
        let array = katana_core::json::Json::array_of(chosen.iter().copied().map(algorithm_json));
        return Ok(answered("GDAL LIST", array.dump()));
    }
    let mut records: Vec<String> = chosen
        .iter()
        .map(|info| {
            if info.container {
                group_record(info, leaves_under(&info.path))
            } else {
                algorithm_record(info)
            }
        })
        .collect();
    let leaves = chosen.iter().filter(|info| !info.container).count();
    records.push(format!(
        "listed algorithms={leaves} groups={} filter={}",
        chosen.len() - leaves,
        value(&text)
    ));
    Ok(answered("GDAL LIST", joined_records(&records)))
}

// ---- HELP -------------------------------------------------------------------------------------

fn help(tokens: &Tokens, at: usize) -> Result<Prepared> {
    let mut json = false;
    let mut words: Vec<String> = Vec::new();
    for (i, word) in tokens.words.iter().enumerate().skip(at) {
        if tokens.is(i, "JSON") {
            json = true;
        } else {
            words.push(word.clone());
        }
    }
    if words.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("GDAL HELP names an algorithm: {USAGE}"),
        ));
    }
    let mut consumed = 0;
    let path = gp::resolve(&words, &mut consumed)?;
    if consumed != words.len() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "GDAL HELP takes one algorithm",
            words[consumed].clone(),
        ));
    }
    let spec = gp::describe(&path)?;
    if json {
        return Ok(answered("GDAL HELP", describe_json(&spec).dump()));
    }
    let mut records = vec![algorithm_record(&spec.info)];
    for arg in &spec.args {
        records.push(arg_record(arg));
    }
    for arg in &spec.args {
        if arg.is_dataset() && arg.is_input && !arg.is_output {
            records.push(binding_record(arg));
        }
    }
    Ok(answered("GDAL HELP", joined_records(&records)))
}

// ---- RUN ---------------------------------------------------------------------------------------

fn arg_named<'s>(spec: &'s AlgorithmSpec, name: &str) -> Option<&'s ArgSpec> {
    spec.args.iter().find(|arg| {
        arg.name == name
            || (!arg.short_name.is_empty() && arg.short_name == name)
            || arg.aliases.iter().any(|alias| alias == name)
    })
}

/// name=value, when name is an argument's, as GDAL's --name=value: the form
/// the other verbs' options take, so GDAL raster hillshade zfactor=2 reads as
/// a person used to Katana would write it.
fn rewritten(spec: &AlgorithmSpec, word: &str, quoted: bool) -> String {
    if quoted || word.is_empty() || word.starts_with('-') {
        return String::from(word);
    }
    let Some(equals) = word.find('=') else {
        return String::from(word);
    };
    if equals == 0 {
        return String::from(word);
    }
    match arg_named(spec, &word[..equals]) {
        Some(arg) => format!("--{}{}", arg.name, &word[equals..]),
        None => String::from(word),
    }
}

/// The long names the tail's options name outright (--input, -i).
fn named_in_tail(spec: &AlgorithmSpec, tail: &[String]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for word in tail {
        let bytes = word.as_bytes();
        let name = if word.len() > 2 && word.starts_with("--") {
            match word.find('=') {
                Some(equals) => &word[2..equals],
                None => &word[2..],
            }
        } else if word.len() == 2 && bytes[0] == b'-' && bytes[1].is_ascii_alphabetic() {
            &word[1..]
        } else {
            ""
        };
        if !name.is_empty()
            && let Some(arg) = arg_named(spec, name)
        {
            names.insert(arg.name.clone());
        }
    }
    names
}

struct FromClause {
    arg: String,
    source: Source,
}

/// The inputs a run reads, bound at prepare and materialised by the work.
#[derive(Clone)]
struct BoundArg {
    arg: String,
    list: bool,
    datasets: Vec<DeferredDataset>,
}

/// The datasets of a run made, bound by argument: a list argument takes all
/// of its datasets.
fn materialise(bound: &[BoundArg], run: &mut RunRequest) -> Status {
    for arg in bound {
        let mut values: Vec<DatasetValue> = Vec::new();
        for make in &arg.datasets {
            values.push(make()?);
        }
        let value = if arg.list {
            ArgValue::Datasets(values)
        } else {
            ArgValue::Dataset(
                values
                    .into_iter()
                    .next()
                    .expect("an argument bound by FROM has a dataset"),
            )
        };
        run.values.push((arg.arg.clone(), value));
    }
    Ok(())
}

fn run_line(
    context: &mut Context<'_>,
    tokens: &Tokens,
    mut at: usize,
    line: &str,
) -> Result<Prepared> {
    let mut words: Vec<String> = Vec::new();
    let mut i = at;
    while i < tokens.size() && !clause(tokens, i) {
        words.push(tokens[i].clone());
        i += 1;
    }
    if words.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("usage: {USAGE}"),
        ));
    }
    let mut consumed = 0;
    let path = gp::resolve(&words, &mut consumed)?;
    at += consumed;
    let algorithm_path = gp::path_text(&path);
    let spec = gp::describe(&path);
    let (info, spec) = match (info_of(&path), spec) {
        (_, Err(error)) => return Err(error),
        (None, Ok(_)) => {
            return Err(make_error(
                ErrorCode::NotFound,
                "no GDAL algorithm has that path",
                algorithm_path,
            ));
        }
        (Some(info), Ok(spec)) => (info, spec),
    };

    // GDAL's own words, up to the first clause.
    let mut tail: Vec<String> = Vec::new();
    while at < tokens.size() && !clause(tokens, at) {
        tail.push(rewritten(&spec, &tokens[at], tokens.quoted[at]));
        at += 1;
    }
    gp::check_tokens(&path, &tail)?;

    let mut froms: Vec<FromClause> = Vec::new();
    let mut target: Option<Target> = None;
    let mut confirm = false;
    let mut overwrite = false;
    let mut preview = false;
    while at < tokens.size() {
        if tokens.is(at, "FROM") {
            at += 1;
            let mut arg_name = String::new();
            if at < tokens.size()
                && !tokens.quoted[at]
                && !is_source_keyword(tokens, at)
                && !clause(tokens, at)
            {
                match arg_named(&spec, &tokens[at]) {
                    Some(arg) if arg.is_dataset() && !arg.is_output => arg_name = arg.name.clone(),
                    _ => {
                        return Err(make_error(
                            ErrorCode::InvalidArgument,
                            format!(
                                "FROM names an input dataset of {algorithm_path} (GDAL HELP lists \
                                 them) or a source"
                            ),
                            tokens[at].clone(),
                        ));
                    }
                }
                at += 1;
            }
            let source = parse_source(tokens, &mut at)?;
            froms.push(FromClause {
                arg: arg_name,
                source,
            });
        } else if tokens.is(at, "TO") {
            if target.is_some() {
                return Err(Error::new(ErrorCode::InvalidArgument, "one TO per line"));
            }
            at += 1;
            if at < tokens.size()
                && !tokens.quoted[at]
                && let Some(arg) = arg_named(&spec, &tokens[at])
                && arg.is_output
                && arg.is_dataset()
            {
                at += 1; // TO output LAYER ...: the only output there is
            }
            target = Some(parse_target(tokens, &mut at)?);
        } else if tokens.is(at, "CONFIRM") {
            confirm = true;
            at += 1;
        } else if tokens.is(at, "OVERWRITE") {
            overwrite = true;
            at += 1;
        } else if tokens.is(at, "PREVIEW") {
            preview = true;
            at += 1;
        } else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "GDAL's own words come before FROM and TO; after them only FROM, TO, CONFIRM, \
                 OVERWRITE and PREVIEW",
                tokens[at].clone(),
            ));
        }
    }

    if info.policy == Policy::Confirm && !confirm {
        return Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "{algorithm_path} changes or removes data that already exists; add CONFIRM to run \
                 it"
            ),
            algorithm_path,
        ));
    }
    // What GDAL's own words change, a pipeline's steps and options included
    // however the pipeline was quoted: the algorithm's policy alone would let
    // "pipeline read a ! update b" write into b.
    let effects = gp::tail_effects(&path, &tail);
    if !effects.confirm_step.is_empty() && !confirm {
        return Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "the pipeline step {} changes data that already exists; add CONFIRM to run it",
                effects.confirm_step
            ),
            effects.confirm_step,
        ));
    }
    if !effects.overwrite_word.is_empty() && !overwrite {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "{} changes a dataset that already exists; add OVERWRITE to the line to allow it",
                effects.overwrite_word
            ),
            effects.overwrite_word,
        ));
    }

    // Each FROM to its dataset argument: named, or the first required input
    // not yet bound; a list takes every FROM that names it.
    let inputs: Vec<&ArgSpec> = spec
        .args
        .iter()
        .filter(|arg| arg.is_dataset() && arg.is_input && !arg.is_output)
        .collect();
    let named = named_in_tail(&spec, &tail);
    let mut from_count: BTreeMap<String, usize> = BTreeMap::new();
    for from in &mut froms {
        if from.arg.is_empty() {
            // The first required input no FROM has taken; one GDAL's words
            // name as well is refused below as given twice.
            if let Some(arg) = inputs
                .iter()
                .find(|arg| arg.required && !from_count.contains_key(&arg.name))
            {
                from.arg = arg.name.clone();
            }
            // Or an algorithm's one input, required or not: a pipeline's
            // `input` is optional (its read step may name a file instead),
            // and with no other input FROM can mean nothing else. Where
            // several optional inputs are left, which one is meant is not
            // guessed.
            if from.arg.is_empty() && inputs.len() == 1 {
                from.arg = inputs[0].name.clone();
            }
            if from.arg.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "FROM has no required dataset left to bind; name one: FROM <arg> <source> \
                         (GDAL HELP {algorithm_path} lists them)"
                    ),
                ));
            }
        }
        let arg = arg_named(&spec, &from.arg).expect("FROM names one of the algorithm's arguments");
        // A source of a kind the argument does not read is refused here,
        // before anything runs, rather than handed to GDAL, which said only
        // "Unable to fetch band #1" of a drawing given to a raster input.
        let reads_raster = (arg.dataset_kinds & DatasetKind::Raster as u32) != 0;
        let reads_vector = (arg.dataset_kinds & DatasetKind::Vector as u32) != 0;
        let grid = matches!(from.source.kind, SourceKind::Raster | SourceKind::Surface);
        if arg.dataset_kinds != 0
            && ((from.source.kind == SourceKind::Drawing && !reads_vector)
                || (grid && !reads_raster))
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "{} reads {} datasets, and FROM gives it {}; it takes {}",
                    from.arg,
                    gp::dataset_kinds_text(arg.dataset_kinds),
                    if grid { "a raster" } else { "drawing data" },
                    sources_for(arg)
                ),
                from.arg.clone(),
            ));
        }
        let count = from_count.entry(from.arg.clone()).or_insert(0);
        *count += 1;
        let list = arg.r#type == ArgType::DatasetList && arg.max_count != 1;
        if *count > 1 && !list {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{} takes one dataset, and FROM gives it more", from.arg),
                from.arg.clone(),
            ));
        }
        if named.contains(&from.arg) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{} is given twice: by GDAL's words and by FROM", from.arg),
                from.arg.clone(),
            ));
        }
    }

    // Where the output goes.
    let output = spec
        .args
        .iter()
        .find(|arg| arg.is_output && arg.r#type == ArgType::Dataset);
    let from_args: Vec<String> = from_count.keys().cloned().collect();
    let given = gp::arguments_given(&path, &tail, &from_args)?;
    let tail_names_output = output.is_some_and(|output| given.contains(&output.name));
    if tail_names_output && target.is_some() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the output is named twice: in GDAL's words and by TO",
        ));
    }
    if target.is_some() && output.is_none() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{algorithm_path} writes no dataset for TO to take"),
        ));
    }
    let mut to = target.unwrap_or_default();
    if tail_names_output {
        to.kind = TargetKind::File; // the name GDAL's words gave
    }
    let kinds = output.map_or(0, |output| output.dataset_kinds);
    let raster_only = kinds == DatasetKind::Raster as u32;
    let vector_only = kinds == DatasetKind::Vector as u32;
    if to.kind == TargetKind::Layer && raster_only {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "{algorithm_path} makes a raster, which cannot go to a layer; TO REFERENCE \
                 [<name>] keeps it as a reference raster, TO FILE <path> writes it"
            ),
        ));
    }
    if to.kind == TargetKind::Reference && vector_only {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "{algorithm_path} makes features, which cannot be a reference raster; TO LAYER \
                 <path> draws them, TO FILE <path> writes them"
            ),
        ));
    }
    if to.kind == TargetKind::File
        && !tail_names_output
        && path_from_text(&to.name).exists()
        && !overwrite
    {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "the file exists; add OVERWRITE to replace it",
            to.name.clone(),
        ));
    }

    let mut request = RunRequest {
        path: path.clone(),
        tokens: tail.clone(),
        overwrite,
        ..RunRequest::default()
    };
    if to.kind == TargetKind::File && !tail_names_output {
        request.output_to = OutputTo::File;
        request.output_path = to.name.clone();
        request.output_format = to.format.clone();
    } else {
        // Every raster result is written where a derived raster is kept, so
        // it becomes a reference raster without passing through memory.
        request.output_to = OutputTo::Memory;
        request.max_memory_cells = 0;
        request.spill_directory = path_text(&derived_folder(context));
    }

    // The sources: the drawing's copied now, the rest made by the work.
    let mut records: Vec<String> = Vec::new();
    let mut bound: Vec<BoundArg> = Vec::new();
    let mut took_nothing = false;
    for from in &mut froms {
        let arg = arg_named(&spec, &from.arg).expect("FROM names one of the algorithm's arguments");
        let slot = match bound.iter().position(|each| each.arg == from.arg) {
            Some(slot) => slot,
            None => {
                bound.push(BoundArg {
                    arg: from.arg.clone(),
                    list: arg.r#type == ArgType::DatasetList,
                    datasets: Vec::new(),
                });
                bound.len() - 1
            }
        };
        if from.source.kind == SourceKind::Drawing {
            let options = DrawingDatasetOptions {
                crs_wkt: project_crs(context),
                // An algorithm that reads Z is given only what has a height at
                // every vertex: GDAL reads a missing one as 0.
                require_heights: gp::reads_heights(&path, &tail),
                ..DrawingDatasetOptions::default()
            };
            let drawing = bind_drawing(context, &from.source.scope, &options)?;
            records.push(input_record(&from.arg, &from.source));
            records.push(scope_record(&from.arg, &drawing));
            for warning in &drawing.dataset.stats.warnings {
                records.push(warning_record(warning));
            }
            took_nothing = took_nothing || drawing.dataset.set.tables.is_empty();
            let set = Arc::new(drawing.dataset.set);
            let deferred: DeferredDataset = Arc::new(move || -> Result<DatasetValue> {
                Ok(DatasetValue::Features((*set).clone()))
            });
            bound[slot].datasets.push(deferred);
            continue;
        }
        if from.source.kind == SourceKind::Surface
            && from.source.cell.is_none()
            && let Some(surface) = context.surfaces.find(&from.source.surface)
        {
            let tin = surface
                .surface
                .as_ref()
                .expect("a drawing's surface has a triangulation");
            from.source.cell = Some(seams::suggested_cell_size(tin.bounds())?);
        }
        let dataset = bind_raster(context, &from.source)?;
        records.push(input_record(&from.arg, &from.source));
        bound[slot].datasets.push(dataset);
    }

    // A scope that took nothing is said, as Global Modify's preview says it,
    // and nothing runs: GDAL has no dataset to be given.
    if took_nothing {
        let mut reply = vec![format!(
            "gdal algorithm={} policy={} seconds=0.000 cancelled=no ran=no",
            value(&algorithm_path),
            gp::to_string(info.policy)
        )];
        reply.extend(records);
        return Ok(answered(
            format!("GDAL {algorithm_path}"),
            joined_records(&reply),
        ));
    }

    if preview {
        let mut check = request.clone();
        materialise(&bound, &mut check)?;
        gp::validate(&check)?;
        let mut reply = vec![format!(
            "gdal algorithm={} policy={} preview=yes",
            value(&algorithm_path),
            gp::to_string(info.policy)
        )];
        reply.extend(records);
        reply.push(String::from("preview valid=yes changed=no"));
        return Ok(answered(
            format!("GDAL {algorithm_path}"),
            joined_records(&reply),
        ));
    }

    let apply = ApplyRequest {
        target: to,
        arg: output.map_or_else(|| String::from("output"), |output| output.name.clone()),
        default_name: path.last().cloned().unwrap_or_default(),
        result: ResultOptions {
            operation: algorithm_path.clone(),
            command_name: String::from(trimmed_text(line)),
            ..ResultOptions::default()
        },
    };
    let algorithm = info.clone();

    // A file TO names is written beside it and put in place by the apply
    // (staged_files.rs), as EXPORT's is: with OVERWRITE, GDAL wrote over the
    // file from the start, and a run cancelled part way left a partial file
    // where the original had been. A file GDAL's own words name is GDAL's to
    // write where they say.
    let staged = request.output_to == OutputTo::File;
    let staged_file = request.output_path.clone();

    let work = make_work(move |stop, progress| {
        let mut run = request;
        materialise(&bound, &mut run)?;
        let mut files: Option<Arc<StagedFiles>> = None;
        if staged {
            let made = StagedFiles::beside(&path_from_text(&staged_file))?;
            run.output_path = path_text(made.write_to());
            files = Some(made);
        }
        let mut kept = match gp::run(&run, stop, progress) {
            Ok(outputs) => outputs,
            Err(error) => {
                return Err(match &files {
                    Some(files) => files.as_target(error),
                    None => error,
                });
            }
        };
        if files.is_some() {
            kept.file = Some(staged_file);
        }
        Ok(make_apply(move |context| {
            if let Some(files) = &files {
                files.place()?;
            }
            let seconds = kept.seconds;
            let applied = apply_outputs(context, &apply, kept)?;
            let mut reply = vec![gdal_record(&algorithm, seconds)];
            reply.extend(records);
            if !applied.is_empty() {
                reply.push(applied);
            }
            Ok(joined_records(&reply))
        }))
    });
    Ok(Prepared {
        title: format!("GDAL {algorithm_path}"),
        reply: None,
        work: Some(work),
    })
}

/// The verb, its row in the verb table's F0 block (verb_table.rs).
pub fn prepare_gdal(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    let mut at = 1;
    if at >= tokens.size() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("usage: {USAGE}"),
        ));
    }
    if tokens.is(at, "VERSION") {
        if at + 1 != tokens.size() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "GDAL VERSION takes nothing more",
                tokens[at + 1].clone(),
            ));
        }
        return Ok(answered("GDAL VERSION", version_record(&gp::versions())));
    }
    if tokens.is(at, "LIST") {
        return list(tokens, at + 1);
    }
    if tokens.is(at, "HELP") {
        return help(tokens, at + 1);
    }
    if tokens.is(at, "RUN") {
        at += 1;
    }
    run_line(context, tokens, at, line)
}

/// The verb's usage, as its refusals give it.
pub fn gdal_usage() -> String {
    String::from(USAGE)
}
