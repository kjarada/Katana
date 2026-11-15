//! The road model on the text command line (docs/road_design.md, "The
//! verbs"; include/katana/cad/road/road_verbs.hpp,
//! src/katana_cad/road/road_verbs.cpp): the interpreter's, so the window's
//! command line, katana_cli, katana_mcp and an AI agent all run them.
//!
//! ```text
//!   ROADTEMPLATE [LIST] [JSON] | INFO <t> | DEFAULTS | SECTION <t>
//!   ROADTEMPLATE NEW <t> [FROM <t>] [description=<text>] | DELETE <t> | RENAME <t> <new>
//!   ROADTEMPLATE SET <t> description=<text>
//!   ROADTEMPLATE ADD <t> LEFT|RIGHT|BOTH <kind> [name=<word>] [at=<n>] [key=value ...]
//!   ROADTEMPLATE SET <t> <component> key=value ... | REMOVE <t> <component>
//!   ROADTEMPLATE COURSE <t> <component> ADD <course> thickness=<m> [key=value ...]
//!   ROADTEMPLATE COURSE <t> <component> SET <course> key=value ... | REMOVE <course>
//!   ROADTEMPLATE EXPORT <t>|ALL <file.json> [OVERWRITE] | IMPORT <file.json> [OVERWRITE]
//!
//!   CORRIDOR [LIST] [JSON] | INFO <c> | PROBLEMS [<c>]
//!   CORRIDOR NEW <c> ALIGNMENT <a> TEMPLATE <t> [surface=<s>] [interval=<m>] [from=] [to=]
//!     [curvetol=<e>]
//!   CORRIDOR SET <c> key=value ... | DELETE <c> | RENAME <c> <new>
//!   CORRIDOR REGION <c> ADD <from> <to> <t> | REMOVE <from> | CLEAR
//!   CORRIDOR SUPERELEVATION <c> ADD <station> <left%> <right%> | REMOVE <station> | CLEAR
//!   CORRIDOR BUILD <c>|ALL | SECTION <c> <station>
//!   CORRIDOR STRINGS <c> [layer=<layer>] [PREVIEW]
//!   CORRIDOR SURFACE <c> [finished=<name>] [subgrade=<name>] [PREVIEW]
//!   CORRIDOR QUANTITIES <c>|ALL [csv=<file> [OVERWRITE]]
//!
//!   ROADMODEL [LIST] | JSON | PROBLEMS | INFO <id>
//!   ROADMODEL SHOW [ON|OFF]    the desktop window's alone (it is taken before it
//!                              reaches here); a session with no views refuses it
//! ```
//!
//! <kind> is lane, shoulder, kerb, footpath, verge, median, drain or batter.
//! <component> is side:name (left:lane) or name, which is every side that has
//! one of that name. Words are case-insensitive; a name with blanks is quoted.
//! Every edit is ONE undo step, and a refused edit changes nothing. A reply is
//! records, one a line, key=value: a definition's numbers exactly
//! (format_exact_real), so a value a reply prints can be typed back; a
//! quantity to three places, the precision a schedule is read to.
//!
//! The template and corridor verbs act on named definitions (and, for EXPORT
//! and IMPORT, on a file), not on drawing entities, so they take no scope
//! (CLAUDE.md 1.1): what a corridor is made from is named in it. What they
//! make that IS drawing data - STRINGS - goes on a layer of its own and
//! replaces only the strings the same corridor made before, found by the
//! corridor's name ignoring case; RENAME carries them to the new name in its
//! own step, and DELETE leaves them in the drawing.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use katana_commands::{
    Transaction, create_entities, create_layer, delete_entities, set_entity_property,
};
use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::path_text::{path_from_utf8, read_file_bytes};
use katana_core::text::{
    equals_ignoring_case, format_exact_real, parse_finite_double, parse_integer, reply_quoted,
};
use katana_core::text_encoding::decode_text;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::colour_names::standard_colour;
use katana_entity::entity::set_heights;
use katana_entity::layer_path::{layer_ancestors, validate_layer_path};
use katana_entity::{Color, Entity, EntityId, EnumName, Geometry, Layer, PropertyValue};
use katana_geometry::{Point2, Polyline2};
use katana_math::tolerance;
use katana_terrain::NamedSurface;

use crate::bim::bim_object::{BimModel, BimObject, BimProblem, bim_model};
use crate::csv_table::{CsvTable, write_csv_table};
use crate::document::Document;
use crate::road::corridor_model::{
    Corridor, CorridorSet, SuperelevationPoint, TemplateRegion, corridors_to_json,
    superelevation_at, template_at,
};
use crate::road::road_section::{
    BuiltCorridor, build_corridor_in, corridor_inputs, corridor_range, corridor_sections,
    corridor_solids, corridor_strings, road_section, sections_surface, template_section,
};
use crate::road::road_template::{
    Component, ComponentKind, KerbShape, PavementCourse, RoadTemplate, RoadTemplateSet, Side,
    check_name, check_word, component_keys, component_kind_from, default_component,
    kerb_facing_from, kerb_shape_from, kerb_width, make_course, road_templates_from_json,
    road_templates_to_json, side_from, standard_templates,
};
use crate::scope_verbs::record_value;
use crate::survey_tools::c_fixed;

// The properties CORRIDOR STRINGS marks its strings with, so that the next
// run replaces them and nothing else, and the layers the road model's
// objects and strings are put on by default.
const CORRIDOR_PROPERTY: &str = "road.corridor";
const STRING_PROPERTY: &str = "road.string";
const STRINGS_LAYER: &str = "road/strings";
const CUT_LAYER: &str = "road/earthworks/cut";
const FILL_LAYER: &str = "road/earthworks/fill";

// The IFC 4.3 IfcCourseTypeEnum (road_template, PavementCourse::ifc_type).
const COURSE_TYPES: [&str; 8] = [
    "ARMOUR",
    "BALLASTBED",
    "CORE",
    "FILTER",
    "PAVEMENT",
    "PROTECTION",
    "USERDEFINED",
    "NOTDEFINED",
];

// The C++ makeCourse's default IFC type.
const DEFAULT_COURSE_TYPE: &str = "PAVEMENT";

fn exact(value: f64) -> String {
    format_exact_real(value)
}

// A computed length, level, area or volume: to three places - a millimetre, a
// thousandth of a square or cubic metre - which is what a schedule is read
// to. std::format's "{:.3f}", locale-independent, so a decimal point always.
fn fixed(value: f64) -> String {
    c_fixed(value, 3)
}

fn text(value: &str) -> String {
    record_value(value)
}

fn is(a: &str, b: &str) -> bool {
    equals_ignoring_case(a.as_bytes(), b.as_bytes())
}

fn upper(value: &str) -> String {
    value.to_ascii_uppercase()
}

fn usage(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::ParseFailure, message)
}

