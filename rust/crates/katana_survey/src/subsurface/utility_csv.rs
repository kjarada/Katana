//! A delimited schedule of located utility vertices, and a design centre line,
//! read from text.
//!
//! One row per located vertex; rows with the same `line` form one service, in
//! the order they appear. Columns are found BY NAME from a header row, in any
//! order and any letter case, so that a schedule exported from a locating
//! contractor's spreadsheet reads without being rearranged first:
//!
//! ```text
//! required  line, point, easting, northing, method
//! optional  level, level_ref, surface, h_unc, v_unc, ql, path, verifies,
//!           type, owner, material, diameter_mm, status, config, description
//! ```
//!
//! The TfNSW Utility Schema's attribute names are accepted too - AssetIdentifier
//! for line, AssetTypeCode for type, LocateMethod, DepthLocation, Depth, Size
//! (millimetres, inside, "W x H") - and the rest of its attributes are kept
//! by name, uninterpreted (docs/subsurface_utilities.md has the table).
//!
//! Each name has aliases (utility_csv_columns() lists them), and a column whose
//! header matches none of them is an error rather than something skipped: a
//! misspelt "survace" column read past would leave every cover uncomputed with
//! nothing to say why.
//!
//! Lengths and levels are metres, except diameter_mm, whose unit is in its name
//! because a diameter in a utility schedule is millimetres by habit and metres
//! everywhere else in this program - the one column where guessing would be
//! wrong by a thousand.
//!
//! The attributes of a service (type, owner, material ...) may be repeated on
//! every row or given once; two rows of one line that give DIFFERENT values
//! are an error naming both rows. An empty cell is "not recorded", never zero.
//!
//! The text is UTF-8 (an optional BOM is skipped). Fields are separated by
//! commas; a field may be double-quoted, with "" for a quote inside it. Blank
//! lines and lines starting with '#' are skipped.

use std::collections::{BTreeMap, BTreeSet};

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, is_ascii_space, lowered, parse_finite_double, trimmed};
use katana_core::{Error, ErrorCode, Result, Status};

use super::clearance::{DesignAlignment, DesignVertex};
use super::delivery_schema::{DeliverySchema, SchemaField};
use super::quality_level::{
    LocationMethod, QualityLevel, parse_location_method, parse_quality_level, to_string,
};
use super::table::{RawRow, at, key, read_raw_table, trim};
use super::utility_network::{
    LevelReference, PathEvidence, UtilityLine, UtilityStatus, UtilityType, UtilityVertex,
    has_vertical_measurement, parse_level_reference, parse_path_evidence, parse_utility_status,
    parse_utility_type,
};
use crate::coordinate::Coordinate2;

/// Where a column that is kept but not interpreted is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CarriedOn {
    /// Read into the model's own fields.
    Interpreted,
    /// UtilityAttributes::fields, one value per service.
    Line,
    /// UtilityVertex::fields.
    Vertex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UtilityCsvColumn {
    /// The canonical name, as listed in the module's documentation.
    pub name: &'static str,
    /// Also accepted, compared without case, blanks, '-', '_'.
    pub aliases: &'static [&'static str],
    pub required: bool,
    pub carried: CarriedOn,
}

const fn interpreted(
    name: &'static str,
    aliases: &'static [&'static str],
    required: bool,
) -> UtilityCsvColumn {
    UtilityCsvColumn {
        name,
        aliases,
        required,
        carried: CarriedOn::Interpreted,
    }
}

const fn kept(name: &'static str, carried: CarriedOn) -> UtilityCsvColumn {
    UtilityCsvColumn {
        name,
        aliases: &[],
        required: false,
        carried,
    }
}

