//! The twins of tests/qt_widgets/terrain/test_surfaces_panel.cpp: the Surfaces panel
//! (katana_window::terrain::surfaces_panel) - a row a surface, and every change a line through the
//! executor, run here by the interpreter on the host's drawing.

use std::cell::RefCell;
use std::rc::Rc;

use katana_geometry::Box2;
use katana_terrain::SurfaceStore;
use katana_window::drive::WINDOW;
use katana_window::host::RecordingHost;
use katana_window::terrain::surfaces_panel::{SurfacesPanel, SurfacesPanelServices};

use super::support::{Bench, ZoomViews, interpreter_host, keep, named, set_surfaces, square};

/// A drawing of two surfaces, the panel on it, and where a Zoom To's box is kept.
struct Rig {
    bench: Bench<SurfacesPanel>,
    zoomed: Rc<RefCell<Option<Box2>>>,
}

impl Rig {
    fn new(with_surfaces: bool) -> Self {
        let (views, zoomed) = ZoomViews::new();
        let mut host: RecordingHost = interpreter_host().with_views(Box::new(views));
        if with_surfaces {
            let mut store = SurfaceStore::new();
            keep(&mut store, named("ground", square(100.0), "raster dem.tif"));
            keep(
                &mut store,
                named("design", square(99.0), "drawing (4 entities)"),
            );
            set_surfaces(&mut host, store, "SURFACE FROM");
        }
        let panel = SurfacesPanel::new(host.document(), SurfacesPanelServices::default());
        Self {
            bench: Bench::new(WINDOW, panel, host),
            zoomed,
        }
    }

    /// The handler's own change is read once it has returned: the next frame.
    fn settle(&mut self) {
        self.bench.draw();
    }

    fn undo_count(&self) -> usize {
        self.bench.host.document().history().undo_count()
    }

    fn visible(&self, name: &str) -> bool {
        self.bench
            .host
            .document()
            .surfaces()
            .find(name)
            .unwrap_or_else(|| panic!("no surface {name}"))
            .visible
    }
}

mod surfaces_panel_widget {
    use super::*;

