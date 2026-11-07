//! Named resource tables that entities refer to by name: layers, styles,
//! linetypes, dimension styles, hatch patterns, alignments and the property
//! definitions.
//!
//! Tables are ordered by name for deterministic iteration and are not
//! thread-safe. Cross-table rules (e.g. "a layer in use cannot be removed") are
//! enforced by commands, which can see every table.

use std::collections::BTreeMap;
use std::ops::Bound;

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_core::text::{equals_ignoring_case, format_exact_real, lowered};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::profile::solve_profile;
use katana_geometry::{HorizontalAlignment, VerticalAlignment};
use katana_math::numerics::tolerance;

use crate::entity::{ArrowHead, Color, EnumName, PropertyValue, to_string};
use crate::entity_geometry::Validate;
use crate::layer_path::{
    LAYER_SEPARATOR, is_layer_under, layer_ancestors, layer_depth, layer_parent,
    rewrite_layer_prefix, validate_layer_path,
};
use crate::named_table::{Named, NamedMap, NamedTable, NamedTablePolicy};
use crate::scalar;
use crate::validation::validate_name;

pub const DEFAULT_LAYER_NAME: &str = "0";
pub const CONTINUOUS_LINETYPE: &str = "continuous";

/// The built-in hatch pattern that draws no fill. Named rather than
/// represented by an empty string so that "this layer is not hatched" and
/// "this layer has not said" stay distinguishable, exactly as "continuous"
/// does for linetypes.
pub const NO_HATCH: &str = "none";

fn validate_line_weight(line_weight: f64) -> Status {
    if !(line_weight.is_finite() && line_weight >= 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "line weight must be finite and not negative",
            format_exact_real(line_weight),
        ));
    }
    Ok(())
}

/// A layer's own fields, its name apart (validate_layer_path). The C++ also
/// refused a linetype, hatch pattern or dimension style name that is not
/// UTF-8 (audit MOD-12), which a `String` cannot be.
fn validate_layer_fields(layer: &Layer) -> Status {
    validate_line_weight(layer.line_weight)
}

// ---- LayerDatabase ---------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub color: Color,
    pub visible: bool,
    /// Locked layers are drawn but their entities cannot be edited.
    pub locked: bool,
    pub linetype: String,
    /// Millimetres on paper.
    pub line_weight: f64,
    /// Empty means the document default ("Standard"). Appended last, as every
    /// field here is, so the storage columns keep their order.
    pub dimension_style: String,
    pub hatch_pattern: String,
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            name: String::from(DEFAULT_LAYER_NAME),
            color: Color::default(),
            visible: true,
            locked: false,
            linetype: String::from(CONTINUOUS_LINETYPE),
            line_weight: 0.25,
            dimension_style: String::new(),
            hatch_pattern: String::from(NO_HATCH),
        }
    }
}

impl Layer {
    /// A layer of `name` with every other field at its default: what an
    /// ancestor created for a nested layer is.
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }
}

/// What drawing and editing need to know about a layer, found with one
/// lookup: the layer itself, and whether it is shown and locked once its
/// ancestors are counted. A layer that is not there is neither shown nor
/// editable, so an entity that points at one is not silently drawn or edited.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedLayer<'a> {
    pub layer: Option<&'a Layer>,
    pub shown: bool,
    pub locked: bool,
}

impl Default for ResolvedLayer<'_> {
    fn default() -> Self {
        Self {
            layer: None,
            shown: false,
            locked: true,
        }
    }
}

/// A layer with its inherited state. `shown` and `locked` are derived, and
/// kept current by every member that changes the table, so a reader asks one
/// lookup instead of walking the path.
#[derive(Clone, Debug, PartialEq)]
struct Node {
    layer: Layer,
    shown: bool,
    locked: bool,
}

impl Node {
    fn new(layer: Layer) -> Self {
        Self {
            layer,
            shown: true,
            locked: false,
        }
    }
}

/// Always contains layer "0", which can be modified but never removed or
/// renamed.
///
/// Layer names are PATHS (layer_path.rs). Adding "design/surface/tin1" also
/// creates "design" and "design/surface" if they are missing, so every node
/// in the tree is a real layer; visibility and lock INHERIT down the tree.
/// Colour and linetype do not inherit - ByLayer already resolves those.
#[derive(Clone, Debug)]
pub struct LayerDatabase {
    layers: BTreeMap<String, Node>,
    revision: u64,
}

impl Default for LayerDatabase {
    fn default() -> Self {
        Self::new()
    }
}

impl LayerDatabase {
    pub fn new() -> Self {
        let mut database = Self {
            layers: BTreeMap::new(),
            revision: 0,
        };
        database.reset();
        database
    }

    /// Restores the pristine state: only the default layer.
    pub fn reset(&mut self) {
        self.layers.clear();
        self.layers.insert(
            String::from(DEFAULT_LAYER_NAME),
            Node::new(Layer::default()),
        );
        self.refresh_subtree(DEFAULT_LAYER_NAME);
        self.revision += 1;
    }

