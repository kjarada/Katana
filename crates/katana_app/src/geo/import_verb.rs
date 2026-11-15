//! IMPORT (docs/interop.md, "IMPORT, EXPORT, INFO, REFS and COPC on every
//! front end"), the port of src/katana_app/geo/import_verb.cpp: a file into
//! the drawing - a DXF natively, vector data and archives (.12da) as entities,
//! rasters and point clouds as reference layers - run as the one executor runs
//! every verb. The work reads the file (on a worker in the window); the apply
//! adds what it read as ONE undo step and says so in records.
//!
//! ```text
//! IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN] [<options>] [<scope> [clip]] [PREVIEW]
//! ```
//!
//! The options (docs/interop.md, "Import options") say what of the file is
//! read and how: for vector data layers=, where=, sql= with dialect=, fields=,
//! target=, max=, oo=K=V (repeated), attributes=no; for every kind crs=
//! (project reprojects vector data into the project's coordinate system,
//! adopt sets the project's from the file when it has none) and srs= (the CRS
//! of a file that declares none); for a raster band=, subdataset=,
//! maxpixels=, name=; for a point cloud budget=, class=, resolution=, name=.
//! A scope (the shared grammar, cad's parse_scope_words) limits vector data to
//! what lies in its box, handed to the driver as its spatial filter; with
//! `clip` features are cut at its edge (AREA, VIEW) or by the closed shapes it
//! takes. PREVIEW reads with the filters and says how many features matched of
//! how many, and imports nothing.
//!
//! The readers take no stop: a cancel lands when the read ends, and the job,
//! cancelled, never applies, so a cancelled IMPORT imports nothing.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(x > 0.0) refuses NaN with the rest; kept as it is."
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use katana_cad::scope_verbs::{
    ScopeMatch, ScopeSource, is_scope_word, match_scope, scope_record as cad_scope_record,
};
use katana_cad::terrain::surface_verbs::surface_record;
use katana_cad::{
    CommandInterpreter, ImportPlacement, ImportPlacementMode, ImportShift, import_placement,
};
use katana_commands::{
    CommandPtr, Transaction, create_alignment, create_entities, create_layer, create_style,
};
use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_dxf::{DxfImport, is_dxf_path};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{ColourTable, Layer, Model};
use katana_geometry::{Box2, Vec2};
use katana_gis::gdal_adapter::{describe_crs, same_crs};
use katana_gis::processing::FeatureSet;
use katana_gis::reproject::crs_to_wkt;
use katana_gis::{CrsBox, GeometryKind};
use katana_interop::archive12d::import_archive12d;
use katana_interop::geo::DrawingDatasetOptions;
use katana_interop::geo::drawing_dataset::drawing_dataset;
use katana_interop::import::{
    advise_placement, import_point_cloud, import_raster, import_vector, kind_for_path,
};
use katana_interop::{
    Archive12dImportOptions, Archive12dImportResult, PointCloudImportOptions, PointCloudLayer,
    RasterImportOptions, RasterOverlay, SourceKind, VectorImportOptions, VectorImportResult,
};
use katana_terrain::NamedSurface;

use super::bindings::{Tokens, crs_differs, project_crs};
use super::geo_verbs::{
    Context, FarApartChoice, ImportShown, Prepared, make_apply, make_work, stop_requested,
    trimmed_text,
};
use super::gis_records::{reference_record_of_point_cloud, reference_record_of_raster};
use super::replies::{value, yes_no};
use super::vector_support::{
    VerbWords, WordRules, choice_option, joined, list_of, number_option, read_verb_words,
};
use crate::dxf_verbs::{apply_dxf_import, read_dxf_import};
use crate::import_records::{
    bounds_text, path_from_text, path_text, placed_record, tally_record, warning_text,
};

const USAGE: &str = "usage: IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN] [layers=a,b] \
                     [where=\"...\"] [sql=\"...\"] [dialect=ogrsql|sqlite] [<scope> [clip]] \
                     [fields=a,b] [target=<layer>] [max=N] [oo=K=V]... [attributes=no] \
                     [crs=project|adopt] [srs=<code>] [band=N] [subdataset=N|name] \
                     [maxpixels=N] [name=<n>] [budget=N] [class=N] [resolution=<m>] [PREVIEW]";

/// One option key, lower case, and the kinds of data it is for. oo= is read
/// apart: it may be given more than once.
struct OptionUse {
    key: &'static str,
    vector: bool,
    raster: bool,
    cloud: bool,
}

const fn option_use(key: &'static str, vector: bool, raster: bool, cloud: bool) -> OptionUse {
    OptionUse {
        key,
        vector,
        raster,
        cloud,
    }
}

const OPTIONS: [OptionUse; 18] = [
    option_use("layers", true, false, false),
    option_use("where", true, false, false),
    option_use("sql", true, false, false),
    option_use("dialect", true, false, false),
    option_use("fields", true, false, false),
    option_use("target", true, false, false),
    option_use("max", true, false, false),
    option_use("attributes", true, false, false),
    option_use("offset", true, false, false),
    option_use("crs", true, true, true),
    option_use("srs", true, true, false),
    option_use("band", false, true, false),
    option_use("subdataset", false, true, false),
    option_use("maxpixels", false, true, false),
    option_use("name", false, true, true),
    option_use("budget", false, false, true),
    option_use("class", false, false, true),
    option_use("resolution", false, false, true),
];

