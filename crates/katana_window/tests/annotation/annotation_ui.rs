//! The twins of tests/qt_widgets/annotation/test_annotation_ui.cpp's AnnotationManagers suite:
//! the text and label style managers, each a thin front end over the verbs, so each edit is
//! checked as the line it runs and one undo step. The managers' runner is the interpreter itself,
//! so a line is checked by what the verb makes of it.
//!
//! The suite's eighth case, TheScaleBoxSetsAndFollowsTheAnnotationScale, installs the annotation
//! workbench and is V3's; AnnotationPaint's four are P's.

use katana_commands::{EntityAttributes, create_polyline};
use katana_entity::{LabelKind, LabelPlacement};
use katana_geometry::{Point2, Polyline2};
use katana_window::annotation::{LabelStyleManagerDialog, TextStyleManagerDialog};

use crate::support::{Bench, interpreter_host};

mod annotation_managers {
    use super::*;

    fn text_styles() -> Bench<TextStyleManagerDialog> {
        Bench::new(interpreter_host(), TextStyleManagerDialog::new)
    }

    fn label_styles() -> Bench<LabelStyleManagerDialog> {
        Bench::new(interpreter_host(), LabelStyleManagerDialog::new)
    }

    fn has_action(dialog: &LabelStyleManagerDialog, name: &str) -> bool {
        dialog
            .insert_menu()
            .actions()
            .iter()
            .any(|action| action.name == name)
    }

    #[test]
    fn the_text_style_manager_applies_as_one_step() {
        let mut bench = text_styles();
        bench.dialog.select(bench.host.document(), "Standard");
        bench.draw();
        bench.fill("textStylePaperHeight", "3.5");
        let steps = bench.steps();
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        let stored = bench
            .document()
            .model()
            .text_styles
            .find("Standard")
            .map(|style| style.paper_height);
        assert_eq!(stored, Some(3.5));
        assert_eq!(bench.steps(), steps + 1);
        assert!(
            bench.dialog.apply(&mut bench.host),
            "applying an unchanged form"
        );
        assert_eq!(bench.steps(), steps + 1, "is no step");

        assert!(bench.dialog.add_style(&mut bench.host));
        assert!(bench.document().model().text_styles.contains("Text Style"));
        bench.dialog.select(bench.host.document(), "Standard");
        assert!(!bench.dialog.delete_style(&mut bench.host));
        assert!(
            !bench.dialog.problem().is_empty(),
            "the refusal is said in the dialog"
        );

        // An undo typed elsewhere shows at once.
        bench.document_mut().undo().expect("the undo is done");
        assert!(!bench.document().model().text_styles.contains("Text Style"));
    }

