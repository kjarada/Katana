//! "Apply to" and "Only those that match": the scope and filter controls every dialog that reads or
//! changes drawing data shows - the ONE set of controls and the ONE reading of them (CLAUDE.md
//! section 1.1; docs/cad.md, "Scope and filter: one grammar for every verb on drawing data"). The
//! port of customisation/scope_filter_widget.
//!
//! ```text
//! Apply to   the selection | what a view shows (the views open in the workspace, with "only
//!            what is on screen" for a plan view) | the checked layers (with or without their
//!            sublayers) | the whole drawing
//! Filter     types, layer and style patterns, colour, a property and its value, text, drawn
//!            only - each optional
//! ```
//!
//! It says what it holds in two forms, read from the same controls, so they agree wherever a line
//! can say what the controls hold - and where it cannot (a ':' in a property's name, a comma in a
//! layer's), `verb_words` refuses rather than write another filter:
//!
//! * `scope()` and `filter()`: the ModifyScope and ModifyFilter that match_entities takes - what
//!   Global Modify plans with;
//! * `verb_words()`: the same as the words of the shared grammar (format_scope_words), for a dialog
//!   that builds a verb line and hands it to the window's executor. A view is named by its id: VIEW
//!   <id> for what is on screen, VIEW <id> EXTENTS for everything it draws. The view list shows each
//!   view's id beside its title ("Plan 2 (VIEW 4)"): the number in a title counts the views of its
//!   kind, the id all of them.
//!
//! Every control's object name is the prefix given, then its part:
//!
//! ```text
//! <prefix>ScopeGroup, <prefix>FilterGroup      the two groups
//! <prefix>ScopeSelection, <prefix>ScopeView, <prefix>ScopeLayers, <prefix>ScopeDrawing
//! <prefix>View, <prefix>OnScreen               the view, only what is on screen
//! <prefix>Layers, <prefix>Sublayers            the layers to tick, with sublayers
//! <prefix>Type<Point|Line|...>                 the type boxes
//! <prefix>FilterLayer, <prefix>FilterStyle, <prefix>FilterColour, <prefix>FilterProperty,
//! <prefix>FilterValue, <prefix>FilterText, <prefix>DrawnOnly
//! ```
//!
//! and the widget itself is <prefix>Scope. Global Modify passes "globalModify"; the utilities
//! dialog passes "utility".
//!
//! An edit is reported (`ui` answers true) for every control a person or a driver changed, and for
//! every setter called since the last frame - as the C++'s signals fire for both - but not for a
//! reload, which refills the lists keeping what is ticked and chosen. It never touches the
//! Document; reload is given the model to read the layers from.

use std::collections::BTreeSet;
use std::sync::Arc;

use egui::Ui;
use katana_cad::global_modify::{ModifyFilter, ModifyScope, ScopeKind};
use katana_cad::layer_overrides::LayerOverrides;
use katana_cad::scope_verbs::{
    ScopeSource, ScopeWords, format_scope_words, parse_colour_or_by_layer,
};
use katana_cad::view_set::{NO_VIEW, ViewId, ViewSet};
use katana_cad::viewport_layout::ViewKind;
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::{ENTITY_TYPES, to_string};
use katana_entity::{EntityType, Model};
use katana_geometry::Box2;

use crate::drive::controls::{Choice, LineOptions, check, choice, line_with, radio};
use crate::drive::mailbox::{Act, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, record, settle, take};

/// One open view, as the View scope offers it: its id, its title, its own hidden layers and, for a
/// plan view, its area on screen. Read at once by whoever asks for the list and never kept: the
/// view may be closed by the next frame.
#[derive(Clone, Debug, PartialEq)]
pub struct ScopeFilterView {
    pub id: ViewId,
    /// "Plan 1".
    pub title: String,
    /// The layers it hides of its own; None for a view that hides nothing of its own.
    pub hidden: Option<Arc<LayerOverrides>>,
    /// A plan view's visible area; None for a view with no plan extent.
    pub on_screen: Option<Box2>,
}

/// The open views of a workspace as the View scope offers them: each one's title, its own hidden
/// layers and, for a plan view, its area on screen. Read afresh each time: a view closed since has
/// no state to point at, and a plan view's area moves with every pan.
pub fn scope_filter_views(views: &ViewSet) -> Vec<ScopeFilterView> {
    views
        .views()
        .into_iter()
        .map(|state| ScopeFilterView {
            id: state.id,
            title: ViewSet::title(state),
            hidden: Some(Arc::new(state.layers.clone())),
            on_screen: (state.kind == ViewKind::Plan).then(|| state.plan.visible_world_bounds()),
        })
        .collect()
}

