//! The scope and filter of the ROADOBJECT verbs
//! (include/katana/cad/road/road_object_scope.hpp,
//! src/katana_cad/road/road_object_scope.cpp; docs/road_objects.md, "Scope and
//! filter"): LIST, QUANTITIES and DELETE, the manager and the
//! katana_road_objects tool act on the same choices as Global Modify
//! (CLAUDE.md 1.1) - the selection, what a view shows, named layers, the whole
//! drawing, an AREA - and the same "only those that match" filter, read by the
//! one parser (scope_verbs.rs) and resolved here the way match_entities
//! resolves it for entities, with the same pieces (global_modify.rs: the layer
//! scope, the layer patterns, the property conditions; selection.rs: whether a
//! layer is drawn).
//!
//! A road object is a DEFINITION kept in the drawing, not an entity, so each
//! choice says what it means for one:
//!
//! ```text
//! DRAWING / ALL       every object.
//! LAYERS a,b [ONLY]   the objects whose layer (layer_of) is one of those, or
//!                     beneath one unless ONLY.
//! AREA x0,y0,x1,y1    the objects whose plan extent meets the box. The plan
//!                     extent is the box round the x and y of the vertices of
//!                     the solids the object is made of (plan_extent), the
//!                     same as the ones the plan and 3D views draw.
//! VIEW [id] [EXTENTS] what a view shows: the objects on a layer the drawing
//!                     shows and the view does not hide, and (without
//!                     EXTENTS) whose plan extent meets the view's visible
//!                     area.
//! SELECTION           the objects placed ALONG a selected line - the object
//!                     remembers the entity it was copied from (`source=#12`)
//!                     - and only those. A road object is not itself
//!                     selectable, and one placed along an alignment or at
//!                     points has no entity to select, so the selection
//!                     never takes it. Rejected: the objects whose extent
//!                     touches the selected entities' boxes, which for one
//!                     long diagonal line is every object in its box, and
//!                     DELETE acts on the answer.
//! (no scope word)     the selection, as everywhere in the grammar.
//! ```
//!
//! WHERE conditions read an object as follows. LAYER=pat[,pat] is the object's
//! layer path; PROP=key[:pat] reads the keys ROADOBJECT SET writes, as the
//! object has them (filter_properties) - id, kind, name, layer, colour,
//! material, facility, placement, alignment, station, to, offset, spacing,
//! heights, surface, crossfall, dz, points and source, then the kind's own keys
//! with the value the object is made with (type, height, ...) - so the kind of
//! an object is PROP=kind:barrier and a barrier of one type
//! PROP=type:thriebeam, the key in any case and the value a pattern with * and
//! ?; COLOUR=#RRGGBB is the colour the object is drawn in, and COLOUR=ByLayer
//! an object with no colour= of its own; DRAWN leaves out an object on a layer
//! the drawing hides or one that is not made. TYPE=, STYLE= and TEXT= are for
//! entities, and are REFUSED for a road object, naming the word - never
//! ignored.
//!
//! A scope that takes nothing is reported (matched=0), not refused. A Layers
//! scope naming a layer the drawing lacks is refused, as it is for entities.

use katana_core::error::make_error;
use katana_core::text::lowered;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_entity::{EntityId, PropertyMap, PropertyValue};
use katana_geometry::{Box2, Point2};

use crate::document::Document;
use crate::global_modify::{
    ModifyFilter, ScopeKind, check_scope_layers, layer_in_scope, layer_passes_filter,
    properties_pass_filter,
};
use crate::layer_overrides::{LayerOverrides, NO_LAYER_OVERRIDES};
use crate::road::road_objects::{
    ParameterValue, PlacementContext, PlacementKind, RoadObject, build_road_object, colour_of,
    effective_parameters, height_source_of, height_word, kind_word, layer_of, material_of,
    placement_context, placement_word,
};
use crate::scope_verbs::{
    ResolvedScope, ScopeViewProvider, ScopeWords, resolve_scope, scope_record_of,
};
use crate::selection::is_layer_drawn;

