//! Checking a utility schedule against a client's delivery schema: which
//! attributes each asset must carry and which values each may take. The TfNSW
//! Utility Schema and Specification (DMS-FT-493) is the one this was written
//! against; nothing here is specific to it.
//!
//! WHY THE SCHEMA IS DATA AND NOT CODE. A delivery schema is a client's
//! document, revised on the client's timetable and under the client's licence -
//! TfNSW's may be used only by those delivering to a NSW Government agency, and
//! is not ours to copy into this repository. So the schema is read at run time
//! from a file its user makes from their own copy
//! (tools/utility_schema_domains.py), and a new revision needs a new file, not
//! a new build. The format is plain CSV so that a schema from any other client
//! can be written by hand:
//!
//! ```text
//! kind,attribute,value,detail,label
//! schema,<title>,<version>,,
//! field,<attribute>,<field type>,Yes|No|Conditional,<field name>
//! domain,<attribute>,<value>,<parent value or empty>,
//! open,<attribute>,,,          the domain lists examples: sizes, arrangements
//! parent,<attribute>,,,        the attribute a domain's parent values belong to
//! identifier,<attribute>,,,    the attribute that names an asset
//! prefix,<attribute>,<other>,, <attribute>'s value should start with <other>'s
//! ```
//!
//! A field type starting "Domain List" is checked against its domain; "Date"
//! against YYYY/MM/DD; "Real Number" or "Integer" as a number; anything else
//! is free text.

use std::collections::{BTreeMap, BTreeSet};

use katana_core::error::make_error;
use katana_core::text::{lowered, parse_finite_double, parse_integer, trimmed};
use katana_core::{Error, ErrorCode, Result};