/// Which scope is chosen, in the order of the radio buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScopeChoice {
    #[default]
    Selection,
    View,
    Layers,
    Drawing,
}

/// One of the filter's text fields, for `set_field`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterField {
    Layer,
    Style,
    Colour,
    Property,
    Value,
    Text,
}

/// The views the widget asks for when it needs the list or a View scope.
pub type ViewsFn = Box<dyn Fn() -> Vec<ScopeFilterView>>;

/// A type box: the kind, whether it is ticked, and its label.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TypeBox {
    kind: EntityType,
    ticked: bool,
    label: String,
}

/// The scope and filter controls (see the module comment).
pub struct ScopeFilterWidget {
    prefix: String,
    /// The workspace's open views, asked whenever the view list or a View scope is needed. None
    /// (a test, or no workspace): the View scope offers one "Whole drawing view" hiding nothing,
    /// which `scope` reads and `verb_words` refuses - a line can name only a view that is open.
    pub views: Option<ViewsFn>,
    choice: ScopeChoice,
    /// The view list: each view's id and the text it shows.
    listed_views: Vec<(ViewId, String)>,
    view: Choice,
    on_screen: bool,
    /// The layer list: each layer's name and whether it is ticked.
    layers: Vec<(String, bool)>,
    sublayers: bool,
    types: Vec<TypeBox>,
    filter_layer: String,
    filter_style: String,
    filter_colour: String,
    filter_property: String,
    filter_value: String,
    filter_text: String,
    drawn_only: bool,
    /// A setter changed a control since the last frame: reported then, as its signal would be.
    pending: bool,
}

/// QString::trimmed, then the text: what a field says.
fn text_of(value: &str) -> String {
    String::from(value.trim())
}

/// A type box's label: the kind as TYPE= and the variant name it.
fn kind_name(kind: EntityType) -> String {
    String::from(to_string(kind))
}

fn no_area_on_screen(view: &ScopeFilterView) -> Error {
    make_error(
        ErrorCode::InvalidArgument,
        "only a plan view has an area on screen to limit to",
        view.title.as_str(),
    )
}

impl ScopeFilterWidget {
    /// The controls named from `name_prefix`: the selection chosen, sublayers ticked, nothing else.
    pub fn new(name_prefix: &str) -> Self {
        Self {
            prefix: String::from(name_prefix),
            views: None,
            choice: ScopeChoice::Selection,
            listed_views: Vec::new(),
            view: Choice::default(),
            on_screen: false,
            layers: Vec::new(),
            sublayers: true,
            // Every geometry kind, in the variant's order: counted off the variant, not listed,
            // so the window can filter to a label, a leader, a curve polyline, an ellipse and a
            // spline as TYPE= reaches them (a hand-written list once stopped at Dimension).
            types: ENTITY_TYPES
                .iter()
                .map(|&kind| TypeBox {
                    kind,
                    ticked: false,
                    label: kind_name(kind),
                })
                .collect(),
            filter_layer: String::new(),
            filter_style: String::new(),
            filter_colour: String::new(),
            filter_property: String::new(),
            filter_value: String::new(),
            filter_text: String::new(),
            drawn_only: false,
            pending: false,
        }
    }

    /// The widget's own object name: the prefix and "Scope".
    pub fn object_name(&self) -> String {
        format!("{}Scope", self.prefix)
    }

    fn named(&self, part: &str) -> String {
        format!("{}{part}", self.prefix)
    }

    /// What the View scope offers when `views` is None: "Whole drawing view", hiding nothing and
    /// with no area. For a dialog that keeps a views function of its own and forwards it.
    pub fn no_workspace_views() -> Vec<ScopeFilterView> {
        vec![ScopeFilterView {
            id: NO_VIEW,
            title: String::from("Whole drawing view"),
            hidden: None,
            on_screen: None,
        }]
    }

    fn open_views(&self) -> Vec<ScopeFilterView> {
        match &self.views {
            Some(views) => views(),
            None => Self::no_workspace_views(),
        }
    }

