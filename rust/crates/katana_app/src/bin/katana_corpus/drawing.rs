//! The drawing the steps act on: the model and its undo stack, the project's
//! metadata, survey jobs and surfaces, and the project it was opened from or
//! saved to. It is the part of cad's Document (src/katana_cad/document.cpp,
//! terrain/surface_document.cpp) that opening, saving, importing and
//! exporting touch, written here because katana_cad is ported in step 4; it
//! goes when that does.
//!
//! Each step does what katana_cli's line of the same verb does
//! (src/katana_app/session.cpp, dxf_verbs.cpp, ifc_verbs.cpp,
//! geo/import_verb.cpp, geo/export_verb.cpp, src/katana_interop/archive12d.cpp)
//! with the same options, origin shifts and curve tolerances, so that what the
//! gate compares is the readers and writers, not two ways of calling them.
//! Where katana_cli's line needs a part of the cad layer to draw what a file
//! holds - an annotation drawn into a DXF, a coordinate system described, a
//! road model built - the step refuses with "ported in step 4" rather than
//! write a file the C++ would not, and the gate's row says so.
//!
//! The C++ runs with no customisation for the gate (KATANA_BUILTIN_CUSTOMISATION
//! is `none`), as this program, which has none, does: a project's record of
//! the customisations it was drawn with is then saved as it was read
//! (cad::customisationRecordToSave keeps every recorded name that is still not
//! loaded), and an archive's colours are resolved by the standard names alone.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use katana_archive12d::{
    ExportSurface, ReadOptions as ArchiveReadOptions, WriteOptions, from_domain, read_archive,
    to_domain, write_archive,
};
use katana_commands::{
    CommandPtr, CommandStack, Transaction, create_alignment, create_entities, create_layer,
    create_style,
};
use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::text::parse_finite_double;
use katana_core::text_encoding::{decode_text, encode_utf16_little_endian, utf8_or_windows1252};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::annotation::DEFAULT_ANNOTATION_SCALE;
use katana_entity::leader_values::is_smart;
use katana_entity::{DimensionKind, EntityId, Geometry, LeaderContext, Model};
use katana_ifc::{
    ExportInput, Georeference, RoadModelInput, SurfaceInput, guid_namespace_for, header_timestamp,
    parse_export_arguments, read_export_files, write_ifc, write_ifc_file,
};
use katana_storage::{
    ProjectMetadata, ProjectStore, SurfaceRecord, SurveyJob, apply_to_model_consuming,
    capture_model,
};
use katana_terrain::{NamedSurface, SurfaceStore, decode_tin, encode_tin};

/// The metadata key the drawing's surface display styles are kept under
/// (cad/terrain/surface_document.cpp, kStylesKey): empty for the built-in set.
const SURFACE_STYLES_KEY: &str = "surface_styles";

/// The metadata key the annotation scale is kept under
/// (cad/annotation/document_annotation.cpp, kAnnotationScaleKey).
const ANNOTATION_SCALE_KEY: &str = "annotation_scale";

/// The metadata keys of the road model, which cad's bim_object.cpp builds the
/// IFC export's road elements from (corridors, and objects placed by hand).
const ROAD_MODEL_KEYS: [&str; 2] = ["corridors", "road_objects"];

/// The largest .12da the import reads: interop's Archive12dImportOptions::
/// maxBytes, a gigabyte (interop/archive12d.hpp says why).
const MAX_ARCHIVE_BYTES: u64 = 1 << 30;

/// Where a smart leader's note reads its code: cad's codePropertyCandidates()
/// (survey_coding.cpp), which the IFC export is handed with the leader context
/// (ifc_verbs.cpp setLeaderNotes).
const CODE_PROPERTY_CANDIDATES: [&str; 5] = ["code", "12d.name", "Code", "CODE", "feature_code"];

/// A path as the C++'s `pathText` and `path.string()` give it on Linux: its
/// bytes, as UTF-8 text.
pub fn text_of(path: &Path) -> String {
    utf8_or_windows1252(path_to_utf8(path))
}

/// The file's name alone, as `pathText(file.filename())` gives it.
fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| text_of(Path::new(name)))
        .unwrap_or_default()
}

/// The extension without its dot, in lower case (interop's lowerExtension).
fn lower_extension(path: &Path) -> String {
    path.extension()
        .map(|extension| text_of(Path::new(extension)).to_ascii_lowercase())
        .unwrap_or_default()
}