    /// Everything under `name` is exactly the keys beginning "name/", and those
    /// are contiguous: they sort from "name/" up to "name0", '0' being the
    /// character after '/'. NOT the keys straight after `name` up to the first
    /// that is not a descendant: a sibling whose name continues with a
    /// character below '/' sorts BETWEEN "design" and "design/surface".
    fn descendant_keys(&self, name: &str) -> Vec<String> {
        let mut from = name.to_owned();
        from.push(LAYER_SEPARATOR);
        let mut to = name.to_owned();
        to.push(char::from(b'/' + 1));
        self.layers
            .range::<str, _>((Bound::Included(from.as_str()), Bound::Excluded(to.as_str())))
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn refresh(&mut self, path: &str) {
        let parent = layer_parent(path);
        // A parent is a proper prefix of its child, so it sorts first and is
        // already current by the time the child is reached.
        let above = if parent.is_empty() {
            None
        } else {
            self.layers
                .get(parent)
                .map(|node| (node.shown, node.locked))
        };
        if let Some(node) = self.layers.get_mut(path) {
            node.shown = node.layer.visible;
            node.locked = node.layer.locked;
            if let Some((shown, locked)) = above {
                node.shown = node.shown && shown;
                node.locked = node.locked || locked;
            }
        }
    }

    /// Recomputes the inherited state of `name` and everything under it,
    /// parents first. A layer's state depends only on itself and its
    /// ancestors, so changing one layer can move only its own subtree.
    fn refresh_subtree(&mut self, name: &str) {
        if !self.layers.contains_key(name) {
            return;
        }
        self.refresh(name);
        for path in self.descendant_keys(name) {
            self.refresh(&path);
        }
    }

    /// The layer and its inherited state, in one lookup.
    pub fn resolve(&self, name: &str) -> ResolvedLayer<'_> {
        match self.layers.get(name) {
            Some(node) => ResolvedLayer {
                layer: Some(&node.layer),
                shown: node.shown,
                locked: node.locked,
            },
            None => ResolvedLayer::default(),
        }
    }