fn yes(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

// One key=value word (the C++'s `Option`).
struct KeyValue {
    key: String,
    value: String,
}

fn option_of(token: &str) -> Option<KeyValue> {
    let equals = token.find('=')?;
    if equals == 0 {
        return None;
    }
    Some(KeyValue {
        key: token[..equals].to_ascii_lowercase(),
        value: String::from(&token[equals + 1..]),
    })
}

// The words from `from` on, every one key=value.
fn options_from(args: &[String], from: usize) -> Result<Vec<KeyValue>> {
    let mut options = Vec::new();
    for arg in args.iter().skip(from) {
        let Some(option) = option_of(arg) else {
            return Err(usage(format!("expected key=value, not \"{arg}\"")));
        };
        options.push(option);
    }
    Ok(options)
}

fn number_of(option: &KeyValue) -> Result<f64> {
    parse_finite_double(&option.value).ok_or_else(|| {
        usage(format!(
            "{}= takes a number, not \"{}\"",
            option.key, option.value
        ))
    })
}

fn switch_of(option: &KeyValue) -> Result<bool> {
    let value = option.value.to_ascii_lowercase();
    match value.as_str() {
        "on" | "yes" | "true" => Ok(true),
        "off" | "no" | "false" => Ok(false),
        _ => Err(usage(format!(
            "{}= is on or off, not \"{}\"",
            option.key, option.value
        ))),
    }
}

fn colour_of(option: &KeyValue) -> Result<Color> {
    let refusal = || {
        usage(format!(
            "{}= is #RRGGBB or a colour's name, not \"{}\"",
            option.key, option.value
        ))
    };
    if option.value.starts_with('#') {
        return Color::from_hex(&option.value).map_err(|_| refusal());
    }
    standard_colour(&option.value).ok_or_else(refusal)
}

// ---- road templates -------------------------------------------------------------------

fn templates_readable(document: &Document) -> Status {
    document.road_templates_status().map_err(|status| {
        make_error(
            ErrorCode::InvalidState,
            "the drawing's road templates cannot be read: nothing is changed",
            status.describe(),
        )
    })
}

fn corridors_readable(document: &Document) -> Status {
    document.corridors_status().map_err(|status| {
        make_error(
            ErrorCode::InvalidState,
            "the drawing's corridors cannot be read: nothing is changed",
            status.describe(),
        )
    })
}

fn no_template(name: &str) -> Error {
    Error::new(
        ErrorCode::NotFound,
        format!("no road template {name}: ROADTEMPLATE LIST names them"),
    )
}

fn no_corridor(name: &str) -> Error {
    Error::new(
        ErrorCode::NotFound,
        format!("no corridor {name}: CORRIDOR LIST names them"),
    )
}

fn template_record(road_template: &RoadTemplate) -> String {
    let courses: usize = road_template
        .components
        .iter()
        .map(|component| component.courses.len())
        .sum();
    format!(
        "template name={} left={} right={} courses={courses} description={}",
        text(&road_template.name),
        road_template.side(Side::Left).len(),
        road_template.side(Side::Right).len(),
        text(&road_template.description)
    )
}

// A component's record; of its template only the name is read, so that a
// record can be made while the component is borrowed for an edit.
fn component_record(template_name: &str, component: &Component) -> String {
    let mut out = format!(
        "component template={} side={} name={} kind={}",
        text(template_name),
        component.side.name(),
        component.name,
        component.kind.name()
    );
    match component.kind {
        ComponentKind::Batter => {
            out.push_str(&format!(
                " cut={} fill={} maxwidth={} benchheight={} benchwidth={}",
                exact(component.cut_slope),
                exact(component.fill_slope),
                exact(component.maximum_width),
                exact(component.bench_height),
                exact(component.bench_width)
            ));
        }
        ComponentKind::Kerb => {
            out.push_str(&format!(" shape={}", component.shape.name()));
            // An edge strip is flush: no face to turn or to give a height,
            // but a crossfall, which a superelevation table may set.
            if component.shape != KerbShape::EdgeStrip {
                out.push_str(&format!(" faces={}", component.faces.name()));
            }
            out.push_str(&format!(" width={}", exact(component.width)));
            if component.shape != KerbShape::EdgeStrip {
                out.push_str(&format!(" height={}", exact(component.height)));
            }
            out.push_str(&format!(" depth={}", exact(component.depth)));
            if component.shape == KerbShape::KerbAndGutter {
                out.push_str(&format!(
                    " gutter={} gutterfall={}",
                    exact(component.gutter_width),
                    exact(component.gutter_fall)
                ));
            }
            if component.shape == KerbShape::EdgeStrip {
                out.push_str(&format!(
                    " crossfall={} superelevated={}",
                    exact(component.crossfall),
                    yes(component.superelevated)
                ));
            }
            out.push_str(&format!(
                " kerbwidth={} material={}",
                fixed(kerb_width(component)),
                text(&component.material)
            ));
        }
        ComponentKind::Drain => {
            out.push_str(&format!(
                " width={} crossfall={} superelevated={} depth={}",
                exact(component.width),
                exact(component.crossfall),
                yes(component.superelevated),
                exact(component.depth)
            ));
        }
        _ => {
            out.push_str(&format!(
                " width={} crossfall={} superelevated={}",
                exact(component.width),
                exact(component.crossfall),
                yes(component.superelevated)
            ));
        }
    }
    out.push_str(&format!(
        " layer={} colour={} courses={}",
        text(&component.layer),
        component.colour.to_hex(),
        component.courses.len()
    ));
    out
}

fn course_record(template_name: &str, component: &Component, course: &PavementCourse) -> String {
    format!(
        "course template={} component={}:{} name={} thickness={} material={} ifc={} colour={}",
        text(template_name),
        component.side.name(),
        component.name,
        course.name,
        exact(course.thickness),
        text(&course.material),
        course.ifc_type,
        course.colour.to_hex()
    )
}

// The components `reference` names: side:name, or name on every side that
// has it.
fn components_named(road_template: &RoadTemplate, reference: &str) -> Result<Vec<usize>> {
    let mut found = Vec::new();
    if let Some(colon) = reference.find(':') {
        let Some(side) = side_from(&reference[..colon]) else {
            return Err(usage(format!(
                "a component is side:name, left or right: \"{reference}\""
            )));
        };
        if let Some(index) = road_template.find(side, &reference[colon + 1..]) {
            found.push(index);
        }
    } else {
        for side in [Side::Left, Side::Right] {
            if let Some(index) = road_template.find(side, reference) {
                found.push(index);
            }
        }
    }
    if found.is_empty() {
        return Err(Error::new(
            ErrorCode::NotFound,
            format!(
                "template {} has no component {reference}: ROADTEMPLATE INFO {} lists them",
                road_template.name, road_template.name
            ),
        ));
    }
    // Ascending in `components`, not left side first: a side's first
    // component is appended at the end of the vector (insertion_point), so a
    // template whose right side was made first stores its right components
    // BEFORE its left ones. REMOVE walks these backwards and erases each, which
    // only keeps every later index valid when the indices ascend.
    found.sort_unstable();
    Ok(found)
}

fn apply_component(component: &mut Component, option: &KeyValue) -> Status {
    // `color` is `colour` and `thickness` is `depth`.
    let key = match option.key.as_str() {
        "color" => "colour",
        "thickness" => "depth",
        other => other,
    };
    // A key the component's kind does not keep would change nothing - the
    // JSON writes only what the kind uses - and be answered "updated", with no
    // step made: refused instead, naming the keys it has.
    let keys = component_keys(component.kind);
    if !keys.contains(&key) {
        return Err(usage(format!(
            "a {} has no {key}=; its keys are {}",
            component.kind.name(),
            keys.join(" ")
        )));
    }
    match key {
        "name" => {
            check_word("a component", &option.value)?;
            component.name.clone_from(&option.value);
        }
        "width" => component.width = number_of(option)?,
        "crossfall" => component.crossfall = number_of(option)?,
        "height" => component.height = number_of(option)?,
        "depth" => component.depth = number_of(option)?,
        "gutter" => component.gutter_width = number_of(option)?,
        "gutterfall" => component.gutter_fall = number_of(option)?,
        "cut" => component.cut_slope = number_of(option)?,
        "fill" => component.fill_slope = number_of(option)?,
        "maxwidth" => component.maximum_width = number_of(option)?,
        "benchheight" => component.bench_height = number_of(option)?,
        "benchwidth" => component.bench_width = number_of(option)?,
        "superelevated" => component.superelevated = switch_of(option)?,
        "layer" => {
            validate_layer_path(&option.value)?;
            component.layer.clone_from(&option.value);
        }
        "colour" => component.colour = colour_of(option)?,
        "shape" => {
            let Some(shape) = kerb_shape_from(&option.value) else {
                return Err(usage(format!(
                    "shape= is barrier, mountable, kerbgutter or edgestrip, not \"{}\"",
                    option.value
                )));
            };
            component.shape = shape;
        }
        "faces" => {
            let Some(facing) = kerb_facing_from(&option.value) else {
                return Err(usage(format!(
                    "faces= is inward (a kerb on the outer edge of a carriageway: it rises going \
                     outward) or outward (a median's or island's: it falls going outward), not \
                     \"{}\"",
                    option.value
                )));
            };
            component.faces = facing;
        }
        "material" => component.material.clone_from(&option.value),
        _ => return Err(usage(format!("unknown component key {key}="))),
    }
    Ok(())
}

fn apply_course(course: &mut PavementCourse, option: &KeyValue) -> Status {
    match option.key.as_str() {
        "name" => {
            check_word("a course", &option.value)?;
            course.name.clone_from(&option.value);
        }
        "thickness" => course.thickness = number_of(option)?,
        "material" => course.material.clone_from(&option.value),
        "ifc" => {
            let kind = upper(&option.value);
            if !COURSE_TYPES.contains(&kind.as_str()) {
                return Err(usage(format!(
                    "ifc= is an IfcCourse type - ARMOUR, BALLASTBED, CORE, FILTER, PAVEMENT, \
                     PROTECTION, USERDEFINED or NOTDEFINED - not \"{}\"",
                    option.value
                )));
            }
            course.ifc_type = kind;
        }
        "colour" | "color" => course.colour = colour_of(option)?,
        _ => {
            return Err(usage(format!(
                "unknown course key {}=; the keys are name thickness material ifc colour",
                option.key
            )));
        }
    }
    Ok(())
}

// Where a new component goes in `components`: the `at`-th of its side (from
// 1), or before the side's batter, or after the side's last.
fn insertion_point(
    road_template: &RoadTemplate,
    side: Side,
    kind: ComponentKind,
    at: Option<usize>,
) -> usize {
    let of_side: Vec<usize> = road_template
        .components
        .iter()
        .enumerate()
        .filter(|(_, component)| component.side == side)
        .map(|(i, _)| i)
        .collect();
    if let Some(at) = at
        && (1..=of_side.len()).contains(&at)
    {
        return of_side[at - 1];
    }
    if at.is_none() && kind != ComponentKind::Batter {
        for &i in &of_side {
            if road_template.components[i].kind == ComponentKind::Batter {
                return i;
            }
        }
    }
    of_side
        .last()
        .map_or(road_template.components.len(), |last| last + 1)
}

// The corridor that uses the template `name`, as its own or in a region.
fn template_used(corridors: &CorridorSet, name: &str) -> Option<String> {
    corridors
        .corridors
        .iter()
        .find(|corridor| {
            is(&corridor.template_name, name)
                || corridor
                    .regions
                    .iter()
                    .any(|region| is(&region.template_name, name))
        })
        .map(|corridor| corridor.name.clone())
}

fn commit_templates(
    document: &mut Document,
    set: &RoadTemplateSet,
    step_name: &str,
    reply: String,
) -> Result<String> {
    document.set_road_templates(set, step_name)?;
    Ok(reply)
}

// The joined text: "" before the first, a line break before each after it.
fn add_line(reply: &mut String, line: &str) {
    if !reply.is_empty() {
        reply.push('\n');
    }
    reply.push_str(line);
}

// ---- template files ---------------------------------------------------------------------

// The most a road template file may be, in bytes: a template with all its
// courses is a few kilobytes of JSON, so this holds thousands. A sanity bound
// with no standard behind it, so that a path typed to another sort of file (a
// drawing, a point cloud) is refused at once and not read into memory.
const MAXIMUM_TEMPLATE_FILE_BYTES: u64 = 16 * 1024 * 1024;

// ROADTEMPLATE EXPORT <template>|ALL <file.json> [OVERWRITE]: the templates as
// the plain-data, versioned JSON ROADTEMPLATE JSON prints, in a file. A file
// already there is only replaced with OVERWRITE, as every verb that writes one.
fn export_templates(document: &Document, args: &[String]) -> Result<String> {
    let overwrite = args.len() == 4 && is(&args[3], "OVERWRITE");
    if args.len() < 3 || args.len() > 4 || (args.len() == 4 && !overwrite) {
        return Err(usage(
            "ROADTEMPLATE EXPORT <template>|ALL <file.json> [OVERWRITE]",
        ));
    }
    let current = document.road_templates();
    let chosen = if is(&args[1], "ALL") {
        RoadTemplateSet::clone(&current)
    } else if let Some(found) = current.find(&args[1]) {
        RoadTemplateSet {
            templates: vec![found.clone()],
        }
    } else {
        return Err(no_template(&args[1]));
    };
    if chosen.templates.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "the drawing has no road templates to export: ROADTEMPLATE DEFAULTS adds the \
             standard ones",
        ));
    }
    let json = road_templates_to_json(&chosen)?;
    let file = path_from_utf8(&args[2]);
    // std::filesystem::exists with an error code: a path that cannot be
    // asked about is taken not to be there, and the open then says why.
    if file.try_exists().unwrap_or(false) && !overwrite {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "the file is already there; OVERWRITE replaces it",
            args[2].as_str(),
        ));
    }
    let Ok(mut out) = std::fs::File::create(file) else {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file cannot be written",
            args[2].as_str(),
        ));
    };
    let mut bytes = json.into_bytes();
    bytes.push(b'\n');
    if std::io::Write::write_all(&mut out, &bytes).is_err() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file could not be finished",
            args[2].as_str(),
        ));
    }
    let mut reply = format!(
        "exported file={} templates={}",
        text(&args[2]),
        chosen.templates.len()
    );
    for each in &chosen.templates {
        reply.push('\n');
        reply.push_str(&template_record(each));
    }
    Ok(reply)
}