static COLUMNS: &[UtilityCsvColumn] = &[
    interpreted(
        "line",
        &[
            "line_id",
            "utility",
            "utility_id",
            "service",
            "asset",
            "AssetIdentifier",
        ],
        true,
    ),
    interpreted("point", &["point_id", "id", "vertex", "pt"], true),
    interpreted("easting", &["e", "mga_e"], true),
    interpreted("northing", &["n", "mga_n"], true),
    interpreted(
        "method",
        &[
            "location_method",
            "locating_method",
            "survey_method",
            "LocateMethod",
        ],
        true,
    ),
    interpreted("level", &["rl", "z", "utility_level", "ahd"], false),
    interpreted(
        "level_ref",
        &["level_reference", "rl_ref", "level_on", "DepthLocation"],
        false,
    ),
    interpreted("depth", &["depth_m"], false),
    interpreted(
        "surface",
        &["surface_level", "surface_rl", "ground", "nsl"],
        false,
    ),
    interpreted(
        "h_unc",
        &["horizontal_uncertainty", "h_accuracy", "hz_unc"],
        false,
    ),
    interpreted(
        "v_unc",
        &["vertical_uncertainty", "v_accuracy", "vt_unc"],
        false,
    ),
    interpreted("ql", &["quality_level", "QualityLevel"], false),
    interpreted("path", &["path_evidence", "path_to_next"], false),
    interpreted("verifies", &["checks", "verifies_point"], false),
    interpreted(
        "type",
        &["utility_type", "service_type", "AssetTypeCode"],
        false,
    ),
    interpreted("owner", &["AssetOwner", "authority", "operator"], false),
    interpreted("material", &[], false),
    interpreted("diameter_mm", &["dia_mm", "od_mm"], false),
    interpreted("size", &["size_mm"], false),
    interpreted("status", &["utility_status", "AssetStatus"], false),
    interpreted("config", &["configuration"], false),
    interpreted("description", &["desc", "comment", "remarks"], false),
    // The rest of the AS 5488 attribute set as the TfNSW Utility Schema
    // names it: read, kept by that name, never interpreted here. Per
    // service, so that two rows of one service may not disagree ...
    kept("AssetType", CarriedOn::Line),
    kept("AssetSubtypeCode", CarriedOn::Line),
    kept("AssetSubtype", CarriedOn::Line),
    kept("AssetSubtypeDescription", CarriedOn::Line),
    kept("AssetFeature", CarriedOn::Line),
    kept("AssetFeatureDescription", CarriedOn::Line),
    kept("Capacity", CarriedOn::Line),
    kept("SizeDescription", CarriedOn::Line),
    kept("ConfigurationDescription", CarriedOn::Line),
    kept("MaterialDescription", CarriedOn::Line),
    kept("Limitation", CarriedOn::Line),
    kept("Condition", CarriedOn::Line),
    kept("UtilityInstallDate", CarriedOn::Line),
    kept("SourceofInformation", CarriedOn::Line),
    kept("Location", CarriedOn::Line),
    kept("Clash", CarriedOn::Line),
    kept("ClashDescription", CarriedOn::Line),
    kept("ClashID", CarriedOn::Line),
    kept("ClashRisk", CarriedOn::Line),
    kept("Treatment", CarriedOn::Line),
    kept("TreatmentDescription", CarriedOn::Line),
    kept("TreatmentReference", CarriedOn::Line),
    kept("TreatmentLength", CarriedOn::Line),
    kept("TreatmentRisk", CarriedOn::Line),
    kept("tbCoordSys", CarriedOn::Line),
    kept("TfNSW_ContractOrgCode", CarriedOn::Line),
    kept("TfNSW_ContractOrgName", CarriedOn::Line),
    // ... and per vertex, where each point may say something of its own.
    kept("DepthDescription", CarriedOn::Vertex),
    kept("DateInfoObtained", CarriedOn::Vertex),
    kept("PotholeReport", CarriedOn::Vertex),
    kept("PitReport", CarriedOn::Vertex),
    kept("Notes", CarriedOn::Vertex),
];

/// Every column the reader knows, in the order of the module's documentation.
pub fn utility_csv_columns() -> &'static [UtilityCsvColumn] {
    COLUMNS
}

/// The column of `known` that `header` names, by its name or an alias; the
/// last that does, as the C++'s loop leaves it.
fn column_named<'a>(known: &'a [UtilityCsvColumn], header: &str) -> Option<&'a UtilityCsvColumn> {
    let name = key(header);
    let mut found = None;
    for candidate in known {
        if name == key(candidate.name) {
            found = Some(candidate);
        }
        for alias in candidate.aliases {
            if name == key(alias) {
                found = Some(candidate);
            }
        }
    }
    found
}

/// The column `header` names, by its name or an alias, compared as the reader
/// compares a header row's; None for a name it does not know. So a column
/// named anywhere else - UTILITY DRAW's FIELDS, which reads an import's own
/// attributes as columns - is the one the reader would take.
pub fn utility_csv_column_named(header: &str) -> Option<&'static UtilityCsvColumn> {
    column_named(utility_csv_columns(), header)
}

struct Table {
    /// Canonical name -> field index.
    columns: BTreeMap<String, usize>,
    /// Canonical name -> the header as written.
    headers: BTreeMap<String, String>,
    rows: Vec<RawRow>,
}

impl Table {
    /// The trimmed cell, or empty when the column is absent.
    fn cell<'row>(&self, row: &'row RawRow, name: &str) -> &'row str {
        match self.columns.get(name) {
            Some(&column) => row.fields[column].as_str(),
            None => "",
        }
    }

    /// Records `name`'s cell under its header as written, when it has one.
    fn keep_written(&self, row: &RawRow, name: &str, written: &mut BTreeMap<String, String>) {
        let value = self.cell(row, name);
        if let Some(header) = self.headers.get(name)
            && !value.is_empty()
        {
            written
                .entry(header.clone())
                .or_insert_with(|| String::from(value));
        }
    }
}