    /// What add() would refuse, with the same error, changing nothing.
    pub fn check_add(&self, layer: &Layer) -> Status {
        validate_layer_path(&layer.name)?;
        validate_layer_fields(layer)?;
        if self.contains(&layer.name) {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "layer already exists",
                layer.name.as_str(),
            ));
        }
        Ok(())
    }

    /// What update() would refuse, with the same error, changing nothing.
    pub fn check_update(&self, layer: &Layer) -> Status {
        if !self.contains(&layer.name) {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                layer.name.as_str(),
            ));
        }
        validate_layer_fields(layer)
    }

    /// What remove() would refuse, with the same error, changing nothing.
    pub fn check_remove(&self, name: &str) -> Status {
        if name == DEFAULT_LAYER_NAME {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the default layer cannot be removed",
            ));
        }
        if !self.contains(name) {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                name,
            ));
        }
        if self.has_children(name) {
            // Deleting the branch silently, or leaving its children pointing at a
            // parent that is gone, are both worse than saying so.
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "layer still has nested layers; remove the subtree instead",
                name,
            ));
        }
        Ok(())
    }

    /// Creates `layer` and any missing ancestors (with default properties).
    pub fn add(&mut self, layer: Layer) -> Status {
        self.check_add(&layer)?;
        // Every ancestor is created as a real layer, so each node in the tree
        // has its own colour, visibility and lock. Validation happened above
        // and covers the ancestors too, so nothing here can fail partway.
        let ancestors = layer_ancestors(&layer.name);
        for ancestor in &ancestors {
            self.layers
                .entry(ancestor.clone())
                .or_insert_with(|| Node::new(Layer::named(ancestor)));
        }
        let name = layer.name.clone();
        // check_add refused a name that is there, so this inserts.
        self.layers.insert(name.clone(), Node::new(layer));
        // From the topmost ancestor: one created just now is as new as the
        // layer, and refreshing from the root of the path covers both.
        let top = ancestors.first().cloned().unwrap_or(name);
        self.refresh_subtree(&top);
        self.revision += 1;
        Ok(())
    }

    /// The same, but succeeds and changes nothing when the layer already
    /// exists. What an importer wants: "make sure this path exists".
    pub fn ensure(&mut self, name: &str) -> Status {
        validate_layer_path(name)?;
        if self.contains(name) {
            return Ok(());
        }
        self.add(Layer::named(name))
    }

    /// Matched by name.
    pub fn update(&mut self, layer: &Layer) -> Status {
        self.check_update(layer)?;
        if let Some(found) = self.layers.get_mut(&layer.name) {
            found.layer = layer.clone();
        }
        self.refresh_subtree(&layer.name);
        self.revision += 1;
        Ok(())
    }

    /// Removes ONE layer. Fails with InvalidArgument when it still has
    /// children. Use remove_subtree to take the branch.
    pub fn remove(&mut self, name: &str) -> Result<Layer> {
        self.check_remove(name)?;
        let removed = self.layers.remove(name).map(|node| node.layer);
        self.revision += 1;
        // A leaf: nothing inherited from it.
        removed.ok_or_else(|| make_error(ErrorCode::NotFound, "layer does not exist", name))
    }

    pub fn find(&self, name: &str) -> Option<&Layer> {
        self.layers.get(name).map(|node| &node.layer)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// Ascending.
    pub fn names(&self) -> Vec<String> {
        self.layers.keys().cloned().collect()
    }

    /// Ascending by name.
    pub fn all(&self) -> Vec<Layer> {
        self.layers
            .values()
            .map(|node| node.layer.clone())
            .collect()
    }

    pub fn size(&self) -> usize {
        self.layers.len()
    }

    // ---- the tree ---------------------------------------------------------
    //
    // Every one of these is a range scan rather than a search, because the map
    // is keyed by full path.

    /// Top-level layers, ascending.
    pub fn roots(&self) -> Vec<String> {
        self.children("")
    }

    /// Immediate children of `name`, ascending. Empty for a leaf or a missing
    /// layer. Pass an empty name for the roots.
    pub fn children(&self, name: &str) -> Vec<String> {
        let wanted = layer_depth(name) + 1;
        self.layers
            .keys()
            .filter(|path| is_layer_under(path, name) && path.as_str() != name)
            .filter(|path| layer_depth(path) == wanted)
            .cloned()
            .collect()
    }

    /// `name` and everything beneath it, in pre-order. Empty when absent.
    pub fn subtree(&self, name: &str) -> Vec<String> {
        if !self.contains(name) {
            return Vec::new();
        }
        self.layers
            .keys()
            .filter(|path| is_layer_under(path, name))
            .cloned()
            .collect()
    }

    pub fn has_children(&self, name: &str) -> bool {
        // Any key beginning "name/". This used to test only the key straight
        // after `name`, on the belief that a child sorts immediately after its
        // parent - but "design 2" sorts between "design" and "design/surface".
        let mut prefix = name.to_owned();
        prefix.push(LAYER_SEPARATOR);
        self.layers
            .range::<str, _>((Bound::Included(prefix.as_str()), Bound::Unbounded))
            .next()
            .is_some_and(|(key, _)| key.starts_with(&prefix))
    }

    /// resolve(name).shown, for a caller with only a name to ask. A layer that
    /// is gone is neither shown nor editable.
    pub fn effectively_visible(&self, name: &str) -> bool {
        self.resolve(name).shown
    }

    pub fn effectively_locked(&self, name: &str) -> bool {
        self.resolve(name).locked
    }

    /// Removes `name` and everything under it, deepest first, and returns what
    /// was removed in that order so an undo can put it back parents-first by
    /// walking the result backwards.
    pub fn remove_subtree(&mut self, name: &str) -> Result<Vec<Layer>> {
        if name == DEFAULT_LAYER_NAME {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the default layer cannot be removed",
                name,
            ));
        }
        if !self.contains(name) {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                name,
            ));
        }
        let paths = self.subtree(name);
        // Deepest first, so a caller replaying the list in reverse recreates
        // parents before children.
        let mut removed = Vec::with_capacity(paths.len());
        for path in paths.iter().rev() {
            if let Some(node) = self.layers.remove(path) {
                removed.push(node.layer);
            }
        }
        self.revision += 1;
        Ok(removed)
    }

    /// Renames `from` and every descendant to sit under `to`. Returns the
    /// (old, new) pairs so the caller can move the entities that reference
    /// them; nothing is changed if any target name is already taken.
    pub fn rename_subtree(&mut self, from: &str, to: &str) -> Result<Vec<(String, String)>> {
        if from == DEFAULT_LAYER_NAME {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the default layer cannot be renamed",
                from,
            ));
        }
        validate_layer_path(to)?;
        if !self.contains(from) {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                from,
            ));
        }
        if from == to {
            return Ok(Vec::new());
        }
        if is_layer_under(to, from) {
            // "design" -> "design/old" would have to be its own descendant.
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a layer cannot be moved inside itself",
                format!("{from} -> {to}"),
            ));
        }

        let mapping: Vec<(String, String)> = self
            .subtree(from)
            .into_iter()
            .map(|path| {
                let renamed = rewrite_layer_prefix(&path, from, to);
                (path, renamed)
            })
            .collect();

        // Every target is checked BEFORE anything moves, so a collision leaves
        // the tree exactly as it was rather than half renamed.
        for (_, after) in &mapping {
            if self.contains(after) && !is_layer_under(after, from) {
                return Err(make_error(
                    ErrorCode::AlreadyExists,
                    "a layer of that name already exists",
                    after.as_str(),
                ));
            }
        }
        // The new parents have to exist too, and creating them cannot be
        // allowed to fail halfway either, so they are validated first.
        for ancestor in layer_ancestors(to) {
            validate_layer_path(&ancestor)?;
        }

        let mut moved = Vec::with_capacity(mapping.len());
        for (before, after) in &mapping {
            if let Some(node) = self.layers.get(before) {
                let mut layer = node.layer.clone();
                layer.name = after.clone();
                moved.push(layer);
            }
        }
        for (before, _) in &mapping {
            self.layers.remove(before);
        }
        let new_ancestors = layer_ancestors(to);
        for ancestor in &new_ancestors {
            self.layers
                .entry(ancestor.clone())
                .or_insert_with(|| Node::new(Layer::named(ancestor)));
        }
        for layer in moved {
            self.layers.insert(layer.name.clone(), Node::new(layer));
        }
        // The moved branch inherits from its new parents now, some perhaps just
        // created: refresh from the topmost of them.
        let top = new_ancestors
            .first()
            .cloned()
            .unwrap_or_else(|| to.to_owned());
        self.refresh_subtree(&top);
        self.revision += 1;
        Ok(mapping)
    }

    /// Counts up by one for every change that succeeds and never down.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