// ROADTEMPLATE IMPORT <file.json> [OVERWRITE]: the templates in a file added
// to the drawing's as ONE undo step, read and checked as the project reads its
// own (road_templates_from_json: the format and version, and check_template of
// each). A name the drawing has already (found ignoring case) is refused, with
// nothing imported, unless OVERWRITE replaces that template where it stands.
fn import_templates(document: &mut Document, args: &[String]) -> Result<String> {
    let overwrite = args.len() == 3 && is(&args[2], "OVERWRITE");
    if args.len() < 2 || args.len() > 3 || (args.len() == 3 && !overwrite) {
        return Err(usage("ROADTEMPLATE IMPORT <file.json> [OVERWRITE]"));
    }
    let file = path_from_utf8(&args[1]);
    let size = match std::fs::metadata(&file) {
        // std::filesystem::file_size refuses what is not a regular file.
        Ok(metadata) if metadata.is_file() => metadata.len(),
        _ => {
            return Err(make_error(
                ErrorCode::NotFound,
                "the file cannot be read: it is not there, or is not a file",
                args[1].as_str(),
            ));
        }
    };
    if size > MAXIMUM_TEMPLATE_FILE_BYTES {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the file is over 16 MiB: it is not a road template file",
            args[1].as_str(),
        ));
    }
    let bytes = read_file_bytes(&file)?;
    let decoded = decode_text(&bytes)?;
    let read = road_templates_from_json(&decoded.text).map_err(|error| {
        make_error(
            error.code,
            format!("road template file {}: {}", args[1], error.message),
            error.context,
        )
    })?;
    if read.templates.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the file holds no road templates",
            args[1].as_str(),
        ));
    }
    let mut set = RoadTemplateSet::clone(&document.road_templates());
    let mut taken: Vec<String> = Vec::new();
    let mut added = 0usize;
    let mut replaced = 0usize;
    let mut records = String::new();
    for each in read.templates {
        let existing = set
            .templates
            .iter()
            .position(|kept| kept.name.eq_ignore_ascii_case(&each.name));
        if existing.is_some() && !overwrite {
            taken.push(each.name);
            continue;
        }
        records.push('\n');
        records.push_str(&template_record(&each));
        records.push_str(if existing.is_some() {
            " replaced"
        } else {
            " added"
        });
        match existing {
            Some(at) => {
                set.templates[at] = each;
                replaced += 1;
            }
            None => {
                set.templates.push(each);
                added += 1;
            }
        }
    }
    if !taken.is_empty() {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            format!(
                "the drawing has road templates called {}: OVERWRITE replaces them; nothing was \
                 imported",
                taken.join(", ")
            ),
            args[1].as_str(),
        ));
    }
    commit_templates(
        document,
        &set,
        "ROADTEMPLATE_IMPORT",
        format!(
            "imported file={} templates={} added={added} replaced={replaced}{records}",
            text(&args[1]),
            added + replaced
        ),
    )
}

// The position at which `parse_integer` finds a position from 1.
fn position_of(option: &KeyValue) -> Result<usize> {
    match parse_integer(&option.value) {
        Some(position) if position >= 1 => Ok(position as usize),
        _ => Err(usage(format!(
            "at= is a position from 1, not \"{}\"",
            option.value
        ))),
    }
}

// The key=value words that follow `at`, up to the next word that is none:
// the next group's name.
fn group_from(args: &[String], at: usize) -> (Vec<KeyValue>, usize) {
    let mut end = at + 1;
    let mut options = Vec::new();
    while end < args.len() {
        let Some(option) = option_of(&args[end]) else {
            break;
        };
        options.push(option);
        end += 1;
    }
    (options, end)
}

