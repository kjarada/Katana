//! Labels made by hand and by rule (include/katana/cad/annotation/auto_label.hpp;
//! docs/annotation.md, "Labels" and "Auto-labelling").
//!
//! Every function here returns a COMMAND, never an edited model: the caller
//! executes it through Document::execute, so each is one validated, undoable
//! step (Rule 2), and the command line, the window and an agent get the same
//! result for the same request.

use std::collections::{BTreeMap, BTreeSet};

use katana_commands::{ChangeSet, ChangeSetCommand, CommandPtr, Transaction, create_layer};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::annotation::glob_matches;
use katana_entity::entity::to_string;
use katana_entity::label_values::{label_anchor, labels};
use katana_entity::layer_path::layer_ancestors;
use katana_entity::tables::DEFAULT_LAYER_NAME;
use katana_entity::{
    Entity, EntityId, Geometry, LabelGeometry, LabelKind, LabelMarker, LabelPiece, LabelPlacement,
    LabelRule, LabelStyle, Layer, Model,
};
use katana_geometry::Point2;

use crate::annotation::label_layout::{label_pieces_of, label_text, survey_code};

/// A label to make by hand.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelRequest {
    /// The entity to label, or
    pub target: EntityId,
    /// a chainage label's alignment.
    pub alignment: String,
    pub style: String,
    pub part: i32,
    /// The layer it goes on; empty puts it on its target's (an alignment's
    /// label on "0").
    pub layer: String,
    pub position: Option<Point2>,
    pub text_override: String,
}

impl Default for LabelRequest {
    fn default() -> Self {
        Self {
            target: 0,
            alignment: String::new(),
            style: String::new(),
            part: -1,
            layer: String::new(),
            position: None,
            text_override: String::new(),
        }
    }
}

/// What AUTOLABEL RUN did or would do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutoLabelReport {
    pub created: usize,
    /// Already there, left alone (a dragged label keeps its place).
    pub kept: usize,
    /// Made by a rule that no longer matches its target.
    pub removed: usize,
    /// By rule name: the labels each rule has after the run.
    pub per_rule: BTreeMap<String, usize>,
    /// Targets a rule matched but could not label: their label layer is
    /// locked, or the label would say nothing.
    pub skipped: usize,
}

// A change set, as a command of its own name.
fn change_command(name: &str, changes: ChangeSet, destructive: bool) -> CommandPtr {
    Box::new(ChangeSetCommand::new(
        name,
        move |_context| Ok(changes.clone()),
        destructive,
    ))
}

// Whether a label with these pieces says anything at all.
fn says_something(label: &LabelGeometry, style: &LabelStyle, pieces: &[LabelPiece]) -> bool {
    pieces
        .iter()
        .any(|piece| !piece.tick_only && !label_text(label, style, piece).is_empty())
}

// The label a rule puts on a target, before it has an id.
fn rule_label(
    rule: &LabelRule,
    style: &LabelStyle,
    target: Option<&Entity>,
    alignment: &str,
    anchor: Point2,
) -> Entity {
    let label = LabelGeometry {
        target: target.map_or(0, |target| target.id),
        alignment: String::from(alignment),
        style: style.name.clone(),
        anchor,
        rule: rule.name.clone(),
        ..LabelGeometry::default()
    };
    let layer = if !rule.label_layer.is_empty() {
        rule.label_layer.clone()
    } else {
        match target {
            Some(target) => target.layer.clone(),
            None => String::from(DEFAULT_LAYER_NAME),
        }
    };
    Entity {
        geometry: Geometry::Label(label),
        layer,
        ..Entity::default()
    }
}

// A label's identity for a re-run: what it labels, how, and which rule made it.
type LabelKey = (EntityId, String, i32, String, String);

fn key_of(label: &LabelGeometry) -> LabelKey {
    (
        label.target,
        label.alignment.clone(),
        label.part,
        label.style.clone(),
        label.rule.clone(),
    )
}

// A rule to run and the label style it names; NotFound when it names none.
fn rule_and_style<'m>(
    model: &'m Model,
    rule: &'m LabelRule,
) -> Result<(&'m LabelRule, &'m LabelStyle)> {
    let Some(style) = model.label_styles.find(&rule.label_style) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "the rule's label style does not exist",
            format!("rule={} style={}", rule.name, rule.label_style),
        ));
    };
    Ok((rule, style))
}