/// What a scope and a filter took of the drawing's road objects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadObjectMatch {
    pub resolved: ResolvedScope,
    /// Their ids, in the order the drawing keeps the objects.
    pub ids: Vec<String>,
    /// Objects the scope and the filter would take but for having no plan
    /// extent, which an AREA, a VIEW and DRAWN ask for: one that is not made
    /// (its alignment gone, its surface removed - ROADOBJECT PROBLEMS says
    /// why) draws nothing, so it is nowhere.
    pub unmade: usize,
}

impl RoadObjectMatch {
    /// The record that leads a reply: scope_record_of(resolved, ids.len()),
    /// then unmade= when there are any.
    pub fn record(&self) -> String {
        let mut out = scope_record_of(&self.resolved, self.ids.len());
        if self.unmade > 0 {
            out += &format!(" unmade={}", self.unmade);
        }
        out
    }
}

/// Why `filter` cannot be put to road objects, or Ok. TYPE, STYLE and TEXT
/// are about entities (see the module comment).
pub fn check_filter_fits(filter: &ModifyFilter) -> Status {
    if !filter.types.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "TYPE= is an entity's type and a road object is not an entity; its kind is \
             PROP=kind:barrier",
            "TYPE",
        ));
    }
    if filter.style.is_some() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "STYLE= is the style an entity wears and a road object wears none",
            "STYLE",
        ));
    }
    if filter.text.is_some() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "TEXT= is the text of a label or a dimension and a road object has none",
            "TEXT",
        ));
    }
    Ok(())
}

/// The box round the x and y of the vertices of the solids `object` is made
/// of, as it is made now; None when it is not made.
pub fn plan_extent(context: &PlacementContext<'_>, object: &RoadObject) -> Option<Box2> {
    let mut extent = Box2::default();
    let built = build_road_object(context, object);
    for part in &built.objects {
        for vertex in &part.mesh.vertices {
            extent.expand(Point2::new(vertex.x, vertex.y));
        }
    }
    if extent.empty() {
        return None;
    }
    Some(extent)
}

/// What WHERE PROP= reads of `object`, key by key (see the module comment).
pub fn filter_properties(object: &RoadObject) -> PropertyMap {
    let mut map = PropertyMap::new();
    let text = |map: &mut PropertyMap, key: &str, value: String| {
        map.insert(String::from(key), PropertyValue::Text(value));
    };
    let number = |map: &mut PropertyMap, key: &str, value: f64| {
        map.insert(String::from(key), PropertyValue::Real(value));
    };
    let whole = |map: &mut PropertyMap, key: &str, value: u64| {
        map.insert(String::from(key), PropertyValue::Integer(value as i64));
    };
    text(&mut map, "id", object.id.clone());
    text(&mut map, "kind", String::from(kind_word(object.kind)));
    if !object.name.is_empty() {
        text(&mut map, "name", object.name.clone());
    }
    text(&mut map, "layer", layer_of(object));
    text(&mut map, "colour", colour_of(object).to_hex());
    text(&mut map, "material", material_of(object));
    if !object.facility.is_empty() {
        text(&mut map, "facility", object.facility.clone());
    }
    let placement = &object.placement;
    text(
        &mut map,
        "placement",
        String::from(placement_word(placement.kind)),
    );
    if placement.kind == PlacementKind::Alignment {
        text(&mut map, "alignment", placement.alignment.clone());
        number(&mut map, "station", placement.station);
        if let Some(to) = placement.to {
            number(&mut map, "to", to);
        }
        number(&mut map, "offset", placement.offset);
    } else {
        whole(&mut map, "points", placement.vertices.len() as u64);
        if placement.source != INVALID_ENTITY_ID {
            whole(&mut map, "source", placement.source);
        }
    }
    if let Some(spacing) = placement.spacing {
        number(&mut map, "spacing", spacing);
    }
    text(
        &mut map,
        "heights",
        String::from(height_word(height_source_of(placement))),
    );
    if !placement.surface.is_empty() {
        text(&mut map, "surface", placement.surface.clone());
    }
    number(&mut map, "crossfall", placement.crossfall);
    number(&mut map, "dz", placement.dz);
    // The kind's own keys with the value the object is made with. A key the
    // object takes in common keeps its meaning (insert, not assign).
    for (key, value) in effective_parameters(object) {
        map.entry(key).or_insert(match value {
            ParameterValue::Number(real) => PropertyValue::Real(real),
            ParameterValue::Text(word) => PropertyValue::Text(word),
        });
    }
    map
}

