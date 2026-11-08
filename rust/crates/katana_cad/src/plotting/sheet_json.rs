//! A SheetSet as versioned JSON: how a project stores its sheets, under the
//! "sheets" key of its metadata, with no schema change (docs/plotting.md;
//! include/katana/cad/plotting/sheet_json.hpp).
//!
//! ```text
//! {"format": "katana-sheets", "version": 1,
//!  "defaults": {...}, "numbering": "{set}-{n:02}", "revisions": [...],
//!  "sheets": [{"id": "s1", "name": "PLAN", "paper": "A1",
//!              "viewports": [{"id": "vp1", "kind": "plan",
//!                             "rect": [x0, y0, x1, y1], "scale": 1000, ...}]}]}
//! ```
//!
//! Only the format, the version and every sheet's and viewport's id (and a
//! viewport's kind) are always written; any other member is written only when
//! it differs from the default of its type (a default Sheet, Viewport,
//! SheetSet), and a member left out reads as that default. That keeps a
//! generated set - hundreds of viewports differing in a few members - small,
//! and small is fast to store and read. So a default never changes without a
//! version increase.
//!
//! Doubles are written in the shortest form that reads back to the same bits,
//! so a save and a load give back exactly the set that was saved. A later
//! version can add members an older reader skips; a version newer than
//! SHEET_SET_VERSION is refused (Unsupported) rather than read wrongly and
//! saved back without what it did not understand.
//!
//! The text is katana_core::json's, nlohmann's: the members sorted by key, as
//! `nlohmann::json` holds them, and the numbers in its Grisu2 form, so a
//! project's "sheets" key is the C++'s byte for byte.

use katana_core::error::make_error;
use katana_core::json::{Json, JsonError, JsonResult};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::EnumName;
use katana_geometry::{Box2, Point2};

use crate::plot::{PaperSize, plot_colour_mode_from};
use crate::plotting::page_setup::PageSetup;
use crate::plotting::plan_grid::grid_style_from;
use crate::plotting::sheet_set::{
    Revision, SHEET_SET_VERSION, Sheet, SheetDefaults, SheetSet, SignOff, Viewport, WorldMark,
    WorldMarkKind, legend_scope_from, viewport_kind_from,
};

const FORMAT: &str = "katana-sheets";

fn paper_text(paper: PaperSize) -> &'static str {
    match paper {
        PaperSize::A0 => "A0",
        PaperSize::A1 => "A1",
        PaperSize::A2 => "A2",
        PaperSize::A3 => "A3",
        PaperSize::A4 => "A4",
    }
}

// What stops a read: a nlohmann exception, a value the reader does not know
// (the C++'s BadValue, thrown inside the reader and caught at its edge, like
// the JSON library's own), or a refusal of the reader's own.
enum ReadFault {
    Json(JsonError),
    Bad(String),
    Refused(Error),
}

impl From<JsonError> for ReadFault {
    fn from(error: JsonError) -> Self {
        Self::Json(error)
    }
}

type Read<T> = std::result::Result<T, ReadFault>;

fn paper_from(text: &str) -> Read<PaperSize> {
    let papers = [
        ("A0", PaperSize::A0),
        ("A1", PaperSize::A1),
        ("A2", PaperSize::A2),
        ("A3", PaperSize::A3),
        ("A4", PaperSize::A4),
    ];
    match papers.iter().find(|(name, _)| *name == text) {
        Some((_, paper)) => Ok(*paper),
        None => Err(ReadFault::Bad(format!("unknown paper size \"{text}\""))),
    }
}

// `json.value(key, default)` for a default that is a value of JSON: the
// member when there is one, the default when there is not, and nlohmann's
// type_error 306 for a value that is not an object.
fn member_or<'a>(json: &'a Json, key: &str, default: &'a Json) -> JsonResult<&'a Json> {
    if !json.is_object() {
        return Err(JsonError::type_error(
            306,
            &format!("cannot use value() with {}", json.type_name()),
        ));
    }
    Ok(json.find(key).unwrap_or(default))
}

fn point_json(point: &Point2) -> Json {
    Json::array_of([Json::from(point.x), Json::from(point.y)])
}