    /// Refills the layer list from `model` and the view list from `views`, keeping what is ticked
    /// and chosen. Not an edit: nothing is reported for it.
    pub fn reload(&mut self, model: &Model) {
        let ticked: BTreeSet<String> = self
            .layers
            .iter()
            .filter(|(_, on)| *on)
            .map(|(name, _)| name.clone())
            .collect();
        self.layers = model
            .layers
            .names()
            .into_iter()
            .map(|name| {
                let on = ticked.contains(&name);
                (name, on)
            })
            .collect();
        self.reload_views();
    }

    /// The view list alone: for a dialog with no drawing to hand, whose views still come from the
    /// window. Keeps the view chosen when it is still listed; else the first is chosen, as a list
    /// refilled chooses its first item.
    pub fn reload_views(&mut self) {
        let kept = self.chosen_view_id();
        self.listed_views = self
            .open_views()
            .into_iter()
            .map(|open| {
                // Each open view by its title and by the id a line names it by, which is not the
                // number in its title: "Plan 2 (VIEW 4)".
                let shown = if open.id == NO_VIEW {
                    open.title.clone()
                } else {
                    format!("{} (VIEW {})", open.title, open.id)
                };
                (open.id, shown)
            })
            .collect();
        let found = kept.and_then(|id| {
            self.listed_views
                .iter()
                .position(|(listed, _)| *listed == id)
        });
        self.view.index = found.or(if self.listed_views.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    /// The id of the view chosen in the list; None when the list is empty.
    fn chosen_view_id(&self) -> Option<ViewId> {
        self.view
            .index
            .and_then(|at| self.listed_views.get(at))
            .map(|(id, _)| *id)
    }

    /// The view list's items as it shows them.
    pub fn view_items(&self) -> Vec<String> {
        self.listed_views
            .iter()
            .map(|(_, shown)| shown.clone())
            .collect()
    }

    /// The layer list's names, in its order (ascending).
    pub fn layer_names(&self) -> Vec<String> {
        self.layers.iter().map(|(name, _)| name.clone()).collect()
    }

    /// The layers ticked, in the list's order.
    pub fn ticked_layers(&self) -> Vec<String> {
        self.layers
            .iter()
            .filter(|(_, on)| *on)
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn choice(&self) -> ScopeChoice {
        self.choice
    }

    /// Chooses a scope, as a click on its radio button does.
    pub fn set_choice(&mut self, choice: ScopeChoice) {
        if self.choice != choice {
            self.choice = choice;
            self.pending = true;
        }
    }

    /// Ticks or clears layer `name` in the list, as its box does. Nothing for a name not listed.
    pub fn tick_layer(&mut self, name: &str, on: bool) {
        if let Some((_, ticked)) = self.layers.iter_mut().find(|(listed, _)| listed == name)
            && *ticked != on
        {
            *ticked = on;
            self.pending = true;
        }
    }

    /// Chooses the view at `index` in the list (QComboBox::setCurrentIndex); out of range is
    /// ignored.
    pub fn set_view_index(&mut self, index: usize) {
        if index < self.listed_views.len() && self.view.index != Some(index) {
            self.view.index = Some(index);
            self.pending = true;
        }
    }

    pub fn set_on_screen(&mut self, on: bool) {
        if self.on_screen != on {
            self.on_screen = on;
            self.pending = true;
        }
    }

    pub fn set_sublayers(&mut self, on: bool) {
        if self.sublayers != on {
            self.sublayers = on;
            self.pending = true;
        }
    }

    pub fn set_type(&mut self, kind: EntityType, on: bool) {
        if let Some(type_box) = self.types.iter_mut().find(|type_box| type_box.kind == kind)
            && type_box.ticked != on
        {
            type_box.ticked = on;
            self.pending = true;
        }
    }

    pub fn set_drawn_only(&mut self, on: bool) {
        if self.drawn_only != on {
            self.drawn_only = on;
            self.pending = true;
        }
    }

    fn field_mut(&mut self, field: FilterField) -> &mut String {
        match field {
            FilterField::Layer => &mut self.filter_layer,
            FilterField::Style => &mut self.filter_style,
            FilterField::Colour => &mut self.filter_colour,
            FilterField::Property => &mut self.filter_property,
            FilterField::Value => &mut self.filter_value,
            FilterField::Text => &mut self.filter_text,
        }
    }

    /// Sets one of the filter's fields, as typing does.
    pub fn set_field(&mut self, field: FilterField, text: &str) {
        let slot = self.field_mut(field);
        if slot.as_str() != text {
            *slot = String::from(text);
            self.pending = true;
        }
    }

    /// The view chosen, as it is open now.
    fn chosen_view(&self) -> Result<ScopeFilterView> {
        // An empty list is the C++'s invalid current data, read as no view.
        let id = self.chosen_view_id().unwrap_or(NO_VIEW);
        self.open_views()
            .into_iter()
            .find(|open| open.id == id)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::NotFound,
                    "that view is no longer open; choose another",
                )
            })
    }

