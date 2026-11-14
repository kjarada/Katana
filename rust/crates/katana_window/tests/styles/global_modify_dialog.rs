//! The twins of tests/qt_widgets/customisation/test_global_modify_dialog.cpp: the form reads into
//! the scope, filter and change that cad's plan_global_modify takes, and its buttons preview,
//! select and apply them. Driven by object name, as a person or the headless driver would, on a
//! small drawing laid out in the fixture's comment.
//!
//! The shared Apply to and Only those that match controls are set through the widget's own
//! setters, as the foundation's twins set them (a layer's box in the list has no name of its own
//! to fill).

use std::rc::Rc;
use std::sync::Arc;

use katana_cad::Document;
use katana_cad::global_modify::{ModifyFilter, match_entities};
use katana_cad::layer_overrides::LayerOverrides;
use katana_commands::{
    EntityAttributes, create_layer, create_line, create_point, create_style, set_entity_property,
    set_entity_style, update_layer,
};
use katana_entity::{Color, Entity, EntityId, EntityType, Layer, PropertyValue, Style};
use katana_geometry::{Box2, Point2};
use katana_window::customisation::scope_filter_widget::{FilterField, ScopeChoice};
use katana_window::customisation::{CustomisationContext, GlobalModifyDialog, GlobalModifyView};
use katana_window::host::RecordingHost;

use crate::support::{Bench, create, interpreter_host, must, steps};

const GREEN: Color = Color::new(0, 128, 0, 255);

fn on_layer(layer: &str) -> EntityAttributes {
    EntityAttributes {
        layer: String::from(layer),
        style: String::new(),
        color: None,
    }
}

/// Two points on "survey" (p1 coded TREE, p2 coded POLE), a line on "roads". The runner is the
/// window's in miniature: each line kept, then run by an interpreter on the same Document.
struct DialogFixture {
    host: RecordingHost,
    p1: EntityId,
    p2: EntityId,
    road: EntityId,
}

impl DialogFixture {
    fn new() -> Self {
        let mut host = interpreter_host();
        for name in ["survey", "roads"] {
            must(
                &mut host,
                create_layer(Layer {
                    name: String::from(name),
                    ..Layer::default()
                }),
            );
        }
        let p1 = create(
            &mut host,
            create_point(Point2::new(0.0, 0.0), on_layer("survey")),
        );
        let p2 = create(
            &mut host,
            create_point(Point2::new(10.0, 0.0), on_layer("survey")),
        );
        let road = create(
            &mut host,
            create_line(
                Point2::new(0.0, 5.0),
                Point2::new(20.0, 5.0),
                on_layer("roads"),
            ),
        );
        must(
            &mut host,
            set_entity_property(vec![p1], "code", PropertyValue::from("TREE")),
        );
        must(
            &mut host,
            set_entity_property(vec![p2], "code", PropertyValue::from("POLE")),
        );
        Self { host, p1, p2, road }
    }

    /// The dialog over the fixture's drawing, its lines run by the host.
    fn dialog(self) -> Bench<GlobalModifyDialog> {
        self.dialog_with(&CustomisationContext::with_runner())
    }

    fn dialog_with(self, context: &CustomisationContext) -> Bench<GlobalModifyDialog> {
        Bench::new(self.host, |document| {
            GlobalModifyDialog::new(context, document)
        })
    }
}

fn entity(bench: &Bench<GlobalModifyDialog>, id: EntityId) -> Entity {
    bench
        .document()
        .model()
        .entities
        .find(id)
        .unwrap_or_else(|| panic!("{id} is drawn"))
        .clone()
}

/// A radio button of the scope chosen, and the frames after it.
fn choose(bench: &mut Bench<GlobalModifyDialog>, choice: ScopeChoice) {
    bench.dialog.scope_filter_mut().set_choice(choice);
    bench.draw();
}

fn tick_type(bench: &mut Bench<GlobalModifyDialog>, kind: EntityType) {
    bench.dialog.scope_filter_mut().set_type(kind, true);
    bench.draw();
}

fn select(bench: &mut Bench<GlobalModifyDialog>, ids: &[EntityId]) {
    bench
        .host
        .document_mut()
        .selection_mut()
        .set(ids.to_vec());
}

mod global_modify_dialog {
    use super::*;