fn point_from(json: &Json) -> Read<Point2> {
    let x = json.at_index(0)?.get::<f64>()?;
    let y = json.at_index(1)?.get::<f64>()?;
    Ok(Point2::new(x, y))
}

// A rectangle as [x0, y0, x1, y1]. The empty rectangle - a viewport not
// placed on the paper yet - is Box2's default, made of infinities JSON has
// no numbers for; it is left out like every default (viewport_json), and a
// null is read as it too. Written as they were, the infinities came out as
// nulls that no reader took back (caught by the round-trip test).
fn box_json(rect: &Box2) -> Json {
    Json::array_of([
        Json::from(rect.min.x),
        Json::from(rect.min.y),
        Json::from(rect.max.x),
        Json::from(rect.max.y),
    ])
}

fn box_from(json: &Json) -> Read<Box2> {
    if json.is_null() {
        return Ok(Box2::default());
    }
    let x0 = json.at_index(0)?.get::<f64>()?;
    let y0 = json.at_index(1)?.get::<f64>()?;
    let x1 = json.at_index(2)?.get::<f64>()?;
    let y1 = json.at_index(3)?.get::<f64>()?;
    Ok(Box2::new(Point2::new(x0, y0), Point2::new(x1, y1)))
}

// Whether every number in `json` is finite. JSON cannot hold an infinity or
// a NaN, and the writer would put a null in its place that reads back as an
// error - so such a set is refused when it is written, where the mistake is.
fn all_finite(json: &Json) -> bool {
    if let Some(number) = json.as_float() {
        return number.is_finite();
    }
    if json.is_structured() {
        return json.iter().all(all_finite);
    }
    true
}

