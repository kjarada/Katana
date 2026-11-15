//! Select Similar and Quick Select (src/katana_cad/tools/select_tools.cpp;
//! see families.rs): tools whose answer is a selection, left in the
//! document's selection set (ToolStep::selection) for the next Modify tool,
//! the Properties panel or Erase to act on. Neither changes the drawing, so
//! neither makes a command, and neither is undone with the drawing - as
//! selecting by hand is not.
//!
//! Both select by the document's own rule (is_selectable): an entity that is
//! hidden, or on a hidden or locked layer, is never selected, however well it
//! matches. A layer hidden in one view only is not yet left out, because a
//! tool is not told which view it runs in (as Move's All; outstanding).
//!
//! Quick Select asks its questions one at a time at the command line, each
//! with a default Enter takes - kind, layer, one condition, and whether to
//! look in the selection or the whole drawing - so it runs the same from a
//! menu, from typed words and from a headless script, with no dialog to wait
//! on.

use std::cell::RefCell;
use std::rc::Rc;

use katana_core::text::{ascii_lower, equals_ignoring_case, parse_finite_double, trimmed};
use katana_entity::entity::{property_to_string, to_string};
use katana_entity::{Entity, EntityId, EntityType, PropertyValue};
use katana_geometry::Point2;

use crate::document::Document;
use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolInfo, ToolInput, ToolStart, ToolStep,
};
use crate::layer_overrides::NO_LAYER_OVERRIDES;
use crate::selection::is_selectable;
use crate::tools::everyday_support::{SelectionStep, counted};
use crate::tools::families::Report;
use crate::tools::modify_edit_support::{
    ToolPtr, base_entity, is_option, tool_factory, trimmed_text,
};

// Every entity `keep` accepts that the document lets be selected, ascending.
fn selectable_where(document: &Document, mut keep: impl FnMut(&Entity) -> bool) -> Vec<EntityId> {
    let mut out = Vec::new();
    let model = document.model();
    model.entities.for_each(|entity| {
        if is_selectable(model, entity, &NO_LAYER_OVERRIDES) && keep(entity) {
            out.push(entity.id);
        }
    });
    out.sort_unstable();
    out
}

fn style_name(entity: &Entity) -> String {
    if entity.style.is_empty() {
        String::from("ByLayer")
    } else {
        entity.style.clone()
    }
}

fn colour_name(entity: &Entity) -> String {
    match entity.color {
        Some(color) => color.to_hex(),
        None => String::from("ByLayer"),
    }
}

// The words of a settings list, "a, b and c".
fn listed(parts: &[&str]) -> String {
    let mut text = String::new();
    for (i, part) in parts.iter().enumerate() {
        text.push_str(if i == 0 {
            ""
        } else if i + 1 == parts.len() {
            " and "
        } else {
            ", "
        });
        text.push_str(part);
    }
    text
}

// The words typed at a settings prompt: separated by blanks or commas.
fn setting_words(text: &str) -> Vec<String> {
    text.replace(',', " ")
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(String::from)
        .collect()
}

// ---- Select Similar ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SimilarSettings {
    kind: bool,
    layer: bool,
    style: bool,
    colour: bool,
}

impl Default for SimilarSettings {
    fn default() -> Self {
        Self {
            kind: true,
            layer: true,
            style: true,
            colour: false,
        }
    }
}

impl SimilarSettings {
    fn words(&self) -> String {
        let mut parts = Vec::new();
        if self.kind {
            parts.push("kind");
        }
        if self.layer {
            parts.push("layer");
        }
        if self.style {
            parts.push("style");
        }
        if self.colour {
            parts.push("colour");
        }
        listed(&parts)
    }
}

thread_local! {
    // The settings the catalogue entry shares between uses (the C++
    // shared_ptr its factory captures); the program runs its tools on one
    // thread, so they are that thread's (cad_plan.md section 7, risk 4).
    static SIMILAR_SETTINGS: Rc<RefCell<SimilarSettings>> =
        Rc::new(RefCell::new(SimilarSettings::default()));
}

// What an entity must share with a seed: the fields not matched on left
// empty so they compare equal.
type SimilarKey = (i32, String, String, String);

struct SelectSimilarTool {
    selection: SelectionStep,
    settings: Rc<RefCell<SimilarSettings>>,
    settings_step: bool,
}