fn road_template_verb(document: &mut Document, args: &[String]) -> Result<String> {
    let sub = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| upper(word));
    let current = document.road_templates();
    if sub == "LIST" || sub == "JSON" {
        let json = sub == "JSON" || args.get(1).is_some_and(|word| is(word, "JSON"));
        document.road_templates_status()?;
        if json {
            return road_templates_to_json(&current);
        }
        if current.templates.is_empty() {
            return Ok(String::from(
                "templates=0 (ROADTEMPLATE DEFAULTS adds the standard ones)",
            ));
        }
        let mut reply = format!("templates={}", current.templates.len());
        for each in &current.templates {
            reply.push('\n');
            reply.push_str(&template_record(each));
        }
        return Ok(reply);
    }
    templates_readable(document)?;
    if sub == "DEFAULTS" {
        let mut set = RoadTemplateSet::clone(&current);
        let mut added = String::new();
        let mut count = 0usize;
        let mut skipped = 0usize;
        for standard in standard_templates() {
            if set.find(&standard.name).is_some() {
                skipped += 1;
                continue;
            }
            added.push('\n');
            added.push_str(&template_record(&standard));
            set.templates.push(standard);
            count += 1;
        }
        return commit_templates(
            document,
            &set,
            "ROADTEMPLATE_DEFAULTS",
            format!("added={count} skipped={skipped}{added}"),
        );
    }
    if sub == "EXPORT" {
        return export_templates(document, args);
    }
    if sub == "IMPORT" {
        return import_templates(document, args);
    }
    if args.len() < 2 {
        return Err(usage(format!(
            "ROADTEMPLATE {sub} needs a template's name (HELP ROADTEMPLATE)"
        )));
    }
    let name = &args[1];
    if sub == "NEW" {
        check_name("a road template", name)?;
        if current.find(name).is_some() {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                format!("there is a road template called {name}"),
            ));
        }
        let mut made = RoadTemplate {
            name: name.clone(),
            ..RoadTemplate::default()
        };
        let mut at = 2;
        if args.len() > 3 && is(&args[2], "FROM") {
            let Some(source) = current.find(&args[3]) else {
                return Err(no_template(&args[3]));
            };
            made.components.clone_from(&source.components);
            made.description.clone_from(&source.description);
            at = 4;
        }
        for option in options_from(args, at)? {
            if option.key != "description" {
                return Err(usage(format!(
                    "ROADTEMPLATE NEW takes description=, not {}=",
                    option.key
                )));
            }
            made.description = option.value;
        }
        let mut set = RoadTemplateSet::clone(&current);
        set.templates.push(made.clone());
        return commit_templates(
            document,
            &set,
            "ROADTEMPLATE_NEW",
            format!("{} created", template_record(&made)),
        );
    }
    // The first template of that name ignoring case, as RoadTemplateSet::find
    // finds it, by its place.
    let Some(index) = current
        .templates
        .iter()
        .position(|each| each.name.eq_ignore_ascii_case(name))
    else {
        return Err(no_template(name));
    };
    let road_template = &current.templates[index];
    if sub == "INFO" {
        let mut reply = template_record(road_template);
        for side in [Side::Left, Side::Right] {
            for component in road_template.side(side) {
                reply.push('\n');
                reply.push_str(&component_record(&road_template.name, component));
                for course in &component.courses {
                    reply.push_str("\n  ");
                    reply.push_str(&course_record(&road_template.name, component, course));
                }
            }
        }
        return Ok(reply);
    }
    if sub == "SECTION" {
        let section = template_section(road_template)?;
        let mut reply = format!(
            "section template={} points={} formation={} solids={} level=0",
            text(&road_template.name),
            section.top.len(),
            section.formation.len(),
            section.solids.len()
        );
        for point in &section.top {
            reply.push_str(&format!(
                "\npoint code={} offset={} level={}",
                point.code,
                fixed(point.offset),
                fixed(point.level)
            ));
        }
        for point in &section.formation {
            reply.push_str(&format!(
                "\nformation code={} offset={} level={}",
                point.code,
                fixed(point.offset),
                fixed(point.level)
            ));
        }
        for solid in &section.solids {
            reply.push_str(&format!(
                "\nsolid key={} columns={}",
                solid.key,
                solid.columns.len()
            ));
        }
        return Ok(reply);
    }
    if sub == "DELETE" {
        // Corridors that cannot be read use the template for all we can tell:
        // an empty set would say none does.
        corridors_readable(document)?;
        if let Some(by) = template_used(&document.corridors(), &road_template.name) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "the road template {} is used by corridor {by}: change or delete the \
                     corridor first",
                    road_template.name
                ),
            ));
        }
        let mut set = RoadTemplateSet::clone(&current);
        set.templates.remove(index);
        return commit_templates(
            document,
            &set,
            "ROADTEMPLATE_DELETE",
            format!("template name={} deleted", text(&road_template.name)),
        );
    }
    if sub == "RENAME" {
        if args.len() < 3 {
            return Err(usage("ROADTEMPLATE RENAME <template> <new name>"));
        }
        let new_name = &args[2];
        check_name("a road template", new_name)?;
        if let Some(other) = current.find(new_name)
            && !std::ptr::eq(other, road_template)
        {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                format!("there is a road template called {new_name}"),
            ));
        }
        corridors_readable(document)?;
        let mut set = RoadTemplateSet::clone(&current);
        set.templates[index].name.clone_from(new_name);
        // The corridors that name it follow it, in the same step.
        let mut corridors = CorridorSet::clone(&document.corridors());
        let mut following = 0usize;
        for corridor in &mut corridors.corridors {
            let mut moved = false;
            if is(&corridor.template_name, &road_template.name) {
                corridor.template_name.clone_from(new_name);
                moved = true;
            }
            for region in &mut corridor.regions {
                if is(&region.template_name, &road_template.name) {
                    region.template_name.clone_from(new_name);
                    moved = true;
                }
            }
            following += usize::from(moved);
        }
        let templates_step = document.road_templates_command(&set, "ROADTEMPLATE_RENAME")?;
        let corridors_step = document.corridors_command(&corridors, "ROADTEMPLATE_RENAME")?;
        let mut transaction = Transaction::new("ROADTEMPLATE_RENAME");
        if let Some(step) = templates_step {
            transaction.add(step);
        }
        if let Some(step) = corridors_step {
            transaction.add(step);
        }
        if transaction.size() > 0 {
            document.execute(Box::new(transaction))?;
        }
        return Ok(format!(
            "template renamed from={} to={} corridors={following}",
            text(&road_template.name),
            text(new_name)
        ));
    }
    if sub == "SET" && args.len() > 2 && option_of(&args[2]).is_some() {
        let options = options_from(args, 2)?;
        let mut set = RoadTemplateSet::clone(&current);
        for option in options {
            if option.key != "description" {
                return Err(usage(
                    "ROADTEMPLATE SET <template> takes description=; a component's keys follow \
                     its name: ROADTEMPLATE SET <template> <component> ...",
                ));
            }
            set.templates[index].description = option.value;
        }
        let reply = format!("{} updated", template_record(&set.templates[index]));
        return commit_templates(document, &set, "ROADTEMPLATE_SET", reply);
    }
    if sub == "ADD" {
        if args.len() < 4 {
            return Err(usage(
                "ROADTEMPLATE ADD <template> left|right|both <kind> [key=value ...]",
            ));
        }
        let sides: Vec<Side> = if is(&args[2], "BOTH") {
            vec![Side::Left, Side::Right]
        } else if let Some(side) = side_from(&args[2]) {
            vec![side]
        } else {
            return Err(usage(format!(
                "a side is left, right or both, not \"{}\"",
                args[2]
            )));
        };
        let Some(kind) = component_kind_from(&args[3]) else {
            return Err(usage(format!(
                "a component is a lane, shoulder, kerb, footpath, verge, median, drain or \
                 batter, not \"{}\"",
                args[3]
            )));
        };
        let options = options_from(args, 4)?;
        let mut set = RoadTemplateSet::clone(&current);
        let edited = &mut set.templates[index];
        let mut reply = String::new();
        for side in sides {
            let mut component = default_component(kind, side);
            let mut at: Option<usize> = None;
            for option in &options {
                if option.key == "at" {
                    at = Some(position_of(option)?);
                    continue;
                }
                apply_component(&mut component, option)?;
            }
            // A second of a kind is named after its place: lane2, lane3.
            if edited.find(side, &component.name).is_some() {
                let base = component.name.clone();
                for n in 2.. {
                    let candidate = format!("{base}{n}");
                    if edited.find(side, &candidate).is_none() {
                        component.name = candidate;
                        break;
                    }
                }
            }
            let position = insertion_point(edited, side, kind, at);
            add_line(
                &mut reply,
                &format!("{} added", component_record(&edited.name, &component)),
            );
            edited.components.insert(position, component);
        }
        return commit_templates(document, &set, "ROADTEMPLATE_ADD", reply);
    }
    if sub == "SET" || sub == "REMOVE" {
        if args.len() < 3 {
            return Err(usage(format!(
                "ROADTEMPLATE {sub} <template> <component>{}",
                if sub == "SET" { " key=value ..." } else { "" }
            )));
        }
        let indices = components_named(road_template, &args[2])?;
        let mut set = RoadTemplateSet::clone(&current);
        let edited = &mut set.templates[index];
        let mut reply = String::new();
        if sub == "REMOVE" {
            for &i in indices.iter().rev() {
                let gone = &edited.components[i];
                let line = format!(
                    "component template={} side={} name={} removed",
                    text(&edited.name),
                    gone.side.name(),
                    gone.name
                );
                reply = if reply.is_empty() {
                    line
                } else {
                    format!("{line}\n{reply}")
                };
                edited.components.remove(i);
            }
            return commit_templates(document, &set, "ROADTEMPLATE_REMOVE", reply);
        }
        // SET takes any number of groups, <component> key=value ..., and
        // applies them all to the one copy before committing it: a grid of
        // edited rows (the Road Template Manager's Apply) is ONE undo step, and
        // a value that any group is refused for changes nothing. Each group
        // names its component in the copy as the groups before it left it, as
        // the same lines typed one after the other would.
        let mut at = 2;
        while at < args.len() {
            let named = components_named(edited, &args[at])?;
            let (options, end) = group_from(args, at);
            if options.is_empty() {
                return Err(usage(format!(
                    "ROADTEMPLATE SET <template> <component> needs key=value: {} has none",
                    args[at]
                )));
            }
            for &i in &named {
                for option in &options {
                    apply_component(&mut edited.components[i], option)?;
                }
                add_line(
                    &mut reply,
                    &format!(
                        "{} updated",
                        component_record(&edited.name, &edited.components[i])
                    ),
                );
            }
            at = end;
        }
        return commit_templates(document, &set, "ROADTEMPLATE_SET", reply);
    }
    if sub == "COURSE" {
        if args.len() < 5 {
            return Err(usage(
                "ROADTEMPLATE COURSE <template> <component> ADD|SET|REMOVE <course> [key=value \
                 ...]",
            ));
        }
        let indices = components_named(road_template, &args[2])?;
        let action = upper(&args[3]);
        let mut set = RoadTemplateSet::clone(&current);
        let edited = &mut set.templates[index];
        let template_name = edited.name.clone();
        let mut reply = String::new();
        if action == "SET" {
            // Any number of groups, <course> key=value ..., on the one copy:
            // as ROADTEMPLATE SET <t> <component> ..., one undo step that a
            // value refused anywhere leaves undone.
            let mut at = 4;
            while at < args.len() {
                let set_name = &args[at];
                let (options, end) = group_from(args, at);
                if options.is_empty() {
                    return Err(usage(format!(
                        "ROADTEMPLATE COURSE ... SET <course> needs key=value: {set_name} has \
                         none"
                    )));
                }
                for &i in &indices {
                    let component = &mut edited.components[i];
                    let Some(course) = component
                        .courses
                        .iter()
                        .position(|course| is(&course.name, set_name))
                    else {
                        return Err(Error::new(
                            ErrorCode::NotFound,
                            format!(
                                "{}:{} has no course {set_name}",
                                component.side.name(),
                                component.name
                            ),
                        ));
                    };
                    for option in &options {
                        apply_course(&mut component.courses[course], option)?;
                    }
                    add_line(
                        &mut reply,
                        &format!(
                            "{} updated",
                            course_record(&template_name, component, &component.courses[course])
                        ),
                    );
                }
                at = end;
            }
            return commit_templates(document, &set, "ROADTEMPLATE_COURSE", reply);
        }
        let course_name = &args[4];
        let options = options_from(args, 5)?;
        for &i in &indices {
            let component = &mut edited.components[i];
            let existing = component
                .courses
                .iter()
                .position(|course| is(&course.name, course_name));
            if action == "ADD" {
                if existing.is_some() {
                    return Err(Error::new(
                        ErrorCode::AlreadyExists,
                        format!(
                            "{}:{} has a course called {course_name}",
                            component.side.name(),
                            component.name
                        ),
                    ));
                }
                check_word("a course", course_name)?;
                let mut made = make_course(course_name, 0.0, "", DEFAULT_COURSE_TYPE);
                let mut at: Option<usize> = None;
                let mut colour_given = false;
                for option in &options {
                    if option.key == "at" {
                        at = Some(position_of(option)?);
                        continue;
                    }
                    colour_given = colour_given || option.key == "colour" || option.key == "color";
                    apply_course(&mut made, option)?;
                }
                if !colour_given {
                    made.colour =
                        make_course(&made.name, 0.0, &made.material, DEFAULT_COURSE_TYPE).colour;
                }
                let position = at.map_or(component.courses.len(), |at| {
                    (at - 1).min(component.courses.len())
                });
                add_line(
                    &mut reply,
                    &format!("{} added", course_record(&template_name, component, &made)),
                );
                component.courses.insert(position, made);
                continue;
            }
            let Some(found) = existing else {
                return Err(Error::new(
                    ErrorCode::NotFound,
                    format!(
                        "{}:{} has no course {course_name}",
                        component.side.name(),
                        component.name
                    ),
                ));
            };
            if action == "REMOVE" {
                let line = format!(
                    "course template={} component={}:{} name={} removed",
                    text(&template_name),
                    component.side.name(),
                    component.name,
                    component.courses[found].name
                );
                add_line(&mut reply, &line);
                component.courses.remove(found);
                continue;
            }
            return Err(usage(format!(
                "ROADTEMPLATE COURSE ... ADD, SET or REMOVE, not {}",
                args[3]
            )));
        }
        return commit_templates(document, &set, "ROADTEMPLATE_COURSE", reply);
    }
    Err(usage(format!(
        "unknown ROADTEMPLATE word {} (HELP ROADTEMPLATE)",
        args[0]
    )))
}

// ---- corridors --------------------------------------------------------------------------

fn corridor_record(corridor: &Corridor) -> String {
    format!(
        "corridor name={} alignment={} template={} surface={} interval={} from={} to={} \
         regions={} superelevation={} curvetol={}",
        text(&corridor.name),
        text(&corridor.alignment),
        text(&corridor.template_name),
        text(&corridor.surface),
        exact(corridor.interval),
        corridor.start.map_or_else(String::new, exact),
        corridor.end.map_or_else(String::new, exact),
        corridor.regions.len(),
        corridor.superelevation.len(),
        exact(corridor.curve_tolerance)
    )
}

// The layers a corridor's objects are shown on that the drawing does not
// have, parents first, the cut's switched off (bim_view).
fn missing_layers(document: &Document, paths: &[String]) -> Vec<Layer> {
    let mut layers = Vec::new();
    let mut planned: BTreeSet<String> = BTreeSet::new();
    for path in paths {
        if path.is_empty() || validate_layer_path(path).is_err() {
            continue;
        }
        let mut chain = layer_ancestors(path);
        chain.push(path.clone());
        for name in chain {
            if document.model().layers.contains(&name) || !planned.insert(name.clone()) {
                continue;
            }
            let visible = name != CUT_LAYER;
            layers.push(Layer {
                name,
                visible,
                ..Layer::default()
            });
        }
    }
    layers
}

fn object_layers(corridor: &Corridor, templates: &RoadTemplateSet) -> Vec<String> {
    let mut paths = Vec::new();
    let mut add = |name: &str| {
        if let Some(road_template) = templates.find(name) {
            for component in &road_template.components {
                paths.push(component.layer.clone());
            }
        }
    };
    add(&corridor.template_name);
    for region in &corridor.regions {
        add(&region.template_name);
    }
    paths.push(String::from(CUT_LAYER));
    paths.push(String::from(FILL_LAYER));
    paths
}

// The corridors replaced by `next` as one step, with the layers its objects
// need made in it. 0: nothing changed.
fn commit_corridors(
    document: &mut Document,
    next: &CorridorSet,
    layers_for: Option<&Corridor>,
    step_name: &str,
) -> Result<usize> {
    let command = document.corridors_command(next, step_name)?;
    let layers = match layers_for {
        Some(corridor) => missing_layers(
            document,
            &object_layers(corridor, &document.road_templates()),
        ),
        None => Vec::new(),
    };
    if command.is_none() && layers.is_empty() {
        return Ok(0);
    }
    let mut transaction = Transaction::new(step_name);
    for layer in &layers {
        transaction.add(create_layer(layer.clone()));
    }
    if let Some(command) = command {
        transaction.add(command);
    }
    document.execute(Box::new(transaction))?;
    Ok(layers.len())
}

