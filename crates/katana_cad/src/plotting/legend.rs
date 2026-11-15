//! The legend a sheet prints: what its plans actually SHOW (docs/plotting.md,
//! "The smart legend"; include/katana/cad/plotting/legend.hpp).
//!
//! A legend that lists every layer of the drawing tells the reader about
//! things that are not on the sheet, and a sheet strung along a road names
//! the whole survey on every page. This gathers the legend from what the plan
//! viewports show instead:
//!
//! ```text
//! which entities   those drawn (a visible entity on a layer the document
//!                  shows) that a plan viewport does not hide, and whose
//!                  shape meets the viewport's window on the ground - the
//!                  ROTATED rectangle a twisted viewport shows, not the box
//!                  around it, so a strip along a road does not list the
//!                  pipe beside the next strip;
//! grouped by       what prints: an entity drawn in a style (a library
//!                  linestyle or symbol, or the style a survey code gave it)
//!                  by that style, any other by its layer; and within that by
//!                  what kind of mark it is - a symbol, a plain point, a
//!                  line, a hatched area or a text - since a point and a line
//!                  of one layer print differently;
//! labelled         by the description the survey code library gives the
//!                  code the entities carry, when every coded one agrees on
//!                  it, else by the style's or the layer's name. A style's
//!                  own description is not used: in practice it holds a
//!                  colour name or the import's provenance, not words for
//!                  a reader;
//! drawn with       the look most of the group's entities resolve to (the
//!                  resolve_display chain), ties to the one met at the
//!                  lowest entity id;
//! in the order     symbols, points, lines, areas, texts, each by label with
//!                  letter case folded.
//! ```
//!
//! The SCOPE is the Legend viewport's own (Viewport::legend_scope): this
//! sheet's plans, every plan of the set, or the whole drawing. A sheet with
//! no plan lists what the set's plans show, and a set with no plan the whole
//! drawing. Key plans are not counted: they show the drawing faded, as a map
//! of the sheets.
//!
//! Everything here is a pure function of the model, the sheet set and the
//! options, so the painter, a test and an agent get the same list; the
//! painter draws it.
//!
//! LegendScope's names (`toString(LegendScope)`, `legendScopeFrom`) are
//! sheet_set.rs's, with the enum.

use std::collections::{BTreeMap, BTreeSet};

use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::display::{ResolvedDisplay, resolve_display};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::tables::{CONTINUOUS_LINETYPE, NO_HATCH, NO_SYMBOL};
use katana_entity::{
    Color, Entity, EntityId, EnumName, Geometry, LineworkCodes, Model, ResolvedLayer, SurveyMap,
};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::chording::{chord_arc, chord_circle};
use katana_geometry::{Box2, Point2, SpatialIndex};
use katana_math::cpp;

use crate::document::Document;
use crate::hatching::resolve_hatch_pattern_of_display;
use crate::layer_overrides::LayerOverrides;
use crate::linework::parse_field_code;
use crate::plot::sheet_scale_at_least;
use crate::plotting::sheet_commands::edit_viewport;
use crate::plotting::sheet_set::{
    LegendScope, PlanWindow, Sheet, SheetSet, Viewport, ViewportKind,
};
use crate::selection::drawn_extent;
use crate::spatial_query::for_each_candidate;
use crate::survey_coding::{find_code_property, survey_code_of};

/// What kind of mark an entry's sample is, in the order the legend lists
/// them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LegendKind {
    /// A point drawn with a symbol.
    Symbol,
    /// A point drawn as the plain mark.
    Point,
    /// A line, arc, polyline or circle; with its style's symbol at each
    /// vertex.
    #[default]
    Line,
    /// A closed polyline filled or hatched.
    Area,
    Text,
}