/// The rows of `text`, with its header's columns resolved against `known`.
fn read_table(text: &str, known: &[UtilityCsvColumn]) -> Result<Table> {
    let raw = read_raw_table(text)?;
    let mut table = Table {
        columns: BTreeMap::new(),
        headers: BTreeMap::new(),
        rows: Vec::new(),
    };
    for (column, header) in raw.header.iter().enumerate() {
        let Some(found) = column_named(known, header) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("unknown column \"{header}\""),
                at(raw.header_line),
            ));
        };
        if table.columns.contains_key(found.name) {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("column \"{}\" given twice", found.name),
                at(raw.header_line),
            ));
        }
        table.columns.insert(String::from(found.name), column);
        table
            .headers
            .entry(String::from(found.name))
            .or_insert_with(|| String::from(trim(header)));
    }
    for column in known {
        if column.required && !table.columns.contains_key(column.name) {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("required column \"{}\" is missing", column.name),
                at(raw.header_line),
            ));
        }
    }
    table.rows = raw.rows;
    Ok(table)
}

fn bad(row: &RawRow, column: &str, value: &str, what: &str) -> Error {
    make_error(
        ErrorCode::ParseFailure,
        format!("{column} \"{value}\" is not {what}"),
        at(row.line_number),
    )
}

/// Parses a present cell with `parse`; an empty cell leaves `out` as it was.
fn optional_cell<T>(
    table: &Table,
    row: &RawRow,
    column: &str,
    what: &str,
    parse: impl Fn(&str) -> Option<T>,
    out: &mut Option<T>,
) -> Status {
    let text = table.cell(row, column);
    if text.is_empty() {
        return Ok(());
    }
    let Some(value) = parse(text) else {
        return Err(bad(row, column, text, what));
    };
    *out = Some(value);
    Ok(())
}

fn number(text: &str) -> Option<f64> {
    parse_finite_double(text)
}

/// A quality level a schedule claims; "Unknown" parses (as no claim, which the
/// caller makes of it) so that it is not refused as a typo.
fn parse_claim(text: &str) -> Option<QualityLevel> {
    if key(text) == "unknown" {
        return Some(QualityLevel::D); // replaced by "no claim" by the caller
    }
    parse_quality_level(text)
}

/// A size in millimetres, as metres: "150", or "1200 x 900" (a culvert's or a
/// duct bank's width by height), whose larger side is taken - the choice that
/// places a top from an invert higher and a service wider, both of which err
/// towards less cover and less clearance. 0 for "Not Applicable" and
/// "Unknown", which record that there is none to give.
fn parse_size(text: &str) -> Option<f64> {
    let name = key(text);
    if name == "notapplicable" || name == "unknown" {
        return Some(0.0);
    }
    let mut largest = 0.0_f64;
    let mut start = 0;
    let lower = lowered(text);
    loop {
        let cross = lower[start..]
            .iter()
            .position(|byte| *byte == b'x')
            .map(|offset| start + offset);
        let end = cross.unwrap_or(lower.len());
        let part = parse_finite_double(trimmed(&lower[start..end]))?;
        if part <= 0.0 {
            return None;
        }
        // std::max(largest, part).
        if largest < part {
            largest = part;
        }
        match cross {
            None => return Some(largest / 1000.0),
            Some(cross) => start = cross + 1,
        }
    }
}

/// Millimetre text that the reader's division by 1000 brings back to exactly
/// `metres`. The product is tried first, then its neighbours a few units in
/// the last place either side: a diameter read from a schedule came from such
/// a text, so one of them divides back; four is ample, since the product and
/// the quotient are each within half a unit of exact.
fn millimetres(metres: f64) -> String {
    let product = metres * 1000.0;
    let mut below = product;
    let mut above = product;
    for _ in 0..4 {
        for candidate in [below, above] {
            let text = format_exact_real(candidate);
            if let Some(back) = parse_finite_double(&text)
                && back / 1000.0 == metres
            {
                return text;
            }
        }
        // std::nextafter towards -infinity and +infinity.
        below = below.next_down();
        above = above.next_up();
    }
    format_exact_real(product)
}

/// One line attribute from one row: set it if unset, refuse if it disagrees.
struct AttributeSource {
    value: String,
    line_number: usize,
}