fn is_option_key(key: &str) -> bool {
    key == "oo" || OPTIONS.iter().any(|each| key == each.key)
}

/// Whether word `i` begins the words after the path: an option, a flag, a
/// placement or a scope word. A path is read up to the first of them, so a
/// path with a blank in it and no quotes still reads - as it always did - and
/// one that holds such a word is given in quotes.
fn begins_options(tokens: &Tokens, i: usize) -> bool {
    let word = &tokens[i];
    if let Some(equals) = word.find('=')
        && equals > 0
        && is_option_key(&word[..equals].to_ascii_lowercase())
    {
        return true;
    }
    if tokens.quoted[i] {
        return false;
    }
    tokens.is(i, "LOCAL")
        || tokens.is(i, "ALONGSIDE")
        || tokens.is(i, "CLIP")
        || tokens.is(i, "PREVIEW")
        || is_scope_word(word)
}

/// `pathText(file.filename())`: the name of the file a path names, as a
/// record writes it; nothing for a path that ends in a separator, which is
/// what the C++'s filename() gives there (Rust's file_name would give the
/// folder's name). EXPORT's titles and records use it too.
pub(super) fn file_name_text(path: &Path) -> String {
    let text = path.as_os_str().as_encoded_bytes();
    if text.is_empty() || text.ends_with(b"/") || (cfg!(windows) && text.ends_with(b"\\")) {
        return String::new();
    }
    path.file_name()
        .map(|name| path_text(Path::new(name)))
        .unwrap_or_default()
}

/// `pathText(file.extension())`: ".xyzzy", or nothing for a name with no
/// extension (".bashrc" has none, "." and ".." neither).
fn extension_text(path: &Path) -> String {
    let name = file_name_text(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 && name != ".." => String::from(&name[dot..]),
        _ => String::new(),
    }
}

fn cancelled() -> Error {
    Error::new(ErrorCode::InvalidState, "cancelled")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Dxf,
    Vector,
    Archive,
    Raster,
    PointCloud,
}

/// What the work read, one alternative per kind.
#[allow(
    clippy::large_enum_variant,
    reason = "The C++ std::variant of the readers' results; one is made per import and moved \
              whole into the apply."
)]
enum Read {
    Dxf(DxfImport),
    Vector(VectorImportResult),
    Archive(Archive12dImportResult),
    Raster(RasterOverlay),
    PointCloud(PointCloudLayer),
}

/// Data that becomes entities can be placed; reference data is drawn at its
/// own coordinates and cannot.
fn placeable(kind: Kind) -> bool {
    matches!(kind, Kind::Dxf | Kind::Vector | Kind::Archive)
}

/// What a line asked of the read and of the apply, beside the placement.
#[derive(Default)]
struct Request {
    archive_colours: ColourTable,
    vector: VectorImportOptions,
    raster: RasterImportOptions,
    cloud: PointCloudImportOptions,
    /// crs=adopt: the project takes the file's coordinate system when it has
    /// none, in the same undo step as what the import adds.
    adopt: bool,
    preview: bool,
    /// Whether the reply says how many features the filters took (matched).
    report_match: bool,
    /// What the scope took, said in the reply (cad's scope_record), and the
    /// box.
    scope_record: String,
}

fn read_at(kind: Kind, file: &Path, shift: Option<Vec2>, request: &Request) -> Result<Read> {
    match kind {
        Kind::Dxf => Ok(Read::Dxf(read_dxf_import(file, shift)?)),
        Kind::Vector => {
            let options = VectorImportOptions {
                origin_shift: shift,
                ..request.vector.clone()
            };
            Ok(Read::Vector(import_vector(file, &options)?))
        }
        Kind::Archive => {
            let options = Archive12dImportOptions {
                colours: request.archive_colours.clone(),
                origin_shift: shift,
                ..Archive12dImportOptions::default()
            };
            Ok(Read::Archive(import_archive12d(file, &options)?))
        }
        Kind::Raster => Ok(Read::Raster(import_raster(file, &request.raster)?)),
        Kind::PointCloud => Ok(Read::PointCloud(import_point_cloud(file, &request.cloud)?)),
    }
}

fn bounds_of(read: &Read) -> Box2 {
    match read {
        Read::Dxf(read) => read.bounds,
        Read::Vector(read) => read.bounds,
        Read::Archive(read) => read.bounds,
        Read::Raster(read) => read.world_bounds(),
        Read::PointCloud(read) => read.world_bounds(),
    }
}

// ---- the applies, one per kind ------------------------------------------------------------------

