//! IMPORT, EXPORT and INFO of a .ifc, natively (katana_ifc, docs/ifc.md); the
//! port of src/katana_app/ifc_verbs.hpp/.cpp. Here and not in the session so
//! that the command line's one hook is a line, as for the DXF steps
//! (dxf_verbs), and in every build because writing IFC needs no third-party
//! library.
//!
//! ```text
//! IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS] [NOELEMENTS] [NOSURFACES]
//!                   [TOLERANCE <m>] [TAKECRS | KEEPCRS]
//! EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]
//!                   [RULES <rules.csv>] [SPACING <m>] [NODRAWING]
//!                   [NOENTITIES] [SELECTED] [NOALIGNMENTS] [NOSURFACES]
//!                   [NOROADMODEL] [PREVIEW]
//! INFO <file.ifc>
//! IFC RULES <file.csv>
//! ```
//!
//! IMPORT brings the file's alignments, elements and annotations into the
//! drawing as ONE undoable step (katana_ifc's import); LOCAL moves them to sit
//! at the origin, as the other importers' LOCAL does; TAKECRS gives a project
//! with no coordinate system the file's. A terrain the file carries joins the
//! drawing's surfaces in the same step (cad's import_surfaces).
//!
//! EXPORT writes the drawing's entities (or the selected ones), alignments and
//! surfaces, with the project's coordinate system as the georeferencing;
//! UTILITIES adds an AS 5488 subsurface utility investigation (the schedule
//! format of survey's subsurface utility_csv, a delivery schema's columns
//! included), SCHEMA the delivery schema it was written to, SPACING the
//! longest detected spacing that keeps QL-B (default 10 m), RULES a project's
//! classification rules. PREVIEW answers what the file would hold and writes
//! nothing. INFO describes a file without importing it, and IFC RULES writes
//! the default classification rules for a project to start its own from. The
//! road model - the 3D objects the drawing's corridors and road objects make
//! (cad's bim_model) - goes out as IfcRoad, its parts and their elements;
//! NOROADMODEL leaves it out.
//!
//! Every reply is key=value records (katana_ifc's front_end, "replies"), and
//! the grammar is front_end's, which the window's command line and its dialogs
//! share: a dialog writes the line, and this is what a line means.
//!
//! # Where the reply goes
//!
//! The verb prints through the console it is lent, its reply to `out` and its
//! refusal to `err` as `error: <what>`, exactly as the C++ writes to std::cout
//! and std::cerr, and answers only whether it worked. Chosen over returning
//! the text for the session to print because the verb has two outcomes on two
//! streams and a third answer, "not a .ifc", that hands the line on: returned,
//! that is an `Option<Result<String>>` the caller would have to take apart into
//! exactly these writes, in each front end. The session's own verbs print
//! through the same console, and a window shows a printing verb by lending a
//! console of its command pane (step 6 plan, section 4.2).

use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use katana_cad::Document;
use katana_cad::annotation::leader_presets::leader_context;
use katana_cad::bim::bim_object::{BimModel, BimObject, bim_model};
use katana_cad::project_crs::describe_coordinate_system;
use katana_cad::survey_coding::code_property_candidates;
use katana_cad::terrain::surface_verbs::import_surfaces;
use katana_core::text::equals_ignoring_case;
use katana_ifc::{
    ExportArguments, ExportFiles, ExportInput, ExportOptions, Georeference, IfcClass,
    ImportArguments, ImportOptions, RoadElement, RoadModelInput, RoadQuantity, SurfaceInput,
    format_description, format_export_reply, format_import_reply, guid_namespace_for,
    header_timestamp, import_command, note_record, parse_export_arguments, parse_import_arguments,
    parse_rules_arguments, path_from_utf8, read_export_files, read_ifc_file, warning_record,
    write_default_rules, write_ifc, write_ifc_file,
};
use katana_terrain::NamedSurface;
use katana_update::version::program_version_text;

use crate::console::Console;
use crate::import_records::path_text;

// The arguments of a line that names the file and nothing else: what INFO
// takes.
fn file_alone(path: &str) -> ImportArguments {
    ImportArguments {
        path: path.to_owned(),
        ..ImportArguments::default()
    }
}

