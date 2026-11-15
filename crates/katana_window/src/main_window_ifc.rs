//! The window's IFC import and export, through the native module (katana_ifc, docs/ifc.md): the
//! port of main_window_ifc.cpp. Its own file, so that the main window - where every feature meets
//! - carries one-line hooks.
//!
//! ONE DOOR: every IFC action in the window is a line - IMPORT, EXPORT, INFO or IFC RULES - with
//! katana_ifc's front_end grammar, which katana_cli reads too. A typed line comes here from the
//! dispatcher; File > Import IFC and Export IFC write their line and hand it to `run_ifc_command`,
//! which echoes it as typed and runs it here; File > Import and a path given to the window do the
//! same. So a dialog cannot do what a line cannot, an agent can do all a dialog can, and the reply
//! - the key=value records katana_cli prints - is the same whoever asked. What only the window has
//! - its surfaces, its selection, a person to ask - is added here.
//!
//! THE DIALOGS' DOOR. An IFC dialog's runner (`IfcRunLine`) is lent the window as a `&mut dyn
//! Host`, which it cannot ask for runIfcCommand by name. So the runner the window gives it arms a
//! shared bridge (`IfcBridge`) and runs its line through `Host::run`; the executor, finding the
//! bridge armed, runs the line as runIfcCommand runs a dialog's - echoed as typed, never offered
//! to the dispatcher - and leaves the result, its error whole, for the runner to hand back.
//! Rejected: the dialog's line as an ordinary executor line, which loses the error's code (a
//! cancel is CommandRejected) and does not echo the line on stderr as the C++ does headless.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::SystemTime;

use katana_app::ifc_verbs::{road_model_input, set_leader_notes};
use katana_cad::bim::bim_object::bim_model;
use katana_cad::project_crs::describe_coordinate_system;
use katana_cad::terrain::surface_verbs::import_surfaces;
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::Box2;
use katana_ifc::{
    ExportArguments, ExportFiles, ExportInput, ExportOptions, Georeference, ImportArguments,
    ImportOptions, SurfaceInput, format_description, format_export_reply, format_import_reply,
    guid_namespace_for, header_timestamp, import_command, is_epsg_code, note_record,
    parse_export_arguments, parse_import_arguments, parse_rules_arguments, path_from_utf8,
    read_export_files, read_ifc_file, warning_record, write_default_rules, write_ifc,
    write_ifc_file,
};
use katana_interop::import::advise_placement;
use katana_terrain::NamedSurface;
use katana_update::version::program_version_text;

use crate::host::{Host, VerbOutcome};
use crate::ifc_dialogs::{IfcExportContext, IfcExportState, IfcImportContext};
use crate::main_window::executor::{Answer, Executor, ask_three, ask_yes_no};
use crate::project_crs_dialog::project_crs_label;

/// Who ran an IFC line: typed (or pasted) on the command line, which asks no one about the file's
/// coordinate system and says how; File > Import or a path given to the window, which asks the
/// person there and shows a failure in a box; a dialog, whose line says TAKECRS or KEEPCRS itself
/// and which shows its own result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IfcLineFrom {
    CommandLine,
    Menu,
    Dialog,
}

#[derive(Default)]
struct BridgeState {
    armed: bool,
    result: Option<Result<String>>,
}

/// The dialogs' door to runIfcCommand (see the module comment).
#[derive(Clone, Default)]
pub struct IfcBridge {
    state: Rc<RefCell<BridgeState>>,
}

impl IfcBridge {
    /// The next line the executor runs is a dialog's IFC line.
    pub fn arm(&self) {
        let mut state = self.state.borrow_mut();
        state.armed = true;
        state.result = None;
    }

    /// Whether the line now run is a dialog's; disarmed by the asking.
    pub fn take_armed(&self) -> bool {
        std::mem::take(&mut self.state.borrow_mut().armed)
    }

    fn set_result(&self, result: Result<String>) {
        self.state.borrow_mut().result = Some(result);
    }

    /// What the dialog's line came to; None when the executor never ran it as one.
    pub fn take_result(&self) -> Option<Result<String>> {
        let mut state = self.state.borrow_mut();
        state.armed = false;
        state.result.take()
    }
}

/// The arguments of a line that names the file and nothing else: what INFO takes.
fn file_alone(path: &str) -> ImportArguments {
    ImportArguments {
        path: String::from(path),
        ..ImportArguments::default()
    }
}