/// "symbol", "point", "line", "area", "text": the C++ `toString(LegendKind)`.
impl EnumName for LegendKind {
    fn name(self) -> &'static str {
        match self {
            LegendKind::Symbol => "symbol",
            LegendKind::Point => "point",
            LegendKind::Line => "line",
            LegendKind::Area => "area",
            LegendKind::Text => "text",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LegendEntry {
    pub kind: LegendKind,
    pub label: String,
    /// The style the entities are drawn in, for an entry grouped by style;
    /// empty for one grouped by layer.
    pub style: String,
    /// The layer, for an entry grouped by layer; empty for one grouped by
    /// style.
    pub layer: String,
    /// The survey code whose description is the label; empty when the label
    /// is a name.
    pub code: String,
    /// How it prints, as ResolvedDisplay gives it (before the paper colour
    /// rule, which the painter applies).
    pub colour: Color,
    /// Millimetres on paper.
    pub line_weight: f64,
    pub linetype: String,
    pub symbol: String,
    /// Model units; 0 for the definition's or the mark's own.
    pub symbol_size: f64,
    pub hatch_pattern: String,
    /// The entities it stands for: each counted once, however many plans
    /// show it.
    pub count: usize,
}

impl Default for LegendEntry {
    fn default() -> Self {
        Self {
            kind: LegendKind::Line,
            label: String::new(),
            style: String::new(),
            layer: String::new(),
            code: String::new(),
            colour: Color::default(),
            line_weight: 0.25,
            linetype: String::from(CONTINUOUS_LINETYPE),
            symbol: String::from(NO_SYMBOL),
            symbol_size: 0.0,
            hatch_pattern: String::from(NO_HATCH),
            count: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Legend {
    /// What the entries were gathered from once a sheet or set without plans
    /// has fallen back: ThisSheet, WholeSet or WholeDrawing.
    pub scope: LegendScope,
    /// The plan viewports looked through (0 for the whole drawing).
    pub windows: usize,
    /// 1 : scale, what a sample sized on the ground (a symbol in metres, a
    /// world linestyle) is drawn at: the first plan on the legend's sheet,
    /// else the first plan the entries came from; 0 when there is none, and
    /// the painter uses the legend viewport's own scale.
    pub scale: f64,
    pub entries: Vec<LegendEntry>,
}

/// How an automatic plan's window is decided, by a caller that has its own
/// rule (the painter's, which counts imagery and meshes too): the C++
/// `std::function<PlanWindow(const Viewport&)>`.
pub type PlanResolver<'a> = dyn Fn(&Viewport) -> PlanWindow + 'a;

#[derive(Clone, Default)]
pub struct LegendOptions<'a> {
    /// How an automatic plan's window is decided; None: fitted_plan_window.
    /// The painter passes its own, so a legend lists what the plan beside it
    /// drew.
    pub window: Option<&'a PlanResolver<'a>>,
    /// Speeds up a plan that shows part of a large drawing; the answer is the
    /// same without it.
    pub index: Option<&'a SpatialIndex>,
    /// The survey code library whose descriptions label coded entities; None
    /// or empty: every label is a name.
    pub codes: Option<&'a SurveyMap>,
    /// The property holding an entity's code; empty: the first of
    /// code_property_candidates() any entity carries (find_code_property).
    pub code_property: String,
}

// ---- the window a plan shows ------------------------------------------------------

// A plan viewport's window on the ground: its rectangle at its scale, about
// its centre, turned by its rotation. Tests are made in the window's own
// frame, where the rectangle is axis-aligned: x along the paper, y up it.
struct Window<'v> {
    hidden: &'v LayerOverrides,
    centre: Point2,
    cosine: f64,
    sine: f64,
    // Metres.
    half_width: f64,
    half_height: f64,
    // The rotated rectangle's box on the ground: the broad phase.
    reach: Box2,
}

impl Window<'_> {
    // A ground point in the window's frame.
    fn local(&self, p: Point2) -> Point2 {
        let dx = p.x - self.centre.x;
        let dy = p.y - self.centre.y;
        Point2::new(
            self.cosine * dx + self.sine * dy,
            -self.sine * dx + self.cosine * dy,
        )
    }

    fn world(&self, x: f64, y: f64) -> Point2 {
        Point2::new(
            self.centre.x + self.cosine * x - self.sine * y,
            self.centre.y + self.sine * x + self.cosine * y,
        )
    }

    fn holds(&self, p: Point2, reach: f64) -> bool {
        let l = self.local(p);
        l.x.abs() <= self.half_width + reach && l.y.abs() <= self.half_height + reach
    }

    // Whether the segment a-b crosses the rectangle: Liang-Barsky clipping in
    // the window's frame, which keeps a part of it or none.
    fn meets_segment(&self, a: Point2, b: Point2) -> bool {
        let p = self.local(a);
        let q = self.local(b);
        let dx = q.x - p.x;
        let dy = q.y - p.y;
        let mut t0 = 0.0;
        let mut t1 = 1.0;
        let mut edge = |direction: f64, room: f64| {
            if direction == 0.0 {
                return room >= 0.0;
            }
            let t = room / direction;
            if direction < 0.0 {
                t0 = cpp::max(t0, t);
            } else {
                t1 = cpp::min(t1, t);
            }
            t0 <= t1
        };
        edge(-dx, p.x + self.half_width)
            && edge(dx, self.half_width - p.x)
            && edge(-dy, p.y + self.half_height)
            && edge(dy, self.half_height - p.y)
    }

    fn meets_points(&self, points: &[Point2], closed: bool) -> bool {
        if points.len() == 1 {
            return self.holds(points[0], 0.0);
        }
        for pair in points.windows(2) {
            if self.meets_segment(pair[0], pair[1]) {
                return true;
            }
        }
        closed && points.len() > 2 && self.meets_segment(points[points.len() - 1], points[0])
    }

    // Whether a box on the ground meets the rotated rectangle: two rectangles
    // are apart exactly when one of their four edge directions separates them.
    fn meets_box(&self, other: &Box2) -> bool {
        if !other.intersects(self.reach) {
            return false;
        }
        let mut turned = Box2::default();
        for corner in [
            other.min,
            other.max,
            Point2::new(other.min.x, other.max.y),
            Point2::new(other.max.x, other.min.y),
        ] {
            turned.expand(self.local(corner));
        }
        turned.min.x <= self.half_width
            && turned.max.x >= -self.half_width
            && turned.min.y <= self.half_height
            && turned.max.y >= -self.half_height
    }
}

fn window_of<'v>(viewport: &'v Viewport, at: &PlanWindow) -> Option<Window<'v>> {
    if viewport.rect.empty()
        || !(at.scale > 0.0)
        || !at.scale.is_finite()
        || !at.centre.is_finite()
        || !viewport.rotation.is_finite()
    {
        return None;
    }
    let mut window = Window {
        hidden: &viewport.hidden_layers,
        centre: at.centre,
        cosine: viewport.rotation.cos(),
        sine: viewport.rotation.sin(),
        // Paper millimetres to metres on the ground.
        half_width: 0.5 * viewport.rect.width() * at.scale / 1000.0,
        half_height: 0.5 * viewport.rect.height() * at.scale / 1000.0,
        reach: Box2::default(),
    };
    let mut reach = Box2::default();
    for x in [-window.half_width, window.half_width] {
        for y in [-window.half_height, window.half_height] {
            reach.expand(window.world(x, y));
        }
    }
    window.reach = reach;
    Some(window)
}