use super::table::{RawRow, at, key, read_raw_table};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Requirement {
    Mandatory,
    /// Required when something else holds; not checked, only counted.
    Conditional,
    #[default]
    Optional,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaField {
    /// The name a deliverable's column carries: "AssetOwner".
    pub attribute: String,
    /// The name a person reads: "Asset Owner".
    pub label: String,
    /// "Domain List: Asset Owner", "Date (YYYY/MM/DD)" ...
    pub r#type: String,
    pub requirement: Requirement,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaValue {
    pub value: String,
    /// The parent attribute's value this one belongs under; empty for any.
    pub parent: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaDomain {
    pub values: Vec<SchemaValue>,
    /// The listed values are examples of an open set: a value not listed is
    /// still accepted when it is a number or a "number x number" arrangement.
    pub open: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeliverySchema {
    pub title: String,
    pub version: String,
    pub fields: Vec<SchemaField>,
    /// By attribute.
    pub domains: BTreeMap<String, SchemaDomain>,
    /// E.g. "AssetTypeCode".
    pub parent_attribute: String,
    /// E.g. "AssetIdentifier".
    pub identifier_attribute: String,
    /// Attribute -> the attribute it starts with.
    pub prefixes: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingSeverity {
    /// The deliverable does not meet the schema.
    #[default]
    Error,
    /// It may not: a spelling that differs only in case, a prefix.
    Warning,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaFinding {
    pub severity: FindingSeverity,
    pub attribute: String,
    /// What is wrong, the same words for every row it is wrong on, so that
    /// findings group: "is empty", "\"Inservice\" is not in the domain" ...
    pub problem: String,
    /// 0 for a finding about the file as a whole.
    pub line_number: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaCheck {
    pub rows: usize,
    /// Distinct identifiers, when the schema names one.
    pub assets: usize,
    /// Columns the schema does not define (coordinates ...).
    pub not_in_schema: Vec<String>,
    /// Conditional fields present, whose condition is not evaluated.
    pub conditional_not_checked: usize,
    /// In file order, file-wide ones first.
    pub findings: Vec<SchemaFinding>,
}

impl SchemaCheck {
    pub fn count(&self, severity: FindingSeverity) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    }
}

fn is_number(text: &[u8]) -> bool {
    parse_finite_double(trimmed(text)).is_some()
}

/// "2 x 1", "1200 x 900": numbers separated by 'x'.
fn is_arrangement(text: &str) -> bool {
    let lower = lowered(text);
    let mut start = 0;
    let mut parts = 0;
    loop {
        let cross = lower[start..]
            .iter()
            .position(|byte| *byte == b'x')
            .map(|offset| start + offset);
        let part = match cross {
            Some(cross) => &lower[start..cross],
            None => &lower[start..],
        };
        if !is_number(part) {
            return false;
        }
        parts += 1;
        match cross {
            None => return parts >= 2,
            Some(cross) => start = cross + 1,
        }
    }
}

/// YYYY/MM/DD, a real calendar date.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'/' || bytes[7] != b'/' {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (
        parse_integer(&bytes[0..4]),
        parse_integer(&bytes[5..7]),
        parse_integer(&bytes[8..10]),
    ) else {
        return false;
    };
    if !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    const DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    day <= DAYS[(month - 1) as usize] + i64::from(month == 2 && leap)
}

fn quoted(text: &str) -> String {
    format!("\"{text}\"")
}

/// ParseFailure, naming the line, for an unknown kind, a field or domain with
/// no attribute, a domain for an attribute that is not a field, or a schema
/// with no fields.
pub fn parse_delivery_schema(text: &str) -> Result<DeliverySchema> {
    let table = read_raw_table(text)?;
    let mut columns: BTreeMap<String, usize> = BTreeMap::new();
    for (i, header) in table.header.iter().enumerate() {
        columns.entry(key(header)).or_insert(i);
    }
    for required in ["kind", "attribute", "value", "detail"] {
        if !columns.contains_key(required) {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("a schema file needs a \"{required}\" column"),
                at(table.header_line),
            ));
        }
    }
    let cell = |row: &RawRow, name: &str| -> String {
        columns
            .get(name)
            .map_or_else(String::new, |&column| row.fields[column].clone())
    };

    let mut schema = DeliverySchema::default();
    // To check once the fields are known.
    let mut domain_lines: Vec<(String, usize)> = Vec::new();
    for row in &table.rows {
        let kind = key(&cell(row, "kind"));
        let attribute = cell(row, "attribute");
        if attribute.is_empty() {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "no attribute",
                at(row.line_number),
            ));
        }
        match kind.as_str() {
            "schema" => {
                schema.title = attribute;
                schema.version = cell(row, "value");
            }
            "field" => {
                let requirement = match key(&cell(row, "detail")).as_str() {
                    "yes" | "mandatory" => Requirement::Mandatory,
                    "conditional" => Requirement::Conditional,
                    "no" | "optional" | "" => Requirement::Optional,
                    _ => {
                        return Err(make_error(
                            ErrorCode::ParseFailure,
                            format!(
                                "requirement {} is not Yes, No or Conditional",
                                quoted(&cell(row, "detail"))
                            ),
                            at(row.line_number),
                        ));
                    }
                };
                schema.fields.push(SchemaField {
                    attribute,
                    label: cell(row, "label"),
                    r#type: cell(row, "value"),
                    requirement,
                });
            }
            "domain" => {
                schema
                    .domains
                    .entry(attribute.clone())
                    .or_default()
                    .values
                    .push(SchemaValue {
                        value: cell(row, "value"),
                        parent: cell(row, "detail"),
                    });
                domain_lines.push((attribute, row.line_number));
            }
            "open" => {
                schema.domains.entry(attribute.clone()).or_default().open = true;
                domain_lines.push((attribute, row.line_number));
            }
            "parent" => schema.parent_attribute = attribute,
            "identifier" => schema.identifier_attribute = attribute,
            "prefix" => {
                schema.prefixes.insert(attribute, cell(row, "value"));
            }
            _ => {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    format!(
                        "kind {} is not schema, field, domain, open, parent, identifier or prefix",
                        quoted(&cell(row, "kind"))
                    ),
                    at(row.line_number),
                ));
            }
        }
    }
    if schema.fields.is_empty() {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "the schema defines no fields",
        ));
    }
    for (attribute, line_number) in &domain_lines {
        let defined = schema
            .fields
            .iter()
            .any(|field| field.attribute == *attribute);
        if !defined {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("a domain for {}, which is not a field", quoted(attribute)),
                at(*line_number),
            ));
        }
    }
    Ok(schema)
}