fn built_record(built: &BuiltCorridor, objects: usize) -> String {
    // Absent is not zero: a corridor that could not be built has no range
    // (station 0 is a real station), and one with no target surface measured
    // no cut or fill - the problems say why. The key is there and empty, as a
    // corridor's own from= and to= are when it gives none.
    let measured = |present: bool, value: f64| {
        if present { fixed(value) } else { String::new() }
    };
    format!(
        "built corridor={} from={} to={} runs={} sections={} incomplete={} objects={objects} \
         cut={} fill={} net={} problems={}",
        text(&built.name),
        measured(built.has_range, built.start),
        measured(built.has_range, built.end),
        built.runs.len(),
        built.sections,
        built.incomplete_sections,
        measured(built.has_earthworks, built.cut),
        measured(built.has_earthworks, built.fill),
        measured(built.has_earthworks, built.fill - built.cut),
        built.problems.len()
    )
}

fn problem_lines(problems: &[String]) -> String {
    let mut out = String::new();
    for problem in problems {
        out.push_str(&format!("\nproblem message={}", reply_quoted(problem)));
    }
    out
}

fn check_names(document: &Document, corridor: &Corridor) -> Status {
    let Some(alignment) = document.model().alignments.find(&corridor.alignment) else {
        return Err(Error::new(
            ErrorCode::NotFound,
            format!("no alignment {}: ALIGN LIST names them", corridor.alignment),
        ));
    };
    if alignment.vertical.is_none() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "the alignment {} has no design profile: ALIGN DESIGN gives it one",
                corridor.alignment
            ),
        ));
    }
    if document
        .road_templates()
        .find(&corridor.template_name)
        .is_none()
    {
        return Err(no_template(&corridor.template_name));
    }
    if !corridor.surface.is_empty() && document.surfaces().find(&corridor.surface).is_none() {
        return Err(Error::new(
            ErrorCode::NotFound,
            format!("no surface {}: SURFACE LIST names them", corridor.surface),
        ));
    }
    Ok(())
}

fn apply_corridor(corridor: &mut Corridor, option: &KeyValue) -> Status {
    let station = |field: &mut Option<f64>| -> Status {
        if option.value.is_empty() {
            *field = None;
            return Ok(());
        }
        *field = Some(number_of(option)?);
        Ok(())
    };
    // A corridor needs an alignment and a template; the other keys can be
    // emptied (surface, from, to) or have a default (interval, 10 m).
    if (option.key == "alignment" || option.key == "template") && option.value.is_empty() {
        return Err(usage(format!(
            "{}= needs a name: a corridor has {}",
            option.key,
            if option.key == "alignment" {
                "an alignment (ALIGN LIST names them)"
            } else {
                "a road template (ROADTEMPLATE LIST names them)"
            }
        )));
    }
    match option.key.as_str() {
        "alignment" => corridor.alignment.clone_from(&option.value),
        "template" => corridor.template_name.clone_from(&option.value),
        "surface" => corridor.surface.clone_from(&option.value),
        "interval" => corridor.interval = number_of(option)?,
        "curvetol" => {
            // "off" is 0, as the record writes it: no section is added in a
            // curve.
            corridor.curve_tolerance = if is(&option.value, "off") {
                0.0
            } else {
                number_of(option)?
            };
        }
        "from" => station(&mut corridor.start)?,
        "to" => station(&mut corridor.end)?,
        _ => {
            return Err(usage(format!(
                "unknown corridor key {}=; the keys are alignment template surface interval \
                 from to curvetol",
                option.key
            )));
        }
    }
    Ok(())
}

fn station_word(word: &str, what: &str) -> Result<f64> {
    parse_finite_double(word).ok_or_else(|| usage(format!("{what} is a number, not \"{word}\"")))
}

// The strings in the drawing that CORRIDOR STRINGS made for `corridor`: the
// entities marked with both properties, the corridor's name read as
// corridors are found, ignoring case.
fn strings_made_for(document: &Document, corridor: &str) -> Vec<EntityId> {
    let mut ids = Vec::new();
    document.model().entities.for_each(|entity| {
        if let Some(PropertyValue::Text(name)) = entity.properties.get(CORRIDOR_PROPERTY)
            && entity.properties.contains_key(STRING_PROPERTY)
            && is(name, corridor)
        {
            ids.push(entity.id);
        }
    });
    ids
}

fn quantity_value(object: &BimObject, name: &str) -> String {
    object
        .quantities
        .iter()
        .find(|quantity| quantity.name == name)
        .map_or_else(String::new, |quantity| fixed(quantity.value))
}

fn object_record(object: &BimObject) -> String {
    let mut out = format!(
        "object id={} kind={} name={}",
        text(&object.id),
        object.kind,
        text(&object.name)
    );
    for quantity in &object.quantities {
        out.push_str(&format!(" {}={}", quantity.name, fixed(quantity.value)));
    }
    out
}

// The class and, where it has one, its predefined type: IfcKerb,
// IfcCourse.PAVEMENT.
fn ifc_name(object: &BimObject) -> String {
    if object.predefined_type.is_empty() {
        object.ifc_class.clone()
    } else {
        format!("{}.{}", object.ifc_class, object.predefined_type)
    }
}

fn belongs_to(object: &BimObject, corridor: Option<&str>) -> bool {
    if !object.id.starts_with("corridor/") {
        return false;
    }
    corridor.is_none_or(|name| is(&object.facility, name))
}

// A problem of the corridors, or of the definitions they are made from: not a
// placed road object's (source "object <id>" or "road objects"), which
// ROADMODEL PROBLEMS lists with them. With a corridor named, its own only.
fn corridor_problem(problem: &BimProblem, corridor: Option<&str>) -> bool {
    if let Some(name) = corridor {
        return is(&problem.source, &format!("corridor {name}"));
    }
    is(&problem.source, "road templates")
        || is(&problem.source, "corridors")
        || problem.source.starts_with("corridor ")
}

// A total has a length, an area or a volume only if some object of its kind
// carries one: a kerb has no area, and a total of nothing is not 0.000.
#[derive(Default)]
struct Total {
    count: usize,
    length: Option<f64>,
    area: Option<f64>,
    volume: Option<f64>,
}

fn quantities_verb(document: &Document, args: &[String]) -> Result<String> {
    if args.len() < 2 {
        return Err(usage(
            "CORRIDOR QUANTITIES <corridor>|ALL [csv=<file> [OVERWRITE]]",
        ));
    }
    let all = is(&args[1], "ALL");
    let corridors = document.corridors();
    let corridor = if all {
        None
    } else {
        Some(
            corridors
                .find(&args[1])
                .ok_or_else(|| no_corridor(&args[1]))?,
        )
    };
    // OVERWRITE is a word, as the other report verbs have it, not a key.
    let mut words: Vec<String> = args[..2].to_vec();
    let mut overwrite = false;
    for arg in &args[2..] {
        if is(arg, "OVERWRITE") {
            overwrite = true;
        } else {
            words.push(arg.clone());
        }
    }
    let mut csv: Option<String> = None;
    for option in options_from(&words, 2)? {
        if option.key != "csv" {
            return Err(usage(format!(
                "CORRIDOR QUANTITIES takes csv=<file>, not {}=",
                option.key
            )));
        }
        csv = Some(option.value);
    }
    if overwrite && csv.is_none() {
        return Err(usage(
            "OVERWRITE replaces the file csv=<file> names: CORRIDOR QUANTITIES <corridor>|ALL \
             csv=<file> OVERWRITE",
        ));
    }
    let model = bim_model(document);
    let name: Option<&str> = corridor.map(|found| found.name.as_str());
    let mut objects: Vec<BimObject> = Vec::new();
    let mut cut = 0.0;
    let mut fill = 0.0;
    let mut totals: BTreeMap<(String, String), Total> = BTreeMap::new();
    for object in &model.objects {
        if !belongs_to(object, name) {
            continue;
        }
        objects.push(object.clone());
        let mut volume = 0.0;
        let total = totals
            .entry((object.kind.clone(), object.material.clone()))
            .or_default();
        total.count += 1;
        for quantity in &object.quantities {
            if quantity.name == "GrossVolume" {
                volume = quantity.value;
                total.volume = Some(total.volume.unwrap_or(0.0) + quantity.value);
            } else if quantity.name == "GrossArea" {
                total.area = Some(total.area.unwrap_or(0.0) + quantity.value);
            } else if quantity.name == "Length" {
                total.length = Some(total.length.unwrap_or(0.0) + quantity.value);
            }
        }
        if object.ifc_class == "IfcEarthworksCut" {
            cut += volume;
        } else if object.ifc_class == "IfcEarthworksFill" {
            fill += volume;
        }
    }
    // Cut and fill are measured only against a target surface: a corridor
    // with none, or whose alignment, profile or surface the drawing lacks,
    // reports them absent (empty), not 0.000, which would read as a road with
    // no cut.
    let mut measured = false;
    for each in &corridors.corridors {
        if name.is_none_or(|wanted| is(&each.name, wanted)) {
            let inputs = corridor_inputs(document, each);
            let has_all =
                inputs.alignment.is_some() && inputs.profile.is_some() && inputs.ground.is_some();
            measured = measured || has_all;
        }
    }
    let sum = |value: f64| {
        if measured {
            fixed(value)
        } else {
            String::new()
        }
    };
    let shown = |value: Option<f64>| value.map_or_else(String::new, fixed);
    let mut reply = format!(
        "quantities corridor={} objects={} cut={} fill={} net={}",
        name.map_or_else(|| String::from("all"), text),
        objects.len(),
        sum(cut),
        sum(fill),
        sum(fill - cut)
    );
    for object in &objects {
        reply.push('\n');
        reply.push_str(&object_record(object));
    }
    for ((kind, material), total) in &totals {
        reply.push_str(&format!(
            "\ntotal kind={kind} material={} count={} length={} area={} volume={}",
            text(material),
            total.count,
            shown(total.length),
            shown(total.area),
            shown(total.volume)
        ));
    }
    for problem in &model.problems {
        if corridor_problem(problem, name) {
            reply.push_str(&format!(
                "\nproblem source={} message={}",
                text(&problem.source),
                reply_quoted(&problem.message)
            ));
        }
    }
    if let Some(csv) = csv {
        let written = write_csv_table(&quantities_table(&objects), &csv, overwrite)?;
        reply.push('\n');
        reply.push_str(&written);
    }
    Ok(reply)
}

