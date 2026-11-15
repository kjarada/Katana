// The interactive tool framework itself (interactive_tool.rs;
// tests/cad/tools/test_tool_framework.cpp): the catalogue's rules,
// typed-input routing and the defaults every tool inherits. Each tool family
// has its own test file beside this one.

use std::sync::Arc;

use katana_cad::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolFactory, ToolInfo, ToolInput, ToolStart,
    ToolStep, ToolStepOutcome, route_typed_input, tool_catalog_problems,
};
use katana_core::ErrorCode;
use katana_geometry::Point2;

use crate::{double_eq, near};

// Records what reached it, so routing can be observed.
struct Recorder {
    wants: ToolInput,
    points: Vec<Point2>,
    values: Vec<String>,
    last: Option<Point2>,
}

impl Recorder {
    fn new(wants: ToolInput) -> Self {
        Self {
            wants,
            points: Vec::new(),
            values: Vec::new(),
            last: None,
        }
    }
}

impl InteractiveTool for Recorder {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from("record")
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        self.wants
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        self.points.push(at);
        self.last = Some(at);
        ToolStep::next("")
    }

    fn value(&mut self, _cx: &ToolContext<'_>, text: &str) -> ToolStep {
        self.values.push(String::from(text));
        ToolStep::next("")
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.last
    }
}

fn make_recorder(_cx: &ToolContext<'_>, _start: &ToolStart) -> Box<dyn InteractiveTool> {
    Box::new(Recorder::new(ToolInput::Point))
}

fn valid_info(id: &str, aliases: &[&str]) -> ToolInfo {
    let make: ToolFactory = Arc::new(make_recorder);
    ToolInfo {
        id: String::from(id),
        name: String::from("Test"),
        category: String::from("Draw"),
        group: String::from("Lines"),
        aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
        make: Some(make),
        ..ToolInfo::default()
    }
}

mod tool_framework {
    use super::*;

    #[test]
    fn typed_coordinates_are_points_and_bare_numbers_are_values() {
        let cx = ToolContext::default();
        let mut tool = Recorder::new(ToolInput::Point);
        assert_eq!(
            route_typed_input(&mut tool, &cx, "10,20").outcome,
            ToolStepOutcome::Continue
        );
        assert_eq!(
            route_typed_input(&mut tool, &cx, "12.5").outcome,
            ToolStepOutcome::Continue
        );
        assert_eq!(tool.points, vec![Point2::new(10.0, 20.0)]);
        assert_eq!(tool.values, vec![String::from("12.5")]);
    }

    #[test]
    fn relative_and_polar_input_measure_from_the_tools_last_point() {
        let cx = ToolContext::default();
        let mut tool = Recorder::new(ToolInput::Point);
        let _ = route_typed_input(&mut tool, &cx, "10,20");
        let _ = route_typed_input(&mut tool, &cx, "@3,4");
        assert_eq!(tool.points.len(), 2);
        assert_eq!(tool.points[1], Point2::new(13.0, 24.0));
        // 5 at 90 degrees is straight up: (13, 24) + (0, 5). cos(90 deg) is
        // not exactly 0 in binary, so x is compared to within a few ulps of
        // 13.
        let _ = route_typed_input(&mut tool, &cx, "@5<90");
        assert_eq!(tool.points.len(), 3);
        near(tool.points[2].x, 13.0, 1e-12);
        double_eq(tool.points[2].y, 29.0);
    }

    #[test]
    fn relative_input_before_any_point_is_refused_not_measured_from_the_origin() {
        let cx = ToolContext::default();
        let mut tool = Recorder::new(ToolInput::Point);
        let step = route_typed_input(&mut tool, &cx, "@3,4");
        assert_eq!(step.outcome, ToolStepOutcome::Rejected);
        assert!(tool.points.is_empty());
    }

    #[test]
    fn a_malformed_point_is_refused_with_a_reason() {
        let cx = ToolContext::default();
        let mut tool = Recorder::new(ToolInput::Point);
        let step = route_typed_input(&mut tool, &cx, "10,abc");
        assert_eq!(step.outcome, ToolStepOutcome::Rejected);
        assert!(!step.message.is_empty());
        assert!(tool.points.is_empty());
    }