// ---- ArrowHead --------------------------------------------------------------------

impl EnumName for ArrowHead {
    fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Tick => "Tick",
            Self::ClosedFilled => "ClosedFilled",
            Self::Open => "Open",
            Self::Dot => "Dot",
        }
    }
}

pub fn arrow_head_from_string(name: &str) -> Result<ArrowHead> {
    for head in [
        ArrowHead::None,
        ArrowHead::Tick,
        ArrowHead::ClosedFilled,
        ArrowHead::Open,
        ArrowHead::Dot,
    ] {
        if equals_ignoring_case(to_string(head).as_bytes(), name.as_bytes()) {
            return Ok(head);
        }
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        "unknown arrow head",
        name,
    ))
}

// ---- DimensionStyle ---------------------------------------------------------------

pub const DEFAULT_DIMENSION_STYLE_NAME: &str = "Standard";

/// Everything that decides how a dimension is drawn and what its text says.
/// Lengths here are MODEL units, unlike Layer::line_weight which is
/// millimetres on paper.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionStyle {
    pub name: String,
    /// DIMTXT.
    pub text_height: f64,
    /// DIMGAP: dimension line to the text.
    pub text_gap: f64,
    /// DIMEXO: gap between the measured point and the start of its extension line.
    pub extension_offset: f64,
    /// DIMEXE: how far the extension line runs past the dimension line.
    pub extension_beyond: f64,
    /// DIMASZ.
    pub arrow_size: f64,
    pub arrow_head: ArrowHead,
    /// Multiplies the measurement before it is shown (DIMLFAC). A factor and a
    /// suffix rather than a unit enum because the entity layer cannot see the
    /// geodesy layer, and a second unit table would be a second definition of
    /// a foot.
    pub unit_scale: f64,
    pub prefix: String,
    pub suffix: String,
    /// DIMDEC.
    pub decimals: i32,
    /// Rounds the scaled measurement to a multiple of this before formatting
    /// (DIMRND). 0 disables it.
    pub round_to: f64,
    /// DIMZIN.
    pub suppress_trailing_zeros: bool,
    /// The sizes above are then MILLIMETRES ON PAPER (an annotative dimension
    /// style).
    pub paper_sized: bool,
}

impl Default for DimensionStyle {
    fn default() -> Self {
        Self {
            name: String::from(DEFAULT_DIMENSION_STYLE_NAME),
            text_height: 2.5,
            text_gap: 0.625,
            extension_offset: 0.625,
            extension_beyond: 1.25,
            arrow_size: 2.5,
            arrow_head: ArrowHead::ClosedFilled,
            unit_scale: 1.0,
            prefix: String::new(),
            suffix: String::new(),
            decimals: 3,
            round_to: 0.0,
            suppress_trailing_zeros: false,
            paper_sized: false,
        }
    }
}

impl Named for DimensionStyle {
    fn name(&self) -> &str {
        &self.name
    }
}

/// Fails with InvalidArgument, naming the specific rule broken. (The C++ also
/// refused a prefix or suffix that is not UTF-8, which a `String` cannot be.)
impl Validate for DimensionStyle {
    fn validate(&self) -> Status {
        validate_name(&self.name, "dimension style")?;

        // Each of these gets its own sentence rather than one "invalid style":
        // the user set one number and needs to know which.
        for (what, value) in [
            ("text height", self.text_height),
            ("arrow size", self.arrow_size),
            ("unit scale", self.unit_scale),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{what} must be finite and greater than zero"),
                    self.name.as_str(),
                ));
            }
        }
        for (what, value) in [
            ("text gap", self.text_gap),
            ("extension offset", self.extension_offset),
            ("extension overshoot", self.extension_beyond),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{what} must be finite and not negative"),
                    self.name.as_str(),
                ));
            }
        }

        // Twelve is where a double stops having digits to give: a measurement
        // of a few thousand metres has about sixteen significant digits.
        if !(0..=12).contains(&self.decimals) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "decimals must be between 0 and 12",
                format!("{} has {}", self.name, self.decimals),
            ));
        }

        if !self.round_to.is_finite() || self.round_to < 0.0 {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the rounding step must not be negative",
                self.name.as_str(),
            ));
        }
        // No new epsilon: rounding to less than the geometric tolerance is
        // rounding to noise, and a tiny step makes value/step overflow before
        // it rounds.
        if self.round_to > 0.0 && self.round_to < tolerance::GEOMETRIC {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the rounding step is finer than the geometric tolerance",
                self.name.as_str(),
            ));
        }
        Ok(())
    }
}