/// crs=adopt: the command that gives the project the file's coordinate
/// system, or None - with `record` saying why not - when the project has one
/// already (adopting is for a drawing that has none; CRS SET changes one) or
/// the file declares none.
fn adopt_crs(context: &Context<'_>, wkt: &str, record: &mut String) -> Result<Option<CommandPtr>> {
    let project = &context.document.metadata().coordinate_system;
    if !project.is_empty() {
        *record = format!(
            "crs adopted=no project={} reason={}",
            value(project),
            value("the project has a coordinate system already")
        );
        return Ok(None);
    }
    if wkt.is_empty() {
        *record = format!(
            "crs adopted=no reason={}",
            value("the file declares no coordinate system; srs= says which it is in")
        );
        return Ok(None);
    }
    context
        .document
        .coordinate_system_command(wkt, "IMPORT crs=adopt")
}

/// crs=adopt for reference data, which adds no undo step of its own: the
/// project's coordinate system alone is the step.
fn adopt_alone(context: &mut Context<'_>, wkt: &str) -> Result<String> {
    let mut record = String::new();
    let Some(command) = adopt_crs(context, wkt, &mut record)? else {
        return Ok(record);
    };
    context.document.execute(command)?;
    Ok(format!(
        "crs adopted=yes id={}",
        value(&context.document.metadata().coordinate_system)
    ))
}

/// The warning an import's reply carries when what it drew is in another
/// coordinate system than the project's: drawn as they are, its coordinates
/// do not register against the drawing's. Nothing when either is unknown, or
/// when the project has just adopted the file's.
fn crs_warning(context: &Context<'_>, wkt: &str, consequence: &str) -> Option<String> {
    crs_differs(wkt, &project_crs(context), "the file", consequence)
        .map(|differs| warning_text(&differs))
}

fn apply_vector(
    context: &mut Context<'_>,
    mut read: VectorImportResult,
    file: &Path,
    request: &Request,
) -> Result<String> {
    // crs=project moved the data into the project's system as it was when the
    // line was prepared; a CRS SET since would draw it in the wrong one.
    if !request.vector.target_crs.is_empty()
        && same_crs(&request.vector.target_crs, &project_crs(context)) != Some(true)
    {
        return Err(make_error(
            ErrorCode::InvalidState,
            "the project's coordinate system changed while the import ran, and crs=project \
             moved the data into the one it had; import again",
            path_text(file),
        ));
    }
    // Layers before the entities on them, and all of it ONE undo step: an
    // import made by mistake is one Ctrl+Z - the project's coordinate system
    // too, when crs=adopt sets it.
    let mut transaction = Transaction::new(format!("IMPORT {}", file_name_text(file)));
    let mut crs_record = String::new();
    let mut adopted = false;
    if request.adopt
        && let Some(command) = adopt_crs(context, &read.projection_wkt, &mut crs_record)?
    {
        transaction.add(command);
        adopted = true;
    }
    for name in &read.layers_needed {
        if !context.document.model().layers.contains(name) {
            transaction.add(create_layer(Layer {
                name: name.clone(),
                ..Layer::default()
            }));
        }
    }
    let count = read.entities.len();
    if count != 0 {
        transaction.add(create_entities(std::mem::take(&mut read.entities)));
    }
    // A file with nothing the drawing can hold leaves the transaction empty,
    // which the stack refuses as a command that changes nothing: not a failed
    // import - the file was read, and its warnings say why.
    if transaction.size() != 0 {
        context.document.execute(Box::new(transaction))?;
    }
    let crs = if read.projection_wkt.is_empty() {
        String::new()
    } else {
        describe_crs(&read.projection_wkt)
    };
    let mut records = vec![format!(
        "imported file={} kind=vector entities={count} layers={} features={} skipped={} \
         bounds={} crs={}",
        value(&path_text(file)),
        read.layers_needed.len(),
        read.features_read,
        read.features_skipped,
        bounds_text(&read.bounds),
        value(&crs)
    )];
    if request.report_match {
        records.push(format!(
            "matched features={} of={}",
            read.features_read, read.features_in_file
        ));
    }
    if adopted {
        crs_record = format!(
            "crs adopted=yes id={}",
            value(&context.document.metadata().coordinate_system)
        );
    }
    if !crs_record.is_empty() {
        records.push(crs_record);
    }
    for warning in &read.warnings {
        records.push(warning_text(warning));
    }
    if let Some(warning) = crs_warning(
        context,
        &read.projection_wkt,
        "its coordinates were drawn as they are; crs=project moves them into the project's",
    ) {
        records.push(warning);
    }
    Ok(joined(&records))
}