/// Lines in order of first appearance. ParseFailure, naming the line of text,
/// for: no header, an unknown or repeated column, a missing required column,
/// a row with the wrong number of fields, an unparseable number / method /
/// quality level / type / status / level reference / path, a non-positive
/// diameter, a line id repeated for the same point id, or conflicting line
/// attributes.
pub fn parse_utility_csv(text: &str) -> Result<Vec<UtilityLine>> {
    let table = read_table(text, utility_csv_columns())?;

    let mut lines: Vec<UtilityLine> = Vec::new();
    let mut line_index: BTreeMap<String, usize> = BTreeMap::new();
    // Per line: attribute column -> the first row that gave it.
    let mut attribute_sources: BTreeMap<String, BTreeMap<String, AttributeSource>> =
        BTreeMap::new();
    // Per line: the text row each vertex came from, for path evidence.
    let mut path_cells: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for row in &table.rows {
        let line_id = String::from(table.cell(row, "line"));
        let point_id = String::from(table.cell(row, "point"));
        if line_id.is_empty() || point_id.is_empty() {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "line and point must not be empty",
                at(row.line_number),
            ));
        }
        let (index, inserted) = match line_index.get(&line_id) {
            Some(&index) => (index, false),
            None => {
                line_index.insert(line_id.clone(), lines.len());
                lines.push(UtilityLine {
                    id: line_id.clone(),
                    ..Default::default()
                });
                (lines.len() - 1, true)
            }
        };
        let line = &mut lines[index];
        if line.vertices.iter().any(|existing| existing.id == point_id) {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("point {point_id} appears twice on line {line_id}"),
                at(row.line_number),
            ));
        }

        let mut vertex = UtilityVertex {
            id: point_id,
            ..Default::default()
        };
        for column in [
            "point",
            "easting",
            "northing",
            "method",
            "level",
            "level_ref",
            "depth",
            "surface",
            "h_unc",
            "v_unc",
            "ql",
            "path",
            "verifies",
        ] {
            table.keep_written(row, column, &mut vertex.written);
        }
        let mut northing = None;
        let mut easting = None;
        let mut method = None;
        optional_cell(&table, row, "northing", "a number", number, &mut northing)?;
        optional_cell(&table, row, "easting", "a number", number, &mut easting)?;
        optional_cell(
            &table,
            row,
            "method",
            "a location method",
            parse_location_method,
            &mut method,
        )?;
        optional_cell(&table, row, "level", "a number", number, &mut vertex.level)?;
        optional_cell(
            &table,
            row,
            "depth",
            "a number of metres",
            number,
            &mut vertex.depth,
        )?;
        optional_cell(
            &table,
            row,
            "surface",
            "a number",
            number,
            &mut vertex.surface_level,
        )?;
        optional_cell(
            &table,
            row,
            "h_unc",
            "a number",
            number,
            &mut vertex.evidence.horizontal_uncertainty,
        )?;
        optional_cell(
            &table,
            row,
            "v_unc",
            "a number",
            number,
            &mut vertex.evidence.vertical_uncertainty,
        )?;
        optional_cell(
            &table,
            row,
            "ql",
            "a quality level",
            parse_claim,
            &mut vertex.claimed,
        )?;
        let (Some(northing), Some(easting), Some(method)) = (northing, easting, method) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "easting, northing and method must be given for every point",
                at(row.line_number),
            ));
        };
        vertex.position = Coordinate2 { northing, easting };
        vertex.evidence.method = method;
        vertex.evidence.has_level = has_vertical_measurement(&vertex);
        if vertex.depth.is_some_and(|depth| depth < 0.0) {
            return Err(bad(
                row,
                "depth",
                table.cell(row, "depth"),
                "a depth below the surface",
            ));
        }
        let claim = table.cell(row, "ql");
        if key(claim) == "unknown" {
            vertex.claimed = None; // a schedule's "Unknown" claims nothing
            vertex
                .recorded
                .entry(String::from("ql"))
                .or_insert_with(|| String::from(claim));
        }
        let mut reference = None;
        optional_cell(
            &table,
            row,
            "level_ref",
            "a level reference",
            parse_level_reference,
            &mut reference,
        )?;
        vertex.level_reference = reference.unwrap_or(LevelReference::Top);
        // On the top with nothing measured is what an empty cell reads as.
        if reference == Some(LevelReference::Top)
            && vertex.level.is_none()
            && vertex.depth.is_none()
        {
            vertex
                .recorded
                .entry(String::from("level_ref"))
                .or_insert_with(|| String::from(table.cell(row, "level_ref")));
        }
        vertex.verifies = String::from(table.cell(row, "verifies"));

        let path = table.cell(row, "path");
        if !path.is_empty() && parse_path_evidence(path).is_none() {
            return Err(bad(row, "path", path, "detected, exposed or assumed"));
        }
        path_cells
            .entry(line_id.clone())
            .or_default()
            .push(String::from(path));

        // Line attributes: the first row to give one sets it; a later row
        // giving a different one is a contradiction in the schedule.
        let sources = attribute_sources.entry(line_id.clone()).or_default();
        if inserted {
            table.keep_written(row, "line", &mut line.attributes.written);
        }
        let mut line_columns: Vec<&str> = vec![
            "type",
            "owner",
            "material",
            "diameter_mm",
            "size",
            "status",
            "config",
            "description",
        ];
        let interpreted_line_columns = line_columns.len();
        for carried in utility_csv_columns() {
            match carried.carried {
                CarriedOn::Line => line_columns.push(carried.name),
                CarriedOn::Vertex => {
                    let value = table.cell(row, carried.name);
                    if !value.is_empty() {
                        vertex
                            .fields
                            .entry(String::from(carried.name))
                            .or_insert_with(|| String::from(value));
                    }
                }
                CarriedOn::Interpreted => {}
            }
        }
        for (index, column) in line_columns.iter().copied().enumerate() {
            let value = table.cell(row, column);
            if value.is_empty() {
                continue;
            }
            if let Some(source) = sources.get(column) {
                if source.value != value {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        format!(
                            "line {line_id} has {column} \"{}\" at {} and \"{value}\" here",
                            source.value,
                            at(source.line_number)
                        ),
                        at(row.line_number),
                    ));
                }
                continue;
            }
            sources.insert(
                String::from(column),
                AttributeSource {
                    value: String::from(value),
                    line_number: row.line_number,
                },
            );
            let attributes = &mut line.attributes;
            if index < interpreted_line_columns {
                table.keep_written(row, column, &mut attributes.written);
            }
            match column {
                "type" => {
                    let Some(r#type) = parse_utility_type(value) else {
                        return Err(bad(row, column, value, "a utility type"));
                    };
                    attributes.r#type = r#type;
                    if r#type == UtilityType::Unknown {
                        attributes
                            .recorded
                            .entry(String::from("type"))
                            .or_insert_with(|| String::from(value));
                    }
                }
                "status" => {
                    let Some(status) = parse_utility_status(value) else {
                        return Err(bad(row, column, value, "a utility status"));
                    };
                    attributes.status = status;
                    if status == UtilityStatus::Unknown {
                        attributes
                            .recorded
                            .entry(String::from("status"))
                            .or_insert_with(|| String::from(value));
                    }
                }
                "diameter_mm" => match parse_finite_double(value) {
                    Some(given) if given > 0.0 => {
                        attributes.diameter = given / 1000.0;
                        attributes.diameter_is_inside = false;
                    }
                    _ => {
                        return Err(bad(row, column, value, "a positive number of millimetres"));
                    }
                },
                "size" => {
                    let Some(size) = parse_size(value) else {
                        return Err(bad(
                            row,
                            column,
                            value,
                            "millimetres, W x H millimetres, Not Applicable or Unknown",
                        ));
                    };
                    // An outside diameter, where both are given, is the better
                    // one to find a top from.
                    if size > 0.0 && !sources.contains_key("diameter_mm") {
                        attributes.diameter = size;
                        attributes.diameter_is_inside = true;
                    }
                }
                "owner" => attributes.owner = String::from(value),
                "material" => attributes.material = String::from(value),
                "config" => attributes.configuration = String::from(value),
                "description" => attributes.description = String::from(value),
                _ => {
                    attributes
                        .fields
                        .entry(String::from(column))
                        .or_insert_with(|| String::from(value));
                }
            }
        }
        line.vertices.push(vertex);
    }

    for line in &mut lines {
        // A size the writer would not say again from the diameter: one it
        // writes nothing for (Not Applicable, Unknown, or a size beside a
        // diameter_mm, which is the diameter), or one it words otherwise
        // ("1200 x 900", "150.0").
        if let Some(sources) = attribute_sources.get(&line.id)
            && let Some(size) = sources.get("size")
        {
            let given = &size.value;
            let read = parse_size(given).unwrap_or(0.0);
            if sources.contains_key("diameter_mm") || !(read > 0.0) || millimetres(read) != *given {
                line.attributes
                    .recorded
                    .entry(String::from("size"))
                    .or_insert_with(|| given.clone());
            }
        }
        let Some(cells) = path_cells.get(&line.id) else {
            continue;
        };
        let segments = &cells[..cells.len().saturating_sub(1)];
        if !segments.iter().any(|cell| !cell.is_empty()) {
            continue;
        }
        for cell in segments {
            // Every non-empty cell was read as a path above; an empty one is
            // Detected.
            line.path_evidence
                .push(parse_path_evidence(cell).unwrap_or(PathEvidence::Detected));
        }
    }
    Ok(lines)
}