/// Why the step must refuse, said the one way the gate's rows look for.
fn needs_cad(what: String, path: &Path) -> Error {
    make_error(
        ErrorCode::Unsupported,
        format!("{what}: that is the cad layer's, ported in step 4"),
        text_of(path),
    )
}

/// What the C++'s DXF export hands to the cad layer to draw for the file
/// (cad/annotation/export_annotation.cpp drawAnnotationForExport: every
/// leader, every label, every dimension but an aligned one), the first one
/// found; None when the drawing holds none, and the map the C++ hands over is
/// then empty.
fn drawn_by_cad(model: &Model) -> Option<String> {
    let mut found = None;
    model.entities.for_each(|entity| {
        if found.is_some() {
            return;
        }
        let kind = match &entity.geometry {
            Geometry::Leader(_) => "a leader",
            Geometry::Label(_) => "a label",
            Geometry::Dimension(dimension) if !matches!(dimension.kind, DimensionKind::Aligned) => {
                "a dimension that is not aligned"
            }
            _ => return,
        };
        found = Some(format!("{kind} (#{})", entity.id));
    });
    found
}

/// What the C++'s IFC export takes from the cad layer for this drawing, the
/// first one found: the description of the project's coordinate system
/// (cad::describeCoordinateSystem), the road model (cad::bim::bimModel, from
/// the corridors and the placed objects) and a smart leader's note, whose
/// values of a surface or an alignment cad's leader context reads.
fn ifc_needs_cad(model: &Model, metadata: &ProjectMetadata) -> Option<String> {
    if !metadata.coordinate_system.is_empty() {
        return Some(format!(
            "the project's coordinate system ({}), which the IFC export describes through project_crs",
            metadata.coordinate_system
        ));
    }
    for key in ROAD_MODEL_KEYS {
        if metadata
            .unknown_keys
            .get(key)
            .is_some_and(|value| !value.is_empty())
        {
            return Some(format!(
                "the drawing's road model ({key}), which the IFC export builds through bim_object"
            ));
        }
    }
    let mut found = None;
    model.entities.for_each(|entity| {
        if found.is_none()
            && let Geometry::Leader(leader) = &entity.geometry
            && is_smart(leader)
        {
            found = Some(format!(
                "a smart leader (#{}), whose note the IFC export reads through the leader context",
                entity.id
            ));
        }
    });
    found
}

/// interop's readWholeFile: the file whole, refused over `max_bytes`.
fn read_whole_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let context = || text_of(path);
    let size = std::fs::metadata(path)
        .map_err(|_| {
            make_error(
                ErrorCode::FileImportFailure,
                "the file cannot be read",
                context(),
            )
        })?
        .len();
    if size > max_bytes {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            format!("the file is larger than the limit of {max_bytes} bytes"),
            context(),
        ));
    }
    std::fs::read(path).map_err(|_| {
        make_error(
            ErrorCode::FileImportFailure,
            "the file cannot be opened",
            context(),
        )
    })
}

/// The surfaces a project holds, decoded, and its surface styles' key
/// (Document::readSurfaceState). The styles are cad's to read, and the key is
/// kept as its text: an empty one is the built-in set, which a save writes as
/// an empty key again, and any other is written back as it came, which is
/// what the C++ writes for a set it reads or cannot read alike.
fn read_surface_state(
    records: Vec<SurfaceRecord>,
    metadata: &mut ProjectMetadata,
) -> Result<(SurfaceStore, Option<String>)> {
    let mut store = SurfaceStore::new();
    for record in records {
        let surface = decode_tin(&record.data).map_err(|error| {
            make_error(
                error.code,
                format!(
                    "surface {} in the project cannot be read: {}",
                    utf8_or_windows1252(&record.name),
                    error.message
                ),
                error.context,
            )
        })?;
        // A name the store would refuse - an earlier build's - is repaired,
        // as the C++ repairs it, rather than refusing the project.
        let name = if SurfaceStore::check_name(&record.name).is_ok() {
            utf8_or_windows1252(&record.name)
        } else {
            store.unique_name(&record.name)
        };
        let named = NamedSurface {
            name,
            surface: Some(Arc::new(surface)),
            source: utf8_or_windows1252(&record.source),
            style: utf8_or_windows1252(&record.style),
            visible: record.visible,
        };
        store.add(named).map_err(|error| {
            make_error(
                error.code,
                format!("the project's surfaces cannot be read: {}", error.message),
                error.context,
            )
        })?;
    }
    let styles = metadata
        .unknown_keys
        .remove(SURFACE_STYLES_KEY)
        .filter(|text| !text.is_empty());
    Ok((store, styles))
}