// ---- what an entity prints as -----------------------------------------------------

// What an entity's look depends on, besides its geometry: resolved once per
// distinct layer, style and colour, as the plan painter does.
struct Resolved<'m> {
    layer: ResolvedLayer<'m>,
    display: ResolvedDisplay,
    // It names a style the drawing has.
    styled: bool,
    // Its hatch pattern draws something.
    hatched: bool,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ResolvedKey {
    layer: String,
    style: String,
    colour: Option<u32>,
}

fn packed(colour: &Color) -> u32 {
    (u32::from(colour.r) << 24)
        | (u32::from(colour.g) << 16)
        | (u32::from(colour.b) << 8)
        | u32::from(colour.a)
}

struct Resolver<'m> {
    model: &'m Model,
    resolved: BTreeMap<ResolvedKey, Resolved<'m>>,
}

impl<'m> Resolver<'m> {
    fn new(model: &'m Model) -> Self {
        Self {
            model,
            resolved: BTreeMap::new(),
        }
    }

    fn get(&mut self, entity: &Entity) -> &Resolved<'m> {
        let key = ResolvedKey {
            layer: entity.layer.clone(),
            style: entity.style.clone(),
            colour: entity.color.as_ref().map(packed),
        };
        let model = self.model;
        self.resolved.entry(key).or_insert_with(|| {
            let display = resolve_display(model, entity);
            let hatched = resolve_hatch_pattern_of_display(model, &display).is_some();
            Resolved {
                layer: model.layers.resolve(&entity.layer),
                display,
                styled: !entity.style.is_empty() && model.styles.find(&entity.style).is_some(),
                hatched,
            }
        })
    }
}

// The mark an entity prints as; none for a dimension, which is annotation a
// legend does not explain.
fn kind_of(entity: &Entity, resolved: &Resolved<'_>) -> Option<LegendKind> {
    match &entity.geometry {
        Geometry::Point(_) => Some(if resolved.display.symbol.is_empty() {
            LegendKind::Point
        } else {
            LegendKind::Symbol
        }),
        Geometry::Text(_) => Some(LegendKind::Text),
        Geometry::Dimension(_) => None,
        // Only a closed polyline is filled or hatched (the plan painter's
        // rule).
        Geometry::Polyline(polyline)
            if polyline.closed && polyline.vertices.len() >= 3 && resolved.hatched =>
        {
            Some(LegendKind::Area)
        }
        _ => Some(LegendKind::Line),
    }
}