/// Every row of the schedule `text` (the delimited format of utility_csv:
/// a header row, commas, optional quotes) against `schema`. Columns are
/// matched to attributes by attribute name or label, ignoring case, blanks,
/// '-' and '_'. Values are matched EXACTLY - a delivery schema asks for its
/// values "using the exact field values" - and a value that matches only when
/// case and spacing are ignored is reported as a warning naming the listed
/// spelling.
///
/// ParseFailure only when the text cannot be read as a table at all.
pub fn check_delivery(text: &str, schema: &DeliverySchema) -> Result<SchemaCheck> {
    let table = read_raw_table(text)?;
    let mut check = SchemaCheck {
        rows: table.rows.len(),
        ..Default::default()
    };

    // Column of each field, by attribute name or label.
    let mut column_of: BTreeMap<String, usize> = BTreeMap::new();
    let mut used: BTreeSet<usize> = BTreeSet::new();
    for field in &schema.fields {
        for (i, header) in table.header.iter().enumerate() {
            let name = key(header);
            if name == key(&field.attribute)
                || (!field.label.is_empty() && name == key(&field.label))
            {
                column_of.entry(field.attribute.clone()).or_insert(i);
                used.insert(i);
                break;
            }
        }
    }
    for (i, header) in table.header.iter().enumerate() {
        if !used.contains(&i) {
            check.not_in_schema.push(header.clone());
        }
    }
    let value_of = |row: &RawRow, attribute: &str| -> Option<String> {
        column_of
            .get(attribute)
            .map(|&column| row.fields[column].clone())
    };

    for field in &schema.fields {
        if field.requirement == Requirement::Mandatory && !column_of.contains_key(&field.attribute)
        {
            check.findings.push(SchemaFinding {
                severity: FindingSeverity::Error,
                attribute: field.attribute.clone(),
                problem: String::from("is mandatory and has no column"),
                line_number: 0,
            });
        }
        if field.requirement == Requirement::Conditional && column_of.contains_key(&field.attribute)
        {
            check.conditional_not_checked += 1;
        }
    }

    let mut identifiers: BTreeSet<String> = BTreeSet::new();
    for row in &table.rows {
        let mut finding = |severity: FindingSeverity, attribute: &str, problem: String| {
            check.findings.push(SchemaFinding {
                severity,
                attribute: String::from(attribute),
                problem,
                line_number: row.line_number,
            });
        };
        if !schema.identifier_attribute.is_empty()
            && let Some(id) = value_of(row, &schema.identifier_attribute)
            && !id.is_empty()
        {
            identifiers.insert(id);
        }
        let parent = if schema.parent_attribute.is_empty() {
            None
        } else {
            value_of(row, &schema.parent_attribute)
        };

        for field in &schema.fields {
            let Some(value) = value_of(row, &field.attribute) else {
                continue;
            };
            if value.is_empty() {
                if field.requirement == Requirement::Mandatory {
                    finding(
                        FindingSeverity::Error,
                        &field.attribute,
                        String::from("is empty"),
                    );
                }
                continue;
            }
            let lower_type = lowered(&field.r#type);
            if lower_type.starts_with(b"domain list") {
                let Some(domain) = schema.domains.get(&field.attribute) else {
                    continue; // a domain the schema file does not list: nothing to test against
                };
                let mut exact: Vec<&SchemaValue> = Vec::new();
                let mut nearly: Option<&SchemaValue> = None;
                for listed in &domain.values {
                    if listed.value == value {
                        exact.push(listed);
                    } else if nearly.is_none() && key(&listed.value) == key(&value) {
                        nearly = Some(listed);
                    }
                }
                if !exact.is_empty() {
                    // A value listed under parents - a subtype under its asset
                    // type - must be listed under this row's.
                    let any_parent = exact.iter().any(|listed| listed.parent.is_empty());
                    if !any_parent
                        && let Some(parent) = parent.as_deref()
                        && !parent.is_empty()
                        && field.attribute != schema.parent_attribute
                        && !exact.iter().any(|listed| listed.parent == parent)
                    {
                        finding(
                            FindingSeverity::Error,
                            &field.attribute,
                            format!(
                                "{} is not listed under {} {}",
                                quoted(&value),
                                schema.parent_attribute,
                                quoted(parent)
                            ),
                        );
                    }
                } else if let Some(nearly) = nearly {
                    finding(
                        FindingSeverity::Warning,
                        &field.attribute,
                        format!(
                            "{} is listed as {}; values must be spelt exactly",
                            quoted(&value),
                            quoted(&nearly.value)
                        ),
                    );
                } else if !(domain.open && (is_number(value.as_bytes()) || is_arrangement(&value)))
                {
                    finding(
                        FindingSeverity::Error,
                        &field.attribute,
                        format!("{} is not in the domain", quoted(&value)),
                    );
                }
            } else if lower_type.starts_with(b"date") {
                if !is_date(&value) {
                    finding(
                        FindingSeverity::Error,
                        &field.attribute,
                        format!("{} is not a date as YYYY/MM/DD", quoted(&value)),
                    );
                }
            } else if (lower_type.starts_with(b"real number") || lower_type.starts_with(b"integer"))
                && !is_number(value.as_bytes())
            {
                finding(
                    FindingSeverity::Error,
                    &field.attribute,
                    format!("{} is not a number", quoted(&value)),
                );
            }
        }

        for (attribute, other) in &schema.prefixes {
            if let (Some(value), Some(start)) = (value_of(row, attribute), value_of(row, other))
                && !value.is_empty()
                && !start.is_empty()
                && !value.starts_with(start.as_str())
            {
                finding(
                    FindingSeverity::Warning,
                    attribute,
                    format!("does not start with its {other} {}", quoted(&start)),
                );
            }
        }
    }
    check.assets = identifiers.len();
    Ok(check)
}

/// Each distinct finding once, in order of first appearance.
struct Group<'a> {
    first: &'a SchemaFinding,
    count: usize,
    lines: Vec<usize>,
}