/// Whether `rule` asks to label `entity` (its filters, and its style's kind).
pub fn rule_matches(rule: &LabelRule, style: &LabelStyle, entity: &Entity) -> bool {
    if !rule.enabled {
        return false;
    }
    if !labels(style.kind, &entity.geometry) {
        return false;
    }
    if !rule.entity_type.is_empty() && !glob_matches(&rule.entity_type, to_string(entity.r#type()))
    {
        return false;
    }
    if !rule.layer.is_empty() {
        // The layer or any ancestor: "survey" matches survey/points/EP, as
        // switching "survey" off would.
        let mut matched = glob_matches(&rule.layer, &entity.layer);
        for ancestor in layer_ancestors(&entity.layer) {
            matched = matched || glob_matches(&rule.layer, &ancestor);
        }
        if !matched {
            return false;
        }
    }
    if !rule.code.is_empty() {
        let code = survey_code(entity);
        if code.is_empty() || !glob_matches(&rule.code, &code) {
            return false;
        }
    }
    true
}

/// One label as a command ("CREATE_LABEL"). Refused when the style or the
/// target is missing, the style's kind cannot label the target (a Segment
/// style on a point), the part does not exist, or the label would say
/// nothing (every line of its template needs a value the target lacks) - so
/// a label that could never draw is not made.
pub fn create_label(model: &Model, request: &LabelRequest) -> Result<CommandPtr> {
    let Some(style) = model.label_styles.find(&request.style) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "label style does not exist",
            request.style.as_str(),
        ));
    };
    let mut label = LabelGeometry {
        style: style.name.clone(),
        part: request.part,
        position: request.position,
        text_override: request.text_override.clone(),
        ..LabelGeometry::default()
    };
    let mut layer = request.layer.clone();
    if style.kind == LabelKind::Chainage {
        if request.alignment.is_empty() || !model.alignments.contains(&request.alignment) {
            return Err(make_error(
                ErrorCode::NotFound,
                "a chainage label needs an alignment",
                request.alignment.as_str(),
            ));
        }
        label.alignment = request.alignment.clone();
        if layer.is_empty() {
            layer = String::from(DEFAULT_LAYER_NAME);
        }
    } else {
        let Some(target) = model.entities.find(request.target) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                format!("id={}", request.target),
            ));
        };
        if !labels(style.kind, &target.geometry) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "a {} label style cannot label a {}",
                    to_string(style.kind),
                    to_string(target.r#type())
                ),
                format!("id={}", request.target),
            ));
        }
        label.target = target.id;
        if layer.is_empty() {
            layer = target.layer.clone();
        }
    }
    let pieces = label_pieces_of(model, &label, style);
    let Some(first) = pieces.first() else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the target has no such part to label",
            format!("part={}", request.part),
        ));
    };
    if !says_something(&label, style, &pieces) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the label would say nothing: the target has none of the values its template asks \
             for",
            style.text.as_str(),
        ));
    }
    label.anchor = label_anchor(model, &label, style).unwrap_or(first.anchor);
    let changes = ChangeSet {
        add: vec![Entity {
            geometry: Geometry::Label(label),
            layer,
            ..Entity::default()
        }],
        ..ChangeSet::default()
    };
    Ok(change_command("CREATE_LABEL", changes, false))
}

/// Every request as ONE command ("CREATE_LABEL"), so one undo takes back the
/// lot: what the LABEL verb and the Label Objects tool both make, so the two
/// cannot differ. The first request refused refuses the whole, its context
/// naming the target ("id=12 ..." or "alignment=MC01 ..."); InvalidArgument
/// for no requests.
pub fn create_labels(model: &Model, requests: &[LabelRequest]) -> Result<CommandPtr> {
    if requests.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "there is nothing to label",
        ));
    }
    let mut transaction = Transaction::new("CREATE_LABEL");
    for one in requests {
        match create_label(model, one) {
            Ok(built) => transaction.add(built),
            Err(error) => {
                let what = if one.target != 0 {
                    format!("id={}", one.target)
                } else {
                    format!("alignment={}", one.alignment)
                };
                return Err(make_error(
                    error.code,
                    error.message,
                    format!("{what} {}", error.context),
                ));
            }
        }
    }
    Ok(Box::new(transaction))
}