// The first problem as a clause, or nothing.
fn first_problem(problems: &[String]) -> String {
    problems
        .first()
        .map_or_else(String::new, |problem| format!(": {problem}"))
}

fn strings_verb(document: &mut Document, corridor: &Corridor, args: &[String]) -> Result<String> {
    let mut layer = String::from(STRINGS_LAYER);
    let mut preview = false;
    for arg in args.iter().skip(2) {
        if is(arg, "PREVIEW") {
            preview = true;
            continue;
        }
        match option_of(arg) {
            Some(option) if option.key == "layer" => {
                validate_layer_path(&option.value)?;
                layer = option.value;
            }
            _ => {
                return Err(usage(
                    "CORRIDOR STRINGS <corridor> [layer=<layer>] [PREVIEW]",
                ));
            }
        }
    }
    let built = build_corridor_in(document, corridor);
    let strings = corridor_strings(&built, false);
    if strings.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            format!(
                "corridor {} has no sections to string{}",
                corridor.name,
                first_problem(&built.problems)
            ),
        ));
    }
    let previous = strings_made_for(document, &corridor.name);
    let mut points = 0usize;
    let mut entities: Vec<Entity> = Vec::new();
    for string in &strings {
        let vertices: Vec<Point2> = string
            .points
            .iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect();
        let heights: Vec<Option<f64>> = string.points.iter().map(|point| Some(point.z)).collect();
        points += string.points.len();
        let mut entity = Entity {
            geometry: Geometry::Polyline(Polyline2::new(vertices, false)),
            layer: layer.clone(),
            ..Entity::default()
        };
        entity.properties.insert(
            String::from(CORRIDOR_PROPERTY),
            PropertyValue::from(corridor.name.as_str()),
        );
        entity.properties.insert(
            String::from(STRING_PROPERTY),
            PropertyValue::from(string.code.as_str()),
        );
        set_heights(&mut entity.properties, &heights);
        entities.push(entity);
    }
    let record = format!(
        " corridor={} layer={} points={points}",
        text(&corridor.name),
        text(&layer)
    );
    if preview {
        return Ok(format!(
            "strings created={} replaced={}{record} preview=yes (nothing changed)",
            entities.len(),
            previous.len()
        ));
    }
    let replaced = previous.len();
    let mut transaction = Transaction::new("CORRIDOR_STRINGS");
    if !previous.is_empty() {
        transaction.add(delete_entities(previous));
    }
    for made in missing_layers(document, std::slice::from_ref(&layer)) {
        transaction.add(create_layer(made));
    }
    let created = entities.len();
    transaction.add(create_entities(entities));
    document.execute(Box::new(transaction))?;
    Ok(format!(
        "strings created={created} replaced={replaced}{record}"
    ))
}

fn surface_verb(document: &mut Document, corridor: &Corridor, args: &[String]) -> Result<String> {
    let mut finished_name = format!("{} finished", corridor.name);
    let mut subgrade_name = format!("{} subgrade", corridor.name);
    let mut preview = false;
    for arg in args.iter().skip(2) {
        if is(arg, "PREVIEW") {
            preview = true;
            continue;
        }
        match option_of(arg) {
            Some(option) if option.key == "finished" && !option.value.is_empty() => {
                finished_name = option.value;
            }
            Some(option) if option.key == "subgrade" && !option.value.is_empty() => {
                subgrade_name = option.value;
            }
            _ => {
                return Err(usage(
                    "CORRIDOR SURFACE <corridor> [finished=<name>] [subgrade=<name>] [PREVIEW]",
                ));
            }
        }
    }
    if is(&finished_name, &subgrade_name) {
        return Err(usage(
            "the finished surface and the subgrade need two names",
        ));
    }
    let built = build_corridor_in(document, corridor);
    let Some(alignment) = &built.alignment else {
        return Err(Error::new(
            ErrorCode::InvalidState,
            format!(
                "corridor {} cannot be built{}",
                corridor.name,
                first_problem(&built.problems)
            ),
        ));
    };
    let sections = corridor_sections(&built);
    if sections.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidState,
            format!(
                "corridor {} has fewer than two sections to make a surface of{}",
                corridor.name,
                first_problem(&built.problems)
            ),
        ));
    }
    let finished = sections_surface(&sections, alignment, false, false)?;
    let subgrade = sections_surface(&sections, alignment, true, false)?;
    let reply = format!(
        "surface name={} kind=finished triangles={} sections={}\nsurface name={} kind=subgrade \
         triangles={} sections={}",
        text(&finished_name),
        finished.surface.triangle_count(),
        finished.sections,
        text(&subgrade_name),
        subgrade.surface.triangle_count(),
        subgrade.sections
    );
    if preview {
        return Ok(format!("{reply}\npreview=yes (nothing changed)"));
    }
    let mut store = document.surfaces().clone();
    let source = format!("corridor {}", corridor.name);
    for (surface_name, surface) in [
        (&finished_name, finished.surface),
        (&subgrade_name, subgrade.surface),
    ] {
        let shared = Some(Arc::new(surface));
        if store.find(surface_name).is_some() {
            store.replace(surface_name, shared)?;
        } else {
            store.add(NamedSurface {
                name: surface_name.clone(),
                surface: shared,
                source: source.clone(),
                style: String::new(),
                visible: true,
            })?;
        }
    }
    document.set_surfaces(store, "CORRIDOR_SURFACE")?;
    Ok(reply)
}