    #[test]
    fn the_label_style_manager_checks_templates_and_runs_rules() {
        let mut bench = label_styles();
        assert!(bench.dialog.add_defaults(&mut bench.host));
        assert_eq!(bench.document().model().label_styles.size(), 7);
        assert_eq!(bench.steps(), 1);

        bench
            .dialog
            .select(bench.host.document(), "Bearing Distance");
        bench.draw();
        bench.fill("labelStyleTemplate", "{area}");
        assert!(
            !bench.dialog.apply(&mut bench.host),
            "a segment has no area"
        );
        assert!(!bench.dialog.problem().is_empty());
        bench.fill("labelStyleTemplate", "{distance:.2f}\\n{bearing:qb}");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        let text = bench
            .document()
            .model()
            .label_styles
            .find("Bearing Distance")
            .map(|style| style.text.clone());
        assert_eq!(text.as_deref(), Some("{distance:.2f}\n{bearing:qb}"));

        // A rule, run and cleared.
        let triangle = Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(20.0, 0.0),
                Point2::new(20.0, 20.0),
            ],
            true,
        );
        bench
            .document_mut()
            .execute(create_polyline(triangle, EntityAttributes::default()))
            .expect("the polyline is made");
        bench.fill("labelStyleTabs", "Auto-Label Rules");
        bench.fill("labelRuleName", "areas");
        bench.fill("labelRuleStyle", "Lot Area");
        assert!(
            bench.dialog.add_rule(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert!(
            bench.dialog.run_rules(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert!(
            bench.dialog.run_report().starts_with("autolabel created=1"),
            "{}",
            bench.dialog.run_report()
        );
        assert!(bench.dialog.clear_rules(&mut bench.host));
        assert_eq!(bench.dialog.run_report(), "removed=1");
    }

    #[test]
    fn new_takes_the_name_typed_and_apply_sends_only_what_changed() {
        let mut bench = text_styles();
        assert_eq!(bench.control("textStyleNewName").text, "Text Style");
        bench.fill("textStyleNewName", "Road Names");
        assert!(
            bench.dialog.add_style(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("TEXTSTYLE NEW \"Road Names\"")
        );
        assert!(bench.document().model().text_styles.contains("Road Names"));
        bench.draw();
        assert!(bench.value("textStyleNewName").is_empty());

        bench.fill("textStylePaperHeight", "3.5");
        bench.fill("textStyleBold", "on");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("TEXTSTYLE SET \"Road Names\" paper=3.5 bold=on")
        );
        let stored = bench
            .document()
            .model()
            .text_styles
            .find("Road Names")
            .map(|style| style.paper_height);
        assert_eq!(stored, Some(3.5));

        // A name a line cannot carry is refused before anything runs.
        let lines = bench.lines().len();
        bench.fill("textStyleNewName", "say \"hi\"");
        assert!(!bench.dialog.add_style(&mut bench.host));
        assert_eq!(bench.lines().len(), lines);
        assert!(
            bench.dialog.problem().contains("double quote"),
            "{}",
            bench.dialog.problem()
        );
    }

    #[test]
    fn a_new_label_style_has_the_name_and_kind_chosen() {
        let mut bench = label_styles();
        bench.fill("labelStyleNewName", "Lot areas");
        bench.fill("labelStyleNewKind", "area");
        assert!(
            bench.dialog.add_style(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("LABELSTYLE NEW \"Lot areas\" kind=area")
        );
        let made = bench
            .document()
            .model()
            .label_styles
            .find("Lot areas")
            .cloned()
            .expect("the style is made");
        assert_eq!(made.kind, LabelKind::Area);
        // LABELSTYLE NEW's own start for an area: at the centroid.
        assert_eq!(made.placement, LabelPlacement::Centroid);
        bench.draw();
        assert_eq!(
            bench.value("labelStyleKind"),
            "area",
            "the new style is the one shown"
        );
    }

    #[test]
    fn the_insert_value_menu_offers_what_the_kind_takes_with_its_steps() {
        let mut bench = label_styles();
        assert!(bench.dialog.add_defaults(&mut bench.host));
        bench
            .dialog
            .select(bench.host.document(), "Bearing Distance");
        // A segment's bearing in DMS and its distance to three places; a segment has no area, and
        // a bearing no square metres.
        assert!(has_action(&bench.dialog, "labelValue:bearing:dms"));
        assert!(has_action(&bench.dialog, "labelValue:distance:.3f"));
        assert!(has_action(&bench.dialog, "labelValue:layer:upper"));
        assert!(!has_action(&bench.dialog, "labelValue:area"));
        assert!(!has_action(&bench.dialog, "labelValue:bearing:m2"));

        bench.draw();
        bench.fill("labelStyleTemplate", "");
        assert!(bench.dialog.trigger_insert("labelValue:bearing:dms"));
        bench.dialog.type_in_template(" ");
        bench.dialog.insert_value("distance:.2f");
        bench.draw();
        assert_eq!(
            bench.value("labelStyleTemplate"),
            "{bearing:dms} {distance:.2f}"
        );
        bench.dialog.insert_value("prop.NAME");
        assert_eq!(
            bench.dialog.template_selected_text(),
            "NAME",
            "left to type over"
        );
        bench.dialog.type_in_template("owner");
        bench.draw();
        assert_eq!(
            bench.value("labelStyleTemplate"),
            "{bearing:dms} {distance:.2f}{prop.owner}"
        );

        // Another kind, other values: a point's level.
        bench.dialog.select(bench.host.document(), "Spot Level");
        assert!(has_action(&bench.dialog, "labelValue:z:.3f"));
        assert!(!has_action(&bench.dialog, "labelValue:bearing"));
    }

    #[test]
    fn rules_are_added_with_their_type_edited_and_switched_by_lines() {
        let mut bench = label_styles();
        assert!(bench.dialog.add_defaults(&mut bench.host));
        bench.draw();
        bench.fill("labelStyleTabs", "Auto-Label Rules");
        bench.fill("labelRuleName", "lots");
        bench.fill("labelRuleStyle", "Lot Area");
        bench.fill("labelRuleType", "Polyline");
        assert!(
            bench.dialog.add_rule(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("AUTOLABEL RULE ADD lots style=\"Lot Area\" type=Polyline enabled=on")
        );
        let rule = |bench: &Bench<LabelStyleManagerDialog>| {
            bench
                .document()
                .model()
                .label_rules
                .find("lots")
                .cloned()
                .expect("the rule is there")
        };
        assert_eq!(rule(&bench).entity_type, "Polyline");

        // Chosen in the table, the rule fills the form; Update stores it whole.
        bench.draw();
        assert_eq!(bench.rows("labelRuleTable"), 1);
        bench.fill("labelRuleName", "");
        bench.fill("labelRuleTable", "lots");
        assert_eq!(bench.value("labelRuleName"), "lots");
        bench.fill("labelRuleLayer", "BDY*");
        assert!(
            bench.dialog.update_rule(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some(
                "AUTOLABEL RULE SET lots style=\"Lot Area\" layer=BDY* code=\"\" type=Polyline \
                 labellayer=\"\" enabled=on"
            )
        );
        assert_eq!(rule(&bench).layer, "BDY*");

        // The table's Enabled box switches the rule with a line of its own.
        let before = bench.steps();
        bench.dialog.tick_rule_enabled(0, false);
        bench.draw(); // run once the box's frame is done
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("AUTOLABEL RULE SET lots enabled=off")
        );
        assert!(!rule(&bench).enabled);
        assert_eq!(bench.steps(), before + 1);
        assert_eq!(
            bench.row("labelRuleTable", 0)[6],
            "no",
            "the table shows it"
        );
    }

    #[test]
    fn run_preview_and_clear_act_on_the_chosen_rules_and_count_their_labels() {
        let mut bench = label_styles();
        assert!(bench.dialog.add_defaults(&mut bench.host));
        bench.run("RECT 0,0 20,20");
        bench.run("AUTOLABEL RULE ADD lots style=\"Lot Area\"");
        bench.run("AUTOLABEL RULE ADD sides style=\"Bearing Distance\"");
        bench.draw();
        bench.fill("labelStyleTabs", "Auto-Label Rules");
        assert_eq!(bench.rows("labelRuleTable"), 2);
        // Rules are listed by name: lots, then sides.
        bench.fill("labelRuleTable", "lots");
        assert_eq!(bench.dialog.chosen_rules(), ["lots"]);

        // One label entity a target a rule matches (auto_label): the rectangle once for its area.
        let entities = bench.document().model().entities.size();
        assert!(
            bench.dialog.preview_rules(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("AUTOLABEL PREVIEW lots")
        );
        assert_eq!(
            bench.dialog.run_report(),
            "preview created=1 kept=0 removed=0 skipped=0"
        );
        assert_eq!(
            bench.document().model().entities.size(),
            entities,
            "a preview makes nothing"
        );
        bench.draw();
        assert_eq!(bench.row("labelRuleTable", 0)[7], "1", "the Labels column");

        assert!(bench.dialog.run_rules(&mut bench.host));
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("AUTOLABEL RUN lots")
        );
        assert_eq!(
            bench.document().model().entities.size(),
            entities + 1,
            "only the chosen rule ran"
        );

        // None chosen: every enabled rule. The area label is kept, the sides' made - one label
        // for the rectangle's four segments.
        bench.dialog.choose_rules(&[]);
        assert!(bench.dialog.run_rules(&mut bench.host));
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("AUTOLABEL RUN")
        );
        assert_eq!(
            bench.dialog.run_report(),
            "autolabel created=1 kept=1 removed=0 skipped=0"
        );
        bench.draw();
        assert_eq!(bench.row("labelRuleTable", 1)[7], "1");

        bench.fill("labelRuleTable", "sides");
        assert!(bench.dialog.clear_rules(&mut bench.host));
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("AUTOLABEL CLEAR sides")
        );
        assert_eq!(bench.dialog.run_report(), "removed=1");
        assert_eq!(bench.document().model().entities.size(), entities + 1);
    }
}