/// A design centre line: header row with easting, northing and optionally
/// level (aliases as above), one row per vertex in order. `id` names the
/// alignment in results and errors.
pub fn parse_design_csv(text: &str, id: String) -> Result<DesignAlignment> {
    let mut columns: Vec<UtilityCsvColumn> = Vec::new();
    for column in utility_csv_columns() {
        if column.name == "easting" || column.name == "northing" {
            columns.push(*column);
        } else if column.name == "level" || column.name == "point" {
            columns.push(UtilityCsvColumn {
                required: false,
                ..*column
            });
        }
    }
    let table = read_table(text, &columns)?;
    let mut design = DesignAlignment {
        id,
        ..Default::default()
    };
    for row in &table.rows {
        let mut northing = None;
        let mut easting = None;
        let mut level = None;
        optional_cell(&table, row, "northing", "a number", number, &mut northing)?;
        optional_cell(&table, row, "easting", "a number", number, &mut easting)?;
        optional_cell(&table, row, "level", "a number", number, &mut level)?;
        let (Some(northing), Some(easting)) = (northing, easting) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "easting and northing must be given",
                at(row.line_number),
            ));
        };
        design.vertices.push(DesignVertex {
            position: Coordinate2 { northing, easting },
            level,
        });
    }
    Ok(design)
}