    #[test]
    fn every_field_starts_unticked_and_its_editor_disabled() {
        let mut bench = DialogFixture::new().dialog();
        assert!(!bench.enabled("globalModifyColour"));
        bench.fill("globalModifySetColour", "on");
        assert!(bench.enabled("globalModifyColour"));
        // Nothing ticked is asking for no change, which the plan refuses; the preview says how
        // many match instead.
        bench.fill("globalModifySetColour", "off");
        choose(&mut bench, ScopeChoice::Drawing);
        assert!(bench.dialog.preview(bench.host.document()));
        assert_eq!(
            bench.dialog.summary_text(),
            "3 entities match. Tick a field to change."
        );
    }

    #[test]
    fn apply_to_checked_layers_with_a_filter_is_one_undo_step() {
        let fixture = DialogFixture::new();
        let (p1, p2) = (fixture.p1, fixture.p2);
        let mut bench = fixture.dialog();
        choose(&mut bench, ScopeChoice::Layers);
        bench.dialog.scope_filter_mut().tick_layer("survey", true);
        bench
            .dialog
            .scope_filter_mut()
            .set_field(FilterField::Property, "code");
        bench
            .dialog
            .scope_filter_mut()
            .set_field(FilterField::Value, "tr*");
        bench.draw();
        bench.fill("globalModifySetColour", "on");
        bench.fill("globalModifyColour", "#008000");
        bench.fill("globalModifySetLayerWeight", "on");
        bench.fill("globalModifyLayerWeight", "0.5");

        assert!(bench.dialog.preview(bench.host.document()));
        assert_eq!(
            bench.dialog.summary_text(),
            "1 entity matched; changing 1 entity and 1 layer."
        );
        let undo_before = bench.steps();

        bench.press("globalModifyApply");

        assert_eq!(entity(&bench, p1).color, Some(GREEN));
        assert_eq!(entity(&bench, p2).color, None);
        let survey = bench.document().model().layers.find("survey");
        assert_eq!(survey.expect("survey is there").line_weight, 0.5);
        assert_eq!(bench.steps(), undo_before + 1);
        let logged = bench.host.logged();
        let (last, is_error) = logged.last().expect("the dialog logged");
        assert!(!is_error);
        assert!(
            last.starts_with("Global Modify: 1 entity matched"),
            "{last}"
        );
    }

    #[test]
    fn a_view_scope_takes_the_views_own_hidden_layers_and_its_area_on_screen() {
        let fixture = DialogFixture::new();
        let road = fixture.road;
        let mut bench = fixture.dialog();
        let mut hides_survey = LayerOverrides::new();
        hides_survey.hide("survey");
        let hides_survey = Arc::new(hides_survey);
        bench.dialog.set_views(Rc::new(move || {
            vec![
                GlobalModifyView {
                    id: 1,
                    title: String::from("Plan 1"),
                    hidden: Some(Arc::clone(&hides_survey)),
                    on_screen: Some(Box2::new(Point2::new(-1.0, 4.0), Point2::new(5.0, 6.0))),
                },
                GlobalModifyView {
                    id: 2,
                    title: String::from("3D 1"),
                    hidden: None,
                    on_screen: None,
                },
            ]
        }));
        bench.dialog.reload(bench.host.document());
        choose(&mut bench, ScopeChoice::View);

        // Plan 1 hides "survey": only the road is drawn there.
        let scope = bench.dialog.scope().expect("the scope reads");
        let matched = match_entities(bench.document(), &scope, &ModifyFilter::default())
            .expect("the scope matches");
        assert_eq!(matched, [road]);

        // The 3D view hides nothing of its own, and has no area on screen to limit to: asking for
        // one is refused, naming why.
        bench.dialog.scope_filter_mut().set_view_index(1);
        let scope = bench.dialog.scope().expect("the scope reads");
        let matched = match_entities(bench.document(), &scope, &ModifyFilter::default())
            .expect("the scope matches");
        assert_eq!(matched.len(), 3);
        bench.dialog.scope_filter_mut().set_on_screen(true);
        assert!(bench.dialog.scope().is_err());
    }

