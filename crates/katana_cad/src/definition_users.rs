//! Who uses a library definition, by name: what is left naming nothing once
//! the definition is removed (include/katana/cad/definition_users.hpp;
//! src/katana_cad/customisation/definition_users.cpp).
//!
//! A linestyle or symbol definition lives in the session's customisation
//! (Document::style_library), and three things name it from outside:
//!
//! * a survey code rule, as its linestyle or as the symbol it draws;
//! * a style of the drawing, as its linetype or as its symbol;
//! * a layer of the drawing, as its linetype.
//!
//! A name is resolved when it is DRAWN (style_resolver), so removing a
//! definition breaks none of them: the rule and the style go on naming it, and
//! the line is drawn solid, or the point as a stand-in shape, with nothing
//! said. That is why a removal asks here first. CUSTOMISE REMOVE refuses a
//! definition this finds a user of until it is given FORCE, and the window's
//! definition editor shows the same list beside its Delete - one function, so
//! what a person reads in the window is what an agent reads in a reply.
//!
//! What counts as naming it is what the resolver draws with it
//! (find_definition): the name itself, and - only for a name the library does
//! not hold - the earlier name of a definition that was renamed, so a drawing
//! saved before the rename is not passed over.

use katana_entity::tables::is_built_in_symbol_name;
use katana_entity::{EnumName, StyleLibrary, SurveySection};

use crate::customisation_record::{
    RenamedDefinition, builtin_definition_renames, definition_name_now_in,
};
use crate::document::Document;

/// One survey code rule that names a definition. A rule has no identity but
/// its place (entity's survey_map.rs), so it is cited as everything else cites
/// one: by its index, with its key and what it is about.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DefinitionRule {
    pub index: usize, // into SurveyMap::rules()
    pub key: String,  // as written: "WM*"
    pub section: SurveySection,
    pub linestyle: bool, // SurveyRule::linestyle names it
    pub symbol: bool,    // SurveyRule::symbol names it
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DefinitionUsers {
    pub rules: Vec<DefinitionRule>,   // in the map's order
    pub linetype_styles: Vec<String>, // styles whose linetype names it, ascending
    pub symbol_styles: Vec<String>,   // styles whose symbol names it, ascending
    pub layers: Vec<String>,          // layers whose linetype names it, ascending
    /// What ELSE answers to the name, and so what its users are drawn with
    /// while no library definition does (style_resolver): the drawing's own
    /// Linetype table holds it (decision D2), and a line naming it is then
    /// dashed by that linetype, not drawn plain; or it is the name of a
    /// built-in symbol shape, and a point naming it is then drawn as that
    /// shape, not as a stand-in for a missing one. Neither is a user - empty()
    /// does not count them and describe() lists neither. They are what a
    /// removal hands the users to, and what a definition made under the name
    /// takes them from: a refusal that said "then drawn plain" of a name the
    /// drawing also holds would be untrue.
    pub drawing_linetype: bool,
    pub built_in_shape: bool,
}

impl DefinitionUsers {
    pub fn empty(&self) -> bool {
        self.rules.is_empty()
            && self.linetype_styles.is_empty()
            && self.symbol_styles.is_empty()
            && self.layers.is_empty()
    }

    /// A line a user, in the order above, for a refusal a person can act on:
    ///
    /// ```text
    ///   rule #0 WM* (feature) names it as its linestyle
    ///   rule #7 AC* (symbol) draws it as its symbol
    ///   style "Water" names it as its linetype
    ///   style "Marks" draws it as its symbol
    ///   layer "survey/services" names it as its linetype
    /// ```
    ///
    /// A rule naming it both ways says "as its linestyle and its symbol".
    pub fn describe(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for rule in &self.rules {
            // "rule #2 KT* (feature)", as an explanation and the lint cite a
            // rule.
            let how = if rule.linestyle && rule.symbol {
                "names it as its linestyle and its symbol"
            } else if rule.linestyle {
                "names it as its linestyle"
            } else {
                "draws it as its symbol"
            };
            lines.push(format!(
                "rule #{} {} ({}) {how}",
                rule.index,
                rule.key,
                rule.section.name()
            ));
        }
        for style in &self.linetype_styles {
            lines.push(user_line("style", style, "names it as its linetype"));
        }
        for style in &self.symbol_styles {
            lines.push(user_line("style", style, "draws it as its symbol"));
        }
        for layer in &self.layers {
            lines.push(user_line("layer", layer, "names it as its linetype"));
        }
        lines
    }

    /// The styles that name it, each ONCE and ascending, however each names
    /// it - the union of linetype_styles and symbol_styles: what a removal
    /// counts, and cites.
    pub fn styles(&self) -> Vec<String> {
        // Both lists are ascending, as the fields promise, so this is the
        // union in the same order (std::set_union).
        let (first, second) = (&self.linetype_styles, &self.symbol_styles);
        let mut named = Vec::with_capacity(first.len() + second.len());
        let (mut i, mut j) = (0, 0);
        while i < first.len() {
            if j == second.len() {
                named.extend(first[i..].iter().cloned());
                return named;
            }
            if second[j] < first[i] {
                named.push(second[j].clone());
                j += 1;
            } else {
                named.push(first[i].clone());
                if first[i] >= second[j] {
                    j += 1;
                }
                i += 1;
            }
        }
        named.extend(second[j..].iter().cloned());
        named
    }