/// Always contains "Standard", which can be neither removed nor renamed,
/// exactly as layer "0" and the continuous linetype cannot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DimensionStylePolicy;

impl NamedTablePolicy<DimensionStyle> for DimensionStylePolicy {
    const NOUN: &'static str = "dimension style";
    fn validate(item: &DimensionStyle) -> Status {
        item.validate()
    }
    fn is_protected(name: &str) -> bool {
        name == DEFAULT_DIMENSION_STYLE_NAME
    }
    fn seed(items: &mut NamedMap<DimensionStyle>) {
        let standard = DimensionStyle::default();
        items.insert(standard.name.clone(), standard);
    }
}

pub type DimensionStyleDatabase = NamedTable<DimensionStyle, DimensionStylePolicy>;

// ---- linetypes ------------------------------------------------------------------
//
// A dash pattern, in MODEL units. The sign convention is DXF's (AutoCAD DXF
// Reference, LTYPE table entry, group code 49): a positive length is a dash
// (pen down), a negative length is a gap (pen up), and exactly zero is a dot.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LinetypeElement {
    pub length: f64,
}

impl LinetypeElement {
    pub const fn is_dash(&self) -> bool {
        self.length > 0.0
    }
    pub const fn is_gap(&self) -> bool {
        self.length < 0.0
    }
    pub const fn is_dot(&self) -> bool {
        self.length == 0.0
    }
    /// Pen down covers both a dash and a dot.
    pub const fn is_pen_down(&self) -> bool {
        self.length >= 0.0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Linetype {
    pub name: String,
    /// DXF group 3, e.g. "Dashed  __  __  __".
    pub description: String,
    /// Empty means continuous. Otherwise the elements must strictly alternate
    /// pen-down and pen-up, start with a dash or a dot, and end with a gap.
    pub pattern: Vec<LinetypeElement>,
}

impl Linetype {
    /// The period, in model units: the sum of the ABSOLUTE element lengths
    /// (DXF group 40). Absolute, because a gap is stored negative and still
    /// occupies its length along the path.
    pub fn pattern_length(&self) -> f64 {
        let mut total = 0.0;
        for element in &self.pattern {
            total += element.length.abs();
        }
        total
    }

    pub fn is_continuous(&self) -> bool {
        self.pattern.is_empty()
    }

    /// Shortest non-dot element. Dots are excluded: a dot has no length by
    /// definition, so including it would make every pattern containing one
    /// report zero and be judged unresolvable at any zoom.
    pub fn shortest_element(&self) -> f64 {
        let mut shortest = f64::INFINITY;
        for element in &self.pattern {
            if element.is_dot() {
                continue;
            }
            shortest = scalar::min(shortest, element.length.abs());
        }
        if shortest.is_finite() { shortest } else { 0.0 }
    }
}

impl Named for Linetype {
    fn name(&self) -> &str {
        &self.name
    }
}

/// Fails with InvalidArgument, naming the specific rule broken. (The C++ also
/// refused a description that is not UTF-8, which a `String` cannot be.)
impl Validate for Linetype {
    fn validate(&self) -> Status {
        validate_name(&self.name, "linetype")?;
        // BYLAYER and BYBLOCK are the values DXF group code 6 takes on an
        // entity, so they can never name a definition.
        for reserved in ["ByLayer", "ByBlock"] {
            if equals_ignoring_case(self.name.as_bytes(), reserved.as_bytes()) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "that linetype name is reserved by the exchange format",
                    self.name.as_str(),
                ));
            }
        }
        let (Some(first), Some(last)) = (self.pattern.first(), self.pattern.last()) else {
            return Ok(()); // continuous
        };

        if self
            .pattern
            .iter()
            .any(|element| !element.length.is_finite())
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "linetype element is not finite",
                self.name.as_str(),
            ));
        }
        // Alignment 'A' (DXF group 72, always 65) fits the pattern so a line
        // begins with a dash, so the first element cannot be a gap.
        if first.is_gap() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a linetype must begin with a dash or a dot, not a gap",
                self.name.as_str(),
            ));
        }
        // Ending on a gap closes the period. Without it the pattern would place
        // two pen-down runs adjacently when it repeats, which is
        // indistinguishable from one longer dash.
        if !last.is_gap() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a linetype must end with a gap",
                self.name.as_str(),
            ));
        }
        for i in 1..self.pattern.len() {
            if self.pattern[i].is_pen_down() == self.pattern[i - 1].is_pen_down() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "linetype elements must alternate between pen down and pen up",
                    format!("{} at element {}", self.name, i),
                ));
            }
        }
        if !(self.pattern_length() > 0.0) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "linetype pattern has no length",
                self.name.as_str(),
            ));
        }
        Ok(())
    }
}