fn fail(console: &mut dyn Console, message: &str) -> bool {
    console.err(&format!("error: {message}\n"));
    false
}

// `path.filename().string()`: the name a reply gives the file.
fn file_name_text(path: &Path) -> String {
    path.file_name()
        .map(|name| path_text(Path::new(name)))
        .unwrap_or_default()
}

// The value of the first of `object`'s properties called any of `names`, in
// any letter case; empty when it has none.
fn property_of(object: &BimObject, names: &[&str]) -> String {
    for (name, value) in &object.properties {
        for wanted in names {
            if equals_ignoring_case(name.as_bytes(), wanted.as_bytes()) {
                return value.clone();
            }
        }
    }
    String::new()
}

// std::toupper in the C locale: the ASCII letters raised, every other byte
// as it is.
fn capitals(text: &str) -> String {
    text.to_ascii_uppercase()
}

fn export_ifc(document: &Document, arguments: &ExportArguments, console: &mut dyn Console) -> bool {
    // The grammar, the files it names and the GlobalId namespace are the
    // window's too (katana_ifc's front_end), so a line means one file in
    // either.
    let files = match read_export_files(arguments) {
        Ok(files) => files,
        Err(error) => return fail(console, &error.describe()),
    };
    // The schema is shared with the investigation that refers to it, so the
    // files' own handle on it may go.
    let ExportFiles {
        utilities, rules, ..
    } = files;
    let mut input = ExportInput {
        model: if arguments.entities || arguments.alignments {
            Some(document.model())
        } else {
            None
        },
        surfaces: Vec::new(),
        utilities,
        road: RoadModelInput::default(),
    };
    // ---- the drawing's surfaces (docs/terrain.md), hidden ones too: hiding
    // is a matter of the picture, and the window's export takes them all -
    // what a road model's cut voids, when it names one. Once: two tasks each
    // wrote this loop, and the merge kept both, so every surface went out
    // twice (cli.ifc_export_writes_each_surface_of_the_drawing_once).
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
    // The model is made once per change of what it is made from and shared
    // (cad's bim_object), so asking for it here costs nothing the views have
    // not already paid.
    if arguments.road_model {
        input.road = road_model_input(&bim_model(document));
    }

    let metadata = document.metadata();
    let mut options = ExportOptions {
        project_name: metadata.name.clone(),
        project_description: metadata.description.clone(),
        application_version: String::from(program_version_text()),
        // The export's clock is the caller's, so that the writer stays a
        // function of what it is given.
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
    set_leader_notes(&mut options, document);
    if arguments.selected {
        options.entities = document.selection().ids();
        // An empty list is every entity (ExportOptions::entities): nothing
        // selected must not become the whole drawing.
        if options.entities.is_empty() {
            return fail(
                console,
                "InvalidState: SELECTED, and nothing is selected: select the entities to \
                 export, or leave SELECTED out",
            );
        }
    }

    let path = path_from_utf8(&arguments.path);
    let written = if arguments.preview {
        write_ifc(&input, &options)
    } else {
        write_ifc_file(&input, &path, &options)
    };
    match written {
        Ok(written) => {
            console.out(&format!(
                "{}\n",
                format_export_reply(&written, &file_name_text(&path), arguments.preview)
            ));
            true
        }
        Err(error) => fail(console, &error.describe()),
    }
}

fn import_ifc(
    document: &mut Document,
    arguments: &ImportArguments,
    console: &mut dyn Console,
) -> bool {
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
    let mut imported = read_ifc_file(&path, &options);
    if arguments.local
        && let Ok(read) = &imported
        && !read.bounds.empty()
    {
        // Read again with the shift, so that the one reader applies it to
        // every kind of geometry, alignments included.
        options.origin_shift = Some(read.bounds.min);
        imported = read_ifc_file(&path, &options);
    }
    let mut imported = match imported {
        Ok(imported) => imported,
        Err(error) => return fail(console, &error.describe()),
    };
    // The count before the command, which takes the entities; the reply
    // after it, which adds its renames to the warnings.
    let entities = imported.entities.len();
    let file_name = file_name_text(&path);
    // ---- the drawing's surfaces (docs/terrain.md, "Surfaces in the drawing") ----
    // A terrain the file carries is the drawing's, in the import's one step,
    // as the window keeps it. Each is moved out where it stands, so the
    // reply still counts the file's surfaces.
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
    let command = import_command(&mut imported, document.model());
    let kept = match import_surfaces(document, command, surfaces, format!("IMPORT {file_name}")) {
        Ok(kept) => kept,
        Err(error) => return fail(console, &error.describe()),
    };
    let mut reply = format_import_reply(&imported, &file_name, entities);
    for record in &kept {
        reply.push('\n');
        reply.push_str(record);
    }
    if arguments.local
        && let Some(shift) = options.origin_shift
    {
        // std::format's "{:.3f}", which keeps the sign of a value that
        // rounds to zero, as Rust's own fixed notation does (core's
        // format_fixed drops it); a shift is a box corner, never NaN.
        reply.push('\n');
        reply.push_str(&note_record(&format!(
            "shifted by {:.3},{:.3} to sit at the origin",
            shift.x, shift.y
        )));
    }
    let project = document.metadata().coordinate_system.clone();
    let file = imported.coordinate_system.clone();
    if !file.is_empty() && project.is_empty() && !arguments.local {
        if arguments.take_coordinate_system.unwrap_or(false) {
            // Its own undoable step, after the import's, as the window's.
            if let Err(error) = document.set_coordinate_system(&file, "IMPORT_CRS") {
                return fail(console, &error.describe());
            }
            reply.push('\n');
            reply.push_str(&note_record(&format!(
                "the project is now in {file}, the file's coordinate system"
            )));
        } else {
            reply.push('\n');
            reply.push_str(&note_record(&format!(
                "the file is in {file} and the project has no coordinate system: CRS SET {file} \
                 sets it"
            )));
        }
    } else if !file.is_empty() && !project.is_empty() && project != file {
        reply.push('\n');
        reply.push_str(&warning_record(&format!(
            "the file is in {file} and the project in {project}; nothing was reprojected"
        )));
    }
    console.out(&format!("{reply}\n"));
    true
}

fn describe_ifc(argument: &str, console: &mut dyn Console) -> bool {
    // INFO takes a path and nothing else: the import's grammar, which reads
    // a quoted or unquoted .ifc, refusing any option word.
    let Some(parsed) = parse_import_arguments(argument) else {
        return false;
    };
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(error) => return fail(console, &error.describe()),
    };
    if parsed != file_alone(&parsed.path) {
        return fail(
            console,
            "InvalidArgument: INFO <file.ifc> takes the file and nothing else",
        );
    }
    let path = path_from_utf8(&parsed.path);
    match read_ifc_file(&path, &ImportOptions::default()) {
        Ok(read) => {
            console.out(&format!(
                "{}\n",
                format_description(&read, &file_name_text(&path))
            ));
            true
        }
        Err(error) => fail(console, &error.describe()),
    }
}