// Stations ascending, as the C++'s std::ranges::sort by them. That sort is
// not stable, so two entries of one station may come in either order there;
// here they keep the order they were given.
fn by_station(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

fn corridor_verb(document: &mut Document, args: &[String]) -> Result<String> {
    let sub = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| upper(word));
    let current = document.corridors();
    if sub == "LIST" || sub == "JSON" {
        let json = sub == "JSON" || args.get(1).is_some_and(|word| is(word, "JSON"));
        document.corridors_status()?;
        if json {
            return corridors_to_json(&current);
        }
        if current.corridors.is_empty() {
            return Ok(String::from("corridors=0 (CORRIDOR NEW makes one)"));
        }
        let mut reply = format!("corridors={}", current.corridors.len());
        for corridor in &current.corridors {
            reply.push('\n');
            reply.push_str(&corridor_record(corridor));
        }
        return Ok(reply);
    }
    if sub == "PROBLEMS" {
        let model = bim_model(document);
        let mut named: Option<String> = None;
        if let Some(wanted) = args.get(1) {
            let Some(corridor) = current.find(wanted) else {
                return Err(no_corridor(wanted));
            };
            named = Some(corridor.name.clone());
        }
        let mut reply = String::new();
        let mut count = 0usize;
        for problem in &model.problems {
            if !corridor_problem(problem, named.as_deref()) {
                continue;
            }
            count += 1;
            reply.push_str(&format!(
                "\nproblem source={} message={}",
                text(&problem.source),
                reply_quoted(&problem.message)
            ));
        }
        return Ok(format!("problems={count}{reply}"));
    }
    if sub == "QUANTITIES" {
        return quantities_verb(document, args);
    }
    if sub == "BUILD" && args.get(1).is_some_and(|word| is(word, "ALL")) {
        // Corridors that cannot be read are not "none": the same refusal as
        // CORRIDOR LIST, not a reply that says to make one.
        corridors_readable(document)?;
        if current.corridors.is_empty() {
            return Ok(String::from("corridors=0 (CORRIDOR NEW makes one)"));
        }
        let mut reply = String::new();
        for corridor in &current.corridors {
            let built = build_corridor_in(document, corridor);
            let objects = corridor_solids(&built);
            add_line(
                &mut reply,
                &format!(
                    "{}{}",
                    built_record(&built, objects.objects.len()),
                    problem_lines(&built.problems)
                ),
            );
        }
        return Ok(reply);
    }
    corridors_readable(document)?;
    if args.len() < 2 {
        return Err(usage(format!(
            "CORRIDOR {sub} needs a corridor's name (HELP CORRIDOR)"
        )));
    }
    let name = &args[1];
    if sub == "NEW" {
        check_name("a corridor", name)?;
        if current.find(name).is_some() {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                format!("there is a corridor called {name}"),
            ));
        }
        let mut made = Corridor {
            name: name.clone(),
            ..Corridor::default()
        };
        let mut at = 2;
        while at + 1 < args.len() && option_of(&args[at]).is_none() {
            if is(&args[at], "ALIGNMENT") {
                made.alignment.clone_from(&args[at + 1]);
            } else if is(&args[at], "TEMPLATE") {
                made.template_name.clone_from(&args[at + 1]);
            } else {
                break;
            }
            at += 2;
        }
        if made.alignment.is_empty() || made.template_name.is_empty() {
            return Err(usage(
                "CORRIDOR NEW <corridor> ALIGNMENT <alignment> TEMPLATE <template> \
                 [surface=<name>] [interval=<m>] [from=<s>] [to=<s>] [curvetol=<e>]",
            ));
        }
        for option in options_from(args, at)? {
            if option.key == "alignment" || option.key == "template" {
                return Err(usage(
                    "CORRIDOR NEW names its alignment and template by ALIGNMENT and TEMPLATE",
                ));
            }
            apply_corridor(&mut made, &option)?;
        }
        check_names(document, &made)?;
        let mut set = CorridorSet::clone(&current);
        set.corridors.push(made.clone());
        let layers = commit_corridors(document, &set, Some(&made), "CORRIDOR_NEW")?;
        let built = build_corridor_in(document, &made);
        let objects = corridor_solids(&built);
        return Ok(format!(
            "{} created layers={layers}\n{}{}",
            corridor_record(&made),
            built_record(&built, objects.objects.len()),
            problem_lines(&built.problems)
        ));
    }
    // The first corridor of that name ignoring case, as CorridorSet::find
    // finds it, by its place.
    let Some(index) = current
        .corridors
        .iter()
        .position(|each| each.name.eq_ignore_ascii_case(name))
    else {
        return Err(no_corridor(name));
    };
    let corridor = &current.corridors[index];
    if sub == "INFO" {
        let mut reply = corridor_record(corridor);
        for region in &corridor.regions {
            reply.push_str(&format!(
                "\nregion from={} to={} template={}",
                exact(region.start),
                exact(region.end),
                text(&region.template_name)
            ));
        }
        for point in &corridor.superelevation {
            reply.push_str(&format!(
                "\nsuperelevation station={} left={} right={}",
                exact(point.station),
                exact(point.left),
                exact(point.right)
            ));
        }
        return Ok(reply);
    }
    if sub == "BUILD" {
        let built = build_corridor_in(document, corridor);
        let objects = corridor_solids(&built);
        return Ok(format!(
            "{}{}",
            built_record(&built, objects.objects.len()),
            problem_lines(&built.problems)
        ));
    }
    if sub == "SECTION" {
        if args.len() < 3 {
            return Err(usage("CORRIDOR SECTION <corridor> <station>"));
        }
        let station = station_word(&args[2], "a station")?;
        let inputs = corridor_inputs(document, corridor);
        let (Some(alignment), Some(profile)) = (&inputs.alignment, &inputs.profile) else {
            return Err(Error::new(
                ErrorCode::InvalidState,
                format!(
                    "corridor {} cannot be built{}",
                    corridor.name,
                    first_problem(&inputs.problems)
                ),
            ));
        };
        // The corridor has sections only over its range (from= to= held to
        // the alignment and the profile): a section outside it would look
        // built and be nothing the corridor has.
        let range = corridor_range(corridor, alignment, profile);
        if !range.ok {
            return Err(Error::new(
                ErrorCode::InvalidState,
                format!(
                    "corridor {} has no stations to section{}",
                    corridor.name,
                    first_problem(&range.problems)
                ),
            ));
        }
        if station < range.start - tolerance::GEOMETRIC
            || station > range.end + tolerance::GEOMETRIC
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "station {} is outside corridor {}'s stations {} to {}",
                    exact(station),
                    corridor.name,
                    exact(range.start),
                    exact(range.end)
                ),
            ));
        }
        let templates = document.road_templates();
        let wanted = template_at(corridor, station);
        let Some(road_template) = templates.find(wanted) else {
            return Err(no_template(wanted));
        };
        let section = road_section(
            road_template,
            alignment,
            profile,
            inputs.ground.as_deref(),
            station,
            superelevation_at(corridor, station),
        )?;
        let mut reply = format!(
            "section corridor={} station={} template={} design={} complete={} cut={} fill={} \
             leftcut={} leftfill={} rightcut={} rightfill={}",
            text(&corridor.name),
            fixed(section.station),
            text(&road_template.name),
            fixed(section.design_level),
            yes(section.complete),
            fixed(section.cut_area),
            fixed(section.fill_area),
            fixed(section.left_cut),
            fixed(section.left_fill),
            fixed(section.right_cut),
            fixed(section.right_fill)
        );
        for point in &section.top {
            reply.push_str(&format!(
                "\npoint code={} offset={} level={}",
                point.code,
                fixed(point.offset),
                fixed(point.level)
            ));
        }
        for point in &section.formation {
            reply.push_str(&format!(
                "\nformation code={} offset={} level={}",
                point.code,
                fixed(point.offset),
                fixed(point.level)
            ));
        }
        return Ok(reply);
    }
    if sub == "STRINGS" {
        return strings_verb(document, corridor, args);
    }
    if sub == "SURFACE" {
        return surface_verb(document, corridor, args);
    }
    let mut set = CorridorSet::clone(&current);
    if sub == "DELETE" {
        set.corridors.remove(index);
        commit_corridors(document, &set, None, "CORRIDOR_DELETE")?;
        return Ok(format!("corridor name={} deleted", text(&corridor.name)));
    }
    if sub == "RENAME" {
        if args.len() < 3 {
            return Err(usage("CORRIDOR RENAME <corridor> <new name>"));
        }
        check_name("a corridor", &args[2])?;
        if let Some(other) = current.find(&args[2])
            && !std::ptr::eq(other, corridor)
        {
            return Err(Error::new(
                ErrorCode::AlreadyExists,
                format!("there is a corridor called {}", args[2]),
            ));
        }
        // The strings it drew carry its name (road.corridor), which is how its
        // next STRINGS finds them to replace: they follow it, in the same step,
        // or the next STRINGS would leave them beside a second set, and a
        // corridor later given the old name would delete them.
        let strings = strings_made_for(document, &corridor.name);
        set.corridors[index].name.clone_from(&args[2]);
        let command = document.corridors_command(&set, "CORRIDOR_RENAME")?;
        let mut transaction = Transaction::new("CORRIDOR_RENAME");
        if let Some(command) = command {
            transaction.add(command);
        }
        if !strings.is_empty() {
            transaction.add(set_entity_property(
                strings.clone(),
                CORRIDOR_PROPERTY,
                PropertyValue::from(args[2].as_str()),
            ));
        }
        if transaction.size() > 0 {
            document.execute(Box::new(transaction))?;
        }
        return Ok(format!(
            "corridor renamed from={} to={} strings={}",
            text(&corridor.name),
            text(&args[2]),
            strings.len()
        ));
    }
    if sub == "SET" {
        let options = options_from(args, 2)?;
        if options.is_empty() {
            return Err(usage("CORRIDOR SET <corridor> key=value ..."));
        }
        for option in &options {
            apply_corridor(&mut set.corridors[index], option)?;
        }
        check_names(document, &set.corridors[index])?;
        let layers = commit_corridors(document, &set, Some(&set.corridors[index]), "CORRIDOR_SET")?;
        return Ok(format!(
            "{} updated layers={layers}",
            corridor_record(&set.corridors[index])
        ));
    }
    if sub == "REGION" || sub == "SUPERELEVATION" {
        let action = args.get(2).map_or_else(String::new, |word| upper(word));
        let edited = &mut set.corridors[index];
        if sub == "REGION" {
            if action == "ADD" {
                if args.len() != 6 {
                    return Err(usage(
                        "CORRIDOR REGION <corridor> ADD <from> <to> <template>",
                    ));
                }
                let from = station_word(&args[3], "a region's start")?;
                let to = station_word(&args[4], "a region's end")?;
                if document.road_templates().find(&args[5]).is_none() {
                    return Err(no_template(&args[5]));
                }
                edited.regions.push(TemplateRegion {
                    start: from,
                    end: to,
                    template_name: args[5].clone(),
                });
                edited.regions.sort_by(|a, b| by_station(a.start, b.start));
            } else if action == "REMOVE" {
                if args.len() != 4 {
                    return Err(usage("CORRIDOR REGION <corridor> REMOVE <from>"));
                }
                let from = station_word(&args[3], "a region's start")?;
                let before = edited.regions.len();
                edited
                    .regions
                    .retain(|region| !((region.start - from).abs() <= tolerance::GEOMETRIC));
                if edited.regions.len() == before {
                    return Err(Error::new(
                        ErrorCode::NotFound,
                        format!("corridor {} has no region from {}", corridor.name, args[3]),
                    ));
                }
            } else if action == "CLEAR" {
                edited.regions.clear();
            } else {
                return Err(usage(
                    "CORRIDOR REGION <corridor> ADD <from> <to> <template> | REMOVE <from> | \
                     CLEAR",
                ));
            }
        } else if action == "ADD" {
            if args.len() != 6 {
                return Err(usage(
                    "CORRIDOR SUPERELEVATION <corridor> ADD <station> <left%> <right%>",
                ));
            }
            let station = station_word(&args[3], "a station")?;
            let left = station_word(&args[4], "a crossfall")?;
            let right = station_word(&args[5], "a crossfall")?;
            // An entry at a station already in the table replaces it.
            edited
                .superelevation
                .retain(|point| !((point.station - station).abs() <= tolerance::GEOMETRIC));
            edited.superelevation.push(SuperelevationPoint {
                station,
                left,
                right,
            });
            edited
                .superelevation
                .sort_by(|a, b| by_station(a.station, b.station));
        } else if action == "REMOVE" {
            if args.len() != 4 {
                return Err(usage("CORRIDOR SUPERELEVATION <corridor> REMOVE <station>"));
            }
            let station = station_word(&args[3], "a station")?;
            let before = edited.superelevation.len();
            edited
                .superelevation
                .retain(|point| !((point.station - station).abs() <= tolerance::GEOMETRIC));
            if edited.superelevation.len() == before {
                return Err(Error::new(
                    ErrorCode::NotFound,
                    format!(
                        "corridor {} has no superelevation at {}",
                        corridor.name, args[3]
                    ),
                ));
            }
        } else if action == "CLEAR" {
            edited.superelevation.clear();
        } else {
            return Err(usage(
                "CORRIDOR SUPERELEVATION <corridor> ADD <station> <left%> <right%> | REMOVE \
                 <station> | CLEAR",
            ));
        }
        let step = format!("CORRIDOR_{sub}");
        commit_corridors(document, &set, Some(&set.corridors[index]), &step)?;
        let edited = &set.corridors[index];
        let mut reply = format!("{} updated", corridor_record(edited));
        if sub == "REGION" {
            for region in &edited.regions {
                reply.push_str(&format!(
                    "\nregion from={} to={} template={}",
                    exact(region.start),
                    exact(region.end),
                    text(&region.template_name)
                ));
            }
        } else {
            for point in &edited.superelevation {
                reply.push_str(&format!(
                    "\nsuperelevation station={} left={} right={}",
                    exact(point.station),
                    exact(point.left),
                    exact(point.right)
                ));
            }
        }
        return Ok(reply);
    }
    Err(usage(format!(
        "unknown CORRIDOR word {} (HELP CORRIDOR)",
        args[0]
    )))
}

// ---- the whole model ------------------------------------------------------------------------