    #[test]
    fn shows_a_row_per_surface_with_its_style_source_and_triangles() {
        let rig = Rig::new(true);
        assert!(rig.bench.has("surfacesTable"));
        assert!(rig.bench.has("surfacesPanel"));
        let rows = rig.bench.dialog.rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "ground");
        assert!(rows[0].shown);
        assert_eq!(rows[0].source, "raster dem.tif");
        assert_eq!(rows[0].triangles, 2);
        assert_eq!(rows[0].style_text(), "Default (Standard)");
        // The default, then the seven built-in styles.
        assert_eq!(rows[0].style_texts.len(), 8);
    }

    #[test]
    fn nothing_picked_leaves_the_row_tools_off() {
        let mut rig = Rig::new(true);
        for name in [
            "surfacesRename",
            "surfacesCopy",
            "surfacesRemove",
            "surfacesZoomTo",
        ] {
            assert!(rig.bench.has(name), "{name}");
            assert!(!rig.bench.enabled(name), "{name}");
        }
        assert!(rig.bench.has("surfacesEditStyles"));
        assert!(rig.bench.enabled("surfacesEditStyles"));
        rig.bench.press("surfacesEditStyles");
        assert_eq!(rig.bench.dialog.take_edit_styles_asked(), 1);

        assert!(rig.bench.dialog.pick_surface("DESIGN"));
        assert_eq!(
            rig.bench.dialog.picked_surface().as_deref(),
            Some("design")
        );
        rig.settle();
        assert!(rig.bench.enabled("surfacesCopy"));
    }

    #[test]
    fn unticking_a_surface_hides_it_as_one_undo_step() {
        let mut rig = Rig::new(true);
        let steps = rig.undo_count();
        // The name item's setCheckState.
        let bench = &mut rig.bench;
        bench.dialog.set_shown(&mut bench.host, 0, false);
        assert!(!rig.bench.host.lines().is_empty());
        assert_eq!(rig.bench.last_line(), "SURFACE HIDE \"ground\"");
        assert!(!rig.visible("ground"));
        assert_eq!(rig.undo_count(), steps + 1);
        rig.settle();
        assert!(!rig.bench.dialog.rows()[0].shown);
        let bench = &mut rig.bench;
        bench.dialog.set_shown(&mut bench.host, 0, true);
        assert_eq!(rig.bench.last_line(), "SURFACE SHOW \"ground\"");
        assert!(rig.visible("ground"));
    }

    #[test]
    fn typing_a_new_name_renames_the_surface() {
        let mut rig = Rig::new(true);
        // The name item's setText.
        let bench = &mut rig.bench;
        bench.dialog.set_name(&mut bench.host, 1, "design pad");
        assert_eq!(
            rig.bench.last_line(),
            "SURFACE RENAME \"design\" \"design pad\""
        );
        assert!(
            rig.bench
                .host
                .document()
                .surfaces()
                .find("design pad")
                .is_some()
        );
        rig.settle();
        assert_eq!(rig.bench.dialog.rows()[1].name, "design pad");
        // A name another surface has is refused by the verb; the row shows the drawing's name
        // again.
        let bench = &mut rig.bench;
        bench.dialog.set_name(&mut bench.host, 1, "ground");
        let reply = String::from(rig.bench.dialog.last_reply());
        assert!(reply.contains("AlreadyExists"), "{reply}");
        rig.settle();
        assert_eq!(rig.bench.dialog.rows()[1].name, "design pad");
    }

    #[test]
    fn choosing_a_style_gives_it_to_the_surface() {
        let mut rig = Rig::new(true);
        let contours = rig.bench.dialog.rows()[0]
            .style_texts
            .iter()
            .position(|style| style == "Contours")
            .expect("Contours is a built-in style");
        assert!(contours > 0);
        // As a person's choice in the row's list does.
        let bench = &mut rig.bench;
        bench.dialog.choose_style(&mut bench.host, 0, contours);
        assert_eq!(
            rig.bench.last_line(),
            "SURFACE STYLE \"ground\" \"Contours\""
        );
        let style_of = |rig: &Rig| {
            rig.bench
                .host
                .document()
                .surfaces()
                .find("ground")
                .expect("ground is there")
                .style
                .clone()
        };
        assert_eq!(style_of(&rig), "Contours");
        rig.settle();
        assert_eq!(rig.bench.dialog.rows()[0].style_text(), "Contours");
        // Back to the drawing's default: the first entry, an empty style.
        let bench = &mut rig.bench;
        bench.dialog.choose_style(&mut bench.host, 0, 0);
        assert_eq!(rig.bench.last_line(), "SURFACE STYLE \"ground\" \"\"");
        assert_eq!(style_of(&rig), "");
    }

    #[test]
    fn copy_remove_and_zoom_to_act_on_the_picked_surface() {
        let mut rig = Rig::new(true);
        assert!(rig.bench.dialog.pick_surface("ground"));
        rig.settle();
        rig.bench.press("surfacesCopy");
        assert_eq!(
            rig.bench.last_line(),
            "SURFACE COPY \"ground\" \"ground (2)\""
        );
        assert!(
            rig.bench
                .host
                .document()
                .surfaces()
                .find("ground (2)")
                .is_some()
        );
        rig.settle();
        assert_eq!(rig.bench.dialog.rows().len(), 3);
        assert_eq!(
            rig.bench.dialog.picked_surface().as_deref(),
            Some("ground"),
            "still picked"
        );

        rig.bench.press("surfacesZoomTo");
        let zoomed = rig.zoomed.borrow().expect("the views were asked to frame it");
        assert_eq!(zoomed.min.x, 0.0);
        assert_eq!(zoomed.max.y, 10.0);

        // The line is what the panel answers for.
        rig.bench.press("surfacesRemove");
        assert_eq!(rig.bench.last_line(), "SURFACE REMOVE \"ground\"");
    }

    #[test]
    fn a_change_made_elsewhere_is_shown_on_the_event_loop_not_inside_the_notification() {
        let mut rig = Rig::new(false);
        assert_eq!(rig.bench.dialog.rows().len(), 0);
        let mut store = SurfaceStore::new();
        keep(&mut store, named("ground", square(5.0), "test"));
        set_surfaces(&mut rig.bench.host, store, "SET_SURFACES");
        // Not rebuilt inside the document's change, which may come from the panel's own control:
        // at the panel's next look.
        assert_eq!(rig.bench.dialog.rows().len(), 0);
        rig.settle();
        assert_eq!(rig.bench.dialog.rows().len(), 1);
        rig.bench
            .host
            .document_mut()
            .undo()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        rig.settle();
        assert_eq!(rig.bench.dialog.rows().len(), 0);
    }

    #[test]
    fn unticking_a_surface_called_all_hides_that_surface_alone() {
        // The panel quotes the name, the tokenizer drops the quotes, and ALL used to be read as
        // the keyword: one click on one box hid every surface.
        let mut rig = Rig::new(true);
        let mut store: SurfaceStore = rig.bench.host.document().surfaces().clone();
        keep(&mut store, named("ALL", square(98.0), "copy of ground"));
        set_surfaces(&mut rig.bench.host, store, "SURFACE COPY");
        rig.settle();
        let row = rig
            .bench
            .dialog
            .rows()
            .iter()
            .position(|row| row.name == "ALL")
            .expect("ALL has a row");
        let bench = &mut rig.bench;
        bench.dialog.set_shown(&mut bench.host, row, false);
        assert_eq!(rig.bench.last_line(), "SURFACE HIDE \"ALL\"");
        assert!(!rig.visible("ALL"));
        assert!(rig.visible("ground"));
        assert!(rig.visible("design"));
    }

    #[test]
    fn a_name_a_line_cannot_carry_is_refused_in_the_panel_and_no_line_is_sent() {
        // A quote wrapped in quotes is a line that names something else: it ended the word early,
        // and a Remove of PAD "A" could remove PAD A.
        let mut rig = Rig::new(true);
        let steps = rig.undo_count();
        let bench = &mut rig.bench;
        bench.dialog.set_name(&mut bench.host, 1, "design \"pad\"");
        assert!(
            rig.bench.host.lines().is_empty(),
            "no line was sent: {}",
            rig.bench.host.lines().join("|")
        );
        let reply = String::from(rig.bench.dialog.last_reply());
        assert!(reply.contains("double quote"), "{reply}");
        assert_eq!(rig.undo_count(), steps);
        assert!(
            rig.bench
                .host
                .document()
                .surfaces()
                .find("design")
                .is_some()
        );
        rig.settle();
        assert_eq!(
            rig.bench.dialog.rows()[1].name,
            "design",
            "the row shows the drawing's name again"
        );
    }
}