pub struct Drawing {
    model: Model,
    stack: CommandStack,
    metadata: ProjectMetadata,
    survey_jobs: Vec<SurveyJob>,
    surfaces: SurfaceStore,
    /// The surface styles' key as the project held it, when it was not the
    /// built-in set (read_surface_state).
    surface_styles: Option<String>,
    store: Option<ProjectStore>,
}

impl Drawing {
    /// A new drawing, as a katana_cli session starts with.
    pub fn empty() -> Self {
        let mut model = Model::default();
        let stack = CommandStack::new(&mut model, None);
        Self {
            model,
            stack,
            metadata: ProjectMetadata::default(),
            survey_jobs: Vec::new(),
            surfaces: SurfaceStore::new(),
            surface_styles: None,
            store: None,
        }
    }

    fn execute(&mut self, command: CommandPtr) -> Status {
        self.stack.execute(&mut self.model, command)
    }

    /// OPEN <project-directory> (Document::open): a failure leaves the drawing
    /// as it was.
    pub fn open(&mut self, directory: &Path) -> Result<String> {
        let store = ProjectStore::open(directory)?;
        let mut contents = store.load()?;
        let mut metadata = std::mem::take(&mut contents.metadata);
        let survey_jobs = std::mem::take(&mut contents.survey_jobs);
        let records = std::mem::take(&mut contents.surfaces);
        let (surfaces, surface_styles) = read_surface_state(records, &mut metadata)?;
        apply_to_model_consuming(contents, &mut self.model)?;
        self.store = Some(store);
        self.metadata = metadata;
        self.survey_jobs = survey_jobs;
        self.surfaces = surfaces;
        self.surface_styles = surface_styles;
        self.stack = CommandStack::new(&mut self.model, None);
        Ok(format!(
            "opened {} ({} entities)",
            text_of(directory),
            self.model.entities.size()
        ))
    }

    /// SAVE <project-directory> (Document::saveAs): a drawing with no name is
    /// named after the directory, and the project takes its creation time
    /// from the store that made it.
    pub fn save_as(&mut self, directory: &Path) -> Result<String> {
        if self.metadata.name.is_empty() || self.metadata.name == "Untitled" {
            self.metadata.name = directory
                .file_stem()
                .map(|stem| text_of(Path::new(stem)))
                .unwrap_or_default();
        }
        let mut store = ProjectStore::create(directory, &self.metadata)?;
        if let Ok(created) = store.load() {
            self.metadata.created_utc = created.metadata.created_utc;
        }
        self.save_contents(&mut store)?;
        self.store = Some(store);
        Ok(format!("saved to {}", text_of(directory)))
    }