/// Always contains "continuous" (an empty pattern), which can be neither
/// removed nor renamed, exactly as layer "0" cannot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinetypePolicy;

impl NamedTablePolicy<Linetype> for LinetypePolicy {
    const NOUN: &'static str = "linetype";
    fn validate(item: &Linetype) -> Status {
        item.validate()
    }
    /// The continuous linetype exists so that every entity has something to
    /// resolve to. Giving it a pattern would change what an unset ByLayer
    /// chain draws as across the whole document, so it is refused on update
    /// as well as on remove.
    fn check_update(item: &Linetype) -> Status {
        if item.name == CONTINUOUS_LINETYPE && !item.is_continuous() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the continuous linetype cannot be given a pattern",
            ));
        }
        Ok(())
    }
    fn is_protected(name: &str) -> bool {
        name == CONTINUOUS_LINETYPE
    }
    fn seed(items: &mut NamedMap<Linetype>) {
        // Only "continuous" is built in. No "dashed", "center" or "hidden" is
        // seeded, deliberately: AutoCAD's acad.lin set is in imperial drawing
        // units and its ISO set is in millimetres, while a survey drawing here
        // is in metres - so any table shipped would be either a conversion of
        // numbers not to hand or an invention (docs/model.md).
        let continuous = Linetype {
            name: String::from(CONTINUOUS_LINETYPE),
            description: String::from("Solid line"),
            pattern: Vec::new(),
        };
        items.insert(continuous.name.clone(), continuous);
    }
}

pub type LinetypeDatabase = NamedTable<Linetype, LinetypePolicy>;

/// The named symbols a point may be drawn with. NO_SYMBOL draws the plain
/// point mark of the viewport.
pub const NO_SYMBOL: &str = "";

/// The set a viewport can paint. Names as the survey packages use them: a
/// "TOPO Natural Surface Point" style, for instance, is a cross.
const SYMBOL_NAMES: [&str; 16] = [
    "circle", "square", "triangle", "diamond", "cross", "plus", "tick", "star", "dot", "ring",
    "tree", "pole", "manhole", "arrow", "flag", "target",
];

/// The symbols Katana draws with no library loaded. NOT the set of names a
/// style may use: a loaded symbol library adds hundreds more, so this is what
/// a chooser offers, not what validation allows.
pub fn symbol_names() -> &'static [&'static str] {
    &SYMBOL_NAMES
}

pub fn is_built_in_symbol_name(name: &str) -> bool {
    if name == NO_SYMBOL {
        return true;
    }
    symbol_names().contains(&name)
}

/// The built-in shape that best fits a name, for a name nothing defines: a
/// linestyle with "Manhole" in it is drawn as a manhole rather than as
/// nothing. Always one of symbol_names(); "circle" when nothing matches.
pub fn built_in_symbol_for(name: &str) -> &'static str {
    // ASCII, whatever the locale.
    let key = String::from_utf8_lossy(&lowered(name)).into_owned();
    let has = |word: &str| key.contains(word);
    // Most specific first: "Pole - Light" is a pole, "Suspended Light" a
    // light, "Gully Pit Point" a pit and not a point.
    if has("tree") || has("shrub") || has("palm") {
        return "tree";
    }
    if has("manhole") || has("pit") || has("chamber") || has("sump") {
        return "manhole";
    }
    if has("pole") || has("post") || has("column") || has("pier") {
        return "pole";
    }
    if has("mark") || has("station") || has("bench") || has("stns") || has("control") {
        return "target";
    }
    if has("sign") || has("flag") {
        return "flag";
    }
    if has("light") || has("lamp") || has("lantern") {
        return "star";
    }
    if has("valve") || has("hydrant") || has("tap") || has("meter") {
        return "diamond";
    }
    if has("bollard") || has("peg") || has("nail") || has("spike") {
        return "dot";
    }
    if has("point")
        || has("spot")
        || has("surface")
        || has("level")
        || has("invert")
        || has("obvert")
    {
        return "cross";
    }
    "circle"
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub name: String,
    /// Empty: ByLayer.
    pub color: Option<Color>,
    /// Millimetres on paper.
    pub line_weight: f64,
    pub linetype: String,
    /// Empty means ByLayer. Unlike the layer field this is NOT defaulted to
    /// "none": a style that says nothing about hatching must not override a
    /// layer that does.
    pub hatch_pattern: String,
    /// What the style is for, or where it came from.
    pub description: String,
    /// How a POINT in this style is drawn: a name RESOLVED when it is drawn,
    /// against the loaded symbol library first and symbol_names() after, or
    /// NO_SYMBOL for the viewport's plain mark.
    pub symbol: String,
    /// The symbol's width in model units, or 0 for the viewport's default.
    pub symbol_size: f64,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            name: String::new(),
            color: None,
            line_weight: 0.25,
            linetype: String::from(CONTINUOUS_LINETYPE),
            hatch_pattern: String::new(),
            description: String::new(),
            symbol: String::from(NO_SYMBOL),
            symbol_size: 0.0,
        }
    }
}