    #[test]
    fn a_text_value_with_a_comma_is_text_when_the_tool_wants_a_value() {
        let cx = ToolContext::default();
        let mut tool = Recorder::new(ToolInput::Value);
        let _ = route_typed_input(&mut tool, &cx, "Road, north side");
        assert!(tool.points.is_empty());
        assert_eq!(tool.values, vec![String::from("Road, north side")]);
    }

    #[test]
    fn the_defaults_refuse_what_a_tool_does_not_take_and_say_what_it_wants() {
        let cx = ToolContext::default();
        let mut tool = Recorder::new(ToolInput::Point);
        // Recorder takes neither an entity nor an undo nor Enter: these are
        // the trait's defaults.
        let step = tool.entity(&cx, 7, Point2::new(0.0, 0.0));
        assert_eq!(step.outcome, ToolStepOutcome::Rejected);
        assert!(step.message.contains("point"), "{}", step.message);
        assert_eq!(tool.undo(&cx).outcome, ToolStepOutcome::Rejected);
        let finished = tool.enter(&cx);
        assert_eq!(finished.outcome, ToolStepOutcome::Done);
        assert!(finished.command.is_none());
    }

    #[test]
    fn the_catalogue_refuses_a_duplicate_id_or_alias() {
        let mut catalog = ToolCatalog::new();
        assert!(catalog.add(valid_info("test.one", &["ONE", "O1"])).is_ok());
        let same_id = catalog
            .add(valid_info("test.one", &["OTHER"]))
            .expect_err("a second test.one");
        assert_eq!(same_id.code, ErrorCode::AlreadyExists);
        let same_alias = catalog
            .add(valid_info("test.two", &["O1"]))
            .expect_err("a second O1");
        assert_eq!(same_alias.code, ErrorCode::AlreadyExists);
        assert_eq!(catalog.size(), 1);
    }

    #[test]
    fn the_catalogue_refuses_incomplete_entries() {
        let mut catalog = ToolCatalog::new();
        let no_factory = ToolInfo {
            make: None,
            ..valid_info("test.a", &["A1"])
        };
        assert!(catalog.add(no_factory).is_err());
        let lower_alias = valid_info("test.b", &["line"]);
        assert!(catalog.add(lower_alias).is_err());
        let letter_shortcut = ToolInfo {
            shortcut: String::from("L"),
            ..valid_info("test.c", &["C1"])
        };
        assert!(catalog.add(letter_shortcut).is_err());
        let no_group = ToolInfo {
            group: String::new(),
            ..valid_info("test.d", &["D1"])
        };
        assert!(catalog.add(no_group).is_err());
        assert_eq!(catalog.size(), 0);
    }

    #[test]
    fn aliases_are_found_whatever_the_case_the_user_types() {
        let mut catalog = ToolCatalog::new();
        assert!(catalog.add(valid_info("test.one", &["ONE"])).is_ok());
        assert_eq!(
            catalog.find_by_alias("one").map(|tool| tool.id.as_str()),
            Some("test.one")
        );
        assert!(catalog.find_by_alias("two").is_none());
    }

    #[test]
    fn the_catalogue_lists_tools_in_menu_order() {
        let mut catalog = ToolCatalog::new();
        let late = ToolInfo {
            order: 2,
            ..valid_info("test.late", &["LATE"])
        };
        let early = ToolInfo {
            order: 1,
            ..valid_info("test.early", &["EARLY"])
        };
        let modify = ToolInfo {
            category: String::from("Modify"),
            ..valid_info("test.modify", &["MOD"])
        };
        assert!(catalog.add(late).is_ok());
        assert!(catalog.add(modify).is_ok());
        assert!(catalog.add(early).is_ok());
        let all: Vec<&str> = catalog
            .all()
            .into_iter()
            .map(|tool| tool.id.as_str())
            .collect();
        assert_eq!(all, vec!["test.early", "test.late", "test.modify"]);
    }

    #[test]
    fn every_tool_in_the_programs_catalogue_registered() {
        // A family's tool that the catalogue refused would be missing from
        // every menu with nothing to say why; the refusals are kept so this
        // can see them.
        let problems = tool_catalog_problems();
        assert!(problems.is_empty(), "{problems:?}");
    }
}