impl SelectSimilarTool {
    fn choose_settings(&mut self, text: &str) -> ToolStep {
        let mut chosen = SimilarSettings {
            kind: false,
            layer: false,
            style: false,
            colour: false,
        };
        for word in setting_words(text) {
            if is_option(&word, "Kind", "K") {
                chosen.kind = true;
            } else if is_option(&word, "Layer", "L") {
                chosen.layer = true;
            } else if is_option(&word, "Style", "S") {
                chosen.style = true;
            } else if is_option(&word, "Colour", "C") || is_option(&word, "Color", "C") {
                chosen.colour = true;
            } else {
                return ToolStep::rejected(format!(
                    "'{word}' is not one of Kind, Layer, Style and Colour"
                ));
            }
        }
        if !(chosen.kind || chosen.layer || chosen.style || chosen.colour) {
            return ToolStep::rejected("name at least one of Kind, Layer, Style and Colour");
        }
        *self.settings.borrow_mut() = chosen;
        self.settings_step = false;
        ToolStep::next(format!("Matching on {}", chosen.words()))
    }
}

impl InteractiveTool for SelectSimilarTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.settings_step {
            return format!(
                "Match on any of [Kind/Layer/Style/Colour] <{}>",
                self.settings.borrow().words()
            );
        }
        String::from("Select objects to find the like of or [SEttings], then press Enter")
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.settings_step {
            ToolInput::Value
        } else {
            ToolInput::Selection
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.settings_step {
            return base_entity(cx, self.expects(cx), id, at);
        }
        self.selection.pick(cx.document, id)
    }

    fn value(&mut self, _cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.settings_step {
            return self.choose_settings(text);
        }
        if is_option(text, "SEttings", "SE") {
            self.settings_step = true;
            return ToolStep::next("");
        }
        ToolStep::rejected(format!(
            "'{text}' is not an option here: pick objects, or type SE"
        ))
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.settings_step {
            self.settings_step = false;
            return ToolStep::next("");
        }
        let Some(document) = cx.document else {
            return ToolStep::rejected("there is no drawing");
        };
        let seeds = self.selection.chosen(Some(document));
        if seeds.is_empty() {
            return ToolStep::rejected(
                "nothing is selected: select the objects to find the like of, then press Enter",
            );
        }
        let settings = *self.settings.borrow();
        let key_of = |entity: &Entity| -> SimilarKey {
            (
                if settings.kind {
                    entity.r#type() as i32
                } else {
                    -1
                },
                if settings.layer {
                    entity.layer.clone()
                } else {
                    String::new()
                },
                if settings.style {
                    style_name(entity)
                } else {
                    String::new()
                },
                if settings.colour {
                    colour_name(entity)
                } else {
                    String::new()
                },
            )
        };
        let mut keys: Vec<SimilarKey> = seeds
            .iter()
            .filter_map(|&id| document.model().entities.find(id))
            .map(key_of)
            .collect();
        keys.sort();
        keys.dedup();
        let found = selectable_where(document, |entity| {
            keys.binary_search(&key_of(entity)).is_ok()
        });
        let mut step = ToolStep::done(
            None,
            format!(
                "{} selected: the same {} as the {} picked",
                counted(found.len(), "entity", "entities"),
                settings.words(),
                seeds.len()
            ),
            false,
        );
        step.selection = Some(found);
        step
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.settings_step {
            self.settings_step = false;
            return ToolStep::next("");
        }
        if self.selection.undo() {
            ToolStep::next("")
        } else {
            ToolStep::rejected("nothing to undo in this tool")
        }
    }
}

// ---- Quick Select ------------------------------------------------------------------