impl Named for Style {
    fn name(&self) -> &str {
        &self.name
    }
}

/// Fails with InvalidArgument for a bad name, or a non-finite or negative line
/// weight or symbol size.
///
/// A symbol name and a linetype are RESOLVED when the style is drawn, and not
/// checked against a closed set here: a project can load a symbol library,
/// and a library linestyle lives in the library, not the model. "ByLayer"
/// (display.rs) is accepted like any other name. The C++ checked only their
/// encoding, which a `String` keeps.
impl Validate for Style {
    fn validate(&self) -> Status {
        validate_name(&self.name, "style")?;
        validate_line_weight(self.line_weight)?;
        if !(self.symbol_size.is_finite() && self.symbol_size >= 0.0) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "symbol size must be finite and not negative",
                format_exact_real(self.symbol_size),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StylePolicy;

impl NamedTablePolicy<Style> for StylePolicy {
    const NOUN: &'static str = "style";
    fn validate(item: &Style) -> Status {
        item.validate()
    }
}

pub type StyleDatabase = NamedTable<Style, StylePolicy>;

// ---- hatch patterns -------------------------------------------------------------
//
// One family of parallel lines. A pattern is a list of these, which is how DXF
// PAT files describe hatching and why a crosshatch is two families rather than
// a special kind of one. Angles are radians counter-clockwise from +x; spacing
// and offset are MODEL units, the offset measured from the world origin.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HatchLineFamily {
    pub angle: f64,
    pub spacing: f64,
    pub offset: f64,
}

impl Default for HatchLineFamily {
    fn default() -> Self {
        Self {
            angle: 0.0,
            spacing: 1.0,
            offset: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HatchPattern {
    pub name: String,
    pub description: String,
    /// A filled area rather than a family of lines. DXF SOLID hatches import
    /// as this.
    pub solid: bool,
    pub families: Vec<HatchLineFamily>,
}

impl HatchPattern {
    /// The built-in "none" is the only pattern allowed to draw nothing.
    pub fn draws_nothing(&self) -> bool {
        !self.solid && self.families.is_empty()
    }
}

impl Named for HatchPattern {
    fn name(&self) -> &str {
        &self.name
    }
}

/// Fails with InvalidArgument, naming the specific rule broken. (The C++ also
/// refused a description that is not UTF-8, which a `String` cannot be.)
impl Validate for HatchPattern {
    fn validate(&self) -> Status {
        validate_name(&self.name, "hatch pattern")?;
        if self.solid && !self.families.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a solid hatch pattern cannot also carry line families",
                self.name.as_str(),
            ));
        }
        // A pattern that draws nothing is almost always a half-finished
        // definition, and it would be invisible on the drawing with no error
        // anywhere. "none" is the one pattern whose purpose is to draw nothing.
        if self.draws_nothing() && self.name != NO_HATCH {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a hatch pattern must be solid or have at least one line family",
                self.name.as_str(),
            ));
        }
        for family in &self.families {
            if !family.angle.is_finite() || !family.offset.is_finite() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "hatch family angle and offset must be finite",
                    self.name.as_str(),
                ));
            }
            if !(family.spacing > 0.0) || !family.spacing.is_finite() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "hatch family spacing must be a positive length",
                    self.name.as_str(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HatchPatternPolicy;

impl NamedTablePolicy<HatchPattern> for HatchPatternPolicy {
    const NOUN: &'static str = "hatch pattern";
    fn validate(item: &HatchPattern) -> Status {
        item.validate()
    }
    /// "none" is what an unhatched layer resolves to, so giving it a fill
    /// would hatch every layer that has never asked to be hatched.
    fn check_update(item: &HatchPattern) -> Status {
        if item.name == NO_HATCH && !item.draws_nothing() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the \"none\" hatch pattern cannot be given a fill",
            ));
        }
        Ok(())
    }
    fn is_protected(name: &str) -> bool {
        name == NO_HATCH
    }
    fn seed(items: &mut NamedMap<HatchPattern>) {
        // Only "none" is built in, for the same reason only "continuous" is:
        // the ISO and ANSI pattern sets are defined in paper millimetres and
        // inches, and a survey drawing here is in metres.
        let none = HatchPattern {
            name: String::from(NO_HATCH),
            description: String::from("Not hatched"),
            solid: false,
            families: Vec::new(),
        };
        items.insert(none.name.clone(), none);
    }
}

pub type HatchPatternDatabase = NamedTable<HatchPattern, HatchPatternPolicy>;