/// The check as text: counts, then each distinct finding once with how many
/// rows it is on and the first few line numbers.
pub fn render_schema_check(check: &SchemaCheck, schema: &DeliverySchema) -> String {
    let mut text = String::from("Delivery schema check");
    if !schema.title.is_empty() {
        text += &format!(" against {}", schema.title);
        if !schema.version.is_empty() {
            text += &format!(" v{}", schema.version);
        }
    }
    text += &format!("\n{} rows", check.rows);
    if !schema.identifier_attribute.is_empty() {
        text += &format!(
            ", {} assets ({})",
            check.assets, schema.identifier_attribute
        );
    }
    text += &format!(
        ": {} errors, {} warnings\n",
        check.count(FindingSeverity::Error),
        check.count(FindingSeverity::Warning)
    );
    if !check.not_in_schema.is_empty() {
        text += &format!(
            "Columns the schema does not define (not checked): {}\n",
            check.not_in_schema.join(", ")
        );
    }
    if check.conditional_not_checked > 0 {
        text += &format!(
            "{} conditional fields present; whether each is required is not evaluated\n",
            check.conditional_not_checked
        );
    }

    // Each distinct finding once, in order of first appearance, errors first.
    let mut groups: Vec<Group<'_>> = Vec::new();
    for finding in &check.findings {
        let index = match groups.iter().position(|group| {
            group.first.severity == finding.severity
                && group.first.attribute == finding.attribute
                && group.first.problem == finding.problem
        }) {
            Some(index) => index,
            None => {
                groups.push(Group {
                    first: finding,
                    count: 0,
                    lines: Vec::new(),
                });
                groups.len() - 1
            }
        };
        let group = &mut groups[index];
        group.count += 1;
        if finding.line_number > 0 && group.lines.len() < 5 {
            group.lines.push(finding.line_number);
        }
    }
    // Stable, as the C++'s std::stable_sort is.
    groups.sort_by_key(|group| group.first.severity);
    if groups.is_empty() {
        return text + "The schedule meets the schema.\n";
    }
    text += "\n";
    for group in &groups {
        text += if group.first.severity == FindingSeverity::Error {
            "  error    "
        } else {
            "  warning  "
        };
        text += &format!("{} {}", group.first.attribute, group.first.problem);
        if !group.lines.is_empty() {
            text += &format!(
                " ({}{}: line",
                group.count,
                if group.count == 1 { " row" } else { " rows" }
            );
            text += if group.lines.len() == 1 { " " } else { "s " };
            let lines: Vec<String> = group.lines.iter().map(|line| format!("{line}")).collect();
            text += &lines.join(", ");
            text += if group.count > group.lines.len() {
                ", ...)"
            } else {
                ")"
            };
        }
        text += "\n";
    }
    text
}