    #[test]
    fn select_matches_runs_the_select_line_the_scope_and_filter_say() {
        let fixture = DialogFixture::new();
        let (p1, p2, road) = (fixture.p1, fixture.p2, fixture.road);
        let mut bench = fixture.dialog();
        choose(&mut bench, ScopeChoice::Drawing);
        tick_type(&mut bench, EntityType::Point);
        assert!(bench.dialog.select_matches(&mut bench.host));
        assert_eq!(bench.lines(), ["SELECT DRAWING WHERE TYPE=point"]);
        assert_eq!(bench.document().selection().ids(), [p1, p2]);
        assert_eq!(bench.dialog.summary_text(), "2 entities selected.");

        // "Selected entities" with a type narrows the selection to that kind.
        select(&mut bench, &[p2, road]);
        choose(&mut bench, ScopeChoice::Selection);
        assert!(bench.dialog.select_matches(&mut bench.host));
        assert_eq!(bench.last_line(), "SELECT SELECTION WHERE TYPE=point");
        assert_eq!(bench.document().selection().ids(), [p2]);
    }

    // SELECT's rule: what is on a locked layer is matched but not selected, and the summary says
    // how many were left out.
    #[test]
    fn select_matches_says_what_a_locked_layer_left_out() {
        let mut fixture = DialogFixture::new();
        let road = fixture.road;
        let mut survey = fixture
            .host
            .document()
            .model()
            .layers
            .find("survey")
            .expect("survey is there")
            .clone();
        survey.locked = true;
        must(&mut fixture.host, update_layer(survey));
        let mut bench = fixture.dialog();
        choose(&mut bench, ScopeChoice::Drawing);
        assert!(bench.dialog.select_matches(&mut bench.host));
        assert_eq!(bench.document().selection().ids(), [road]);
        assert_eq!(
            bench.dialog.summary_text(),
            "1 entity selected. 2 hidden or on a locked layer left out."
        );

        // Only the locked points: nothing selected, and select_matches says so.
        tick_type(&mut bench, EntityType::Point);
        assert!(!bench.dialog.select_matches(&mut bench.host));
        assert!(bench.document().selection().ids().is_empty());
    }

    #[test]
    fn with_no_runner_select_matches_says_the_line_and_selects_nothing() {
        let mut bench = DialogFixture::new().dialog_with(&CustomisationContext::default());
        choose(&mut bench, ScopeChoice::Drawing);
        tick_type(&mut bench, EntityType::Line);
        assert!(!bench.dialog.select_matches(&mut bench.host));
        assert!(
            bench
                .dialog
                .summary_text()
                .contains("SELECT DRAWING WHERE TYPE=line"),
            "{}",
            bench.dialog.summary_text()
        );
        assert!(bench.document().selection().ids().is_empty());
    }

    // Each type box says how many of its kind the scope and the rest of the filter take - what
    // ticking it would select - counted off the fixture's two points and one line.
    #[test]
    fn each_type_box_says_how_many_of_its_kind_the_scope_takes() {
        let mut bench = DialogFixture::new().dialog();
        choose(&mut bench, ScopeChoice::Drawing);
        assert!(bench.dialog.preview(bench.host.document()));
        bench.draw();
        assert_eq!(bench.text("globalModifyTypePoint"), "Point (2)");
        assert_eq!(bench.text("globalModifyTypeLine"), "Line (1)");
        assert_eq!(bench.text("globalModifyTypeLeader"), "Leader (0)");

        // A ticked type does not count itself out: the boxes count the rest.
        bench
            .dialog
            .scope_filter_mut()
            .set_type(EntityType::Point, true);
        bench
            .dialog
            .scope_filter_mut()
            .set_field(FilterField::Layer, "roads");
        assert!(bench.dialog.preview(bench.host.document()));
        bench.draw();
        assert_eq!(bench.text("globalModifyTypePoint"), "Point (0)");
        assert_eq!(bench.text("globalModifyTypeLine"), "Line (1)");

        // A filter that does not read counts nothing: the plain names.
        bench
            .dialog
            .scope_filter_mut()
            .set_field(FilterField::Colour, "greenish");
        assert!(!bench.dialog.preview(bench.host.document()));
        bench.draw();
        assert_eq!(bench.text("globalModifyTypeLine"), "Line");
    }

    #[test]
    fn opened_with_nothing_selected_it_applies_to_the_whole_drawing() {
        let fixture = DialogFixture::new();
        let p1 = fixture.p1;
        let mut bench = fixture.dialog();
        assert!(bench.checked("globalModifyScopeDrawing"));
        // With a selection it opens on the selection, as it always did.
        select(&mut bench, &[p1]);
        let Bench { host, .. } = bench;
        let bench = Bench::new(host, |document: &Document| {
            GlobalModifyDialog::new(&CustomisationContext::with_runner(), document)
        });
        assert!(bench.checked("globalModifyScopeSelection"));
    }