// ---- writing a schedule --------------------------------------------------------------------
//
// The other direction, for a schedule made from the drawing (UTILITY SCHEDULE,
// docs/subsurface_utilities.md): a drawing edited in CAD becomes a
// deliverable again, in the format the reader above takes.

/// The words a schedule is written in. Empty, the reader's own: each column
/// under its canonical name, each value as to_string spells it. A client's
/// delivery schema names some columns and spells some values its own way
/// (utility_csv_dialect); those are what the maps hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UtilityCsvDialect {
    /// Canonical column name -> the header written for it. Each header must be
    /// one the reader takes for that column: the name or one of its aliases.
    pub headers: BTreeMap<String, String>,
    /// Canonical column name -> (the value as the reader's own spelling writes
    /// it -> the value written). Only for the columns whose values are words:
    /// type, status, method, level_ref, ql and path. Each spelling must read
    /// back as the same value.
    pub spellings: BTreeMap<String, BTreeMap<String, String>>,
}

/// The reader's own spelling of each word value: what to_string says, except
/// an unknown method, whose "unknown method" the reader would not take back.
fn own_spelling(method: LocationMethod) -> &'static str {
    if method == LocationMethod::Unknown {
        "unknown"
    } else {
        to_string(method)
    }
}

/// A cell as the reader splits it back: quoted when it holds a comma or a
/// quote, starts or ends with a blank (the reader trims an unquoted cell), or
/// starts with '#' (a row whose first cell does is a comment).
fn csv_cell(value: &str) -> String {
    let bytes = value.as_bytes();
    let quote = value.contains([',', '"'])
        || (!bytes.is_empty()
            && (is_ascii_space(bytes[0])
                || is_ascii_space(bytes[bytes.len() - 1])
                || bytes[0] == b'#'));
    if !quote {
        return String::from(value);
    }
    let mut out = String::from("\"");
    for character in value.chars() {
        out.push(character);
        if character == '"' {
            out.push('"');
        }
    }
    out.push('"');
    out
}

/// A cell kept as the schedule wrote it (UtilityAttributes::recorded), when
/// it still reads as the value held - `reads` says whether it does - and so
/// can be written back without saying something else.
fn recorded_as(
    recorded: &BTreeMap<String, String>,
    column: &str,
    reads: impl Fn(&str) -> bool,
) -> Option<String> {
    let found = recorded.get(column)?;
    if found.is_empty() || !reads(found) {
        return None;
    }
    Some(found.clone())
}

/// The column whose canonical name is `name`.
fn column_by_name(name: &str) -> Option<&'static UtilityCsvColumn> {
    utility_csv_columns()
        .iter()
        .find(|column| column.name == name)
}

fn unwritable(line: &UtilityLine, vertex: Option<&UtilityVertex>, what: &str) -> Error {
    let point = vertex.map_or_else(String::new, |vertex| format!(" point {}", vertex.id));
    Error::new(
        ErrorCode::InvalidArgument,
        format!("line {}{point}: {what}; a schedule cannot hold it", line.id),
    )
}

/// Sets `column` in `row`, unless `value` is empty.
fn put(row: &mut BTreeMap<String, String>, column: &str, value: String) {
    if !value.is_empty() {
        row.insert(String::from(column), value);
    }
}

