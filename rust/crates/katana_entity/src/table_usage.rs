//! Who uses each named thing in a model: every style, linetype name, symbol
//! name, hatch pattern name and layer, and the layers, styles and entities
//! that reach it - found in ONE pass over the entities.
//!
//! One function serves three readers that must not disagree: the delete
//! guards (an item still used cannot be deleted), purge (what is used by
//! nothing can go) and the managers (a "Used" column, "select the entities
//! using this", and the names used that resolve to nothing).
//!
//! "Reaches" is resolve_display's chain, read through the same two functions
//! resolve_display reads (resolved_linetype, resolved_hatch_pattern), so an
//! entity is counted against exactly the linetype it is drawn with. A per-row
//! scan would be one pass per row - 800 styles over 250 000 entities - which
//! is why this is one pass for everything.
//!
//! Names are kept whether or not a table defines them: a library linestyle a
//! style names lives in the library, not the model, and a name nothing
//! defines is exactly what a manager has to show.

use std::collections::BTreeMap;

use crate::display::{is_by_layer, resolved_hatch_pattern, resolved_linetype};
use crate::entity::{EntityId, INVALID_ENTITY_ID};
use crate::model::Model;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsageOptions {
    /// Collect the entities' ids as well as counting them. Off by default: a
    /// count is all a column or a guard needs, and a vector per name over a
    /// quarter of a million entities is not free.
    pub entity_ids: bool,
}

/// Everything that reaches one name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Users {
    /// Naming it directly, ascending.
    pub layers: Vec<String>,
    /// Naming it directly, ascending.
    pub styles: Vec<String>,
    /// The entities reaching it: for a style, the ones wearing it; for a
    /// layer, the ones on it (not its subtree); for a linetype, symbol or
    /// hatch pattern, the ones resolve_display resolves to it.
    pub entities: usize,
    /// The lowest of their ids.
    pub first_entity: EntityId,
    /// Ascending; UsageOptions::entity_ids only.
    pub entity_ids: Vec<EntityId>,
}

impl Default for Users {
    fn default() -> Self {
        Self {
            layers: Vec::new(),
            styles: Vec::new(),
            entities: 0,
            first_entity: INVALID_ENTITY_ID,
            entity_ids: Vec::new(),
        }
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

impl Users {
    /// Named by a layer or a style. What holds a linetype or hatch pattern in
    /// place: an entity reaches one only through a layer or a style.
    pub fn named(&self) -> bool {
        !self.layers.is_empty() || !self.styles.is_empty()
    }

    pub fn used(&self) -> bool {
        self.named() || self.entities > 0
    }

    /// "used by 2 layers, 1 style and 418 entities, e.g. layer=survey" - the
    /// count and the first holder, for a refusal a person can act on. Layers
    /// come first because a layer is what a person fixes first; an entity id
    /// is the last resort. Empty when nothing uses it.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.layers.is_empty() {
            parts.push(counted(self.layers.len(), "layer", "layers"));
        }
        if !self.styles.is_empty() {
            parts.push(counted(self.styles.len(), "style", "styles"));
        }
        if self.entities > 0 {
            parts.push(counted(self.entities, "entity", "entities"));
        }
        if parts.is_empty() {
            return String::new();
        }
        let mut text = String::from("used by ");
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                text.push_str(if i + 1 == parts.len() { " and " } else { ", " });
            }
            text.push_str(part);
        }
        text.push_str(", e.g. ");
        if let Some(layer) = self.layers.first() {
            text.push_str(&format!("layer={layer}"));
        } else if let Some(style) = self.styles.first() {
            text.push_str(&format!("style={style}"));
        } else {
            text.push_str(&format!("id={}", self.first_entity));
        }
        text
    }

    fn count_entity(&mut self, id: EntityId, keep_ids: bool) {
        if self.entities == 0 || id < self.first_entity {
            self.first_entity = id;
        }
        self.entities += 1;
        if keep_ids {
            self.entity_ids.push(id);
        }
    }
}

pub type UsageMap = BTreeMap<String, Users>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableUsage {
    /// Every style in the table, and every name an entity wears that is not.
    pub styles: UsageMap,
    /// Every model linetype, and every other name a layer or style gives as
    /// its linetype. ByLayer is not a name: an entity reaching a linetype
    /// through it is counted under its layer's linetype.
    pub linetypes: UsageMap,
    /// Every non-empty Style::symbol.
    pub symbols: UsageMap,
    /// Every hatch pattern in the table, and every other name.
    pub hatch_patterns: UsageMap,
    /// Every layer, and every name an entity is on that is not a layer.
    pub layers: UsageMap,
}