/// The road model as the IFC export takes it (docs/ifc.md, "Road model"):
/// each object a RoadElement, field for field, and each problem a line saying
/// where it is. Here, between the two, because katana_ifc may not see
/// katana_cad; both front ends call it - katana_cli's EXPORT and the window's
/// - on cad's bim_model of the document.
///
/// Three of an object's properties say what its fields cannot, matched in any
/// letter case: "ObjectType" is what a USERDEFINED class is (without it, the
/// object's kind in capitals); "Host" is the id of the element a feature is
/// on - the course a line marking is painted on, the element a cut voids; and
/// "Ground" (or "Surface", "Target Surface") the surface a cut is taken from.
/// All of its properties are written as they are, these too.
pub fn road_model_input(model: &BimModel) -> RoadModelInput {
    let mut out = RoadModelInput {
        elements: Vec::with_capacity(model.objects.len()),
        problems: Vec::with_capacity(model.problems.len()),
    };
    for object in &model.objects {
        let mut element = RoadElement {
            id: object.id.clone(),
            name: object.name.clone(),
            kind: object.kind.clone(),
            ifc_class: IfcClass {
                entity: object.ifc_class.clone(),
                predefined_type: object.predefined_type.clone(),
                object_type: String::new(),
            },
            facility: object.facility.clone(),
            part: object.part.clone(),
            material: object.material.clone(),
            colour: object.colour,
            mesh: object.mesh.clone(),
            plan: object.plan.clone(),
            quantities: object
                .quantities
                .iter()
                .map(|quantity| RoadQuantity {
                    name: quantity.name.clone(),
                    value: quantity.value,
                    unit: quantity.unit.clone(),
                })
                .collect(),
            properties: object.properties.clone(),
            layer: object.layer.clone(),
            host: property_of(object, &["Host"]),
            ground: property_of(object, &["Ground", "Surface", "Target Surface"]),
        };
        if object.predefined_type == "USERDEFINED" {
            // What a USERDEFINED one is, which the schema requires (each
            // class's CorrectPredefinedType rule): the object's own word for
            // it, else its kind.
            element.ifc_class.object_type = property_of(object, &["ObjectType"]);
            if element.ifc_class.object_type.is_empty() {
                element.ifc_class.object_type = capitals(&object.kind);
            }
        }
        out.elements.push(element);
    }
    for problem in &model.problems {
        out.problems.push(if problem.source.is_empty() {
            problem.message.clone()
        } else {
            format!("{}: {}", problem.source, problem.message)
        });
    }
    out
}