// Whether what `entity` prints reaches into `window`. By its shape, not its
// box: a pipe running diagonally past a twisted strip has a box that meets
// the strip and a line that never does. A hatched area that covers the whole
// window prints inside it with no edge there; a symbol reaches half its size
// beyond its point.
fn shows(window: &Window<'_>, entity: &Entity, resolved: &Resolved<'_>) -> bool {
    // Arcs and circles are chorded to a thousandth of the window, far finer
    // than the question needs.
    let tolerance = cpp::max(cpp::min(window.half_width, window.half_height) * 1e-3, 1e-6);
    match &entity.geometry {
        Geometry::Point(point) => {
            let reach = if resolved.display.symbol.is_empty() {
                0.0
            } else {
                0.5 * resolved.display.symbol_size
            };
            window.holds(point.position, reach)
        }
        Geometry::Line(segment) => window.meets_segment(segment.start, segment.end),
        Geometry::Polyline(polyline) => {
            if !window.meets_box(&polyline.bounding_box()) {
                return false;
            }
            if window.meets_points(&polyline.vertices, polyline.closed) {
                return true;
            }
            polyline.closed && resolved.hatched && polyline.contains(window.centre)
        }
        Geometry::Arc(arc) => {
            window.meets_box(&arc.bounding_box())
                && window.meets_points(&chord_arc(arc, tolerance), false)
        }
        Geometry::Circle(circle) => {
            window.meets_box(&circle.bounding_box())
                && window.meets_points(&chord_circle(circle, tolerance), true)
        }
        // A text: the box its letters take.
        _ => window.meets_box(&bounding_box(&entity.geometry)),
    }
}

// ---- grouping -----------------------------------------------------------------------

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    kind: LegendKind,
    // For a styled group.
    style: String,
    // For any other.
    layer: String,
}

// How an entry looks, as a value to count: the C++'s tuple of colour, line
// weight, linetype, symbol, symbol size and hatch.
struct Look {
    colour: u32,
    line_weight: f64,
    linetype: String,
    symbol: String,
    symbol_size: f64,
    hatch_pattern: String,
}

fn look_of(display: &ResolvedDisplay) -> Look {
    Look {
        colour: packed(&display.color),
        line_weight: display.line_weight,
        linetype: display.linetype.clone(),
        symbol: display.symbol.clone(),
        symbol_size: display.symbol_size,
        hatch_pattern: display.hatch_pattern.clone(),
    }
}

// Whether two looks are one key of the C++'s std::map<Look, ...>: neither
// orders before the other, a number compared by its `<` as the tuple
// compares it. Only which look most entities share is asked of the map, and
// each look's first id is its own, so its order is never observed and a
// list searched by this equivalence gives the same answer.
fn same_look(a: &Look, b: &Look) -> bool {
    let same = |x: f64, y: f64| !(x < y) && !(y < x);
    a.colour == b.colour
        && same(a.line_weight, b.line_weight)
        && a.linetype == b.linetype
        && a.symbol == b.symbol
        && same(a.symbol_size, b.symbol_size)
        && a.hatch_pattern == b.hatch_pattern
}

struct Seen {
    count: usize,
    first_id: EntityId,
    display: ResolvedDisplay,
}

#[derive(Default)]
struct Group {
    count: usize,
    looks: Vec<(Look, Seen)>,
    // The codes of its coded entities and what the library says of them;
    // `undescribed` when a coded entity's code has no description.
    descriptions: BTreeSet<String>,
    codes: BTreeSet<String>,
    undescribed: bool,
}

// The description the code library gives an entity's code, or empty.
struct CodeLabels<'a> {
    map: Option<&'a SurveyMap>,
    property: String,
    described: BTreeMap<String, String>,
}

impl<'a> CodeLabels<'a> {
    fn new(map: Option<&'a SurveyMap>, property: String) -> Self {
        Self {
            map,
            property,
            described: BTreeMap::new(),
        }
    }

    fn active(&self) -> bool {
        self.map.is_some_and(|map| !map.empty())
    }