    /// The same users as CUSTOMISE REMOVE's refusal cites them, a line each,
    /// in the order above, a style once:
    ///
    /// ```text
    ///   "TEST Valve": rule #1 AC* (symbol) names it
    ///   "TEST Valve": the drawing's style "Marks" names it
    ///   "TEST Valve": the drawing's layer "survey/marks" names it
    /// ```
    ///
    /// `name` is the definition's. The verb's words live here, beside
    /// describe()'s, so that the window's definition editor can tell that a
    /// refusal it shows already lists who names the definition, and does not
    /// say its own list a second time under it.
    pub fn cited(&self, name: &str) -> Vec<String> {
        let of = format!("\"{name}\": ");
        let mut lines = Vec::new();
        for rule in &self.rules {
            lines.push(format!(
                "{of}rule #{} {} ({}) names it",
                rule.index,
                rule.key,
                rule.section.name()
            ));
        }
        for style in self.styles() {
            lines.push(format!("{of}the drawing's style \"{style}\" names it"));
        }
        for layer in &self.layers {
            lines.push(format!("{of}the drawing's layer \"{layer}\" names it"));
        }
        lines
    }

    /// Something names it as a linetype - a rule's linestyle, a style's or a
    /// layer's linetype - or as a symbol: which of the two notes above
    /// matter.
    pub fn named_as_linetype(&self) -> bool {
        !self.linetype_styles.is_empty()
            || !self.layers.is_empty()
            || self.rules.iter().any(|rule| rule.linestyle)
    }

    pub fn named_as_symbol(&self) -> bool {
        !self.symbol_styles.is_empty() || self.rules.iter().any(|rule| rule.symbol)
    }
}

/// Whether `used` - a name a rule, a style or a layer gives - is drawn with
/// the definition `name`, as find_definition would answer: the name itself,
/// or, for a name the library has no definition of, the earlier name of a
/// definition renamed to `name`. A library that does define `used` draws that
/// definition, and so does not use this one.
fn names(library: &StyleLibrary, used: &str, name: &str, renamed: &[RenamedDefinition]) -> bool {
    if used == name {
        return true;
    }
    !used.is_empty() && !library.contains(used) && definition_name_now_in(used, renamed) == name
}

/// `style "Marks" draws it as its symbol`.
fn user_line(kind: &str, name: &str, how: &str) -> String {
    format!("{kind} \"{name}\" {how}")
}

/// Everything in `document` that names the definition `name`, with
/// builtin_definition_renames: the C++ two-argument `definitionUsers`.
pub fn definition_users(document: &Document, name: &str) -> DefinitionUsers {
    definition_users_in(document, name, builtin_definition_renames())
}

/// Everything in `document` that names the definition `name`, the earlier
/// names `renamed` knows counted. Any rule may carry a linestyle or a symbol,
/// whatever it is about (the model allows it, and SurveyMap::styles_referenced
/// reads them so), so every rule is asked. Names compare with regard to case,
/// as they do wherever one is resolved. Nothing for "": no name is not a name.
pub fn definition_users_in(
    document: &Document,
    name: &str,
    renamed: &[RenamedDefinition],
) -> DefinitionUsers {
    let mut users = DefinitionUsers::default();
    if name.is_empty() {
        return users;
    }
    let library = document.style_library();
    let uses = |used: &str| names(library, used, name, renamed);

    for (index, rule) in document.survey_map().rules().iter().enumerate() {
        let linestyle = uses(rule.linestyle.as_str());
        let symbol = rule
            .symbol
            .as_ref()
            .is_some_and(|symbol| uses(symbol.style.as_str()));
        if linestyle || symbol {
            users.rules.push(DefinitionRule {
                index,
                key: rule.key.clone(),
                section: rule.section,
                linestyle,
                symbol,
            });
        }
    }
    // Both tables are walked in name order, which is the order the lists
    // promise.
    let model = document.model();
    users.drawing_linetype = model.linetypes.contains(name);
    users.built_in_shape = is_built_in_symbol_name(name);
    model.styles.for_each(|style| {
        if uses(style.linetype.as_str()) {
            users.linetype_styles.push(style.name.clone());
        }
        if uses(style.symbol.as_str()) {
            users.symbol_styles.push(style.name.clone());
        }
    });
    for layer_name in model.layers.names() {
        if model
            .layers
            .find(&layer_name)
            .is_some_and(|layer| uses(layer.linetype.as_str()))
        {
            users.layers.push(layer_name);
        }
    }
    users
}