/// What a smart leader's note is read from: the document's surfaces and
/// alignments (cad's leader_context) and the places survey coding is looked
/// for (cad's code_property_candidates), set in `options` so that the file
/// holds the note each leader SAYS, as the plan, LIST and the DXF do, not its
/// template. Both front ends call it, so a leader reads the same in either's
/// file (docs/ifc.md, "Leaders").
pub fn set_leader_notes(options: &mut ExportOptions, document: &Document) {
    options.leader_context = leader_context(document);
    options.code_properties = code_property_candidates()
        .iter()
        .map(|name| String::from(*name))
        .collect();
}

/// `verb` is IMPORT, EXPORT, INFO or IFC, upper case; `argument` the rest of
/// the line. None when the path is not a .ifc, so the caller carries on to its
/// other importers and exporters; otherwise whether it worked, which has been
/// reported through `console` - the reply on `out`, a refusal on `err`.
pub fn run_ifc_verb(
    document: &mut Document,
    verb: &str,
    argument: &str,
    console: &mut dyn Console,
) -> Option<bool> {
    match verb {
        "EXPORT" => Some(match parse_export_arguments(argument)? {
            Ok(arguments) => export_ifc(document, &arguments, console),
            Err(error) => fail(console, &error.describe()),
        }),
        "IMPORT" => Some(match parse_import_arguments(argument)? {
            Ok(arguments) => import_ifc(document, &arguments, console),
            Err(error) => fail(console, &error.describe()),
        }),
        // Not a .ifc: GIS's INFO.
        "INFO" => parse_import_arguments(argument).map(|_| describe_ifc(argument, console)),
        "IFC" => Some(match parse_rules_arguments(argument) {
            None => fail(console, "InvalidArgument: usage: IFC RULES <file.csv>"),
            Some(Err(error)) => fail(console, &error.describe()),
            Some(Ok(path)) => match write_default_rules(&path) {
                Ok(written) => {
                    console.out(&format!("{written}\n"));
                    true
                }
                Err(error) => fail(console, &error.describe()),
            },
        }),
        _ => None,
    }
}

/// The lines --help prints for it.
pub fn ifc_help_text() -> &'static str {
    concat!(
        "IFC       IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS] [NOELEMENTS] [NOSURFACES]\n",
        "          [TOLERANCE <m>] [TAKECRS | KEEPCRS]  alignments to PIs and PVIs,\n",
        "          elements and annotations with their property sets, one undo step\n",
        "          EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]\n",
        "          [RULES <rules.csv>] [SPACING <m>] [NODRAWING] [NOENTITIES] [SELECTED]\n",
        "          [NOALIGNMENTS] [NOSURFACES] [NOROADMODEL] [PREVIEW]  IFC 4.3:\n",
        "          alignments, the drawing by class, the road model as IfcRoad and its\n",
        "          parts, an AS 5488 investigation graded, typed and with its delivery\n",
        "          schema; PREVIEW writes nothing\n",
        "          INFO <file.ifc>  what the file holds, without importing it\n",
        "          IFC RULES <file.csv>  the default classification rules, to edit\n",
        "          (replies are key=value records, docs/ifc.md)\n",
    )
}