fn road_model_verb(document: &Document, args: &[String]) -> Result<String> {
    let sub = args
        .first()
        .map_or_else(|| String::from("LIST"), |word| upper(word));
    let model = bim_model(document);
    if sub == "JSON" {
        return Ok(road_model_json(&model));
    }
    if sub == "PROBLEMS" || sub == "LIST" {
        let mut reply = format!(
            "road model objects={} problems={}",
            model.objects.len(),
            model.problems.len()
        );
        if sub == "LIST" {
            for object in &model.objects {
                reply.push_str(&format!(
                    "\nobject id={} kind={} facility={} part={} ifc={} layer={} triangles={} \
                     volume={}",
                    text(&object.id),
                    object.kind,
                    text(&object.facility),
                    object.part,
                    ifc_name(object),
                    text(&object.layer),
                    object.mesh.triangle_count(),
                    quantity_value(object, "GrossVolume")
                ));
            }
        }
        for problem in &model.problems {
            reply.push_str(&format!(
                "\nproblem source={} message={}",
                text(&problem.source),
                reply_quoted(&problem.message)
            ));
        }
        return Ok(reply);
    }
    if sub == "INFO" {
        let Some(wanted) = args.get(1) else {
            return Err(usage("ROADMODEL INFO <id>"));
        };
        let Some(object) = model.objects.iter().find(|object| object.id == *wanted) else {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!("the road model has no object {wanted}: ROADMODEL LIST names them"),
            ));
        };
        let mut reply = format!(
            "{} facility={} part={} ifc={} material={} colour={} layer={} triangles={}",
            object_record(object),
            text(&object.facility),
            object.part,
            ifc_name(object),
            text(&object.material),
            object.colour.to_hex(),
            text(&object.layer),
            object.mesh.triangle_count()
        );
        for (key, value) in &object.properties {
            reply.push_str(&format!(
                "\nproperty name={} value={}",
                text(key),
                text(value)
            ));
        }
        return Ok(reply);
    }
    if sub == "SHOW" {
        // The window's switch, as GRID is: whether its views draw the road
        // model. The window takes the line before it reaches here; a session
        // with no views is told so by name, and where to hide objects instead.
        return Err(Error::new(
            ErrorCode::Unsupported,
            "ROADMODEL SHOW is the desktop window's: it sets whether its views draw the road \
             model, and katana_cli and katana_mcp have no views; hide the objects there by their \
             layers (LAYER HIDE road), or run katana headless with --command",
        ));
    }
    Err(usage(format!(
        "unknown ROADMODEL word {} (HELP ROADMODEL)",
        args[0]
    )))
}

/// True for ROADTEMPLATE, CORRIDOR and ROADMODEL, in any case.
pub fn is_road_verb(verb: &str) -> bool {
    is(verb, "ROADTEMPLATE") || is(verb, "CORRIDOR") || is(verb, "ROADMODEL")
}

/// Runs one road verb: `tokens` is the whole line split into words with the
/// quotes removed (CommandInterpreter::tokenize), the verb first. Returns the
/// reply, or an error saying specifically what was refused and why.
pub fn run_road_verb(document: &mut Document, tokens: &[String]) -> Result<String> {
    let Some(verb) = tokens.first().filter(|verb| is_road_verb(verb)) else {
        return Err(usage("not a road verb"));
    };
    let args = &tokens[1..];
    if is(verb, "ROADTEMPLATE") {
        return road_template_verb(document, args);
    }
    if is(verb, "CORRIDOR") {
        return corridor_verb(document, args);
    }
    road_model_verb(document, args)
}

/// Every road verb and option, for HELP ROADTEMPLATE, HELP CORRIDOR and HELP
/// ROADMODEL.
pub fn road_verb_help() -> String {
    String::from(
        r#"Road model (docs/road_design.md): road templates, corridors and their 3D objects.

ROADTEMPLATE [LIST] [JSON]             the drawing's road templates
ROADTEMPLATE INFO <t>                  its components and their courses
ROADTEMPLATE DEFAULTS                  adds the standard templates it lacks: rural two-lane,
                                       urban street, divided road
ROADTEMPLATE SECTION <t>               the template on a level straight: its points by code
ROADTEMPLATE NEW <t> [FROM <t>] [description=<text>] | DELETE <t> | RENAME <t> <new>
ROADTEMPLATE SET <t> description=<text>
ROADTEMPLATE ADD <t> left|right|both <kind> [name=<word>] [at=<n>] [key=value ...]
  kinds: lane shoulder kerb footpath verge median drain batter. A component goes before
  the side's batter, or at=<n> (from 1, outward); a second of a name is name2.
ROADTEMPLATE SET <t> <component> key=value ... [<component> key=value ...]  |  REMOVE <t> <component>
  <component> is side:name (left:lane) or name (both sides that have it). Several groups are
  one undo step, and one refused value changes none of them. Keys:
  name= width= crossfall= (percent, outward: -3 falls) superelevated=on|off layer= colour=
  kerb: shape=barrier|mountable|kerbgutter|edgestrip faces=inward|outward height= depth=
        gutter= gutterfall= material= (width= is its top)   faces=outward is a median's
        or island's kerb, which falls going outward   drain: depth= (its invert below its
        inner edge)
  batter: cut= fill= (run per rise: 2 is 1V:2H) maxwidth= benchheight= benchwidth=
ROADTEMPLATE COURSE <t> <component> ADD <course> thickness=<m> [material=] [ifc=] [colour=]
  [at=<n>]  |  SET <course> key=value ... [<course> key=value ...]  |  REMOVE <course>
  courses are top down; SET of several courses is one undo step
  ifc=: ARMOUR BALLASTBED CORE FILTER PAVEMENT PROTECTION USERDEFINED NOTDEFINED
ROADTEMPLATE EXPORT <t>|ALL <file.json> [OVERWRITE]   the template, or every one, as a JSON
  file (the form ROADTEMPLATE JSON prints); a file already there is only replaced with OVERWRITE
ROADTEMPLATE IMPORT <file.json> [OVERWRITE]   the file's templates into the drawing, one undo
  step, each checked as ROADTEMPLATE SET checks; a name the drawing has is refused (nothing is
  imported) unless OVERWRITE replaces that template where it stands

CORRIDOR [LIST] [JSON] | INFO <c> | PROBLEMS [<c>]
CORRIDOR NEW <c> ALIGNMENT <a> TEMPLATE <t> [surface=<s>] [interval=<m>] [from=<s>] [to=<s>]
  [curvetol=<e>]   makes the layers its objects are on; road/earthworks/cut switched off
CORRIDOR SET <c> alignment= template= surface= interval= from= to= curvetol=
  (surface= from= to= empty clear them; a corridor always has an alignment, a template and
  an interval)
  curvetol= is the relative error of a course's area and volume on a curve, at most (0.001
  by default, up to 0.1; 0 or off adds no sections inside curves): sections are added
  inside a curve until the chords between them are that close
CORRIDOR DELETE <c> | RENAME <c> <new>
CORRIDOR REGION <c> ADD <from> <to> <t> | REMOVE <from> | CLEAR    another template there
CORRIDOR SUPERELEVATION <c> ADD <station> <left%> <right%> | REMOVE <station> | CLEAR
  the superelevated components' crossfalls, outward, linear between entries
CORRIDOR BUILD <c>|ALL                 sections, objects, end-area cut and fill, problems
CORRIDOR SECTION <c> <station>         one section: its points, its formation, its areas
CORRIDOR STRINGS <c> [layer=<layer>] [PREVIEW]   3D strings as polylines, replacing the
                                       corridor's earlier ones, one undo step
CORRIDOR SURFACE <c> [finished=<name>] [subgrade=<name>] [PREVIEW]   both surfaces into
                                       the drawing's surfaces, one undo step
CORRIDOR QUANTITIES <c>|ALL [csv=<file> [OVERWRITE]]   each object's quantities, totals by
                                       kind and material, the earthworks cut and fill (empty
                                       with no target surface); a file already there is only
                                       replaced with OVERWRITE

ROADMODEL [LIST] | JSON | PROBLEMS | INFO <id>   every 3D object of the road model
ROADMODEL SHOW [ON|OFF]                the desktop window's: draw the road model in its views,
                                       or not (BIM > Show Road Model); katana_cli and
                                       katana_mcp have no views and refuse it"#,
    )
}

/// The road model as ROADMODEL JSON gives it - every object without its mesh
/// (its triangle count and box instead), every problem - for the MCP resource
/// katana://road-model. (The C++ wrote a name that is not UTF-8 with U+FFFD;
/// a String holds none.)
pub fn road_model_json(model: &BimModel) -> String {
    let objects = Json::array_of(model.objects.iter().map(|object| {
        let quantities = Json::array_of(object.quantities.iter().map(|quantity| {
            Json::object_of([
                ("name", Json::from(quantity.name.as_str())),
                ("value", Json::from(quantity.value)),
                ("unit", Json::from(quantity.unit.as_str())),
            ])
        }));
        let properties = Json::array_of(object.properties.iter().map(|(key, value)| {
            Json::object_of([
                ("name", Json::from(key.as_str())),
                ("value", Json::from(value.as_str())),
            ])
        }));
        let r#box = object.mesh.bounds();
        let bounds = if r#box.empty() {
            Json::array()
        } else {
            Json::array_of(
                [
                    r#box.min.x,
                    r#box.min.y,
                    r#box.min.z,
                    r#box.max.x,
                    r#box.max.y,
                    r#box.max.z,
                ]
                .into_iter()
                .map(Json::from),
            )
        };
        Json::object_of([
            ("id", Json::from(object.id.as_str())),
            ("name", Json::from(object.name.as_str())),
            ("kind", Json::from(object.kind.as_str())),
            ("facility", Json::from(object.facility.as_str())),
            ("part", Json::from(object.part.as_str())),
            ("ifcClass", Json::from(object.ifc_class.as_str())),
            (
                "predefinedType",
                Json::from(object.predefined_type.as_str()),
            ),
            ("material", Json::from(object.material.as_str())),
            ("colour", Json::from(object.colour.to_hex())),
            ("layer", Json::from(object.layer.as_str())),
            ("triangles", Json::from(object.mesh.triangle_count())),
            ("bounds", bounds),
            ("quantities", quantities),
            ("properties", properties),
        ])
    }));
    let problems = Json::array_of(model.problems.iter().map(|problem| {
        Json::object_of([
            ("source", Json::from(problem.source.as_str())),
            ("message", Json::from(problem.message.as_str())),
        ])
    }));
    let root = Json::object_of([("objects", objects), ("problems", problems)]);
    root.dump_with(2, ' ', false)
}

/// The quantities of `objects` as CORRIDOR QUANTITIES' csv= writes them (by
/// write_csv_table, the one CSV writer): a header, then a row per object: id,
/// name, kind, facility, part, material, then Length, Width, Depth, Height,
/// GrossArea and GrossVolume (empty where an object has none), to three
/// places.
pub fn quantities_table(objects: &[BimObject]) -> CsvTable {
    const QUANTITIES: [&str; 6] = [
        "Length",
        "Width",
        "Depth",
        "Height",
        "GrossArea",
        "GrossVolume",
    ];
    let mut table = CsvTable {
        header: ["id", "name", "kind", "facility", "part", "material"]
            .into_iter()
            .map(String::from)
            .collect(),
        rows: Vec::new(),
    };
    table
        .header
        .extend(QUANTITIES.into_iter().map(String::from));
    for object in objects {
        let mut row = vec![
            object.id.clone(),
            object.name.clone(),
            object.kind.clone(),
            object.facility.clone(),
            object.part.clone(),
            object.material.clone(),
        ];
        row.extend(
            QUANTITIES
                .into_iter()
                .map(|name| quantity_value(object, name)),
        );
        table.rows.push(row);
    }
    table
}