/// Applies the rules named (every ENABLED rule when `rule_names` is empty) as
/// ONE command ("AUTOLABEL"): each matching entity (or alignment, for a
/// Chainage style) gets the rule's label unless it already has it; a label a
/// rule made earlier that the rule no longer asks for is removed; labels
/// placed by hand and labels of other rules are not touched. Re-running with
/// nothing changed is None with no error: there is nothing to undo. Refused
/// for a rule that does not exist or names a label style that does not.
///
/// A rule's label layer that does not exist yet is created in the same step.
/// Matching is in entity id order, so two runs on one model make the same
/// labels in the same order (Rule 7). `report`, when given, says what the run
/// did (the C++'s pointer, null by default).
pub fn auto_label(
    model: &Model,
    rule_names: &[String],
    report: Option<&mut AutoLabelReport>,
) -> Result<Option<CommandPtr>> {
    let mut local = AutoLabelReport::default();
    let out = match report {
        Some(report) => report,
        None => &mut local,
    };
    *out = AutoLabelReport::default();

    // The rules to run, in name order, and their styles.
    let mut rules: Vec<(&LabelRule, &LabelStyle)> = Vec::new();
    if rule_names.is_empty() {
        // Through the names: the table's visitor lends each rule for the call
        // only, and these references are kept.
        for name in model.label_rules.names() {
            if let Some(rule) = model.label_rules.find(&name)
                && rule.enabled
            {
                rules.push(rule_and_style(model, rule)?);
            }
        }
    } else {
        for name in rule_names {
            let Some(rule) = model.label_rules.find(name) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "label rule does not exist",
                    name.as_str(),
                ));
            };
            rules.push(rule_and_style(model, rule)?);
        }
    }
    if rules.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "there is no enabled label rule to run",
        ));
    }
    let running: BTreeSet<&str> = rules.iter().map(|(rule, _)| rule.name.as_str()).collect();

    // What the rules made before, by identity.
    let mut existing: BTreeMap<LabelKey, EntityId> = BTreeMap::new();
    model.entities.for_each(|entity| {
        if let Geometry::Label(label) = &entity.geometry
            && running.contains(label.rule.as_str())
        {
            existing.entry(key_of(label)).or_insert(entity.id);
        }
    });

    // What they ask for now.
    let mut changes = ChangeSet::default();
    let mut wanted: BTreeSet<LabelKey> = BTreeSet::new();
    let mut layers_needed: BTreeSet<String> = BTreeSet::new();
    let mut want =
        |rule: &LabelRule, style: &LabelStyle, target: Option<&Entity>, alignment: &str| {
            let probe = LabelGeometry {
                target: target.map_or(0, |target| target.id),
                alignment: String::from(alignment),
                style: style.name.clone(),
                rule: rule.name.clone(),
                ..LabelGeometry::default()
            };
            let key = key_of(&probe);
            if wanted.contains(&key) {
                return;
            }
            let pieces = label_pieces_of(model, &probe, style);
            let Some(first) = pieces.first() else {
                out.skipped += 1;
                return;
            };
            if !says_something(&probe, style, &pieces) {
                out.skipped += 1;
                return;
            }
            let entity = rule_label(
                rule,
                style,
                target,
                alignment,
                label_anchor(model, &probe, style).unwrap_or(first.anchor),
            );
            if model.layers.find(&entity.layer).is_some() {
                if model.layers.resolve(&entity.layer).locked {
                    out.skipped += 1;
                    return;
                }
            } else {
                layers_needed.insert(entity.layer.clone());
            }
            let known = existing.contains_key(&key);
            wanted.insert(key);
            *out.per_rule.entry(rule.name.clone()).or_default() += 1;
            if known {
                out.kept += 1;
                return;
            }
            changes.add.push(entity);
            out.created += 1;
        };
    for &(rule, style) in &rules {
        if style.kind == LabelKind::Chainage {
            model.alignments.for_each(|alignment| {
                // An alignment has no layer or code; a rule's type filter,
                // when it has one, must say Alignment.
                if rule.entity_type.is_empty() || glob_matches(&rule.entity_type, "Alignment") {
                    want(rule, style, None, &alignment.name);
                }
            });
            continue;
        }
        model.entities.for_each(|entity| {
            if matches!(entity.geometry, Geometry::Label(_)) || !rule_matches(rule, style, entity) {
                return;
            }
            want(rule, style, Some(entity), "");
        });
    }
    for (key, &id) in &existing {
        if !wanted.contains(key)
            && let Some(entity) = model.entities.find(id)
            && !model.layers.resolve(&entity.layer).locked
        {
            changes.remove.push(id);
            out.removed += 1;
        }
    }
    if changes.empty() {
        return Ok(None);
    }
    if layers_needed.is_empty() {
        return Ok(Some(change_command("AUTOLABEL", changes, false)));
    }
    // A rule's label layer that does not exist yet, made in the same step.
    let mut transaction = Transaction::new("AUTOLABEL");
    for name in layers_needed {
        transaction.add(create_layer(Layer {
            name,
            ..Layer::default()
        }));
    }
    transaction.add(change_command("AUTOLABEL", changes, false));
    Ok(Some(Box::new(transaction)))
}