fn apply_archive(
    context: &mut Context<'_>,
    mut read: Archive12dImportResult,
    file: &Path,
) -> Result<String> {
    // Layers, styles, entities and alignments: ONE undo step.
    let step = format!("IMPORT {}", file_name_text(file));
    let mut transaction = Transaction::new(step.clone());
    for layer in &read.layers_needed {
        if !context.document.model().layers.contains(&layer.name) {
            transaction.add(create_layer(layer.clone()));
        }
    }
    for style in &read.styles_needed {
        if !context.document.model().styles.contains(&style.name) {
            transaction.add(create_style(style.clone()));
        }
    }
    let count = read.entities.len();
    if count != 0 {
        transaction.add(create_entities(std::mem::take(&mut read.entities)));
    }
    // Every alignment is added, renamed when it must be.
    let alignments = read.alignments.len();
    for alignment in &mut read.alignments {
        // A name the drawing already has would fail the whole transaction.
        let base = alignment.name.clone();
        let mut copy = 2;
        while context
            .document
            .model()
            .alignments
            .contains(&alignment.name)
        {
            alignment.name = format!("{base} ({copy})");
            copy += 1;
        }
        transaction.add(create_alignment(alignment.clone()));
    }
    // The archive's TINs are the drawing's surfaces, in the SAME step as its
    // entities: one Ctrl+Z takes back the whole import. A surface whose name
    // is taken is "name (2)". Put in the phase's staging copy, then committed
    // inside the transaction; the executor then finds nothing left to commit.
    let surface_count = read.surfaces.len();
    let mut surface_records = Vec::new();
    for surface in std::mem::take(&mut read.surfaces) {
        // The source text a surface keeps is persisted with the project, and
        // the C++ writes it with the archive format's name: ported as the
        // C++ has it, so both programs' projects compare equal, until the
        // owner's pending decision on that word changes both at once.
        let named = NamedSurface {
            name: context.surfaces.unique_name(surface.name.as_bytes()),
            surface: Some(Arc::new(surface.surface)),
            source: format!("12d archive {}", file_name_text(file)),
            ..NamedSurface::default()
        };
        context.surfaces.add(named.clone())?;
        surface_records.push(surface_record(&named, context.document.surface_styles()));
    }
    if surface_count != 0
        && let Some(command) = context
            .document
            .surfaces_command(context.surfaces.clone(), step)?
    {
        transaction.add(command);
    }
    // An archive of meshes or clouds alone has nothing for the drawing, and
    // the stack refuses an empty transaction: only one with something in it
    // runs, and the session data below is added either way.
    if transaction.size() != 0 {
        context.document.execute(Box::new(transaction))?;
    }

    let mut records = vec![format!(
        "imported file={} kind=archive entities={count} alignments={alignments} surfaces={} \
         meshes={} clouds={} layers={} styles={} encoding={} version={} member={} bounds={}",
        value(&path_text(file)),
        surface_count,
        read.meshes.len(),
        read.clouds.len(),
        read.layers_needed.len(),
        read.styles_needed.len(),
        value(&read.encoding),
        value(&read.archive_version),
        value(&read.member_name),
        bounds_text(&read.bounds)
    )];
    for tally in &read.tally {
        records.push(tally_record(&tally.keyword, tally.read, tally.imported));
    }

    records.extend(surface_records);
    // Clouds are session data, outside undo (interop's reference_data),
    // added after the transaction so a refused import leaves none behind.
    let cloud_count = read.clouds.len();
    for cloud in std::mem::take(&mut read.clouds) {
        let id = context.reference.add(cloud);
        if let Some(added) = context.reference.find_point_cloud(id) {
            records.push(reference_record_of_point_cloud(added));
        }
    }
    let mut shown = ImportShown {
        surfaces: surface_count,
        ..ImportShown::default()
    };
    if !read.meshes.is_empty() {
        let triangles: usize = read
            .meshes
            .iter()
            .map(|mesh| mesh.mesh.triangle_count())
            .sum();
        let held = context.front_end.imported.is_some();
        records.push(format!(
            "meshes count={} triangles={triangles} held={}",
            read.meshes.len(),
            yes_no(held)
        ));
        if !held {
            // Read and checked, and nowhere to keep them: said in numbers so
            // they are not taken for kept.
            records.push(warning_text(
                "this session holds no meshes; import the archive in the window to see them",
            ));
        }
        shown.meshes = std::mem::take(&mut read.meshes);
    }
    for warning in &read.warnings {
        records.push(warning_text(warning));
    }
    if cloud_count != 0
        && let Some(changed) = &context.front_end.changed
    {
        changed();
    }
    if let Some(imported) = &context.front_end.imported
        && (shown.surfaces != 0 || !shown.meshes.is_empty())
    {
        imported(shown);
    }
    Ok(joined(&records))
}

fn apply_raster(
    context: &mut Context<'_>,
    read: RasterOverlay,
    file: &Path,
    request: &Request,
) -> Result<String> {
    let mut crs_record = String::new();
    if request.adopt {
        crs_record = adopt_alone(context, &read.projection_wkt)?;
    }
    let georeferenced = read.has_geotransform;
    let differs = crs_warning(
        context,
        &read.projection_wkt,
        "it is drawn at its own coordinates; RASTER REPROJECT makes a copy in the project's",
    );
    let id = context.reference.add(read);
    let mut records = vec![format!(
        "imported file={} kind=raster",
        value(&path_text(file))
    )];
    if let Some(added) = context.reference.find_raster(id) {
        records.push(reference_record_of_raster(added));
    }
    if !crs_record.is_empty() {
        records.push(crs_record);
    }
    if !georeferenced {
        // Placing it at the origin is a guess, and whoever imported it has to
        // know that.
        records.push(warning_text(
            "this file carries no georeferencing; it is placed at the origin at one model unit \
             per pixel",
        ));
    }
    if let Some(differs) = differs {
        records.push(differs);
    }
    if let Some(changed) = &context.front_end.changed {
        changed();
    }
    Ok(joined(&records))
}