    /// What the controls say as the scope match_entities takes. Fails naming the field for what
    /// does not read: no layer ticked, a view that has closed, "only what is on screen" for a view
    /// with no plan extent.
    pub fn scope(&self) -> Result<ModifyScope> {
        let mut scope = ModifyScope::default();
        match self.choice {
            ScopeChoice::Selection => scope.kind = ScopeKind::Selection,
            ScopeChoice::Drawing => scope.kind = ScopeKind::Drawing,
            ScopeChoice::Layers => {
                scope.kind = ScopeKind::Layers;
                scope.sublayers = self.sublayers;
                scope.layers = self
                    .layers
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(name, _)| text_of(name))
                    .collect();
                if scope.layers.is_empty() {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        "tick at least one layer to apply to",
                    ));
                }
            }
            ScopeChoice::View => {
                scope.kind = ScopeKind::View;
                let found = self.chosen_view()?;
                scope.view = found.hidden.clone();
                if self.on_screen {
                    let Some(area) = found.on_screen else {
                        return Err(no_area_on_screen(&found));
                    };
                    scope.area = Some(area);
                }
            }
        }
        Ok(scope)
    }

    /// What the controls say as the filter match_entities takes. Fails for a colour that is not
    /// #RRGGBB or ByLayer, read by the grammar's own reading of a colour.
    pub fn filter(&self) -> Result<ModifyFilter> {
        let mut filter = ModifyFilter::default();
        for type_box in &self.types {
            if type_box.ticked {
                filter.types.insert(type_box.kind);
            }
        }
        // Qt::SkipEmptyParts: an empty part is passed over, then each part trimmed.
        for part in self.filter_layer.split(',').filter(|part| !part.is_empty()) {
            let pattern = text_of(part);
            if !pattern.is_empty() {
                filter.layers.push(pattern);
            }
        }
        let style = text_of(&self.filter_style);
        if !style.is_empty() {
            filter.style = Some(style);
        }
        let typed = text_of(&self.filter_colour);
        if !typed.is_empty() {
            // The grammar's own reading of a colour, so the words and the controls cannot take
            // different colours.
            let colour = parse_colour_or_by_layer(&typed).map_err(|_| {
                make_error(
                    ErrorCode::InvalidArgument,
                    "the filter's colour: type #RRGGBB or ByLayer",
                    typed.as_str(),
                )
            })?;
            filter.colour = Some(colour);
        }
        let key = text_of(&self.filter_property);
        if !key.is_empty() {
            filter.property = Some(key);
        }
        let value = text_of(&self.filter_value);
        if !value.is_empty() {
            filter.property_value = Some(value);
        }
        let words = text_of(&self.filter_text);
        if !words.is_empty() {
            filter.text = Some(words);
        }
        filter.drawn_only = self.drawn_only;
        Ok(filter)
    }

    /// The same as plain words (ScopeWords). Fails as `scope` and `filter` do, and for the
    /// fallback view, which is no view a line can name.
    pub fn words(&self) -> Result<ScopeWords> {
        let mut words = ScopeWords {
            filter: self.filter()?,
            ..ScopeWords::default()
        };
        match self.choice {
            ScopeChoice::Selection => words.source = ScopeSource::Selection,
            ScopeChoice::Drawing => words.source = ScopeSource::Drawing,
            ScopeChoice::Layers => {
                let layers = self.scope()?;
                words.source = ScopeSource::Layers;
                words.layers = layers.layers;
                words.sublayers = layers.sublayers;
            }
            ScopeChoice::View => {
                let found = self.chosen_view()?;
                if found.id == NO_VIEW {
                    return Err(Error::new(
                        ErrorCode::InvalidState,
                        "no view is open to name in a line; choose another scope",
                    ));
                }
                words.source = ScopeSource::View;
                words.view = Some(found.id);
                // On screen: the view's area, which the window reads when the line runs.
                // Otherwise the area is dropped - its layers anywhere.
                if self.on_screen {
                    if found.on_screen.is_none() {
                        return Err(no_area_on_screen(&found));
                    }
                } else {
                    words.extents = true;
                }
            }
        }
        Ok(words)
    }

    /// The words as a line takes them (format_scope_words). Fails as `words` does, and as
    /// format_scope_words does for what a line cannot say (a comma in a layer name, a double
    /// quote).
    pub fn verb_words(&self) -> Result<String> {
        format_scope_words(&self.words()?)
    }

    /// Puts beside each type box how many of that kind `counts` holds - "Leader (774)", "Arc (0)"
    /// for a kind it lacks - so a person sees what the scope holds before ticking one. The boxes
    /// keep their object names and what they filter.
    pub fn show_type_counts(&mut self, counts: &[(EntityType, usize)]) {
        for type_box in &mut self.types {
            let count = counts
                .iter()
                .find(|(kind, _)| *kind == type_box.kind)
                .map_or(0, |(_, count)| *count);
            type_box.label = format!("{} ({count})", kind_name(type_box.kind));
        }
    }

    /// Takes the numbers off again: the plain kind names.
    pub fn hide_type_counts(&mut self) {
        for type_box in &mut self.types {
            type_box.label = kind_name(type_box.kind);
        }
    }

    /// Draws the controls; true when any was edited this frame - by a person or a driver - or by a
    /// setter since the last frame.
    pub fn ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = std::mem::take(&mut self.pending);
        let outer = ui.vertical(|ui| {
            // ---- Apply to ----
            let scope_group = ui.group(|ui| {
                ui.label("Apply to");
                let choices = [
                    (
                        "ScopeSelection",
                        "Selected entities",
                        ScopeChoice::Selection,
                    ),
                    ("ScopeView", "What a view shows", ScopeChoice::View),
                    ("ScopeLayers", "The checked layers", ScopeChoice::Layers),
                    ("ScopeDrawing", "The whole drawing", ScopeChoice::Drawing),
                ];
                for (part, label, choice_of) in choices {
                    let name = self.named(part);
                    ui.horizontal(|ui| {
                        let chosen = self.choice == choice_of;
                        if radio(ui, &name, label, chosen).clicked() && !chosen {
                            self.choice = choice_of;
                            changed = true;
                        }
                        match choice_of {
                            ScopeChoice::View => {
                                ui.add_enabled_ui(self.choice == ScopeChoice::View, |ui| {
                                    let items = self.view_items();
                                    let name = self.named("View");
                                    if choice(ui, &name, &items, &mut self.view).changed() {
                                        changed = true;
                                    }
                                });
                            }
                            ScopeChoice::Layers => {
                                ui.add_enabled_ui(self.choice == ScopeChoice::Layers, |ui| {
                                    let name = self.named("Sublayers");
                                    if check(ui, &name, "With their sublayers", &mut self.sublayers)
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                });
                            }
                            ScopeChoice::Selection | ScopeChoice::Drawing => {}
                        }
                    });
                    match choice_of {
                        ScopeChoice::View => {
                            ui.add_enabled_ui(self.choice == ScopeChoice::View, |ui| {
                                let name = self.named("OnScreen");
                                if check(ui, &name, "Only what is on screen", &mut self.on_screen)
                                    .on_hover_text(
                                        "A plan view: only what lies in the area it shows now",
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                        }
                        ScopeChoice::Layers => {
                            ui.add_enabled_ui(self.choice == ScopeChoice::Layers, |ui| {
                                if self.layer_list(ui) {
                                    changed = true;
                                }
                            });
                        }
                        ScopeChoice::Selection | ScopeChoice::Drawing => {}
                    }
                }
            });
            let group_name = self.named("ScopeGroup");
            crate::drive::controls::custom(
                ui,
                &scope_group.response,
                ControlRecord::new(&group_name, ControlKind::Other),
            );

            // ---- Only those that match ----
            let filter_group = ui.group(|ui| {
                ui.label("Only those that match (optional)");
                ui.label("Types (none: all)");
                let prefix = self.prefix.clone();
                let types = &mut self.types;
                for row in types.chunks_mut(4) {
                    ui.horizontal(|ui| {
                        for type_box in row {
                            let name = format!("{prefix}Type{}", kind_name(type_box.kind));
                            if check(ui, &name, &type_box.label, &mut type_box.ticked).changed() {
                                changed = true;
                            }
                        }
                    });
                }
                let wild = " - * and ? are wildcards";
                let fields = [
                    (
                        "Layer",
                        "FilterLayer",
                        "e.g. survey/*, roads",
                        format!("Layer paths, comma separated{wild}"),
                    ),
                    (
                        "Style",
                        "FilterStyle",
                        "a style, or ByLayer",
                        format!("The style worn; ByLayer for none{wild}"),
                    ),
                    (
                        "Colour",
                        "FilterColour",
                        "#RRGGBB or ByLayer",
                        String::new(),
                    ),
                ];
                for (label, part, placeholder, tip) in fields {
                    let name = self.named(part);
                    let field = match part {
                        "FilterLayer" => &mut self.filter_layer,
                        "FilterStyle" => &mut self.filter_style,
                        _ => &mut self.filter_colour,
                    };
                    ui.horizontal(|ui| {
                        ui.label(label);
                        let mut response = line_with(
                            ui,
                            &name,
                            field,
                            LineOptions {
                                placeholder,
                                ..LineOptions::default()
                            },
                        );
                        if !tip.is_empty() {
                            response = response.on_hover_text(tip.as_str());
                        }
                        if response.changed() {
                            changed = true;
                        }
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Property");
                    let key = self.named("FilterProperty");
                    if line_with(
                        ui,
                        &key,
                        &mut self.filter_property,
                        LineOptions {
                            placeholder: "key",
                            ..LineOptions::default()
                        },
                    )
                    .changed()
                    {
                        changed = true;
                    }
                    let value = self.named("FilterValue");
                    if line_with(
                        ui,
                        &value,
                        &mut self.filter_value,
                        LineOptions {
                            placeholder: "value, e.g. TREE*",
                            ..LineOptions::default()
                        },
                    )
                    .on_hover_text(format!("The value as the properties panel prints it{wild}"))
                    .changed()
                    {
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Text");
                    let name = self.named("FilterText");
                    if line_with(
                        ui,
                        &name,
                        &mut self.filter_text,
                        LineOptions {
                            placeholder: "a text's words, e.g. CH *",
                            ..LineOptions::default()
                        },
                    )
                    .changed()
                    {
                        changed = true;
                    }
                });
                let name = self.named("DrawnOnly");
                if check(
                    ui,
                    &name,
                    "Only what is drawn (not hidden)",
                    &mut self.drawn_only,
                )
                .changed()
                {
                    changed = true;
                }
            });
            let group_name = self.named("FilterGroup");
            crate::drive::controls::custom(
                ui,
                &filter_group.response,
                ControlRecord::new(&group_name, ControlKind::Other),
            );
        });
        let name = self.object_name();
        crate::drive::controls::custom(
            ui,
            &outer.response,
            ControlRecord::new(&name, ControlKind::Other),
        );
        changed
    }

    /// The layer list, each layer a box to tick: a list (QListWidget) named <prefix>Layers. A
    /// fill makes the row of that name current, as fillField's does, which ticks nothing. True
    /// when a box was ticked or cleared.
    fn layer_list(&mut self, ui: &mut Ui) -> bool {
        let name = self.named("Layers");
        let taken = take(ui, &name);
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => {
                    if ui.is_visible() && !ui.is_enabled() {
                        settle(
                            ui,
                            taken,
                            Err(crate::drive::mailbox::fill_disabled(&taken.posted)),
                        );
                    } else if self.layers.iter().any(|(listed, _)| listed == text) {
                        settle(ui, taken, Ok(()));
                    } else {
                        settle(ui, taken, Err(lists_no(&name, text)));
                    }
                }
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }
        let mut changed = false;
        let response = egui::ScrollArea::vertical()
            .id_salt(name.as_str())
            .min_scrolled_height(90.0)
            .max_height(90.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (layer, on) in &mut self.layers {
                    if ui.checkbox(on, layer.as_str()).changed() {
                        changed = true;
                    }
                }
            })
            .inner_rect;
        let response = ui.interact(response, ui.id().with(name.as_str()), egui::Sense::hover());
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        let mut list = ControlRecord::new(&name, ControlKind::Rows);
        list.items = self.layers.iter().map(|(layer, _)| layer.clone()).collect();
        list.enabled = ui.is_enabled();
        list.visible = ui.is_visible();
        record(ui, &response, list);
        changed
    }
}