// A member at its default is left out: most of a generated set's viewports
// differ from the defaults in a few members only, and writing every member
// made a 116-sheet set 300 KB of JSON whose writing and reading were most of
// what an edit cost (bench_sheets.cpp). The reader takes its defaults from a
// default value of the same type, so what is left out reads back as exactly
// what was left out. A default therefore never changes without a version
// increase - the reader would supply the old defaults to an old set.
//
// Doubles are compared bit for bit, so -0.0 is written and reads back as -0.0.
fn same_bits(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

fn put_number(json: &mut Json, key: &str, value: f64, fallback: f64) {
    if !same_bits(value, fallback) {
        json[key] = Json::from(value);
    }
}

fn put_flag(json: &mut Json, key: &str, value: bool, fallback: bool) {
    if value != fallback {
        json[key] = Json::from(value);
    }
}

fn put_text(json: &mut Json, key: &str, value: &str, fallback: &str) {
    if value != fallback {
        json[key] = Json::from(value);
    }
}

// An array or object, left out when it is empty.
fn put_list(json: &mut Json, key: &str, value: Json) {
    if !value.empty() {
        json[key] = value;
    }
}

fn put_point(json: &mut Json, key: &str, value: &Point2, fallback: &Point2) {
    if !same_bits(value.x, fallback.x) || !same_bits(value.y, fallback.y) {
        json[key] = point_json(value);
    }
}

// The page setup: the colour mode by name, so the enum may grow and reorder.
fn page_setup_json(setup: &PageSetup) -> Json {
    let d = PageSetup::default();
    let mut json = Json::object();
    if setup.colour_mode != d.colour_mode {
        json["colour_mode"] = Json::from(setup.colour_mode.name());
    }
    put_number(
        &mut json,
        "line_weight_scale",
        setup.line_weight_scale,
        d.line_weight_scale,
    );
    put_number(&mut json, "dpi", setup.dpi, d.dpi);
    put_text(
        &mut json,
        "file_name_pattern",
        &setup.file_name_pattern,
        &d.file_name_pattern,
    );
    put_flag(
        &mut json,
        "file_per_sheet",
        setup.file_per_sheet,
        d.file_per_sheet,
    );
    json
}

fn page_setup_from(json: &Json) -> Read<PageSetup> {
    let d = PageSetup::default();
    let mode = json.value("colour_mode", String::from(d.colour_mode.name()))?;
    let Some(colour_mode) = plot_colour_mode_from(&mode) else {
        return Err(ReadFault::Bad(format!("unknown colour mode \"{mode}\"")));
    };
    Ok(PageSetup {
        colour_mode,
        line_weight_scale: json.value("line_weight_scale", d.line_weight_scale)?,
        dpi: json.value("dpi", d.dpi)?,
        file_name_pattern: json.value("file_name_pattern", d.file_name_pattern.clone())?,
        file_per_sheet: json.value("file_per_sheet", d.file_per_sheet)?,
    })
}

fn sign_off_json(sign_off: &SignOff) -> Json {
    let mut json = Json::object();
    put_text(&mut json, "name", &sign_off.name, "");
    put_text(&mut json, "date", &sign_off.date, "");
    json
}

fn sign_off_from(json: &Json) -> Read<SignOff> {
    let name = json.value("name", String::new())?;
    let date = json.value("date", String::new())?;
    Ok(SignOff { name, date })
}

fn mark_json(mark: &WorldMark) -> Json {
    let points: Vec<Json> = mark.points.iter().map(point_json).collect();
    let kind = if mark.kind == WorldMarkKind::MatchLine {
        "match_line"
    } else {
        "sheet_outline"
    };
    let mut json = Json::object_of([("kind", Json::from(kind))]);
    put_list(&mut json, "points", Json::from(points));
    put_text(&mut json, "label", &mark.label, "");
    put_text(&mut json, "sheet", &mark.sheet, "");
    json
}

fn mark_from(json: &Json) -> Read<WorldMark> {
    let mut mark = WorldMark::default();
    let kind = json.value("kind", String::from("match_line"))?;
    if kind == "sheet_outline" {
        mark.kind = WorldMarkKind::SheetOutline;
    } else if kind != "match_line" {
        return Err(ReadFault::Bad(format!("unknown mark kind \"{kind}\"")));
    }
    let none = Json::array();
    for point in member_or(json, "points", &none)?.iter() {
        mark.points.push(point_from(point)?);
    }
    mark.label = json.value("label", String::new())?;
    mark.sheet = json.value("sheet", String::new())?;
    Ok(mark)
}

fn viewport_json(viewport: &Viewport) -> Json {
    let d = Viewport::default();
    let mut json = Json::object_of([
        ("id", Json::from(&viewport.id)),
        ("kind", Json::from(viewport.kind.name())),
    ]);
    if viewport.rect != d.rect {
        json["rect"] = box_json(&viewport.rect);
    }
    put_number(&mut json, "scale", viewport.scale, d.scale);
    put_flag(&mut json, "auto_scale", viewport.auto_scale, d.auto_scale);
    put_point(&mut json, "centre", &viewport.centre, &d.centre);
    put_flag(
        &mut json,
        "auto_centre",
        viewport.auto_centre,
        d.auto_centre,
    );
    put_number(&mut json, "rotation", viewport.rotation, d.rotation);
    put_number(
        &mut json,
        "vertical_exaggeration",
        viewport.vertical_exaggeration,
        d.vertical_exaggeration,
    );
    put_number(
        &mut json,
        "tilt_degrees",
        viewport.tilt_degrees,
        d.tilt_degrees,
    );
    let source = &viewport.source;
    let mut source_json = Json::object();
    put_text(
        &mut source_json,
        "alignment",
        &source.alignment,
        &d.source.alignment,
    );
    put_number(
        &mut source_json,
        "chainage_from",
        source.chainage_from,
        d.source.chainage_from,
    );
    put_number(
        &mut source_json,
        "chainage_to",
        source.chainage_to,
        d.source.chainage_to,
    );
    put_number(
        &mut source_json,
        "section_interval",
        source.section_interval,
        d.source.section_interval,
    );
    put_list(
        &mut source_json,
        "stations",
        Json::array_of(source.stations.iter().map(|&station| Json::from(station))),
    );
    put_number(
        &mut source_json,
        "section_half_width",
        source.section_half_width,
        d.source.section_half_width,
    );
    put_list(&mut json, "source", source_json);
    put_list(
        &mut json,
        "hidden_layers",
        Json::array_of(viewport.hidden_layers.hidden().iter().map(Json::from)),
    );
    put_text(&mut json, "title", &viewport.title, &d.title);
    put_flag(
        &mut json,
        "north_arrow",
        viewport.north_arrow,
        d.north_arrow,
    );
    put_flag(&mut json, "scale_bar", viewport.scale_bar, d.scale_bar);
    put_flag(&mut json, "locked", viewport.locked, d.locked);
    put_text(&mut json, "text", &viewport.text, &d.text);
    put_list(
        &mut json,
        "marks",
        Json::array_of(viewport.marks.iter().map(mark_json)),
    );
    if viewport.revision_limit != d.revision_limit {
        json["revision_limit"] = Json::from(viewport.revision_limit);
    }
    if viewport.legend_scope != d.legend_scope {
        json["legend_scope"] = Json::from(viewport.legend_scope.name());
    }
    if viewport.grid_style != d.grid_style {
        json["grid_style"] = Json::from(viewport.grid_style.name());
    }
    put_number(
        &mut json,
        "grid_interval",
        viewport.grid_interval,
        d.grid_interval,
    );
    json
}

fn viewport_from(json: &Json) -> Read<Viewport> {
    let d = Viewport::default();
    let mut viewport = Viewport {
        id: json.value("id", d.id.clone())?,
        ..Viewport::default()
    };
    let kind = json.value("kind", String::from(d.kind.name()))?;
    let Some(parsed_kind) = viewport_kind_from(&kind) else {
        return Err(ReadFault::Bad(format!("unknown viewport kind \"{kind}\"")));
    };
    viewport.kind = parsed_kind;
    if json.contains("rect") {
        viewport.rect = box_from(json.at("rect")?)?;
    }
    viewport.scale = json.value("scale", d.scale)?;
    viewport.auto_scale = json.value("auto_scale", d.auto_scale)?;
    if json.contains("centre") {
        viewport.centre = point_from(json.at("centre")?)?;
    }
    viewport.auto_centre = json.value("auto_centre", d.auto_centre)?;
    viewport.rotation = json.value("rotation", d.rotation)?;
    viewport.vertical_exaggeration =
        json.value("vertical_exaggeration", d.vertical_exaggeration)?;
    viewport.tilt_degrees = json.value("tilt_degrees", d.tilt_degrees)?;
    if json.contains("source") {
        let source = json.at("source")?;
        let s = &d.source;
        viewport.source.alignment = source.value("alignment", s.alignment.clone())?;
        viewport.source.chainage_from = source.value("chainage_from", s.chainage_from)?;
        viewport.source.chainage_to = source.value("chainage_to", s.chainage_to)?;
        viewport.source.section_interval = source.value("section_interval", s.section_interval)?;
        viewport.source.stations = source.value("stations", s.stations.clone())?;
        viewport.source.section_half_width =
            source.value("section_half_width", s.section_half_width)?;
    }
    let none = Json::array();
    for layer in member_or(json, "hidden_layers", &none)?.iter() {
        viewport.hidden_layers.hide(&layer.get::<String>()?);
    }
    viewport.title = json.value("title", d.title.clone())?;
    viewport.north_arrow = json.value("north_arrow", d.north_arrow)?;
    viewport.scale_bar = json.value("scale_bar", d.scale_bar)?;
    viewport.locked = json.value("locked", d.locked)?;
    viewport.text = json.value("text", d.text.clone())?;
    for mark in member_or(json, "marks", &none)?.iter() {
        viewport.marks.push(mark_from(mark)?);
    }
    if json.contains("revision_limit") {
        // A count: -1 or 2.5 would read as some other count, not as an error.
        let limit = json.at("revision_limit")?;
        if !limit.is_number_unsigned() {
            return Err(ReadFault::Bad(String::from(
                "the revision limit is not a whole number of at least 0",
            )));
        }
        viewport.revision_limit = limit.get::<usize>()?;
    }
    let scope = json.value("legend_scope", String::from(d.legend_scope.name()))?;
    let Some(parsed_scope) = legend_scope_from(&scope) else {
        return Err(ReadFault::Bad(format!("unknown legend scope \"{scope}\"")));
    };
    viewport.legend_scope = parsed_scope;
    if json.contains("grid_style") {
        let grid = json.at("grid_style")?.get::<String>()?;
        let Some(style) = grid_style_from(&grid) else {
            return Err(ReadFault::Bad(format!("unknown grid style \"{grid}\"")));
        };
        viewport.grid_style = style;
    }
    viewport.grid_interval = json.value("grid_interval", d.grid_interval)?;
    Ok(viewport)
}

fn sheet_json(sheet: &Sheet) -> Json {
    let d = Sheet::default();
    let mut json = Json::object_of([("id", Json::from(&sheet.id))]);
    put_text(&mut json, "name", &sheet.name, &d.name);
    if sheet.paper != d.paper {
        json["paper"] = Json::from(paper_text(sheet.paper));
    }
    put_flag(&mut json, "landscape", sheet.landscape, d.landscape);
    put_text(&mut json, "frame", &sheet.frame, &d.frame);
    put_flag(
        &mut json,
        "frame_legend",
        sheet.frame_legend,
        d.frame_legend,
    );
    let mut fields = Json::object();
    for (name, value) in &sheet.fields {
        fields[name.as_str()] = Json::from(value);
    }
    put_list(&mut json, "fields", fields);
    put_list(
        &mut json,
        "viewports",
        Json::array_of(sheet.viewports.iter().map(viewport_json)),
    );
    json
}

fn sheet_from(json: &Json) -> Read<Sheet> {
    let d = Sheet::default();
    let mut sheet = Sheet {
        id: json.value("id", d.id.clone())?,
        name: json.value("name", d.name.clone())?,
        ..Sheet::default()
    };
    sheet.paper = paper_from(&json.value("paper", String::from(paper_text(d.paper)))?)?;
    sheet.landscape = json.value("landscape", d.landscape)?;
    sheet.frame = json.value("frame", d.frame.clone())?;
    sheet.frame_legend = json.value("frame_legend", d.frame_legend)?;
    let no_fields = Json::object();
    for (name, value) in member_or(json, "fields", &no_fields)?.items() {
        sheet
            .fields
            .insert(name.into_owned(), value.get::<String>()?);
    }
    let none = Json::array();
    for viewport in member_or(json, "viewports", &none)?.iter() {
        sheet.viewports.push(viewport_from(viewport)?);
    }
    Ok(sheet)
}

fn defaults_json(d: &SheetDefaults) -> Json {
    let mut json = Json::object();
    put_text(&mut json, "organisation", &d.organisation, "");
    put_list(
        &mut json,
        "project_lines",
        Json::array_of(d.project_lines.iter().map(Json::from)),
    );
    put_text(&mut json, "client", &d.client, "");
    put_list(&mut json, "locator", sign_off_json(&d.locator));
    put_list(&mut json, "surveyor", sign_off_json(&d.surveyor));
    put_list(&mut json, "compiler", sign_off_json(&d.compiler));
    put_list(&mut json, "reviewer", sign_off_json(&d.reviewer));
    put_list(&mut json, "approver", sign_off_json(&d.approver));
    put_text(&mut json, "notes", &d.notes, "");
    put_text(&mut json, "height_datum", &d.height_datum, "");
    put_text(&mut json, "coordinate_system", &d.coordinate_system, "");
    put_text(&mut json, "model_name", &d.model_name, "");
    put_text(&mut json, "set_number", &d.set_number, "");
    put_text(&mut json, "logo_asset", &d.logo_asset, "");
    json
}

fn defaults_from(json: &Json) -> Read<SheetDefaults> {
    let sign_off = |key: &str| -> Read<SignOff> {
        if json.contains(key) {
            sign_off_from(json.at(key)?)
        } else {
            Ok(SignOff::default())
        }
    };
    Ok(SheetDefaults {
        organisation: json.value("organisation", String::new())?,
        project_lines: json.value("project_lines", Vec::<String>::new())?,
        client: json.value("client", String::new())?,
        locator: sign_off("locator")?,
        surveyor: sign_off("surveyor")?,
        compiler: sign_off("compiler")?,
        reviewer: sign_off("reviewer")?,
        approver: sign_off("approver")?,
        notes: json.value("notes", String::new())?,
        height_datum: json.value("height_datum", String::new())?,
        coordinate_system: json.value("coordinate_system", String::new())?,
        model_name: json.value("model_name", String::new())?,
        set_number: json.value("set_number", String::new())?,
        logo_asset: json.value("logo_asset", String::new())?,
    })
}

/// Fails (InvalidArgument) for a number that is not finite - it has no JSON
/// form that reads back. (An unplaced viewport's empty rectangle is the
/// default, and is simply left out.) The C++ also refuses text that is not
/// UTF-8, which a Rust String cannot hold.
pub fn sheet_set_to_json(set: &SheetSet) -> Result<String> {
    let revisions: Vec<Json> = set
        .revisions
        .iter()
        .map(|revision: &Revision| {
            Json::object_of([
                ("code", Json::from(&revision.code)),
                ("date", Json::from(&revision.date)),
                ("description", Json::from(&revision.description)),
                ("by", Json::from(&revision.by)),
            ])
        })
        .collect();
    let mut root = Json::object_of([
        ("format", Json::from(FORMAT)),
        ("version", Json::from(SHEET_SET_VERSION)),
    ]);
    put_list(&mut root, "defaults", defaults_json(&set.defaults));
    put_text(
        &mut root,
        "numbering",
        &set.numbering,
        &SheetSet::default().numbering,
    );
    put_list(&mut root, "revisions", Json::from(revisions));
    put_list(
        &mut root,
        "sheets",
        Json::array_of(set.sheets.iter().map(sheet_json)),
    );
    put_list(&mut root, "page_setup", page_setup_json(&set.page_setup));
    if !all_finite(&root) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the sheet set holds a number that is not finite (an infinity or NaN)",
        ));
    }
    Ok(root.dump())
}