fn apply_cloud(
    context: &mut Context<'_>,
    read: PointCloudLayer,
    file: &Path,
    request: &Request,
) -> Result<String> {
    let mut crs_record = String::new();
    if request.adopt {
        crs_record = format!("\n{}", adopt_alone(context, &read.projection_wkt)?);
    }
    let differs = crs_warning(
        context,
        &read.projection_wkt,
        "it is drawn at its own coordinates, where the drawing's are not",
    );
    let id = context.reference.add(read);
    let reference = context
        .reference
        .find_point_cloud(id)
        .map(reference_record_of_point_cloud)
        .unwrap_or_default();
    let records = format!(
        "imported file={} kind=pointcloud\n{reference}{crs_record}{}",
        value(&path_text(file)),
        differs.map_or_else(String::new, |differs| format!("\n{differs}"))
    );
    if let Some(changed) = &context.front_end.changed {
        changed();
    }
    Ok(records)
}

/// What the work read, placed and applied.
struct Imported {
    kind: Kind,
    file: PathBuf,
    placement: ImportPlacement,
    placed: ImportShift,
    /// The drawing's extent when the line was prepared.
    drawing: Box2,
    read: Read,
    request: Request,
}

/// `text` with `record` put in after its first line: where a record that
/// belongs to the first goes.
fn after_first_line(text: &mut String, record: &str) {
    let end = text.find('\n').unwrap_or(text.len());
    text.insert_str(end, &format!("\n{record}"));
}

fn apply_import(context: &mut Context<'_>, mut imported: Imported) -> Result<String> {
    let mut extra = Vec::new();
    // Data that keeps its own coordinates and lands far from the drawing:
    // merged, one of the two would be a dot at a zoom that shows both. The
    // window asks (Shift Alongside, Keep, Cancel) BEFORE anything is added; a
    // session, and a window with nobody to ask, keeps the coordinates and says
    // what to type instead.
    if placeable(imported.kind) && imported.placement.mode == ImportPlacementMode::Keep {
        let incoming = bounds_of(&imported.read);
        let advice = advise_placement(&imported.drawing, &incoming);
        if advice.far_apart {
            let choice = match &context.front_end.far_apart {
                Some(ask) => ask(&imported.drawing, &incoming, advice.message.as_str()),
                None => FarApartChoice::Keep,
            };
            if choice == FarApartChoice::Cancel {
                return Err(make_error(
                    ErrorCode::InvalidState,
                    "cancelled: nothing was imported",
                    path_text(&imported.file),
                ));
            }
            if choice == FarApartChoice::Alongside {
                imported.placement = ImportPlacement {
                    mode: ImportPlacementMode::Alongside,
                    offset: Vec2::default(),
                };
                imported.placed = import_placement::resolve_import_shift(
                    &imported.placement,
                    &imported.drawing,
                    &incoming,
                );
                // Read again with the shift, as a line saying ALONGSIDE is:
                // the one reader moves everything alike.
                imported.read = read_at(
                    imported.kind,
                    &imported.file,
                    imported.placed.shift,
                    &imported.request,
                )?;
            } else {
                extra.push(warning_text(&format!(
                    "{}; UNDO, then IMPORT <file> LOCAL moves it as one piece so its lower-left \
                     corner sits at 0,0, or IMPORT <file> ALONGSIDE puts that corner on the \
                     drawing's",
                    advice.message
                )));
            }
        }
    }

    let bounds = bounds_of(&imported.read);
    let Imported {
        kind,
        file,
        placement,
        placed,
        read,
        request,
        ..
    } = imported;
    let mut reply = match read {
        Read::Dxf(read) => {
            apply_dxf_import(&mut *context.document, read, &file, &placement, &placed)
        }
        Read::Vector(read) => apply_vector(context, read, &file, &request),
        Read::Archive(read) => apply_archive(context, read, &file),
        Read::Raster(read) => apply_raster(context, read, &file, &request),
        Read::PointCloud(read) => apply_cloud(context, read, &file, &request),
    }?;
    // The DXF records carry their own placed record; the others' goes after
    // the imported record, their first line.
    if kind != Kind::Dxf {
        let record = placed_record(&placement, &placed);
        if !record.is_empty() {
            after_first_line(&mut reply, &record);
        }
    }
    if !request.scope_record.is_empty() {
        after_first_line(&mut reply, &request.scope_record);
    }
    for record in &extra {
        reply.push('\n');
        reply += record;
    }
    if let Some(frame) = &context.front_end.frame
        && !bounds.empty()
    {
        frame(&bounds);
    }
    Ok(reply)
}

// ---- reading the line ---------------------------------------------------------------------------

/// The option `key` as a whole number in [low, high]; None when not given.
fn whole_option(words: &VerbWords, key: &str, low: f64, high: f64) -> Result<Option<u64>> {
    let Some(given) = number_option(words, key)? else {
        return Ok(None);
    };
    if given.floor() != given || given < low || given > high {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "{key}= is a whole number from {} to {}",
                format_exact_real(low),
                format_exact_real(high)
            ),
            format!("{key}={}", words.option(key).map_or("", String::as_str)),
        ));
    }
    // In range, and whole: the cast is exact.
    Ok(Some(given as u64))
}