// ---- alignments -----------------------------------------------------------------
//
// A named horizontal alignment: the plan centreline a section is cut along, a
// profile is drawn against and a corridor is built around. A NAMED TABLE and
// not a geometry alternative: it is a definition other things reference by
// name. What is stored is the PI definition; the elements are derived on
// demand by solve_alignment, so a stored alignment cannot be discontinuous.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Alignment {
    pub name: String,
    pub description: String,
    pub horizontal: HorizontalAlignment,
    /// The design grade line along it, when one has been designed.
    pub vertical: Option<VerticalAlignment>,
}

impl Named for Alignment {
    fn name(&self) -> &str {
        &self.name
    }
}

/// Fails with InvalidArgument for a bad name, and with whatever
/// solve_alignment or solve_profile reports for a definition that cannot be
/// built - so an alignment that would fail to draw is refused on the way in,
/// naming the PI or PVI.
impl Validate for Alignment {
    fn validate(&self) -> Status {
        validate_name(&self.name, "alignment")?;
        // An alignment with no PIs is a name with nothing behind it. Two is
        // the least that has a direction; solve_alignment says so itself, and
        // its message is the one the user should read.
        if let Err(error) = solve_alignment(&self.horizontal) {
            let detail = if error.context.is_empty() {
                String::new()
            } else {
                format!(": {}", error.context)
            };
            return Err(make_error(
                error.code,
                error.message,
                format!("alignment {}{}", self.name, detail),
            ));
        }
        if let Some(vertical) = &self.vertical
            && let Err(error) = solve_profile(vertical)
        {
            let detail = if error.context.is_empty() {
                String::new()
            } else {
                format!(": {}", error.context)
            };
            return Err(make_error(
                error.code,
                error.message,
                format!("alignment {} profile{}", self.name, detail),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AlignmentPolicy;

impl NamedTablePolicy<Alignment> for AlignmentPolicy {
    const NOUN: &'static str = "alignment";
    fn validate(item: &Alignment) -> Status {
        item.validate()
    }
}

pub type AlignmentDatabase = NamedTable<Alignment, AlignmentPolicy>;

// ---- property definitions -------------------------------------------------------

/// The kinds of PropertyValue, numbered as its alternatives are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PropertyType {
    Boolean,
    Integer,
    Real,
    #[default]
    Text,
}

pub fn type_of(value: &PropertyValue) -> PropertyType {
    match value {
        PropertyValue::Boolean(_) => PropertyType::Boolean,
        PropertyValue::Integer(_) => PropertyType::Integer,
        PropertyValue::Real(_) => PropertyType::Real,
        PropertyValue::Text(_) => PropertyType::Text,
    }
}

/// Schema entry for an entity property. Defining a property makes it typed:
/// values of the wrong type are rejected before they reach an entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertyDefinition {
    pub name: String,
    pub r#type: PropertyType,
    pub description: String,
    pub default_value: Option<PropertyValue>,
}

/// Registry of property definitions. Properties without a definition are
/// free-form and always accepted.
#[derive(Clone, Debug, Default)]
pub struct PropertyDatabase {
    definitions: BTreeMap<String, PropertyDefinition>,
    revision: u64,
}

impl PropertyDatabase {
    /// (The C++ also refused a description that is not UTF-8, which a
    /// `String` cannot be.)
    pub fn define(&mut self, definition: PropertyDefinition) -> Status {
        validate_name(&definition.name, "property")?;
        if self.find(&definition.name).is_some() {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "property is already defined",
                definition.name.as_str(),
            ));
        }
        if let Some(default_value) = &definition.default_value
            && type_of(default_value) != definition.r#type
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "default value does not match the property type",
                definition.name.as_str(),
            ));
        }
        self.definitions.insert(definition.name.clone(), definition);
        self.revision += 1;
        Ok(())
    }

    pub fn undefine(&mut self, name: &str) -> Result<PropertyDefinition> {
        match self.definitions.remove(name) {
            Some(removed) => {
                self.revision += 1;
                Ok(removed)
            }
            None => Err(make_error(
                ErrorCode::NotFound,
                "property is not defined",
                name,
            )),
        }
    }

    pub fn reset(&mut self) {
        self.definitions.clear();
        self.revision += 1;
    }

    pub fn find(&self, name: &str) -> Option<&PropertyDefinition> {
        self.definitions.get(name)
    }

    /// Ascending by name.
    pub fn all(&self) -> Vec<PropertyDefinition> {
        self.definitions.values().cloned().collect()
    }

    /// Ok when `name` is undefined, or defined with the type of `value`. (The
    /// C++ also refused a name or a text value that is not UTF-8, which its
    /// JSON writer would throw on; a `&str` cannot be one.)
    pub fn validate(&self, name: &str, value: &PropertyValue) -> Status {
        if let Some(definition) = self.find(name)
            && type_of(value) != definition.r#type
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "property value has the wrong type",
                name,
            ));
        }
        if let PropertyValue::Real(real) = value
            && !real.is_finite()
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "property value is not finite",
                name,
            ));
        }
        Ok(())
    }

    /// Counts up by one for every change that succeeds and never down.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}