    // The code the library is asked about, and its description; nothing for
    // an entity that carries no code.
    fn of(&mut self, entity: &Entity) -> Option<(String, String)> {
        let map = self.map?;
        let carried = survey_code_of(entity, &self.property)?;
        // A point's field code is its string name and its linework controls
        // ("PABB ST"); the library knows the name (survey_coding.rs).
        let mut code = String::from(carried);
        if matches!(entity.geometry, Geometry::Point(_)) {
            let field = parse_field_code(&code, &LineworkCodes::default());
            if !field.name.is_empty() {
                code = field.name;
            }
        }
        if let Some(description) = self.described.get(&code) {
            return Some((code, description.clone()));
        }
        let found = map.lookup(&code);
        let description = if found.matched() {
            found.resolved.comment
        } else {
            String::new()
        };
        self.described.insert(code.clone(), description.clone());
        Some((code, description))
    }
}

// Letter case folded, A to Z only.
fn folded(text: &str) -> String {
    text.to_ascii_lowercase()
}

// ---- the viewports -------------------------------------------------------------------

fn placed_plans<'s>(sheet: &'s Sheet, out: &mut Vec<&'s Viewport>) {
    for viewport in &sheet.viewports {
        if viewport.kind == ViewportKind::Plan && !viewport.rect.empty() {
            out.push(viewport);
        }
    }
}

// ---- the window of an automatic plan --------------------------------------------------

/// The window `viewport` shows: its own scale and centre, or for auto_scale
/// and auto_centre the fit to its stretch of its alignment or else to what it
/// draws (the entities it lets through and the alignments), with 4% to spare:
/// the painter's rule (resolvePlanViewport), without the imagery and meshes
/// the painter also counts, which the model does not hold.
pub fn fitted_plan_window(model: &Model, viewport: &Viewport) -> PlanWindow {
    let mut at = PlanWindow {
        scale: viewport.scale,
        centre: viewport.centre,
    };
    if (!viewport.auto_scale && !viewport.auto_centre) || viewport.rect.empty() {
        return at;
    }
    // What the viewport shows: its stretch of an alignment, or the drawing.
    let mut points: Vec<Point2> = Vec::new();
    let from = &viewport.source;
    if !from.alignment.is_empty()
        && let Some(alignment) = model.alignments.find(&from.alignment)
        && let Ok(solved) = solve_alignment(&alignment.horizontal)
    {
        let mut a = from.chainage_from;
        let mut b = from.chainage_to;
        if !(b > a) {
            a = solved.start_station();
            b = solved.end_station();
        }
        for i in 0..=64 {
            if let Some(p) = solved.point_at_station(a + (b - a) * f64::from(i) / 64.0) {
                points.push(p);
            }
        }
    }
    if points.is_empty() {
        let mut extent = drawn_extent(model, &viewport.hidden_layers);
        model.alignments.for_each(|alignment| {
            if let Ok(solved) = solve_alignment(&alignment.horizontal) {
                for &vertex in &solved.to_polyline(1.0).vertices {
                    extent.expand(vertex);
                }
            }
        });
        if extent.empty() {
            return at;
        }
        points = vec![
            extent.min,
            extent.max,
            Point2::new(extent.min.x, extent.max.y),
            Point2::new(extent.max.x, extent.min.y),
        ];
    }
    if viewport.auto_centre {
        let mut extent = Box2::default();
        for &p in &points {
            extent.expand(p.rotated(-viewport.rotation));
        }
        at.centre = extent.center().rotated(viewport.rotation);
    }
    if viewport.auto_scale {
        let mut half_w = 0.0;
        let mut half_h = 0.0;
        for &p in &points {
            let d = (p - at.centre).rotated(-viewport.rotation);
            half_w = cpp::max(half_w, d.x.abs());
            half_h = cpp::max(half_h, d.y.abs());
        }
        let needed = cpp::max(
            2.0 * half_w * 1000.0 / viewport.rect.width(),
            2.0 * half_h * 1000.0 / viewport.rect.height(),
        ) * 1.04;
        if needed > 0.0
            && let Ok(scale) = sheet_scale_at_least(needed)
        {
            at.scale = scale;
        }
    }
    at
}

// ---- the legend ---------------------------------------------------------------------------