// `text` against `pattern`, * for any run and ? for any one character, in
// any case: layer names and codes are typed by hand, in whatever case.
fn matches_wildcard(text: &str, pattern: &str) -> bool {
    // The classic two-pointer match, backtracking to the last *: linear in
    // practice, and no regex to compile per entity.
    let text = text.as_bytes();
    let pattern = pattern.as_bytes();
    let mut t = 0;
    let mut p = 0;
    let mut star: Option<usize> = None;
    let mut resume = 0;
    while t < text.len() {
        if p < pattern.len()
            && (pattern[p] == b'?' || ascii_lower(pattern[p]) == ascii_lower(text[t]))
        {
            t += 1;
            p += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            resume = t;
        } else if let Some(at) = star {
            p = at + 1;
            resume += 1;
            t = resume;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compare {
    Equal,
    NotEqual,
    Less,
    Greater,
    LessOrEqual,
    GreaterOrEqual,
}

#[derive(Clone, Debug, PartialEq)]
struct Condition {
    key: String,
    compare: Compare,
    operand: String,
    /// The operand as a number, when it is one.
    number: Option<f64>,
    /// As typed, for the report.
    text: String,
}

fn parse_condition(typed: &str) -> Option<Condition> {
    // Two-character operators first, so "<=" is not read as "<".
    const OPERATORS: [(&str, Compare); 6] = [
        ("<>", Compare::NotEqual),
        ("<=", Compare::LessOrEqual),
        (">=", Compare::GreaterOrEqual),
        ("=", Compare::Equal),
        ("<", Compare::Less),
        (">", Compare::Greater),
    ];
    for (symbol, compare) in OPERATORS {
        let Some(at) = typed.find(symbol) else {
            continue;
        };
        let key = String::from(trimmed_text(&typed[..at]));
        let mut operand = trimmed_text(&typed[at + symbol.len()..]);
        if operand.len() >= 2 && operand.starts_with('"') && operand.ends_with('"') {
            operand = &operand[1..operand.len() - 1];
        }
        if key.is_empty() {
            return None;
        }
        return Some(Condition {
            key,
            compare,
            operand: String::from(operand),
            number: parse_finite_double(operand),
            text: String::from(trimmed_text(typed)),
        });
    }
    None
}

fn ordered(compare: Compare) -> bool {
    compare != Compare::Equal && compare != Compare::NotEqual
}

fn is_key(key: &str, word: &str) -> bool {
    equals_ignoring_case(key.as_bytes(), word.as_bytes())
}

// Whether `entity` meets `condition`. The keys Layer, Style and Colour are
// the entity's own; any other key is a property, and an entity without that
// property never meets the condition, <> included - "code <> TB" means the
// coded points whose code is not TB, not everything that has no code.
fn meets(entity: &Entity, condition: &Condition) -> bool {
    let mut number: Option<f64> = None;
    let text = if is_key(&condition.key, "layer") {
        entity.layer.clone()
    } else if is_key(&condition.key, "style") {
        style_name(entity)
    } else if is_key(&condition.key, "colour") || is_key(&condition.key, "color") {
        colour_name(entity)
    } else {
        // Keys are typed by hand; "Code" finds "code".
        let found = entity.properties.get(&condition.key).or_else(|| {
            entity
                .properties
                .iter()
                .find(|(key, _)| is_key(key, &condition.key))
                .map(|(_, value)| value)
        });
        let Some(value) = found else {
            return false;
        };
        number = match value {
            PropertyValue::Integer(integer) => Some(*integer as f64),
            PropertyValue::Real(real) => Some(*real),
            PropertyValue::Text(string) => parse_finite_double(trimmed(string.as_bytes())),
            PropertyValue::Boolean(_) => None,
        };
        property_to_string(value)
    };
    if let (Some(b), Some(a)) = (condition.number, number) {
        return match condition.compare {
            Compare::Equal => a == b,
            Compare::NotEqual => a != b,
            Compare::Less => a < b,
            Compare::Greater => a > b,
            Compare::LessOrEqual => a <= b,
            Compare::GreaterOrEqual => a >= b,
        };
    }
    if ordered(condition.compare) {
        return false; // "code > TB" orders nothing; refused when typed
    }
    let equal = matches_wildcard(&text, &condition.operand);
    if condition.compare == Compare::Equal {
        equal
    } else {
        !equal
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickStep {
    Kind,
    Layer,
    Condition,
    Scope,
}

// The kinds Quick Select offers, in the order of their kind bytes: the first
// seven (the C++ casts 0 to 6 to EntityType).
const QUICK_KINDS: [EntityType; 7] = [
    EntityType::Point,
    EntityType::Line,
    EntityType::Arc,
    EntityType::Polyline,
    EntityType::Circle,
    EntityType::Text,
    EntityType::Dimension,
];

struct QuickSelectTool {
    /// Ascending, as ToolStart gives it.
    given: Vec<EntityId>,
    step: QuickStep,
    kind: Option<EntityType>,
    layer: String,
    condition: Option<Condition>,
}

impl QuickSelectTool {
    fn kind_named(text: &str) -> Option<EntityType> {
        QUICK_KINDS
            .into_iter()
            .find(|&kind| is_key(text, to_string(kind)))
    }

    fn after_condition(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        // Asked only when there is a selection to look in.
        if !self.given.is_empty() {
            self.step = QuickStep::Scope;
            return ToolStep::next("");
        }
        self.select(cx, false)
    }

    fn select(&self, cx: &ToolContext<'_>, within_selection: bool) -> ToolStep {
        let Some(document) = cx.document else {
            return ToolStep::rejected("there is no drawing");
        };
        let found = selectable_where(document, |entity| {
            if within_selection && self.given.binary_search(&entity.id).is_err() {
                return false;
            }
            if self.kind.is_some_and(|kind| entity.r#type() != kind) {
                return false;
            }
            if self.layer != "*" && !matches_wildcard(&entity.layer, &self.layer) {
                return false;
            }
            self.condition
                .as_ref()
                .is_none_or(|condition| meets(entity, condition))
        });
        let mut text = format!("{} selected: ", counted(found.len(), "entity", "entities"));
        match self.kind {
            Some(kind) => text.push_str(&format!("{}s", to_string(kind))),
            None => text.push_str("any kind"),
        }
        if self.layer == "*" {
            text.push_str(" on any layer");
        } else {
            text.push_str(&format!(" on layer {}", self.layer));
        }
        if let Some(condition) = &self.condition {
            text.push_str(&format!(" where {}", condition.text));
        }
        text.push_str(if within_selection {
            ", within the selection"
        } else {
            ", in the drawing"
        });
        let mut step = ToolStep::done(None, text, false);
        step.selection = Some(found);
        step
    }
}

impl InteractiveTool for QuickSelectTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from(match self.step {
            QuickStep::Kind => {
                "Object kind [Any/Point/Line/Arc/Polyline/Circle/Text/Dimension] <Any>"
            }
            QuickStep::Layer => "Layer name, with * and ? as wildcards <*>",
            QuickStep::Condition => {
                "Condition such as code = TB or elevation > 100 (= <> < > <= >=; Layer, Style, \
                 Colour or a property), Enter for none"
            }
            QuickStep::Scope => "Look in [Drawing/Selection] <Selection>",
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Value
    }

    fn value(&mut self, cx: &ToolContext<'_>, typed: &str) -> ToolStep {
        let text = trimmed_text(typed);
        match self.step {
            QuickStep::Kind => {
                if is_key(text, "Any") {
                    self.kind = None;
                } else if let Some(kind) = Self::kind_named(text) {
                    self.kind = Some(kind);
                } else {
                    return ToolStep::rejected(format!(
                        "'{text}' is not a kind: Any, Point, Line, Arc, Polyline, Circle, Text \
                         or Dimension"
                    ));
                }
                self.step = QuickStep::Layer;
                ToolStep::next("")
            }
            QuickStep::Layer => {
                self.layer = String::from(if text.is_empty() { "*" } else { text });
                self.step = QuickStep::Condition;
                ToolStep::next("")
            }
            QuickStep::Condition => {
                let Some(condition) = parse_condition(text) else {
                    return ToolStep::rejected(
                        "a condition is a name, an operator (= <> < > <= >=) and a value, such \
                         as code = TB",
                    );
                };
                if ordered(condition.compare) && condition.number.is_none() {
                    return ToolStep::rejected(
                        "< and > compare numbers; compare text with = or <>",
                    );
                }
                self.condition = Some(condition);
                self.after_condition(cx)
            }
            QuickStep::Scope => {
                if is_option(text, "Drawing", "D") {
                    return self.select(cx, false);
                }
                if is_option(text, "Selection", "S") {
                    return self.select(cx, true);
                }
                ToolStep::rejected(format!(
                    "'{text}' is not an option here: Drawing or Selection"
                ))
            }
        }
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            QuickStep::Kind => {
                self.kind = None;
                self.step = QuickStep::Layer;
                ToolStep::next("")
            }
            QuickStep::Layer => {
                self.layer = String::from("*");
                self.step = QuickStep::Condition;
                ToolStep::next("")
            }
            QuickStep::Condition => {
                self.condition = None;
                self.after_condition(cx)
            }
            QuickStep::Scope => self.select(cx, true),
        }
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        self.step = match self.step {
            QuickStep::Kind => return ToolStep::rejected("nothing to undo in this tool"),
            QuickStep::Layer => QuickStep::Kind,
            QuickStep::Condition => QuickStep::Layer,
            QuickStep::Scope => QuickStep::Condition,
        };
        ToolStep::next("")
    }
}

fn select_similar_tool(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    Box::new(SelectSimilarTool {
        selection: SelectionStep::new(),
        settings: SIMILAR_SETTINGS.with(Rc::clone),
        settings_step: false,
    })
}

fn quick_select_tool(_cx: &ToolContext<'_>, start: &ToolStart) -> ToolPtr {
    Box::new(QuickSelectTool {
        given: start.selection.clone(),
        step: QuickStep::Kind,
        kind: None,
        layer: String::from("*"),
        condition: None,
    })
}

pub fn add_select_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    report(catalog.add(ToolInfo {
        id: String::from("select.similar"),
        name: String::from("Select Similar"),
        category: String::from("Tools"),
        group: String::from("Select"),
        order: 10,
        aliases: vec![String::from("SELECTSIMILAR")],
        tip: String::from(
            "Selects every object of the same kind, layer and style as those picked (SE to \
             match on kind, layer, style or colour).",
        ),
        make: Some(tool_factory(select_similar_tool)),
        ..ToolInfo::default()
    }));
    report(catalog.add(ToolInfo {
        id: String::from("select.quick"),
        name: String::from("Quick Select"),
        category: String::from("Tools"),
        group: String::from("Select"),
        order: 20,
        aliases: vec![String::from("QSELECT")],
        tip: String::from(
            "Selects by kind, layer and one condition on a property, such as code = TB or \
             elevation > 100, in the drawing or within the selection.",
        ),
        make: Some(tool_factory(quick_select_tool)),
        ..ToolInfo::default()
    }));
}