    #[test]
    fn text_that_does_not_read_is_said_in_the_summary_and_changes_nothing() {
        let fixture = DialogFixture::new();
        let p1 = fixture.p1;
        let mut bench = fixture.dialog();
        select(&mut bench, &[p1]);
        bench.fill("globalModifySetColour", "on");
        bench.fill("globalModifyColour", "greenish");
        let undo_before = bench.steps();
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(
            bench.dialog.summary_text().contains("#RRGGBB"),
            "{}",
            bench.dialog.summary_text()
        );
        assert_eq!(bench.steps(), undo_before);
        let logged = bench.host.logged();
        assert!(logged.last().expect("the dialog logged").1);
    }

    #[test]
    fn a_symbol_and_a_style_setting_go_through_their_own_tabs() {
        let fixture = DialogFixture::new();
        let (p1, p2, road) = (fixture.p1, fixture.p2, fixture.road);
        let mut bench = fixture.dialog();
        select(&mut bench, &[p1, p2, road]);
        bench.fill("globalModifySetSymbol", "on");
        bench.fill("globalModifySymbol", "tree");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.summary_text()
        );
        let style = entity(&bench, p1).style;
        assert!(!style.is_empty());
        assert_eq!(entity(&bench, p2).style, style);
        assert_eq!(entity(&bench, road).style, "");
        assert!(
            bench
                .dialog
                .summary_text()
                .contains("1 entity is not a point")
        );

        // Now the style they wear: its symbol size, through the Styles tab.
        bench.fill("globalModifySetSymbol", "off");
        bench.fill("globalModifySetStyleSymbolSize", "on");
        bench.fill("globalModifyStyleSymbolSize", "2");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.summary_text()
        );
        let stored = bench.document().model().styles.find(&style);
        assert_eq!(stored.expect("the style is there").symbol_size, 2.0);
    }

    // Their Styles' "Scale symbol size by" (the owner's "change the symbols size", 2026-11-03):
    // the style p1 wears, a square 0.3 wide, doubled to 0.6 - exactly, since doubling a binary
    // fraction only moves its exponent.
    #[test]
    fn scale_symbol_size_by_multiplies_the_symbol_size_of_the_styles_they_wear() {
        let mut fixture = DialogFixture::new();
        let p1 = fixture.p1;
        let pit = Style {
            name: String::from("Pit"),
            symbol: String::from("square"),
            symbol_size: 0.3,
            ..Style::default()
        };
        must(&mut fixture.host, create_style(pit));
        must(&mut fixture.host, set_entity_style(vec![p1], "Pit"));
        fixture
            .host
            .document_mut()
            .selection_mut()
            .set(vec![p1]);
        let mut bench = fixture.dialog();
        assert!(
            !bench.enabled("globalModifyStyleSymbolScale"),
            "unticked, as every field starts"
        );
        bench.fill("globalModifySetStyleSymbolScale", "on");
        bench.fill("globalModifyStyleSymbolScale", "2");
        let undo_before = bench.steps();
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.summary_text()
        );
        let size = |bench: &Bench<GlobalModifyDialog>| {
            bench
                .document()
                .model()
                .styles
                .find("Pit")
                .expect("Pit is there")
                .symbol_size
        };
        assert_eq!(size(&bench), 0.6);
        assert_eq!(bench.steps(), undo_before + 1);
        assert_eq!(
            bench.dialog.summary_text(),
            "Done: 1 entity matched; changing 1 style. One Undo puts it all back."
        );

        // Symbol size ticked too: a size set and scaled at once is refused.
        bench.fill("globalModifySetStyleSymbolSize", "on");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(
            bench.dialog.summary_text().contains("not both"),
            "{}",
            bench.dialog.summary_text()
        );
        assert_eq!(size(&bench), 0.6);
    }

    #[test]
    fn the_dialog_outlives_its_document_and_then_does_nothing() {
        let fixture = DialogFixture::new();
        let mut dialog = GlobalModifyDialog::new(
            &CustomisationContext::with_runner(),
            fixture.host.document(),
        );
        drop(fixture);
        // A stranger's drawing and executor stand where the fixture's were.
        let mut stranger = DialogFixture::new().host;
        let before = steps(&stranger);
        assert!(!dialog.preview(stranger.document()));
        assert!(!dialog.apply(&mut stranger));
        assert!(!dialog.select_matches(&mut stranger));
        assert_eq!(steps(&stranger), before);
        assert!(stranger.lines().is_empty());
    }
}