/// The legend of sheet `sheet_index` of `set` for `scope`. InvalidArgument
/// for an index past the end. The C++'s default options are
/// LegendOptions::default().
pub fn compute_legend(
    model: &Model,
    set: &SheetSet,
    sheet_index: usize,
    scope: LegendScope,
    options: &LegendOptions<'_>,
) -> Result<Legend> {
    let Some(sheet) = set.sheets.get(sheet_index) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "no sheet at that index",
            format!("{sheet_index}"),
        ));
    };
    let resolve_window = |viewport: &Viewport| match options.window {
        Some(window) => window(viewport),
        None => fitted_plan_window(model, viewport),
    };

    // The plans looked through, after the fall-backs: a sheet without a plan
    // lists what the set shows, and a set without one the whole drawing.
    let mut legend = Legend::default();
    let mut own: Vec<&Viewport> = Vec::new();
    placed_plans(sheet, &mut own);
    let mut plans: Vec<&Viewport> = Vec::new();
    let mut used = scope;
    if used == LegendScope::ThisSheet {
        plans = own.clone();
        if plans.is_empty() {
            used = LegendScope::WholeSet;
        }
    }
    if used == LegendScope::WholeSet {
        for each in &set.sheets {
            placed_plans(each, &mut plans);
        }
        if plans.is_empty() {
            used = LegendScope::WholeDrawing;
        }
    }
    legend.scope = used;
    if used == LegendScope::WholeDrawing {
        plans.clear();
    }

    // The samples' scale: the plan the legend stands beside, when it has one.
    if let Some(&first) = own.first() {
        legend.scale = resolve_window(first).scale;
    } else if let Some(&first) = plans.first() {
        legend.scale = resolve_window(first).scale;
    }

    let mut resolve = Resolver::new(model);
    // By id: an entity is found again in the model by it.
    let mut shown: Vec<EntityId> = Vec::new();
    if used == LegendScope::WholeDrawing {
        model.entities.for_each(|entity| {
            let resolved = resolve.get(entity);
            if entity.visible && resolved.layer.shown && kind_of(entity, resolved).is_some() {
                shown.push(entity.id);
            }
        });
    } else {
        // A symbol reaches past its point, so the index is asked for the
        // window grown by the furthest a style's symbol reaches.
        let mut reach = 0.0;
        model.styles.for_each(|style| {
            if !style.symbol.is_empty() && style.symbol_size.is_finite() {
                reach = cpp::max(reach, 0.5 * style.symbol_size);
            }
        });
        let mut scratch = Vec::new();
        for &plan in &plans {
            let Some(window) = window_of(plan, &resolve_window(plan)) else {
                continue;
            };
            legend.windows += 1;
            for_each_candidate(
                model,
                options.index,
                window.reach.inflated(reach),
                &mut scratch,
                |entity| {
                    let resolved = resolve.get(entity);
                    if !entity.visible
                        || !resolved.layer.shown
                        || window.hidden.hides(&entity.layer)
                        || kind_of(entity, resolved).is_none()
                        || !shows(&window, entity, resolved)
                    {
                        return;
                    }
                    shown.push(entity.id);
                },
            );
        }
        // An entity two plans show is counted once.
        shown.sort_unstable();
        shown.dedup();
    }

    // The code library's words, when there is a library.
    let mut property = options.code_property.clone();
    let coded = options.codes.is_some_and(|codes| !codes.empty());
    if coded && property.is_empty() {
        property = find_code_property(model, &shown);
    }
    let mut labels = CodeLabels::new(if coded { options.codes } else { None }, property);

    let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
    for &id in &shown {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        let resolved = resolve.get(entity);
        let Some(kind) = kind_of(entity, resolved) else {
            continue;
        };
        let key = if resolved.styled {
            GroupKey {
                kind,
                style: entity.style.clone(),
                layer: String::new(),
            }
        } else {
            GroupKey {
                kind,
                style: String::new(),
                layer: entity.layer.clone(),
            }
        };
        let group = groups.entry(key).or_default();
        group.count += 1;
        let look = look_of(&resolved.display);
        let at = match group
            .looks
            .iter()
            .position(|(known, _)| same_look(known, &look))
        {
            Some(at) => at,
            None => {
                group.looks.push((
                    look,
                    Seen {
                        count: 0,
                        first_id: entity.id,
                        display: resolved.display.clone(),
                    },
                ));
                group.looks.len() - 1
            }
        };
        group.looks[at].1.count += 1;
        if labels.active()
            && let Some((code, description)) = labels.of(entity)
        {
            group.codes.insert(code);
            if description.is_empty() {
                group.undescribed = true;
            } else {
                group.descriptions.insert(description);
            }
        }
    }

    for (key, group) in &groups {
        // The look most of them print with; a tie goes to the one met first.
        let mut best: Option<&Seen> = None;
        for (_, seen) in &group.looks {
            let better = best.is_none_or(|kept| {
                seen.count > kept.count
                    || (seen.count == kept.count && seen.first_id < kept.first_id)
            });
            if better {
                best = Some(seen);
            }
        }
        let Some(best) = best else {
            continue;
        };
        let mut entry = LegendEntry {
            kind: key.kind,
            style: key.style.clone(),
            layer: key.layer.clone(),
            label: if key.style.is_empty() {
                key.layer.clone()
            } else {
                key.style.clone()
            },
            ..LegendEntry::default()
        };
        if !group.undescribed
            && group.descriptions.len() == 1
            && let (Some(description), Some(code)) =
                (group.descriptions.first(), group.codes.first())
        {
            entry.label = description.clone();
            entry.code = code.clone();
        }
        let display = &best.display;
        entry.colour = display.color;
        entry.line_weight = display.line_weight;
        entry.linetype = display.linetype.clone();
        entry.symbol = display.symbol.clone();
        entry.symbol_size = display.symbol_size;
        entry.hatch_pattern = display.hatch_pattern.clone();
        entry.count = group.count;
        legend.entries.push(entry);
    }
    legend.entries.sort_by(|a, b| {
        (a.kind, folded(&a.label), &a.label, &a.style, &a.layer).cmp(&(
            b.kind,
            folded(&b.label),
            &b.label,
            &b.style,
            &b.layer,
        ))
    });
    Ok(legend)
}