/// Removes the labels the named rules made (every rule's when empty) as one
/// command ("AUTOLABEL_CLEAR"); None with no error when there are none.
/// `removed`, when given, is set to how many go.
pub fn clear_auto_labels(
    model: &Model,
    rule_names: &[String],
    removed: Option<&mut usize>,
) -> Result<Option<CommandPtr>> {
    let named: BTreeSet<&str> = rule_names.iter().map(String::as_str).collect();
    for &name in &named {
        if !model.label_rules.contains(name) {
            // A rule that was deleted may still have labels; they are asked
            // for by the name they carry, so a missing rule is not an error
            // unless it also has none.
            let mut any = false;
            model.entities.for_each(|entity| {
                if let Geometry::Label(label) = &entity.geometry {
                    any = any || label.rule == name;
                }
            });
            if !any {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "no label rule or rule labels of that name",
                    name,
                ));
            }
        }
    }
    let mut changes = ChangeSet::default();
    model.entities.for_each(|entity| {
        let Geometry::Label(label) = &entity.geometry else {
            return;
        };
        if label.rule.is_empty() {
            return;
        }
        if (named.is_empty() || named.contains(label.rule.as_str()))
            && !model.layers.resolve(&entity.layer).locked
        {
            changes.remove.push(entity.id);
        }
    });
    if let Some(removed) = removed {
        *removed = changes.remove.len();
    }
    if changes.empty() {
        return Ok(None);
    }
    Ok(Some(change_command("AUTOLABEL_CLEAR", changes, true)))
}

fn style_of(name: &str, kind: LabelKind, text: &str) -> LabelStyle {
    LabelStyle {
        name: String::from(name),
        kind,
        text: String::from(text),
        ..LabelStyle::default()
    }
}

/// The standard label styles LABELSTYLE DEFAULTS adds: point number, spot
/// level, bearing and distance, arc data, lot area, chainage. Every size in
/// paper millimetres, the templates in the formats survey plans use.
pub fn default_label_styles() -> Vec<LabelStyle> {
    vec![
        // A point's number, beside it.
        style_of("Point Number", LabelKind::Point, "{point}"),
        // A spot level: a cross at the point and its level to 3 decimals, as
        // a survey plan shows one.
        LabelStyle {
            marker: LabelMarker::Cross,
            marker_size: 1.5,
            placement: LabelPlacement::Right,
            ..style_of("Spot Level", LabelKind::Point, "{z:.3f}")
        },
        // Number, code and level, a line each.
        style_of(
            "Point Details",
            LabelKind::Point,
            "{point}\n{code}\nRL {z:.3f}",
        ),
        // A boundary's bearing in whole seconds and its distance to the
        // millimetre, along the line - the form a survey plan carries.
        LabelStyle {
            minimum_length: 5.0,
            ..style_of(
                "Bearing Distance",
                LabelKind::Segment,
                "{bearing:dms} {distance:.3f}",
            )
        },
        style_of("Arc Data", LabelKind::Arc, "R {radius:.3f}\nA {length:.3f}"),
        // A lot's area in square metres, and in hectares on a second line.
        LabelStyle {
            placement: LabelPlacement::Centroid,
            priority: 10,
            ..style_of(
                "Lot Area",
                LabelKind::Area,
                "{area:m2:.1f} m\u{b2}\n{area:ha:.4f} ha",
            )
        },
        // Chainages every 20 m with a tick every 10 m, across the line.
        LabelStyle {
            interval: 20.0,
            tick_interval: 10.0,
            priority: 5,
            ..style_of("Chainage", LabelKind::Chainage, "{chainage:ch}")
        },
    ]
}