/// `lines` as a schedule that parse_utility_csv reads back as `lines`, field for
/// field and doubles to the bit (UtilityLine's ==): one row per vertex in
/// order, the service's attributes on every row of it, numbers in the shortest
/// text that reads back exactly (format_exact_real), a diameter in the
/// millimetres its column is in. A column is written only when some row has a
/// value for it, except the five required ones. A quality level claimed is
/// written as claimed; a vertex claiming none leaves the cell empty, which the
/// reader reads as none.
///
/// A diameter in metres that no millimetre figure divides back to exactly - one
/// typed in metres rather than read from a schedule - is written to the nearest
/// millimetre text there is, and reads back a unit in the last place away.
///
/// InvalidArgument, naming the line and vertex, for what a schedule cannot
/// hold: a value with a line break in it; a kept attribute whose name is no
/// column the reader knows, or is carried by the other of a service and a
/// vertex (utility_csv_columns); a path_evidence list of the wrong size.
pub fn write_utility_csv(lines: &[UtilityLine], dialect: &UtilityCsvDialect) -> Result<String> {
    let spelled = |column: &str, own: &str| -> String {
        if let Some(word) = dialect
            .spellings
            .get(column)
            .and_then(|words| words.get(own))
        {
            return word.clone();
        }
        String::from(own)
    };
    let real = format_exact_real;

    // Each row as column -> text; a column absent from every row is not
    // written unless the reader requires it.
    let mut rows: Vec<BTreeMap<String, String>> = Vec::new();
    let mut used: BTreeSet<String> = BTreeSet::new();
    for line in lines {
        if line.id.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a line with no id; a schedule cannot hold it",
            ));
        }
        let count = line.vertices.len();
        if !line.path_evidence.is_empty() && line.path_evidence.len() + 1 != count {
            return Err(unwritable(
                line,
                None,
                &format!(
                    "{} path evidences for {count} vertices",
                    line.path_evidence.len()
                ),
            ));
        }
        let attributes = &line.attributes;
        if !attributes.diameter.is_finite() || attributes.diameter < 0.0 {
            return Err(unwritable(
                line,
                None,
                "a diameter that is not a positive number",
            ));
        }
        let mut point_ids: BTreeSet<&str> = BTreeSet::new();
        for (i, vertex) in line.vertices.iter().enumerate() {
            let mut row: BTreeMap<String, String> = BTreeMap::new();
            if vertex.id.is_empty() || !point_ids.insert(vertex.id.as_str()) {
                return Err(unwritable(
                    line,
                    Some(vertex),
                    if vertex.id.is_empty() {
                        "a point with no id"
                    } else {
                        "a point id twice"
                    },
                ));
            }
            let numbers = [
                vertex.position.easting,
                vertex.position.northing,
                vertex.level.unwrap_or(0.0),
                vertex.depth.unwrap_or(0.0),
                vertex.surface_level.unwrap_or(0.0),
                vertex.evidence.horizontal_uncertainty.unwrap_or(0.0),
                vertex.evidence.vertical_uncertainty.unwrap_or(0.0),
            ];
            if numbers.iter().any(|value| !value.is_finite()) {
                return Err(unwritable(
                    line,
                    Some(vertex),
                    "a number that is not finite",
                ));
            }
            if vertex.depth.is_some_and(|depth| depth < 0.0) {
                return Err(unwritable(line, Some(vertex), "a depth above the surface"));
            }
            put(&mut row, "line", line.id.clone());
            put(&mut row, "point", vertex.id.clone());
            put(&mut row, "easting", real(vertex.position.easting));
            put(&mut row, "northing", real(vertex.position.northing));
            put(
                &mut row,
                "method",
                spelled("method", own_spelling(vertex.evidence.method)),
            );
            if let Some(level) = vertex.level {
                put(&mut row, "level", real(level));
            }
            // Written wherever it bears on something, and wherever it is not
            // what an empty cell reads as; where it is, as the schedule wrote
            // it, if it did.
            if vertex.level.is_some()
                || vertex.depth.is_some()
                || vertex.level_reference != LevelReference::Top
            {
                put(
                    &mut row,
                    "level_ref",
                    spelled("level_ref", to_string(vertex.level_reference)),
                );
            } else if let Some(kept) = recorded_as(&vertex.recorded, "level_ref", |text| {
                parse_level_reference(text) == Some(LevelReference::Top)
            }) {
                put(&mut row, "level_ref", kept);
            }
            if let Some(depth) = vertex.depth {
                put(&mut row, "depth", real(depth));
            }
            if let Some(surface) = vertex.surface_level {
                put(&mut row, "surface", real(surface));
            }
            if let Some(horizontal) = vertex.evidence.horizontal_uncertainty {
                put(&mut row, "h_unc", real(horizontal));
            }
            if let Some(vertical) = vertex.evidence.vertical_uncertainty {
                put(&mut row, "v_unc", real(vertical));
            }
            if let Some(claimed) = vertex.claimed {
                put(&mut row, "ql", spelled("ql", to_string(claimed)));
            } else if let Some(kept) =
                recorded_as(&vertex.recorded, "ql", |text| key(text) == "unknown")
            {
                put(&mut row, "ql", kept);
            }
            if !line.path_evidence.is_empty() && i + 1 < count {
                put(
                    &mut row,
                    "path",
                    spelled("path", to_string(line.path_evidence[i])),
                );
            }
            put(&mut row, "verifies", vertex.verifies.clone());
            if attributes.r#type != UtilityType::Unknown {
                put(
                    &mut row,
                    "type",
                    spelled("type", to_string(attributes.r#type)),
                );
            } else if let Some(kept) = recorded_as(&attributes.recorded, "type", |text| {
                parse_utility_type(text) == Some(UtilityType::Unknown)
            }) {
                put(&mut row, "type", kept);
            }
            put(&mut row, "owner", attributes.owner.clone());
            put(&mut row, "material", attributes.material.clone());
            // The size as the schedule wrote it while it still reads as the
            // diameter held, or beside a diameter_mm, which the reader takes
            // over it; else the diameter's own millimetres.
            let kept_size = recorded_as(&attributes.recorded, "size", |text| {
                parse_size(text).is_some()
            });
            let kept_reads = kept_size.as_deref().and_then(parse_size);
            if attributes.diameter > 0.0 && attributes.diameter_is_inside {
                let size = match kept_size {
                    Some(kept) if kept_reads == Some(attributes.diameter) => kept,
                    _ => millimetres(attributes.diameter),
                };
                put(&mut row, "size", size);
            } else if attributes.diameter > 0.0 {
                put(&mut row, "diameter_mm", millimetres(attributes.diameter));
                put(&mut row, "size", kept_size.unwrap_or_default());
            } else if kept_reads == Some(0.0)
                && let Some(kept) = kept_size
            {
                put(&mut row, "size", kept);
            }
            if attributes.status != UtilityStatus::Unknown {
                put(
                    &mut row,
                    "status",
                    spelled("status", to_string(attributes.status)),
                );
            } else if let Some(kept) = recorded_as(&attributes.recorded, "status", |text| {
                parse_utility_status(text) == Some(UtilityStatus::Unknown)
            }) {
                put(&mut row, "status", kept);
            }
            put(&mut row, "config", attributes.configuration.clone());
            put(&mut row, "description", attributes.description.clone());
            for (fields, carried) in [
                (&attributes.fields, CarriedOn::Line),
                (&vertex.fields, CarriedOn::Vertex),
            ] {
                for (name, value) in fields {
                    let column = column_by_name(name);
                    if column.is_none_or(|column| column.carried != carried) {
                        let why = if column.is_none() {
                            " is no column the schedule has"
                        } else {
                            " is carried by the other of a service and a point"
                        };
                        return Err(unwritable(
                            line,
                            Some(vertex),
                            &format!("kept attribute {name}{why}"),
                        ));
                    }
                    put(&mut row, name, value.clone());
                }
            }
            for (column, value) in &row {
                if value.contains(['\r', '\n']) {
                    return Err(unwritable(
                        line,
                        Some(vertex),
                        &format!("{column} holds a line break"),
                    ));
                }
                used.insert(column.clone());
            }
            rows.push(row);
        }
    }

    let columns: Vec<&str> = utility_csv_columns()
        .iter()
        .filter(|column| column.required || used.contains(column.name))
        .map(|column| column.name)
        .collect();
    let mut text = String::new();
    for (c, column) in columns.iter().enumerate() {
        if c > 0 {
            text.push(',');
        }
        let header = dialect.headers.get(*column).map_or(*column, String::as_str);
        text += &csv_cell(header);
    }
    text.push('\n');
    for row in &rows {
        for (c, column) in columns.iter().enumerate() {
            if c > 0 {
                text.push(',');
            }
            if let Some(value) = row.get(*column) {
                text += &csv_cell(value);
            }
        }
        text.push('\n');
    }
    Ok(text)
}