fn read_sheet_set(text: &str) -> Read<SheetSet> {
    let root = Json::parse(text)?;
    if root.value("format", String::new())? != FORMAT {
        return Err(ReadFault::Refused(Error::new(
            ErrorCode::ParseFailure,
            "not a sheet set",
        )));
    }
    let version = root.at("version")?.get::<i32>()?;
    if version > SHEET_SET_VERSION {
        return Err(ReadFault::Refused(make_error(
            ErrorCode::Unsupported,
            "the sheets were written by a newer version of Katana",
            format!("version {version}"),
        )));
    }
    let mut set = SheetSet::default();
    if root.contains("defaults") {
        set.defaults = defaults_from(root.at("defaults")?)?;
    }
    set.numbering = root.value("numbering", set.numbering.clone())?;
    let none = Json::array();
    for revision in member_or(&root, "revisions", &none)?.iter() {
        let code = revision.value("code", String::new())?;
        let date = revision.value("date", String::new())?;
        let description = revision.value("description", String::new())?;
        let by = revision.value("by", String::new())?;
        set.revisions.push(Revision {
            code,
            date,
            description,
            by,
        });
    }
    for sheet in member_or(&root, "sheets", &none)?.iter() {
        set.sheets.push(sheet_from(sheet)?);
    }
    if root.contains("page_setup") {
        set.page_setup = page_setup_from(root.at("page_setup")?)?;
    }
    Ok(set)
}

/// ParseFailure for text that is not a sheet set; Unsupported for one written
/// by a newer version.
pub fn sheet_set_from_json(text: &str) -> Result<SheetSet> {
    match read_sheet_set(text) {
        Ok(set) => Ok(set),
        Err(ReadFault::Json(error)) => Err(make_error(
            ErrorCode::ParseFailure,
            "the sheet set is not in the expected format",
            error.what,
        )),
        Err(ReadFault::Bad(what)) => Err(make_error(
            ErrorCode::ParseFailure,
            "the sheet set is not in the expected format",
            what,
        )),
        Err(ReadFault::Refused(error)) => Err(error),
    }
}