static NOBODY: Users = Users {
    layers: Vec::new(),
    styles: Vec::new(),
    entities: 0,
    first_entity: INVALID_ENTITY_ID,
    entity_ids: Vec::new(),
};

impl TableUsage {
    /// The users of one name; an empty Users for a name nothing uses.
    pub fn of<'a>(map: &'a UsageMap, name: &str) -> &'a Users {
        map.get(name).unwrap_or(&NOBODY)
    }
}

fn users_of<'a>(map: &'a mut UsageMap, name: &str) -> &'a mut Users {
    map.entry(name.to_owned()).or_default()
}

/// Where one (layer, style) pair's entities are counted: the names, resolved
/// once. Entities come in long runs of the same pair - an archive import puts
/// thousands of points on one layer in one style - so the lookups per entity
/// become one.
struct Slot {
    layer: String,
    /// None when the entity names no style.
    style: Option<String>,
    linetype: String,
    hatch: String,
    /// None when its style draws no symbol.
    symbol: Option<String>,
}

pub fn table_usage(model: &Model, options: UsageOptions) -> TableUsage {
    let mut usage = TableUsage::default();

    // Every defined item is present, used or not: a purge is looking for
    // exactly the ones with nobody.
    model.styles.for_each(|style| {
        usage.styles.entry(style.name.clone()).or_default();
    });
    model.linetypes.for_each(|linetype| {
        usage.linetypes.entry(linetype.name.clone()).or_default();
    });
    model.hatch_patterns.for_each(|pattern| {
        usage
            .hatch_patterns
            .entry(pattern.name.clone())
            .or_default();
    });
    let layer_names = model.layers.names();
    for name in &layer_names {
        usage.layers.entry(name.clone()).or_default();
    }

    // Who names what. Walked in name order, so every holder list comes out
    // ascending with no sort.
    for name in &layer_names {
        let Some(layer) = model.layers.find(name) else {
            continue;
        };
        if !is_by_layer(&layer.linetype) {
            users_of(&mut usage.linetypes, &layer.linetype)
                .layers
                .push(name.clone());
        }
        if !layer.hatch_pattern.is_empty() {
            users_of(&mut usage.hatch_patterns, &layer.hatch_pattern)
                .layers
                .push(name.clone());
        }
    }
    model.styles.for_each(|style| {
        if !is_by_layer(&style.linetype) {
            users_of(&mut usage.linetypes, &style.linetype)
                .styles
                .push(style.name.clone());
        }
        if !style.hatch_pattern.is_empty() {
            users_of(&mut usage.hatch_patterns, &style.hatch_pattern)
                .styles
                .push(style.name.clone());
        }
        if !style.symbol.is_empty() {
            users_of(&mut usage.symbols, &style.symbol)
                .styles
                .push(style.name.clone());
        }
    });

    // The one pass over the entities.
    let mut slots: BTreeMap<(String, String), Slot> = BTreeMap::new();
    model.entities.for_each(|entity| {
        let key = (entity.layer.clone(), entity.style.clone());
        let slot = slots.entry(key).or_insert_with(|| {
            let layer = model.layers.find(&entity.layer);
            let style = if entity.style.is_empty() {
                None
            } else {
                model.styles.find(&entity.style)
            };
            // A style an entity names but the table lacks is still counted
            // under its name - the manager has to be able to show it - but it
            // draws, and so reaches, nothing of its own.
            Slot {
                layer: entity.layer.clone(),
                style: (!entity.style.is_empty()).then(|| entity.style.clone()),
                linetype: resolved_linetype(layer, style).to_owned(),
                hatch: resolved_hatch_pattern(layer, style).to_owned(),
                symbol: style
                    .filter(|style| !style.symbol.is_empty())
                    .map(|style| style.symbol.clone()),
            }
        });
        let keep = options.entity_ids;
        users_of(&mut usage.layers, &slot.layer).count_entity(entity.id, keep);
        if let Some(style) = &slot.style {
            users_of(&mut usage.styles, style).count_entity(entity.id, keep);
        }
        users_of(&mut usage.linetypes, &slot.linetype).count_entity(entity.id, keep);
        users_of(&mut usage.hatch_patterns, &slot.hatch).count_entity(entity.id, keep);
        if let Some(symbol) = &slot.symbol {
            users_of(&mut usage.symbols, symbol).count_entity(entity.id, keep);
        }
    });

    if options.entity_ids {
        // for_each walks in id order today; sorting keeps "ascending" true
        // whatever the database's order becomes.
        for map in [
            &mut usage.styles,
            &mut usage.linetypes,
            &mut usage.symbols,
            &mut usage.hatch_patterns,
            &mut usage.layers,
        ] {
            for users in map.values_mut() {
                users.entity_ids.sort_unstable();
            }
        }
    }
    usage
}