/// The first listed spelling of a value in the reader's reading: the reader's
/// own spelling of what `listed` reads as in the word column `column`.
fn spelling_of(column: &str, listed: &str) -> Option<&'static str> {
    Some(match column {
        "type" => to_string(parse_utility_type(listed)?),
        "status" => to_string(parse_utility_status(listed)?),
        "method" => own_spelling(parse_location_method(listed)?),
        "level_ref" => to_string(parse_level_reference(listed)?),
        "ql" => to_string(parse_quality_level(listed)?),
        "path" => to_string(parse_path_evidence(listed)?),
        _ => return None,
    })
}

/// The dialect of a client's delivery schema: each column the reader knows
/// under the schema attribute that is one of its names (an attribute or label
/// equal to the column's name or an alias, ignoring case, blanks, '-' and
/// '_'), and each word value in the FIRST spelling the schema's domain lists
/// that reads as it. So a schedule written from the drawing is written as the
/// schema asks, and a value the schema has no spelling for is written in the
/// reader's own - for a check to find.
pub fn utility_csv_dialect(schema: &DeliverySchema) -> UtilityCsvDialect {
    let mut dialect = UtilityCsvDialect::default();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for column in utility_csv_columns() {
        // A kept column is written by its own name, which is the schema's.
        if column.carried != CarriedOn::Interpreted {
            continue;
        }
        let mut names: BTreeSet<String> = BTreeSet::from([key(column.name)]);
        for alias in column.aliases {
            names.insert(key(alias));
        }
        let mut field: Option<&SchemaField> = None;
        let mut header = String::new();
        for candidate in &schema.fields {
            // The attribute when it is one of the reader's names for the
            // column, else the label: the reader must take the header back,
            // and the check finds a column by either.
            if names.contains(&key(&candidate.attribute)) {
                header = candidate.attribute.clone();
            } else if !candidate.label.is_empty() && names.contains(&key(&candidate.label)) {
                header = candidate.label.clone();
            } else {
                continue;
            }
            if taken.insert(header.clone()) {
                field = Some(candidate);
                break;
            }
        }
        let Some(field) = field else {
            continue;
        };
        dialect
            .headers
            .entry(String::from(column.name))
            .or_insert(header);

        let Some(domain) = schema.domains.get(&field.attribute) else {
            continue;
        };
        for listed in &domain.values {
            if let Some(own) = spelling_of(column.name, &listed.value) {
                dialect
                    .spellings
                    .entry(String::from(column.name))
                    .or_default()
                    .entry(String::from(own))
                    .or_insert_with(|| listed.value.clone());
            }
        }
    }
    dialect
}