/// The box a scope stands for: an AREA's box; a VIEW's visible area; else the
/// extent of what it took. Empty when it took nothing.
fn scope_box(found: &ScopeMatch, model: &Model) -> Box2 {
    if found.resolved.words.source == ScopeSource::Area {
        return found.resolved.words.area;
    }
    if found.resolved.words.source == ScopeSource::View
        && let Some(area) = found.resolved.scope.area
    {
        return area;
    }
    let mut taken = Box2::default();
    for &id in &found.matched {
        if let Some(entity) = model.entities.find(id) {
            let one = bounding_box(&entity.geometry);
            if !one.empty() {
                taken.expand(one.min);
                taken.expand(one.max);
            }
        }
    }
    taken
}

fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Dxf => "dxf",
        Kind::Vector => "vector",
        Kind::Archive => "archive",
        Kind::Raster => "raster",
        Kind::PointCloud => "pointcloud",
    }
}

/// The verb, its row in the verb table's I0 block (verb_table.rs).
pub fn prepare_import(context: &mut Context<'_>, tokens: &Tokens, line: &str) -> Result<Prepared> {
    // The path, then the words after it. A quoted path is one word; an
    // unquoted one runs to the first option, flag, placement or scope word, so
    // a path with blanks still reads without quotes. With nothing after it but
    // a placement, the line reads exactly as the interpreter reads an IMPORT
    // line (CommandInterpreter::import_argument), whichever front end it came
    // from.
    if tokens.size() < 2 {
        return Err(Error::new(ErrorCode::InvalidArgument, USAGE));
    }
    let mut first = 2;
    let mut path = if tokens.quoted[1] {
        tokens[1].clone()
    } else {
        while first < tokens.size() && !begins_options(tokens, first) {
            first += 1;
        }
        tokens.words[1..first].join(" ")
    };
    let mut placement = ImportPlacement::default();
    let placement_only = first == tokens.size()
        || (first + 1 == tokens.size()
            && import_placement::parse_placement_word(&tokens[first])
                .ok()
                .flatten()
                .is_some());
    if placement_only {
        let body = trimmed_text(line);
        let rest = body.find([' ', '\t']).map_or("", |blank| &body[blank..]);
        let argument = CommandInterpreter::import_argument(rest)?;
        path = argument.path;
        placement = argument.placement;
        first = tokens.size();
    }
    if trimmed_text(&path).is_empty() || begins_options(tokens, 1) {
        return Err(Error::new(ErrorCode::InvalidArgument, USAGE));
    }
    let file = path_from_text(&path);

    // oo= may be given more than once: taken out before the rest are read.
    let mut rest = Tokens::default();
    let mut open_options: Vec<String> = Vec::new();
    for (word, &quoted) in tokens.words[first..].iter().zip(&tokens.quoted[first..]) {
        if word.len() > 3 && word.as_bytes()[..3].eq_ignore_ascii_case(b"oo=") {
            open_options.push(String::from(&word[3..]));
            continue;
        }
        rest.words.push(word.clone());
        rest.quoted.push(quoted);
    }
    let rules = WordRules {
        options: OPTIONS.iter().map(|each| String::from(each.key)).collect(),
        flags: ["LOCAL", "ALONGSIDE", "CLIP", "PREVIEW"]
            .map(String::from)
            .to_vec(),
        usage: String::from(USAGE),
        ..WordRules::default()
    };
    let words = read_verb_words(&rest, 0, rest.size(), &rules)?;

    // The placement, from its words.
    let placements = usize::from(words.has("LOCAL"))
        + usize::from(words.has("ALONGSIDE"))
        + usize::from(words.option("offset").is_some());
    if placements > 1 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "one placement per line: LOCAL, ALONGSIDE or OFFSET=dE,dN",
        ));
    }
    if words.has("LOCAL") {
        placement = ImportPlacement {
            mode: ImportPlacementMode::Local,
            offset: Vec2::default(),
        };
    } else if words.has("ALONGSIDE") {
        placement = ImportPlacement {
            mode: ImportPlacementMode::Alongside,
            offset: Vec2::default(),
        };
    } else if let Some(offset) = words.option("offset")
        && let Some(parsed) = import_placement::parse_placement_word(&format!("OFFSET={offset}"))?
    {
        placement = parsed;
    }

    let kind = if is_dxf_path(&file) {
        Kind::Dxf
    } else {
        match kind_for_path(&file) {
            SourceKind::Vector => Kind::Vector,
            SourceKind::Archive12d => Kind::Archive,
            SourceKind::Raster => Kind::Raster,
            SourceKind::PointCloud => Kind::PointCloud,
            SourceKind::Unknown => {
                // `katana <path>` opens a folder as a project and imports
                // anything else, so a project folder mistyped arrives here as
                // a file with no extension that is not there. That is said as
                // what it is; a file that IS there with no extension keeps the
                // message every unknown extension has.
                let extension = extension_text(&file);
                if extension.is_empty() && !file.exists() {
                    return Err(Error::new(
                        ErrorCode::NotFound,
                        format!("no such project directory or file: {}", path_text(&file)),
                    ));
                }
                return Err(make_error(
                    ErrorCode::Unsupported,
                    format!("no importer for '{extension}'"),
                    path_text(&file),
                ));
            }
        }
    };
    // Refused by name rather than dropped - or, as LOCAL once was, left on the
    // end of the path to fail as a missing file.
    if !placeable(kind) && placement.mode != ImportPlacementMode::Keep {
        let word = if placement.mode == ImportPlacementMode::Offset {
            String::from("OFFSET")
        } else {
            import_placement::placement_word(&placement)
        };
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "{word} is not supported for rasters and point clouds, which are reference data \
                 drawn at their own coordinates"
            ),
        ));
    }
    // An option for another kind of data is refused by name, never ignored: a
    // band= on a shapefile would otherwise look as if it had been used.
    for (key, text) in &words.options {
        if key == "offset" {
            continue;
        }
        // The keys are the table's own (read_verb_words takes no other), each
        // there once.
        let fits = OPTIONS.iter().any(|each| {
            key == each.key
                && ((kind == Kind::Vector && each.vector)
                    || (kind == Kind::Raster && each.raster)
                    || (kind == Kind::PointCloud && each.cloud))
        });
        if !fits {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{key}= is not an option of a {} import", kind_word(kind)),
                format!("{key}={text}"),
            ));
        }
    }
    if let Some(open_option) = open_options.first()
        && kind != Kind::Vector
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "oo= is an option of a vector import",
            format!("oo={open_option}"),
        ));
    }
    if (words.scope_given || words.has("CLIP")) && kind != Kind::Vector {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a scope limits a vector import; a raster or a point cloud is read whole (RASTER \
             CLIP cuts a raster to a scope)",
        ));
    }

    let mut request = Request {
        preview: words.has("PREVIEW"),
        archive_colours: context.document.customisation_state().colours.clone(),
        ..Request::default()
    };
    request.vector.open_options = open_options;
    if let Some(layers) = words.option("layers") {
        request.vector.layer_names = list_of(layers);
        request.report_match = true;
    }
    if let Some(filter) = words.option("where") {
        request.vector.attribute_filter = filter.clone();
        request.report_match = true;
    }
    if let Some(sql) = words.option("sql") {
        request.vector.sql = sql.clone();
        request.report_match = true;
    }
    if words.option("dialect").is_some() {
        if request.vector.sql.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "dialect= is the dialect of sql=",
            ));
        }
        request.vector.sql_dialect = choice_option(&words, "dialect", &["ogrsql", "sqlite"], "")?;
    }
    if let Some(fields) = words.option("fields") {
        request.vector.fields = list_of(fields);
    }
    if let Some(target) = words.option("target") {
        let target = trimmed_text(target);
        if target.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "target= names a layer",
            ));
        }
        request.vector.target_layer = String::from(target);
    }
    request.vector.max_features = whole_option(&words, "max", 1.0, 1e15)?.unwrap_or(0);
    let attributes = choice_option(&words, "attributes", &["yes", "no"], "yes")?;
    request.vector.attributes_as_properties = attributes == "yes";

    // crs= and srs=: the one question of every kind.
    let crs = choice_option(&words, "crs", &["project", "adopt"], "")?;
    let project = context.document.metadata().coordinate_system.clone();
    if crs == "project" {
        if kind != Kind::Vector {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "crs=project moves vector data; a raster or a point cloud is drawn at its own \
                 coordinates (RASTER REPROJECT makes a copy in another)",
            ));
        }
        if project.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidCrs,
                "crs=project moves the data into the project's coordinate system, and the \
                 project has none: CRS SET <code> first, or crs=adopt",
            ));
        }
        request.vector.target_crs = project;
    }
    request.adopt = crs == "adopt";
    if let Some(srs) = words.option("srs") {
        crs_to_wkt(srs)?;
        request.vector.source_crs = srs.clone();
        request.raster.assumed_crs = srs.clone();
    }

    // A raster's and a point cloud's own.
    // Within 1..=65535 (or none, 0): the cast is exact.
    request.raster.band = whole_option(&words, "band", 1.0, 65535.0)?.unwrap_or(0) as i32;
    if let Some(subdataset) = words.option("subdataset") {
        request.raster.subdataset = subdataset.clone();
    }
    if let Some(max_pixels) = whole_option(&words, "maxpixels", 1.0, 65536.0)? {
        request.raster.max_pixels = max_pixels as i32;
    }
    if let Some(name) = words.option("name") {
        request.raster.name = name.clone();
        request.cloud.name = name.clone();
    }
    if let Some(budget) = whole_option(&words, "budget", 1.0, 1e12)? {
        request.cloud.budget = budget;
    }
    // The reader filters by one classification (PointCloudReadOptions).
    if let Some(classes) = words.option("class")
        && list_of(classes).len() > 1
    {
        return Err(make_error(
            ErrorCode::Unsupported,
            "class= takes one class: the point cloud reader filters by one",
            format!("class={classes}"),
        ));
    }
    if let Some(point_class) = whole_option(&words, "class", 0.0, 255.0)? {
        request.cloud.classification = Some(point_class as u8);
    }
    if let Some(resolution) = number_option(&words, "resolution")? {
        if !(resolution > 0.0) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "resolution= is a spacing above 0",
            ));
        }
        request.cloud.resolution = Some(resolution);
    }

    let title = format!("IMPORT {}", file_name_text(&file));
    // The scope, in the drawing's coordinates: its box is the driver's spatial
    // filter; with clip, the features are cut at its edge (AREA, a VIEW's
    // area) or by the closed shapes it took.
    if words.has("CLIP") && !words.scope_given {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "clip cuts at a scope's edge: give AREA, VIEW, DRAWING, SELECTION or LAYERS with it",
        ));
    }
    if words.scope_given {
        if placement.mode != ImportPlacementMode::Keep {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "a scope is in the drawing's coordinates, which {} moves the data away from: \
                     give one or the other",
                    import_placement::placement_word(&placement)
                ),
            ));
        }
        let found = match_scope(
            context.document,
            &words.scope,
            context.interpreter.scope_context(),
        )?;
        let taken = scope_box(&found, context.document.model());
        let clip = words.has("CLIP");
        let record = format!(
            "scope {} box={} clip={}",
            cad_scope_record(&found),
            if taken.empty() {
                String::new()
            } else {
                bounds_text(&taken)
            },
            yes_no(clip)
        );
        if taken.empty() {
            // Taking nothing is an answer, not a failure: said, nothing read.
            return Ok(Prepared {
                title,
                reply: Some(format!(
                    "{record}\nimport file={} kind=vector ran=no reason={}",
                    value(&path_text(&file)),
                    value("the scope took nothing")
                )),
                work: None,
            });
        }
        request.vector.area = Some(CrsBox {
            min_x: taken.min.x,
            min_y: taken.min.y,
            max_x: taken.max.x,
            max_y: taken.max.y,
        });
        let source = found.resolved.words.source;
        let boxed = source == ScopeSource::Area
            || (source == ScopeSource::View && found.resolved.scope.area.is_some());
        if clip && boxed {
            request.vector.clip_to_area = true;
        } else if clip {
            let drawn = drawing_dataset(
                context.document.model(),
                &found.matched,
                &DrawingDatasetOptions::default(),
            )?;
            let mut shapes = FeatureSet::default();
            for table in drawn.set.tables {
                if table.kind == GeometryKind::Polygon && !table.features.is_empty() {
                    shapes.tables.push(table);
                }
            }
            if shapes.tables.is_empty() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        "clip cuts by the closed shapes a scope takes, and this one took {} \
                         entities, none of them closed",
                        found.matched.len()
                    ),
                    record,
                ));
            }
            request.vector.clip_shapes = Some(shapes);
        }
        request.scope_record = record;
        request.report_match = true;
    }

    // ALONGSIDE and the far-apart advice are judged against the drawing as it
    // was when the line was given.
    let drawing = context.document.model().entities.bounds();
    if request.preview && kind != Kind::Vector {
        // Only vector data has filters to count; the rest is read whole.
        return Ok(Prepared {
            title,
            reply: Some(format!(
                "import file={} kind={} preview=yes",
                value(&path_text(&file)),
                kind_word(kind)
            )),
            work: None,
        });
    }
    let work = make_work(move |stop, _progress| {
        let first_read = read_at(kind, &file, None, &request)?;
        if stop_requested(stop) {
            return Err(cancelled());
        }
        if request.preview {
            // Read with every filter, counted, and nothing kept.
            let Read::Vector(read) = &first_read else {
                unreachable!("only vector data is previewed by its work");
            };
            let mut reply = format!(
                "import file={} kind=vector preview=yes features={} of={} entities={} layers={}",
                value(&path_text(&file)),
                read.features_read,
                read.features_in_file,
                read.entities.len(),
                read.layers_needed.len()
            );
            if !request.scope_record.is_empty() {
                reply = format!("{}\n{reply}", request.scope_record);
            }
            for warning in &read.warnings {
                reply.push('\n');
                reply += &warning_text(warning);
            }
            return Ok(make_apply(move |_context| Ok(reply)));
        }
        let mut imported = Imported {
            kind,
            file,
            placement,
            placed: ImportShift::default(),
            drawing,
            read: first_read,
            request,
        };
        if placeable(kind) {
            // Read at its own coordinates, then again with the shift the
            // placement resolves to (cad's import_placement).
            imported.placed = import_placement::resolve_import_shift(
                &placement,
                &drawing,
                &bounds_of(&imported.read),
            );
            if let Some(shift) = imported.placed.shift {
                imported.read = read_at(kind, &imported.file, Some(shift), &imported.request)?;
            }
        }
        if stop_requested(stop) {
            return Err(cancelled());
        }
        Ok(make_apply(move |context| apply_import(context, imported)))
    });
    Ok(Prepared {
        title,
        reply: None,
        work: Some(work),
    })
}