/// The road objects `words` take. Fails with Unsupported (the reason) for a
/// record of road objects that cannot be read, NotFound for a Layers scope
/// naming a layer the drawing lacks, InvalidState for a VIEW with no window
/// (`views` answers it, as for entities; None headless, the C++ default) and
/// InvalidArgument naming the word for a TYPE=, STYLE= or TEXT= condition.
pub fn match_road_objects(
    document: &Document,
    words: &ScopeWords,
    views: ScopeViewProvider<'_>,
) -> Result<RoadObjectMatch> {
    // A record that cannot be read is empty to road_objects(): without this a
    // scope would say it took nothing of objects that are there and kept.
    document.road_objects_status()?;
    check_filter_fits(&words.filter)?;
    let resolved = resolve_scope(words, views)?;
    let model = document.model();
    check_scope_layers(model, &resolved.scope)?;

    let scope = &resolved.scope;
    let mut filter = resolved.filter.clone();
    // The keys are lower case words; a person types PROP=Kind as readily as
    // PROP=kind, and a key is exact in the shared condition.
    if let Some(property) = &mut filter.property {
        *property = String::from_utf8_lossy(&lowered(property.as_str())).into_owned();
    }
    let reads_properties = filter.property.is_some() || filter.property_value.is_some();
    let hidden: &LayerOverrides = scope.view.as_deref().unwrap_or(&NO_LAYER_OVERRIDES);
    // The selection, ascending (SelectionSet keeps it ordered).
    let selected: Vec<EntityId> = if scope.kind == ScopeKind::Selection {
        document.selection().ids()
    } else {
        Vec::new()
    };
    let context = placement_context(document);

    let mut ids: Vec<String> = Vec::new();
    let mut unmade = 0;
    let objects = document.road_objects();
    for object in &objects.objects {
        let layer = layer_of(object);
        match scope.kind {
            ScopeKind::Selection => {
                if object.placement.source == INVALID_ENTITY_ID
                    || selected.binary_search(&object.placement.source).is_err()
                {
                    continue;
                }
            }
            ScopeKind::Drawing => {}
            ScopeKind::Layers => {
                if !layer_in_scope(scope, &layer) {
                    continue;
                }
            }
            ScopeKind::View => {
                if !is_layer_drawn(&model.layers.resolve(&layer), &layer, hidden) {
                    continue;
                }
            }
        }
        if !layer_passes_filter(&filter, &layer) {
            continue;
        }
        // An object's own colour is none until colour= gives one: ByLayer
        // here, as for an entity; a colour is the one it is drawn in.
        if let Some(colour) = &filter.colour {
            let matches = match colour {
                Some(colour) => colour_of(object) == *colour,
                None => object.colour.is_none(),
            };
            if !matches {
                continue;
            }
        }
        if reads_properties && !properties_pass_filter(&filter, &filter_properties(object)) {
            continue;
        }
        if filter.drawn_only
            && !is_layer_drawn(&model.layers.resolve(&layer), &layer, &NO_LAYER_OVERRIDES)
        {
            continue;
        }
        // The plan extent is worked out last: it makes the object.
        let needs_extent = scope.kind == ScopeKind::View || filter.drawn_only;
        if needs_extent {
            let Some(extent) = plan_extent(&context, object) else {
                unmade += 1;
                continue;
            };
            if scope.kind == ScopeKind::View
                && let Some(area) = scope.area
                && !extent.intersects(area)
            {
                continue;
            }
        }
        ids.push(object.id.clone());
    }
    Ok(RoadObjectMatch {
        resolved,
        ids,
        unmade,
    })
}