/// The legend the Legend viewport `viewport_id` of the document's sheets
/// lists, at its own scope, with the document's spatial index and survey
/// code library. NotFound for no such viewport; InvalidArgument for a
/// viewport that is not a legend.
pub fn legend_for(document: &Document, viewport_id: &str) -> Result<Legend> {
    let set = document.sheet_set();
    for (i, sheet) in set.sheets.iter().enumerate() {
        for viewport in &sheet.viewports {
            if viewport.id != viewport_id {
                continue;
            }
            if viewport.kind != ViewportKind::Legend {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "the viewport is not a legend",
                    format!("{} is a {}", viewport.id, viewport.kind.name()),
                ));
            }
            let options = LegendOptions {
                index: Some(document.spatial_index()),
                codes: Some(document.survey_map()),
                ..LegendOptions::default()
            };
            return compute_legend(document.model(), &set, i, viewport.legend_scope, &options);
        }
    }
    Err(make_error(
        ErrorCode::NotFound,
        "no viewport with that id",
        viewport_id,
    ))
}

/// Sets what the Legend viewport `viewport_id` lists, as one undoable step
/// (an unchanged scope records none). NotFound for no such viewport;
/// InvalidArgument for one that is not a legend.
pub fn set_legend_scope(document: &mut Document, viewport_id: &str, scope: LegendScope) -> Status {
    edit_viewport(
        document,
        viewport_id,
        |viewport| {
            if viewport.kind != ViewportKind::Legend {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "the viewport is not a legend",
                    format!("{} is a {}", viewport.id, viewport.kind.name()),
                ));
            }
            viewport.legend_scope = scope;
            Ok(())
        },
        "LEGEND_SCOPE",
    )
}

/// The legend as JSON, for an agent or a report:
///
/// ```text
/// {"scope": "this_sheet", "windows": 1, "scale": 500,
///  "entries": [{"kind": "symbol", "label": "...", "style": "...", "layer": "...",
///               "code": "...", "colour": "#RRGGBB", "weight": 0.25,
///               "linetype": "...", "symbol": "...", "symbol_size": 0,
///               "hatch": "none", "count": 3}]}
/// ```
///
/// Every member of an entry is written, so a reader need not know a default.
/// The C++ dumps with nlohmann's `replace` error handler, so a name with a
/// stray byte cannot cost the whole answer; a String holds no such byte.
pub fn legend_json(legend: &Legend) -> String {
    let entries = Json::array_of(legend.entries.iter().map(|entry| {
        Json::object_of([
            ("kind", Json::from(entry.kind.name())),
            ("label", Json::from(&entry.label)),
            ("style", Json::from(&entry.style)),
            ("layer", Json::from(&entry.layer)),
            ("code", Json::from(&entry.code)),
            ("colour", Json::from(entry.colour.to_hex())),
            ("weight", Json::from(entry.line_weight)),
            ("linetype", Json::from(&entry.linetype)),
            ("symbol", Json::from(&entry.symbol)),
            ("symbol_size", Json::from(entry.symbol_size)),
            ("hatch", Json::from(&entry.hatch_pattern)),
            ("count", Json::from(entry.count)),
        ])
    }));
    let root = Json::object_of([
        ("scope", Json::from(legend.scope.name())),
        ("windows", Json::from(legend.windows)),
        ("scale", Json::from(legend.scale)),
        ("entries", entries),
    ]);
    root.dump()
}

