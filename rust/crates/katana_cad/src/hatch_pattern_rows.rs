//! What the Hatch Patterns tab of Format > Styles and Linetypes shows, below
//! the window so it can be tested - the counterpart of style_manager_rows for
//! the model's hatch patterns (include/katana/cad/hatch_pattern_rows.hpp;
//! docs/desktop.md, "Styles and Linetypes").
//!
//! The tab only renders these rows and runs HATCH lines; who uses a pattern is
//! entity::table_usage's one answer, so the tab, HATCH DELETE's refusal and
//! PURGE HATCHES cannot disagree about what is used.

use katana_core::text::{format_exact_real, format_general};
use katana_entity::table_usage::table_usage;
use katana_entity::tables::HatchPatternPolicy;
use katana_entity::{HatchPattern, Model, NamedTablePolicy, TableUsage, UsageOptions, Users};
use katana_math::numerics::RAD_TO_DEG;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HatchPatternRow {
    pub name: String,
    pub solid: bool,
    pub families: usize,
    pub description: String,
    /// The layers and styles naming it and the entities drawn with it.
    pub users: Users,
    /// "none", what an unhatched layer resolves to: not given a fill, not
    /// deleted (entity::HatchPatternPolicy).
    pub built_in: bool,
}

/// Every pattern in the table, ascending by name.
pub fn hatch_pattern_rows(model: &Model) -> Vec<HatchPatternRow> {
    let usage = table_usage(model, UsageOptions::default());
    let mut rows = Vec::with_capacity(model.hatch_patterns.size());
    model.hatch_patterns.for_each(|pattern| {
        rows.push(HatchPatternRow {
            name: pattern.name.clone(),
            solid: pattern.solid,
            families: pattern.families.len(),
            description: pattern.description.clone(),
            users: TableUsage::of(&usage.hatch_patterns, &pattern.name).clone(),
            built_in: <HatchPatternPolicy as NamedTablePolicy<HatchPattern>>::is_protected(
                &pattern.name,
            ),
        });
    });
    rows
}

/// "solid", "1 family", "3 families" or "draws nothing".
pub fn hatch_pattern_kind(row: &HatchPatternRow) -> String {
    if row.solid {
        return String::from("solid");
    }
    if row.families == 0 {
        return String::from("draws nothing");
    }
    format!(
        "{}{}",
        row.families,
        if row.families == 1 {
            " family"
        } else {
            " families"
        }
    )
}

/// A family's angle (radians, as the model holds it) in the degrees HATCH
/// takes, to twelve significant digits: the degrees a person typed. The
/// radians cannot hold most of those exactly - 30 degrees is stored as
/// 0.5235987755982988, which multiplies back to 29.999999999999996 - and
/// twelve digits give "30" again, which HATCH turns into the same radians.
pub fn hatch_angle_degrees(radians: f64) -> String {
    // The C++ std::format("{:.12g}").
    let text = format_general(radians * RAD_TO_DEG, 12);
    // A hair below zero is zero to the digits shown.
    if text == "-0" {
        String::from("0")
    } else {
        text
    }
}

/// The families as HATCH NEW and HATCH SET take them - "45 0.25 135 0.25":
/// hatch_angle_degrees, and the spacing as the shortest text that reads back
/// as the same double. Empty for a solid pattern or one that draws nothing.
pub fn hatch_families_text(pattern: &HatchPattern) -> String {
    let mut text = String::new();
    if pattern.solid {
        return text;
    }
    for family in &pattern.families {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&hatch_angle_degrees(family.angle));
        text.push(' ');
        text.push_str(&format_exact_real(family.spacing));
    }
    text
}

/// `base`, else "base 2", "base 3" ... - the first name no hatch pattern has.
pub fn free_hatch_pattern_name(model: &Model, base: &str) -> String {
    let mut name = String::from(base);
    let mut suffix: i64 = 2;
    while model.hatch_patterns.contains(&name) {
        name = format!("{base} {suffix}");
        suffix += 1;
    }
    name
}
