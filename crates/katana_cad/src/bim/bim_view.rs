//! How the views show the road model's objects (include/katana/cad/bim/
//! bim_view.hpp; its three functions are in the C++ bim_object.cpp, and here a
//! module of their own follows the header; docs/road_design.md, "In the
//! views"): which are drawn, and the 3D view's meshes of them.
//!
//! AN OBJECT FOLLOWS ITS LAYER. Its layer is a path ("road/earthworks/cut");
//! the deepest part of it the drawing has decides - an object on
//! "road/pavement" is hidden when "road" is switched off, even before
//! "road/pavement" is made - and a path the drawing has none of is drawn,
//! since an object is not an entity and is drawn in its own colour. CORRIDOR
//! NEW makes the layers its objects are on (road/earthworks/cut switched off:
//! a cut is the ground taken away, and drawn solid it would bury the road it
//! was taken away for), so after that the Layers panel shows and hides them
//! like anything else. A view that hides a layer of its own hides its objects
//! there too: a plan view through is_shown(..., its overrides), a 3D view
//! through SceneMesh::layer, which it tests against its own hidden layers
//! (the window's shownMeshes) because the window makes ONE list of meshes for
//! all of them, with the drawing's layers only.

use katana_entity::Model;
use katana_geometry::{Box2, Point2};
use katana_render::rgba;

use crate::bim::bim_object::{BimModel, BimObject};
use crate::layer_overrides::LayerOverrides;
use crate::scene::SceneMesh;
use crate::terrain::surface_style::SurfaceStyle;

pub fn is_shown(object: &BimObject, model: &Model, view: &LayerOverrides) -> bool {
    if !object.layer.is_empty() && view.hides(&object.layer) {
        return false;
    }
    let mut path = object.layer.as_str();
    while !path.is_empty() {
        if model.layers.contains(path) {
            return model.layers.resolve(path).shown;
        }
        let Some(slash) = path.rfind('/') else {
            break;
        };
        path = &path[..slash];
    }
    true
}

/// A 3D view's mesh for each object shown: its name, a reference to its mesh -
/// so `objects` must outlive them - its colour, Shaded, and plan_footprint off
/// (the plan view draws the object's own outline, not its hull).
pub fn scene_meshes<'a>(
    objects: &'a BimModel,
    model: &Model,
    view: &LayerOverrides,
) -> Vec<SceneMesh<'a>> {
    let mut meshes = Vec::new();
    for object in &objects.objects {
        if object.mesh.empty() || !is_shown(object, model, view) {
            continue;
        }
        meshes.push(SceneMesh {
            name: object.name.clone(),
            mesh: Some(&object.mesh),
            style: SurfaceStyle::Shaded,
            flat_color: rgba(object.colour.r, object.colour.g, object.colour.b),
            plan_footprint: false,
            layer: object.layer.clone(),
            ..SceneMesh::default()
        });
    }
    meshes
}

/// The box round the plan outlines of the objects shown (and round their
/// meshes for an object with no outline). Empty when none is shown.
pub fn plan_bounds(objects: &BimModel, model: &Model, view: &LayerOverrides) -> Box2 {
    let mut bounds = Box2::default();
    for object in &objects.objects {
        if !is_shown(object, model, view) {
            continue;
        }
        if object.plan.is_empty() {
            let space = object.mesh.bounds();
            if !space.empty() {
                bounds.expand(Point2::new(space.min.x, space.min.y));
                bounds.expand(Point2::new(space.max.x, space.max.y));
            }
            continue;
        }
        for line in &object.plan {
            bounds.expand_box(line.bounding_box());
        }
    }
    bounds
}
