//! The window's part of surface display (terrain/main_window_surface_display.cpp; docs/
//! surface_display.md, "In the window"): Terrain > Surface Styles... and the 3D views' surfaces
//! drawn in their styles. The main window's own, kept out of its file so that file's edit is a few
//! lines; and with it the scene's surfaces made again from the drawing's (syncSceneSurfaces),
//! which the C++ kept in main_window.cpp.
//!
//! An edit of the styles (SURFACESTYLE, the manager, an undo of either) or another drawing
//! restyles the 3D views' surfaces: the C++ queued it on the event loop from a Document listener,
//! once for a burst of changes; here the executor's look at the drawing at the next frame does it
//! (`Executor::follow_drawing`), once however many changes came first.

use std::rc::Rc;

use katana_cad::scene::{SceneSurface, apply_surface_display};
use katana_cad::terrain::surface_style::style_of;
use katana_cad::viewport_layout::ViewKind;

use crate::drive::Action;
use crate::main_window::executor::{Executor, SurfaceItem, WindowRequest};
use crate::view_workspace::Scenery;

/// Terrain > Surface Styles...: the style manager it shows names itself to --dialog.
pub fn surface_styles_action() -> Action {
    Action::new("terrainSurfaceStyles", "Surface St&yles...")
        .icon("SurfaceStyles")
        .tip(
            "Make, edit and choose how surfaces are drawn: triangles, contours, faces coloured by \
             height, slope or aspect, in plan and 3D",
        )
        .opens("surfaceStyleManagerDialog")
}

/// The surface manager's dialog name.
pub const SURFACE_STYLE_MANAGER: &str = "surfaceStyleManagerDialog";

impl Executor {
    /// MainWindow::styleSceneSurfaces: each of `items` drawn in its surface's display style
    /// (cad's apply_surface_display) and shown as the drawing shows the surface.
    pub fn style_scene_surfaces(&self, items: &mut [SurfaceItem]) {
        let styles = self.document.surface_styles();
        for item in items {
            let Some(named) = self.document.surfaces().find(&item.scene.name) else {
                continue;
            };
            let same = named
                .surface
                .as_ref()
                .is_some_and(|surface| std::sync::Arc::ptr_eq(surface, &item.held));
            if !same {
                continue;
            }
            apply_surface_display(&mut item.scene, style_of(styles, named));
            item.scene.visible = named.visible;
        }
    }

    /// MainWindow::restyleSceneSurfaces: the scene's surfaces styled again, and the 3D views told.
    pub fn restyle_scene_surfaces(&mut self) {
        let mut store = (*self.scene).clone();
        self.style_scene_surfaces(&mut store.surfaces);
        self.scene = Rc::new(store);
        self.workspace.scenery_changed();
    }

    /// MainWindow::syncSceneSurfaces: the scene's surfaces made again from the drawing's
    /// (Document::surfaces) when they have changed, each keeping how the 3D view shows it. A
    /// surface the drawing gained is shown in a 3D view, framed - one the person cannot see is not
    /// obviously made - unless `show_gained` is false (a New or an Open).
    pub fn sync_scene_surfaces(&mut self, show_gained: bool) {
        let store = self.document.surfaces();
        if self.scene.surfaces_generation == self.document.surfaces_generation()
            && self.scene.styles_generation == self.document.surface_styles_generation()
        {
            return;
        }
        // Whether the drawing gained a shown surface the scene has not drawn: neither its
        // triangulation NOR its name is among the scene's. An edit, its undo and its redo put a
        // different surface under the same name, which is not a surface gained.
        let gained = store.all().iter().any(|named| {
            named.visible
                && !self.scene.surfaces.iter().any(|shown| {
                    named
                        .surface
                        .as_ref()
                        .is_some_and(|surface| std::sync::Arc::ptr_eq(surface, &shown.held))
                        || katana_core::text::equals_ignoring_case(
                            shown.scene.name.as_bytes(),
                            named.name.as_bytes(),
                        )
                })
        });
        let mut next: Vec<SurfaceItem> = Vec::with_capacity(store.all().len());
        for named in store.all() {
            let Some(surface) = &named.surface else {
                continue;
            };
            let kept = self
                .scene
                .surfaces
                .iter()
                .find(|shown| std::sync::Arc::ptr_eq(&shown.held, surface));
            // Its shading and colouring, when the scene drew it already.
            let mut scene = kept.map_or_else(SceneSurface::default, |kept| kept.scene.clone());
            scene.name.clone_from(&named.name);
            scene.surface = None;
            // Shown or hidden is the drawing's to say.
            scene.visible = named.visible;
            next.push(SurfaceItem {
                scene,
                held: std::sync::Arc::clone(surface),
            });
        }
        self.style_scene_surfaces(&mut next);
        let mut scene = (*self.scene).clone();
        scene.surfaces = next;
        scene.surfaces_generation = self.document.surfaces_generation();
        scene.styles_generation = self.document.surface_styles_generation();
        self.scene = Rc::new(scene);
        self.workspace.scenery_changed();
        if !gained || !show_gained {
            return;
        }
        // Show it. A surface the person cannot see is not obviously a success.
        self.with_parts(|workspace, parts| {
            let _ = workspace.ensure_view(parts, ViewKind::Model3D);
        });
        self.request(WindowRequest::ViewsChanged);
        if let Some(id) = self.workspace.active_render_view() {
            let scene = Rc::clone(&self.scene);
            let surfaces = scene.surfaces();
            let meshes = scene.meshes(self.document.model());
            let _ = self.workspace.with_render_view(
                id,
                &self.document,
                Scenery {
                    surfaces: &surfaces,
                    meshes: &meshes,
                },
                |view, scene, state| {
                    view.invalidate_scene();
                    view.zoom_extents(scene, state);
                },
            );
        }
    }
}