/// `path.filename()`: the name a reply gives the file.
fn file_name_of(utf8_path: &str) -> String {
    path_from_utf8(utf8_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn make(code: ErrorCode, message: &str) -> Error {
    Error::new(code, message)
}

impl Executor {
    // ---- import -----------------------------------------------------------------------------

    /// IMPORT <file.ifc>: the file's alignments, elements, annotations and surfaces as one step,
    /// placed beside the drawing when it lands far from it (asked of a person, kept headless), and
    /// the file's coordinate system taken, asked or said.
    pub fn import_ifc(&mut self, arguments: &ImportArguments, from: IfcLineFrom) -> Result<String> {
        let path = path_from_utf8(&arguments.path);
        let mut options = ImportOptions {
            import_alignments: arguments.alignments,
            import_elements: arguments.elements,
            import_surfaces: arguments.surfaces,
            ..ImportOptions::default()
        };
        if let Some(tolerance) = arguments.tolerance {
            options.curve_tolerance = tolerance;
        }
        let mut imported = read_ifc_file(&path, &options)?;

        // LOCAL, as katana_cli has it: read again shifted by the extent's corner. Otherwise the
        // question every import asks, weighed against everything the drawing has.
        let mut notes: Vec<String> = Vec::new();
        let mut shifted = false;
        if arguments.local && !imported.bounds.empty() {
            options.origin_shift = Some(imported.bounds.min);
            shifted = true;
        } else if !arguments.local {
            let mut existing: Box2 = self.document.model().entities.bounds();
            for alignment in self.document.model().alignments.all() {
                for pi in &alignment.horizontal.pis {
                    existing.expand(pi.point);
                }
            }
            for named in self.document.surfaces().all() {
                if let Some(surface) = &named.surface
                    && surface.vertex_count() > 0
                {
                    existing.expand_box(*surface.bounds());
                }
            }
            let advice = advise_placement(&existing, &imported.bounds);
            if advice.far_apart && self.headless {
                notes.push(format!("{} (kept: no one to ask)", advice.message));
            } else if advice.far_apart {
                match ask_three(
                    "Far from the current drawing",
                    &format!(
                        "{}.\n\nShifting moves everything in the file as one piece so it sits \
                         beside the drawing; its shape and internal dimensions are unchanged.\n\n\
                         Yes shifts it alongside; No keeps the survey coordinates.",
                        advice.message
                    ),
                ) {
                    // Declined, not failed: the dialog says it as a cancel.
                    Answer::Cancel => {
                        return Err(make(ErrorCode::CommandRejected, "Import cancelled."));
                    }
                    Answer::Yes => {
                        options.origin_shift = Some(advice.suggested_shift);
                        shifted = true;
                    }
                    Answer::No => notes.push(format!("{} (kept)", advice.message)),
                }
            }
        }
        if shifted {
            imported = read_ifc_file(&path, &options)?;
            let shift = options.origin_shift.unwrap_or_default();
            notes.push(format!(
                "shifted by {:.3},{:.3} {}",
                shift.x,
                shift.y,
                if arguments.local {
                    "to sit at the origin"
                } else {
                    "to sit beside the drawing"
                }
            ));
        }

        // Layers, alignments, entities and the file's surfaces: ONE transaction, one Ctrl+Z.
        let entities = imported.entities.len();
        let file_name = file_name_of(&arguments.path);
        let surfaces: Vec<NamedSurface> = imported
            .surfaces
            .iter_mut()
            .map(|surface| NamedSurface {
                name: surface.name.clone(),
                surface: Some(Arc::new(std::mem::take(&mut surface.surface))),
                source: format!("IFC {file_name}"),
                ..NamedSurface::default()
            })
            .collect();
        let command = import_command(&mut imported, self.document.model());
        let kept = import_surfaces(
            &mut self.document,
            command,
            surfaces,
            format!("IMPORT {file_name}"),
        )?;
        let mut reply = format_import_reply(&imported, &file_name, entities);
        for record in &kept {
            reply.push('\n');
            reply.push_str(record);
        }

        // The file's coordinate system: taken only when the project has none and the data is
        // where the file put it. TAKECRS and KEEPCRS answer; without either, File > Import asks
        // the person there, and a typed line is answered in the log.
        let project = self.document.metadata().coordinate_system.clone();
        let file = imported.coordinate_system.clone();
        if !file.is_empty() && project.is_empty() && !shifted {
            let mut take = arguments.take_coordinate_system.unwrap_or(false);
            if arguments.take_coordinate_system.is_none()
                && from == IfcLineFrom::Menu
                && !self.headless
            {
                let mut named = file.clone();
                if let Ok(described) = describe_coordinate_system(&file) {
                    named.push_str(&format!(" ({})", described.name));
                }
                take = ask_yes_no(
                    "Project Coordinate System",
                    &format!(
                        "The file is in {named}, and the project has no coordinate system.\n\n\
                         Set the project's to the file's? Nothing is moved; the drawing is then \
                         known to be in it."
                    ),
                );
            }
            if take {
                match self.document.set_coordinate_system(&file, "IMPORT_CRS") {
                    Err(error) => notes.push(format!(
                        "the file's coordinate system {file} could not be set: {}",
                        error.describe()
                    )),
                    Ok(()) => notes.push(format!(
                        "the project is now in {file}, the file's coordinate system"
                    )),
                }
            } else {
                notes.push(format!(
                    "the file is in {file} and the project has no coordinate system: File > \
                     Project Coordinate System or CRS SET {file} sets it"
                ));
            }
        } else if !file.is_empty() && !project.is_empty() && project != file {
            reply.push('\n');
            reply.push_str(&warning_record(&format!(
                "the file is in {file} and the project in {project}; nothing was reprojected"
            )));
        }
        for note in &notes {
            reply.push('\n');
            reply.push_str(&note_record(note));
        }

        self.workspace.scenery_changed();
        self.zoom_extents_all();
        Ok(reply)
    }

    /// What an IFC file holds, read but not imported, as INFO answers it.
    pub fn describe_ifc_file(&mut self, path: &Path) -> Result<String> {
        let read = read_ifc_file(path, &ImportOptions::default())?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(format_description(&read, &name))
    }

    // ---- export -----------------------------------------------------------------------------

    /// EXPORT <file.ifc>: the drawing, its alignments, surfaces, road model and a utility
    /// investigation, with the options katana_cli sets from the same metadata.
    pub fn export_ifc(&mut self, arguments: &ExportArguments) -> Result<String> {
        let ExportFiles {
            utilities, rules, ..
        } = read_export_files(arguments)?;
        let document = &self.document;
        let mut input = ExportInput {
            model: if arguments.entities || arguments.alignments {
                Some(document.model())
            } else {
                None
            },
            surfaces: Vec::new(),
            utilities,
            road: Default::default(),
        };
        // The drawing's surfaces, hidden ones too: hiding is a matter of the picture.
        if arguments.surfaces {
            for named in document.surfaces().all() {
                if let Some(surface) = named.surface.as_deref() {
                    input.surfaces.push(SurfaceInput {
                        name: named.name.clone(),
                        surface: Some(surface),
                    });
                }
            }
        }
        if arguments.road_model {
            input.road = road_model_input(&bim_model(document));
        }
        let metadata = document.metadata();
        let mut options = ExportOptions {
            project_name: metadata.name.clone(),
            project_description: metadata.description.clone(),
            application_version: String::from(program_version_text()),
            timestamp: header_timestamp(SystemTime::now()),
            guid_namespace: guid_namespace_for(&metadata.name, &metadata.created_utc),
            rules,
            georeference: Georeference {
                name: metadata.coordinate_system.clone(),
                ..Georeference::default()
            },
            export_entities: arguments.entities,
            export_alignments: arguments.alignments,
            ..ExportOptions::default()
        };
        if !metadata.coordinate_system.is_empty()
            && let Ok(described) = describe_coordinate_system(&metadata.coordinate_system)
        {
            options.georeference.description = described.name;
        }
        // A leader is written as the note it says, as katana_cli's is.
        set_leader_notes(&mut options, document);
        if arguments.selected {
            options.entities = document.selection().ids();
            // An empty list is every entity: a selection cleared since it was asked for must not
            // become the whole drawing.
            if options.entities.is_empty() {
                return Err(make(
                    ErrorCode::InvalidState,
                    "SELECTED, and nothing is selected: select the entities to export, or leave \
                     SELECTED out",
                ));
            }
        }
        let path = path_from_utf8(&arguments.path);
        let written = if arguments.preview {
            write_ifc(&input, &options)
        } else {
            write_ifc_file(&input, &path, &options)
        }?;
        Ok(format_export_reply(
            &written,
            &file_name_of(&arguments.path),
            arguments.preview,
        ))
    }

    // ---- the lines --------------------------------------------------------------------------

    /// MainWindow::runIfcLine: the line's reply - key=value records - or its error, both in the
    /// log; None when `verb` and `rest` are not an IFC line (another format's). A cancelled import
    /// is CommandRejected.
    pub fn run_ifc_line(
        &mut self,
        verb: &str,
        rest: &str,
        from: IfcLineFrom,
    ) -> Option<Result<String>> {
        let reply: Result<String> = match verb {
            "EXPORT" => match parse_export_arguments(rest)? {
                Ok(arguments) => self.export_ifc(&arguments),
                Err(error) => Err(error),
            },
            "IMPORT" => match parse_import_arguments(rest)? {
                Ok(arguments) => self.import_ifc(&arguments, from),
                Err(error) => Err(error),
            },
            "INFO" => match parse_import_arguments(rest)? {
                Err(error) => Err(error),
                Ok(parsed) if parsed != file_alone(&parsed.path) => Err(make(
                    ErrorCode::InvalidArgument,
                    "INFO <file.ifc> takes the file and nothing else",
                )),
                Ok(parsed) => self.describe_ifc_file(&path_from_utf8(&parsed.path)),
            },
            "IFC" => match parse_rules_arguments(rest)? {
                Ok(path) => write_default_rules(&path),
                Err(error) => Err(error),
            },
            _ => return None,
        };
        match &reply {
            Ok(text) => self.log_message(text, false),
            // A cancel, said as one.
            Err(error) if error.code == ErrorCode::CommandRejected => {
                self.log_message(&error.message, false);
            }
            Err(error) => {
                self.log_message(&error.describe(), true);
                if from == IfcLineFrom::Menu {
                    let title = if verb == "EXPORT" {
                        "Export failed"
                    } else {
                        "Import failed"
                    };
                    self.warn_user(title, &error.describe());
                }
            }
        }
        Some(reply)
    }

    /// MainWindow::runIfcCommand: `line` echoed as typed and run by run_ifc_line - never offered to
    /// a running tool first, since the line is never an answer to one.
    pub fn run_ifc_command(&mut self, line: &str, from: IfcLineFrom) -> Result<String> {
        self.log.append(&format!("> {line}"));
        if self.headless {
            // What a script - or a ctest check - can see of which line a dialog ran.
            eprintln!("> {line}");
        }
        let trimmed = line.trim();
        let (verb, rest) = match trimmed.find(' ') {
            Some(space) => (trimmed[..space].to_uppercase(), &trimmed[space..]),
            None => (trimmed.to_uppercase(), ""),
        };
        if let Some(reply) = self.run_ifc_line(&verb, rest, from) {
            return reply;
        }
        let refused = make(
            ErrorCode::InvalidArgument,
            &format!("not an IFC line: {line}"),
        );
        self.log_message(&refused.describe(), true);
        Err(refused)
    }

    /// A dialog's line through the bridge (see the module comment): run as runIfcCommand runs it,
    /// its result left for the dialog's runner.
    pub fn run_ifc_dialog_line(&mut self, line: &str) -> VerbOutcome {
        let errors = self.log.errors_logged();
        let result = self.run_ifc_command(line, IfcLineFrom::Dialog);
        let outcome = match &result {
            Ok(reply) => VerbOutcome::new(self.log.errors_logged() == errors, reply, ""),
            Err(error) => VerbOutcome::new(false, "", &error.describe()),
        };
        self.ifc.set_result(result);
        outcome
    }

    // ---- the dialogs' contexts ----------------------------------------------------------------

    /// The runner the IFC dialogs are given: their line through the bridge.
    fn ifc_runner(&self) -> crate::ifc_dialogs::IfcRunLine {
        let bridge = self.ifc.clone();
        Box::new(move |host: &mut dyn Host, line: &str| -> Result<String> {
            bridge.arm();
            let outcome = host.run(line);
            bridge.take_result().unwrap_or_else(|| {
                if outcome.ok {
                    Ok(outcome.reply)
                } else {
                    Err(make(ErrorCode::InvalidArgument, &outcome.error))
                }
            })
        })
    }

    /// MainWindow::ifcExportContext: the window's counts, read each time the dialog asks.
    pub fn ifc_export_context(&self) -> IfcExportContext {
        IfcExportContext {
            state: Some(Box::new(|host: &dyn Host| {
                let document = host.document();
                let code = &document.metadata().coordinate_system;
                IfcExportState {
                    entities: document.model().entities.size(),
                    selected: document.selection().size(),
                    alignments: document.model().alignments.size(),
                    surfaces: document.surfaces().all().len(),
                    coordinate_system: if code.is_empty() {
                        String::new()
                    } else {
                        project_crs_label(document)
                    },
                    // The writer's own test, so the dialog cannot promise what the file will not
                    // do.
                    georeferenced: is_epsg_code(code),
                    road_objects: bim_model(document).objects.len(),
                }
            })),
            run_line: Some(self.ifc_runner()),
        }
    }

    /// MainWindow::ifcImportContext.
    pub fn ifc_import_context(&self) -> IfcImportContext {
        IfcImportContext {
            run_line: Some(self.ifc_runner()),
        }
    }
}