    /// SAVE (Document::save): a backup of the project as it was, then the
    /// drawing over it.
    pub fn save(&mut self) -> Result<String> {
        let Some(mut store) = self.store.take() else {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the drawing has not been saved to a project yet; use saveAs()",
            ));
        };
        let saved = store
            .backup(ProjectStore::DEFAULT_BACKUPS_TO_KEEP)
            .and_then(|_| self.save_contents(&mut store));
        self.store = Some(store);
        saved.map(|()| String::from("saved"))
    }

    /// Document::saveContents: the model and the metadata, the surfaces and
    /// their styles' key, and the survey jobs.
    fn save_contents(&self, store: &mut ProjectStore) -> Status {
        let mut contents = capture_model(&self.model, self.metadata.clone(), Vec::new());
        for named in self.surfaces.all() {
            // The store refuses a surface without one (SurfaceStore::add).
            let Some(surface) = &named.surface else {
                continue;
            };
            contents.surfaces.push(SurfaceRecord {
                name: named.name.clone().into_bytes(),
                source: named.source.clone().into_bytes(),
                style: named.style.clone().into_bytes(),
                visible: named.visible,
                data: encode_tin(surface),
            });
        }
        contents.metadata.unknown_keys.insert(
            String::from(SURFACE_STYLES_KEY),
            self.surface_styles.clone().unwrap_or_default(),
        );
        contents.survey_jobs = self.survey_jobs.clone();
        store.save(&contents)
    }

    /// Document::annotationScale: the project's, when it holds a positive
    /// number, else the default.
    fn annotation_scale(&self) -> f64 {
        if let Some(text) = self.metadata.unknown_keys.get(ANNOTATION_SCALE_KEY)
            && let Some(scale) = parse_finite_double(text)
            && scale > 0.0
        {
            return scale;
        }
        DEFAULT_ANNOTATION_SCALE
    }

    /// IMPORT <file>, by the file's extension as katana_cli routes it: a .ifc
    /// to ifc_verbs, a .dxf and a .12da to the executor's import.
    pub fn import(&mut self, path: &Path) -> Result<String> {
        if katana_ifc::is_ifc_path(path) {
            return self.import_ifc(path);
        }
        if katana_dxf::is_dxf_path(path) {
            return self.import_dxf(path);
        }
        match lower_extension(path).as_str() {
            "12da" => self.import_archive(path),
            "12daz" => Err(make_error(
                ErrorCode::Unsupported,
                "a .12daz is a zip container, which the gis layer opens (step 5)",
                text_of(path),
            )),
            extension => Err(make_error(
                ErrorCode::Unsupported,
                format!("no importer for '.{extension}' in katana_corpus"),
                text_of(path),
            )),
        }
    }

    /// dxf_verbs.cpp readDxfImport and applyDxfImport: read where it stands
    /// (IMPORT with no placement keeps the file's coordinates, so there is no
    /// second read with a shift), the entities' source the file's name, and
    /// the import as one step.
    fn import_dxf(&mut self, path: &Path) -> Result<String> {
        let options = katana_dxf::ImportOptions {
            source_name: file_name_of(path),
            origin_shift: None,
            ..katana_dxf::ImportOptions::default()
        };
        let mut imported = katana_dxf::read_dxf_file(path, &options)?;
        let entities = imported.entities.len();
        if let Some(command) = katana_dxf::import_command(&mut imported, &self.model) {
            self.execute(command)?;
        }
        Ok(format!(
            "imported file={} kind=dxf entities={entities}",
            text_of(path)
        ))
    }

    /// interop's importArchive12d and the executor's applyArchive: the file
    /// decoded as every text file is, read, mapped with the import's defaults
    /// (no layer prefix, a millimetre of chord, no shift, the colours of no
    /// customisation), and its layers, styles, entities, alignments and
    /// surfaces added as one step. Its meshes and point clouds are the
    /// session's in the C++, never saved but for the reference layer a cloud
    /// becomes (interop's reference data, step 5), which the gate's row of the
    /// one archive with a cloud leaves out of its comparison.
    fn import_archive(&mut self, path: &Path) -> Result<String> {
        let name = file_name_of(path);
        let bytes = read_whole_file(path, MAX_ARCHIVE_BYTES)?;
        let decoded = decode_text(bytes.as_slice())
            .map_err(|error| make_error(error.code, error.message, text_of(path)))?;
        let archive = read_archive(decoded.text.as_bytes(), &ArchiveReadOptions::default())
            .map_err(|error| {
                let at = if error.context.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", error.context)
                };
                make_error(error.code, error.message, format!("{name}{at}"))
            })?;
        let options = katana_archive12d::ImportOptions {
            origin_shift: None,
            layer_prefix: String::new(),
            curve_tolerance: 0.001,
            source_name: name.clone(),
            ..katana_archive12d::ImportOptions::default()
        };
        let domain = to_domain(&archive, &options)?;

        let mut transaction = Transaction::new(format!("IMPORT {name}"));
        for layer in &domain.layers_needed {
            if !self.model.layers.contains(&layer.name) {
                transaction.add(create_layer(layer.clone()));
            }
        }
        for style in &domain.styles_needed {
            if !self.model.styles.contains(&style.name) {
                transaction.add(create_style(style.clone()));
            }
        }
        let entities = domain.entities.len();
        if entities != 0 {
            transaction.add(create_entities(domain.entities));
        }
        for mut alignment in domain.alignments {
            // A name the drawing already has would fail the whole step.
            let base = alignment.name.clone();
            let mut copy = 2;
            while self.model.alignments.contains(&alignment.name) {
                alignment.name = format!("{base} ({copy})");
                copy += 1;
            }
            transaction.add(create_alignment(alignment));
        }
        // The archive's TINs are the drawing's surfaces, in the same step: a
        // name that is taken becomes "name (2)". The source is the C++'s words
        // (geo/import_verb.cpp applyArchive), which the project keeps, and so
        // ported as they are; whether they keep the format's vendor in them is
        // the owner's question of the archive layer's names (docs/rust.md,
        // "Decisions waiting on the owner").
        let mut staged = self.surfaces.clone();
        let surfaces = domain.surfaces.len();
        for surface in domain.surfaces {
            let named = NamedSurface {
                name: staged.unique_name(surface.name.as_bytes()),
                surface: Some(Arc::new(surface.surface)),
                source: format!("12d archive {name}"),
                ..NamedSurface::default()
            };
            staged.add(named)?;
        }
        // An archive of meshes or clouds alone adds nothing to the drawing,
        // and an empty step is refused: only one with something in it runs.
        if transaction.size() != 0 {
            self.execute(Box::new(transaction))?;
        }
        if surfaces != 0 {
            self.surfaces = staged;
        }
        Ok(format!(
            "imported file={} kind=archive entities={entities} surfaces={surfaces}",
            text_of(path)
        ))
    }

    /// ifc_verbs.cpp importIfc for IMPORT <file.ifc> with no option words:
    /// the import's defaults, read where it stands, and its surfaces the
    /// drawing's in the import's one step (cad::importSurfaces), each source
    /// "IFC <file>". The project's coordinate system is left as it is: with
    /// neither TAKECRS nor KEEPCRS katana_cli only says what the file is in.
    fn import_ifc(&mut self, path: &Path) -> Result<String> {
        let mut imported = katana_ifc::read_ifc_file(path, &katana_ifc::ImportOptions::default())?;
        let name = file_name_of(path);
        let surfaces: Vec<NamedSurface> = std::mem::take(&mut imported.surfaces)
            .into_iter()
            .map(|surface| NamedSurface {
                name: surface.name,
                surface: Some(Arc::new(surface.surface)),
                source: format!("IFC {name}"),
                ..NamedSurface::default()
            })
            .collect();
        let entities = imported.entities.len();
        let count = surfaces.len();
        let command = katana_ifc::import_command(&mut imported, &self.model);
        if surfaces.is_empty() {
            if let Some(command) = command {
                self.execute(command)?;
            }
        } else {
            let mut staged = self.surfaces.clone();
            for mut surface in surfaces {
                surface.name = staged.unique_name(surface.name.as_bytes());
                staged.add(surface)?;
            }
            if let Some(command) = command {
                self.execute(command)?;
            }
            self.surfaces = staged;
        }
        Ok(format!(
            "imported file={} kind=ifc entities={entities} surfaces={count}",
            text_of(path)
        ))
    }

    /// EXPORT <file>, by the file's extension as katana_cli routes it.
    pub fn export(&self, path: &Path, application_version: &str) -> Result<String> {
        if katana_ifc::is_ifc_path(path) {
            // The line katana_cli would hand ifc_verbs: the file, quoted, and
            // no option words.
            return self.export_ifc(&format!(" \"{}\"", text_of(path)), application_version);
        }
        if katana_dxf::is_dxf_path(path) {
            return self.export_dxf(path);
        }
        match lower_extension(path).as_str() {
            "12da" => self.export_archive(path),
            "12daz" => Err(make_error(
                ErrorCode::Unsupported,
                "a .12daz is a zip container, which the gis layer writes (step 5)",
                text_of(path),
            )),
            _ => Err(make_error(
                ErrorCode::Unsupported,
                "katana_corpus writes a .dxf, a .12da and a .ifc; the other formats are GDAL's, the gis layer's (step 5)",
                text_of(path),
            )),
        }
    }

    /// dxf_verbs.cpp writeDxfExport for the whole drawing: at the drawing's
    /// annotation scale, with what the cad layer drew for the file - which,
    /// for a drawing this step does not refuse, is nothing.
    fn export_dxf(&self, path: &Path) -> Result<String> {
        if let Some(what) = drawn_by_cad(&self.model) {
            return Err(needs_cad(
                format!(
                    "the drawing holds {what}, which a DXF export draws through the export annotation"
                ),
                path,
            ));
        }
        let drawn: BTreeMap<EntityId, Vec<Geometry>> = BTreeMap::new();
        let options = katana_dxf::ExportOptions {
            annotation_scale: self.annotation_scale(),
            drawn: Some(&drawn),
            ..katana_dxf::ExportOptions::default()
        };
        let written = katana_dxf::write_dxf_file(&self.model, path, &options)?;
        Ok(format!(
            "exported file={} kind=dxf entities={} skipped={}",
            text_of(path),
            written.entities_written,
            written.entities_skipped
        ))
    }

    /// interop's exportArchive12d for the whole drawing: its surfaces with it,
    /// the export's defaults, eight decimal places, the banner naming the
    /// file, and UTF-16 little-endian with a byte order mark, as the format's
    /// own program writes it.
    fn export_archive(&self, path: &Path) -> Result<String> {
        let surfaces: Vec<ExportSurface<'_>> = self
            .surfaces
            .all()
            .iter()
            .map(|named| ExportSurface {
                name: named.name.clone(),
                surface: named.surface.as_deref(),
            })
            .collect();
        let domain = from_domain(
            &self.model,
            &surfaces,
            &katana_archive12d::ExportOptions::default(),
            &[],
        )?;
        let writing = WriteOptions {
            decimal_places: 8,
            banner: format!(
                "Written by Katana\nexport_file_name : {}",
                file_name_of(path)
            ),
            ..WriteOptions::default()
        };
        let text = write_archive(&domain.archive, &writing);
        let encoded = encode_utf16_little_endian(text.as_bytes()).map_err(|error| {
            make_error(ErrorCode::FileExportFailure, error.message, text_of(path))
        })?;
        std::fs::write(path, encoded.as_slice()).map_err(|_| {
            make_error(
                ErrorCode::FileExportFailure,
                "the file could not be written",
                text_of(path),
            )
        })?;
        Ok(format!(
            "exported file={} kind=archive entities={} skipped={} surfaces={} bytes={}",
            text_of(path),
            domain.entities_written,
            domain.entities_skipped,
            domain.surfaces_written,
            encoded.len()
        ))
    }

    /// ifc_verbs.cpp exportIfc: `argument` is the rest of an EXPORT line for a
    /// .ifc, read by the grammar both front ends read it with. The GlobalIds
    /// are seeded by the project's name and creation time, and the header's
    /// time stamp is this program's clock, as the C++'s is its own.
    pub fn export_ifc(&self, argument: &str, application_version: &str) -> Result<String> {
        let arguments = match parse_export_arguments(argument) {
            None => {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "export-ifc takes the rest of an EXPORT line for a .ifc",
                ));
            }
            Some(parsed) => parsed?,
        };
        if let Some(why) = ifc_needs_cad(&self.model, &self.metadata) {
            return Err(needs_cad(why, Path::new(&arguments.path)));
        }
        let files = read_export_files(&arguments)?;
        let surfaces: Vec<SurfaceInput<'_>> = if arguments.surfaces {
            self.surfaces
                .all()
                .iter()
                .filter_map(|named| {
                    named.surface.as_deref().map(|surface| SurfaceInput {
                        name: named.name.clone(),
                        surface: Some(surface),
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        let input = ExportInput {
            model: (arguments.entities || arguments.alignments).then_some(&self.model),
            surfaces,
            utilities: files.utilities,
            // ifc_needs_cad has refused a drawing with a road model, so cad's
            // model of it would be empty.
            road: RoadModelInput::default(),
        };
        let options = katana_ifc::ExportOptions {
            project_name: self.metadata.name.clone(),
            project_description: self.metadata.description.clone(),
            application_version: String::from(application_version),
            timestamp: header_timestamp(SystemTime::now()),
            guid_namespace: guid_namespace_for(&self.metadata.name, &self.metadata.created_utc),
            rules: files.rules,
            georeference: Georeference {
                name: self.metadata.coordinate_system.clone(),
                ..Georeference::default()
            },
            export_entities: arguments.entities,
            export_alignments: arguments.alignments,
            // cad::annotation::leaderContext: the drawing's surfaces and
            // alignments by name. Its readers of a surface's level and an
            // alignment's place serve a smart leader alone, which
            // ifc_needs_cad has refused.
            leader_context: LeaderContext {
                surfaces: self
                    .surfaces
                    .all()
                    .iter()
                    .map(|named| named.name.clone())
                    .collect(),
                alignments: self.model.alignments.names(),
                ..LeaderContext::default()
            },
            code_properties: Vec::from(CODE_PROPERTY_CANDIDATES.map(String::from)),
            ..katana_ifc::ExportOptions::default()
        };
        if arguments.selected {
            // An empty list is every entity, and this program, as katana_cli,
            // has no selection.
            return Err(Error::new(
                ErrorCode::InvalidState,
                "SELECTED, and nothing is selected: select the entities to export, or leave SELECTED out",
            ));
        }
        let path = katana_ifc::path_from_utf8(arguments.path.as_bytes());
        let written = if arguments.preview {
            write_ifc(&input, &options)?
        } else {
            write_ifc_file(&input, &path, &options)?
        };
        Ok(format!(
            "exported file={} kind=ifc instances={} bytes={}",
            text_of(&path),
            written.instances,
            written.bytes_written
        ))
    }
}