// ---- laying the legend out on the paper ------------------------------------

/// The legend panel's measures, in paper millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegendMetrics {
    /// Inside the viewport's edge.
    pub margin: f64,
    /// From the top edge to the first row's top.
    pub heading: f64,
    /// From one row to the next.
    pub pitch: f64,
    /// The sample's cell.
    pub sample_width: f64,
    pub sample_height: f64,
    /// From the sample to its label.
    pub label_gap: f64,
    /// Between columns.
    pub column_gap: f64,
}

impl Default for LegendMetrics {
    fn default() -> Self {
        Self {
            margin: 2.5,
            heading: 8.25,
            pitch: 4.5,
            sample_width: 10.0,
            sample_height: 3.6,
            label_gap: 2.0,
            column_gap: 4.0,
        }
    }
}

/// Where one entry goes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LegendCell {
    pub entry: usize,
    pub sample: Box2,
    /// The label's left end, on its middle line.
    pub label: Point2,
    /// How wide the label may be.
    pub label_room: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LegendLayout {
    /// In entry order.
    pub cells: Vec<LegendCell>,
    /// Rows in the tallest column.
    pub rows: usize,
    /// Columns used.
    pub columns: usize,
    pub column_width: f64,
    /// Entries left out because they do not fit, and where "+N more" goes:
    /// the last cell's place, which the entry there gives up.
    pub more: usize,
    pub more_at: Option<Point2>,
}

/// Flows `label_widths.len()` entries down columns and then across, inside
/// `rect`: each column as wide as the widest label needs (at most the room
/// there is), as few columns as hold every entry, rows shared evenly between
/// them. When they do not all fit, the last cell says how many are left out.
/// The C++'s default metrics are LegendMetrics::default().
pub fn layout_legend(rect: &Box2, label_widths: &[f64], metrics: &LegendMetrics) -> LegendLayout {
    let mut layout = LegendLayout::default();
    let count = label_widths.len();
    if count == 0 || rect.empty() {
        return layout;
    }
    let left = rect.min.x + metrics.margin;
    let top = rect.max.y - metrics.heading;
    let width = rect.width() - 2.0 * metrics.margin;
    let height = top - (rect.min.y + metrics.margin);
    if !(width > 0.0) || !(height >= metrics.pitch) {
        layout.more = count;
        return layout;
    }
    let rows_fit = (height / metrics.pitch + 1e-9).floor() as usize;
    let mut widest = 0.0;
    for &w in label_widths {
        widest = cpp::max(widest, if w.is_finite() { w } else { 0.0 });
    }
    let natural = metrics.sample_width + metrics.label_gap + widest;
    layout.column_width = cpp::min(natural, width);
    let columns_fit = std::cmp::max(
        1,
        ((width + metrics.column_gap) / (layout.column_width + metrics.column_gap) + 1e-9).floor()
            as usize,
    );
    let capacity = rows_fit * columns_fit;
    let mut shown = count;
    if count > capacity {
        // The last place says how many are left out.
        shown = capacity - 1;
        layout.more = count - shown;
    }
    // As few columns as hold them, the rows shared out evenly.
    let places = shown + usize::from(layout.more > 0);
    layout.columns = places.div_ceil(rows_fit);
    layout.rows = places.div_ceil(layout.columns);
    let place_of = |n: usize| {
        let column = n / layout.rows;
        let row = n % layout.rows;
        let x = left + column as f64 * (layout.column_width + metrics.column_gap);
        let y = top - (row as f64 + 0.5) * metrics.pitch;
        Point2::new(x, y)
    };
    let mut cells = Vec::with_capacity(shown);
    for n in 0..shown {
        let at = place_of(n);
        cells.push(LegendCell {
            entry: n,
            sample: Box2::new(
                Point2::new(at.x, at.y - 0.5 * metrics.sample_height),
                Point2::new(
                    at.x + metrics.sample_width,
                    at.y + 0.5 * metrics.sample_height,
                ),
            ),
            label: Point2::new(at.x + metrics.sample_width + metrics.label_gap, at.y),
            label_room: cpp::max(
                layout.column_width - metrics.sample_width - metrics.label_gap,
                0.0,
            ),
        });
    }
    if layout.more > 0 {
        layout.more_at = Some(place_of(shown));
    }
    layout.cells = cells;
    layout
}
