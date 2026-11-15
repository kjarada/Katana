// Match Properties (modify_properties.rs), Select Similar and Quick Select
// (select_tools.rs), and what Esc keeps of Copy's work (modify_transform.rs,
// InteractiveTool::cancel). Each drawing is small enough that which entities
// a selection must hold is counted by hand from the set-up
// (tests/cad/tools/test_select_and_match.cpp).

use katana_commands::{create_entities, create_layer, create_style};
use katana_entity::{Color, Entity, EntityId, Geometry, Layer, PropertyMap, Style};
use katana_geometry::Segment2;

use crate::tool_driver::ToolDriver;
use crate::{ok, pt};

#[track_caller]
fn add_layer(driver: &mut ToolDriver, name: &str) {
    let layer = Layer {
        name: String::from(name),
        ..Layer::default()
    };
    ok(driver.document_mut().execute(create_layer(layer)));
}

#[track_caller]
fn add_style(driver: &mut ToolDriver, name: &str) {
    let style = Style {
        name: String::from(name),
        ..Style::default()
    };
    ok(driver.document_mut().execute(create_style(style)));
}

// The C++ addEntity, its defaults ("0", no style, no colour, no
// properties) given by the caller.
#[track_caller]
fn add_entity(
    driver: &mut ToolDriver,
    geometry: Geometry,
    layer: &str,
    style: &str,
    color: Option<Color>,
    properties: PropertyMap,
) -> EntityId {
    let entity = Entity {
        geometry,
        layer: String::from(layer),
        style: String::from(style),
        color,
        properties,
        ..Entity::default()
    };
    driver.add(create_entities(vec![entity]))
}

#[track_caller]
fn entity_of(driver: &ToolDriver, id: EntityId) -> Entity {
    match driver.document().model().entities.find(id) {
        Some(entity) => entity.clone(),
        None => panic!("entity {id} is not in the drawing"),
    }
}

fn selected(driver: &ToolDriver) -> Vec<EntityId> {
    driver.document().selection().ids()
}

fn line(y: f64) -> Geometry {
    Geometry::Line(Segment2::new(pt(0.0, y), pt(10.0, y)))
}

// ---- Match Properties --------------------------------------------------------------

mod match_properties {
    use katana_commands::update_layer;
    use katana_entity::{Color, Geometry, Layer, PropertyMap};
    use katana_geometry::Circle2;

    use super::{add_entity, add_layer, add_style, entity_of, line};
    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE, REJECTED, ok, pt};

    #[test]
    fn the_targets_take_the_sources_layer_style_and_colour_in_one_command() {
        let mut driver = ToolDriver::new();
        add_layer(&mut driver, "ROAD");
        add_style(&mut driver, "TB");
        let Ok(red) = Color::from_hex("#FF0000") else {
            panic!("#FF0000 is a colour");
        };
        let source = add_entity(
            &mut driver,
            line(0.0),
            "ROAD",
            "TB",
            Some(red),
            PropertyMap::new(),
        );
        let a = add_entity(&mut driver, line(5.0), "0", "", None, PropertyMap::new());
        let b = add_entity(
            &mut driver,
            Geometry::Circle(Circle2::new(pt(0.0, 0.0), 1.0)),
            "0",
            "",
            None,
            PropertyMap::new(),
        );
        driver.start("modify.match_properties");
        assert_eq!(
            driver.expects(),
            katana_cad::interactive_tool::ToolInput::Entity
        );
        assert_eq!(
            driver.pick(source, 5.0, 0.0).message,
            "Source: layer ROAD, style TB, colour #FF0000"
        );
        assert_eq!(
            driver.expects(),
            katana_cad::interactive_tool::ToolInput::Selection
        );
        driver.pick(a, 5.0, 5.0);
        driver.pick(b, 1.0, 0.0);
        let step = driver.enter();
        assert_eq!(step.outcome, DONE);
        assert_eq!(step.message, "Matched layer, colour and style on 2 objects");
        assert_eq!(driver.executed(), 1);
        for id in [a, b] {
            assert_eq!(entity_of(&driver, id).layer, "ROAD");
            assert_eq!(entity_of(&driver, id).style, "TB");
            assert_eq!(entity_of(&driver, id).color, Some(red));
        }
        ok(driver.document_mut().undo());
        assert_eq!(entity_of(&driver, a).layer, "0");
        assert_eq!(entity_of(&driver, b).style, "");
    }

    #[test]
    fn settings_choose_which_properties_go_across() {
        let mut driver = ToolDriver::new();
        add_layer(&mut driver, "ROAD");
        add_style(&mut driver, "TB");
        let Ok(green) = Color::from_hex("#00FF00") else {
            panic!("#00FF00 is a colour");
        };
        let source = add_entity(
            &mut driver,
            line(0.0),
            "ROAD",
            "TB",
            Some(green),
            PropertyMap::new(),
        );
        let target = add_entity(&mut driver, line(5.0), "0", "", None, PropertyMap::new());
        driver.start("modify.match_properties");
        driver.pick(source, 5.0, 0.0);
        assert_eq!(driver.r#type("S").outcome, CONTINUE);
        assert_eq!(driver.r#type("banana").outcome, REJECTED);
        assert_eq!(driver.r#type("L").message, "Copying layer");
        driver.pick(target, 5.0, 5.0);
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(entity_of(&driver, target).layer, "ROAD");
        assert_eq!(entity_of(&driver, target).style, "");
        assert!(entity_of(&driver, target).color.is_none());
        // Remembered for the next use, as AutoCAD remembers them; put back.
        driver.start("modify.match_properties");
        driver.pick(source, 5.0, 0.0);
        assert_eq!(
            driver.prompt(),
            "Select destination objects or [Settings], then press Enter"
        );
        driver.r#type("S");
        assert_eq!(
            driver.r#type("All").message,
            "Copying layer, colour and style"
        );
    }

    #[test]
    fn a_target_on_a_locked_layer_is_left_and_said_so() {
        let mut driver = ToolDriver::new();
        add_layer(&mut driver, "ROAD");
        add_layer(&mut driver, "LOCKED");
        let source = add_entity(&mut driver, line(0.0), "ROAD", "", None, PropertyMap::new());
        let open = add_entity(&mut driver, line(5.0), "0", "", None, PropertyMap::new());
        let locked = add_entity(
            &mut driver,
            line(10.0),
            "LOCKED",
            "",
            None,
            PropertyMap::new(),
        );
        let locked_layer = Layer {
            name: String::from("LOCKED"),
            locked: true,
            ..Layer::default()
        };
        ok(driver.document_mut().execute(update_layer(locked_layer)));
        driver.start("modify.match_properties");
        driver.pick(source, 5.0, 0.0);
        driver.pick(open, 5.0, 5.0);
        driver.pick(locked, 5.0, 10.0);
        let step = driver.enter();
        assert_eq!(step.outcome, DONE);
        assert_eq!(
            step.message,
            "Matched layer, colour and style on 1 object; 1 on a locked layer left as it is"
        );
        assert_eq!(entity_of(&driver, open).layer, "ROAD");
        assert_eq!(entity_of(&driver, locked).layer, "LOCKED");
    }
}

// ---- Select Similar ----------------------------------------------------------------

mod select_similar {
    use katana_entity::{Geometry, PropertyMap};
    use katana_geometry::Circle2;

    use super::{add_entity, add_layer, add_style, line, selected};
    use crate::tool_driver::ToolDriver;
    use crate::{DONE, pt};

    #[test]
    fn a_picked_line_selects_the_lines_of_its_layer_and_style_only() {
        let mut driver = ToolDriver::new();
        add_layer(&mut driver, "A");
        add_layer(&mut driver, "B");
        add_style(&mut driver, "TB");
        let a1 = add_entity(&mut driver, line(0.0), "A", "", None, PropertyMap::new());
        let a2 = add_entity(&mut driver, line(1.0), "A", "", None, PropertyMap::new());
        let a3 = add_entity(&mut driver, line(2.0), "A", "", None, PropertyMap::new());
        // other layer
        add_entity(&mut driver, line(3.0), "B", "", None, PropertyMap::new());
        // other style
        let styled = add_entity(&mut driver, line(4.0), "A", "TB", None, PropertyMap::new());
        let c1 = add_entity(
            &mut driver,
            Geometry::Circle(Circle2::new(pt(0.0, 0.0), 1.0)),
            "A",
            "",
            None,
            PropertyMap::new(),
        );
        let c2 = add_entity(
            &mut driver,
            Geometry::Circle(Circle2::new(pt(5.0, 0.0), 1.0)),
            "A",
            "",
            None,
            PropertyMap::new(),
        );
        driver.document_mut().selection_mut().set(vec![a1]);
        driver.start("select.similar");
        let step = driver.enter();
        assert_eq!(step.outcome, DONE);
        assert!(step.command.is_none());
        // The three plain lines on A: not the one on B, the styled one or the
        // circles.
        assert_eq!(selected(&driver), vec![a1, a2, a3]);
        assert_eq!(
            step.message,
            "3 entities selected: the same kind, layer and style as the 1 picked"
        );

        // Matched on the layer alone: everything on A.
        driver.start("select.similar");
        driver.r#type("SE");
        assert_eq!(driver.r#type("L").message, "Matching on layer");
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(selected(&driver), vec![a1, a2, a3, styled, c1, c2]);
        // The settings are the program's, kept between uses: put them back.
        driver.start("select.similar");
        driver.r#type("SE");
        assert_eq!(
            driver.r#type("kind,layer,style").message,
            "Matching on kind, layer and style"
        );
    }
}

// ---- Quick Select ------------------------------------------------------------------

mod quick_select {
    use katana_cad::interactive_tool::{ToolInput, ToolStep};
    use katana_entity::{EntityId, Geometry, PointGeometry, PropertyMap, PropertyValue};

    use super::{add_entity, add_layer, line, selected};
    use crate::tool_driver::ToolDriver;
    use crate::{DONE, REJECTED, pt};

    // The C++ fixture QuickSelect: three survey points and a line.
    struct QuickSelect {
        driver: ToolDriver,
        p1: EntityId,
        p2: EntityId,
        p3: EntityId,
        road: EntityId,
    }

    fn point_at(x: f64, code: &str, elevation: f64) -> (Geometry, PropertyMap) {
        let geometry = Geometry::Point(PointGeometry {
            position: pt(x, 0.0),
        });
        let properties = PropertyMap::from([
            (String::from("code"), PropertyValue::from(code)),
            (String::from("elevation"), PropertyValue::from(elevation)),
        ]);
        (geometry, properties)
    }

    impl QuickSelect {
        #[track_caller]
        fn new() -> Self {
            let mut driver = ToolDriver::new();
            add_layer(&mut driver, "SURVEY");
            add_layer(&mut driver, "ROAD");
            let (geometry, properties) = point_at(0.0, "TB", 10.0);
            let p1 = add_entity(&mut driver, geometry, "SURVEY", "", None, properties);
            let (geometry, properties) = point_at(1.0, "TB", 20.0);
            let p2 = add_entity(&mut driver, geometry, "SURVEY", "", None, properties);
            let (geometry, properties) = point_at(2.0, "FC", 30.0);
            let p3 = add_entity(&mut driver, geometry, "SURVEY", "", None, properties);
            let road = add_entity(&mut driver, line(5.0), "ROAD", "", None, PropertyMap::new());
            Self {
                driver,
                p1,
                p2,
                p3,
                road,
            }
        }

        // Kind, layer and condition typed in turn; "" is Enter.
        #[track_caller]
        fn run(&mut self, kind: &str, layer: &str, condition: &str) -> ToolStep {
            self.driver.start("select.quick");
            assert_eq!(self.driver.expects(), ToolInput::Value);
            self.answer(kind);
            self.answer(layer);
            self.answer(condition)
        }

        fn answer(&mut self, text: &str) -> ToolStep {
            if text.is_empty() {
                self.driver.enter()
            } else {
                self.driver.r#type(text)
            }
        }
    }

    #[test]
    fn a_numeric_condition_compares_heights_as_numbers() {
        let mut fixture = QuickSelect::new();
        let step = fixture.run("", "", "elevation > 15");
        assert_eq!(step.outcome, DONE);
        // 20 and 30 exceed 15; 10 does not, and the line has no elevation.
        assert_eq!(selected(&fixture.driver), vec![fixture.p2, fixture.p3]);
        assert_eq!(
            step.message,
            "2 entities selected: any kind on any layer where elevation > 15, in the drawing"
        );
    }

    #[test]
    fn kind_layer_wildcard_and_a_text_wildcard_combine() {
        let mut fixture = QuickSelect::new();
        assert_eq!(fixture.run("point", "surv*", "code = t?").outcome, DONE);
        assert_eq!(selected(&fixture.driver), vec![fixture.p1, fixture.p2]);
        // With nothing selected, so it does not ask where to look.
        fixture.driver.document_mut().selection_mut().clear();
        assert_eq!(fixture.run("Line", "", "").outcome, DONE);
        assert_eq!(selected(&fixture.driver), vec![fixture.road]);
    }

    #[test]
    fn not_equal_selects_only_entities_that_have_the_property() {
        let mut fixture = QuickSelect::new();
        assert_eq!(fixture.run("", "", "code <> TB").outcome, DONE);
        // FC only: the line has no code at all, which is not "a code other
        // than TB".
        assert_eq!(selected(&fixture.driver), vec![fixture.p3]);
    }

    #[test]
    fn ordering_text_and_a_malformed_condition_are_refused() {
        let mut fixture = QuickSelect::new();
        assert_eq!(fixture.run("", "", "code > TB").outcome, REJECTED);
        assert_eq!(fixture.driver.r#type("no operator here").outcome, REJECTED);
        // still at the condition
        assert_eq!(fixture.driver.r#type("Point").outcome, REJECTED);
        fixture.driver.start("select.quick");
        assert_eq!(fixture.driver.r#type("Banana").outcome, REJECTED);
    }

    #[test]
    fn with_a_selection_given_it_asks_where_to_look() {
        let mut fixture = QuickSelect::new();
        let (p1, p2, p3, road) = (fixture.p1, fixture.p2, fixture.p3, fixture.road);
        let driver = &mut fixture.driver;
        driver
            .document_mut()
            .selection_mut()
            .set(vec![p1, p3, road]);
        driver.start("select.quick");
        driver.r#type("Point");
        driver.enter();
        driver.enter();
        assert_eq!(driver.prompt(), "Look in [Drawing/Selection] <Selection>");
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(selected(driver), vec![p1, p3]);

        driver.document_mut().selection_mut().set(vec![p1]);
        driver.start("select.quick");
        driver.r#type("Point");
        driver.enter();
        driver.enter();
        assert_eq!(driver.r#type("D").outcome, DONE);
        assert_eq!(selected(driver), vec![p1, p2, p3]);
    }
}

// ---- Esc keeps what Copy placed -------------------------------------------------------

fn line_count(driver: &ToolDriver) -> usize {
    let mut count = 0;
    driver.document().model().entities.for_each(|entity| {
        if matches!(entity.geometry, Geometry::Line(_)) {
            count += 1;
        }
    });
    count
}

mod copy_escape {
    use katana_commands::{EntityAttributes, create_line};

    use super::line_count;
    use crate::tool_driver::ToolDriver;
    use crate::{DONE, ok, pt};

    #[test]
    fn esc_keeps_the_copies_already_placed_as_one_command() {
        let mut driver = ToolDriver::new();
        let original = driver.add(create_line(
            pt(0.0, 0.0),
            pt(1.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.document_mut().selection_mut().set(vec![original]);
        driver.start("modify.copy");
        driver.click(0.0, 0.0); // base point
        driver.click(10.0, 0.0); // a copy at +10
        driver.click(20.0, 0.0); // and at +20
        let step = driver.cancel();
        assert_eq!(step.outcome, DONE);
        assert_eq!(driver.executed(), 1);
        assert_eq!(line_count(&driver), 3);
        ok(driver.document_mut().undo());
        assert_eq!(line_count(&driver), 1);
    }

    #[test]
    fn esc_with_no_copy_placed_applies_no_default() {
        let mut driver = ToolDriver::new();
        let original = driver.add(create_line(
            pt(0.0, 0.0),
            pt(1.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.document_mut().selection_mut().set(vec![original]);
        driver.start("modify.copy");
        // At the second point with none placed, Enter would copy by the base
        // point as a displacement; Esc must not.
        driver.click(5.0, 5.0);
        driver.cancel();
        assert_eq!(driver.executed(), 0);
        assert_eq!(line_count(&driver), 1);
    }
}

mod line_escape {
    use super::{entity_of, line_count};
    use crate::tool_driver::ToolDriver;

    #[test]
    fn esc_keeps_the_chain_but_move_keeps_nothing() {
        let mut driver = ToolDriver::new();
        driver.start("draw.line");
        driver.click(0.0, 0.0);
        driver.click(10.0, 0.0);
        driver.click(10.0, 10.0);
        let step = driver.cancel();
        assert_eq!(step.message, "2 lines");
        assert_eq!(line_count(&driver), 2);

        let first = driver.document().last_created_entities()[0];
        let before = entity_of(&driver, first).geometry;
        driver.document_mut().selection_mut().set(vec![first]);
        driver.start("modify.move");
        driver.click(3.0, 3.0); // base point; Enter now would move by (3, 3)
        driver.cancel();
        assert_eq!(driver.executed(), 1);
        assert_eq!(entity_of(&driver, first).geometry, before);
    }
}

mod fillet_escape {
    use katana_cad::interactive_tool::ToolInput;

    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE};

    #[test]
    fn esc_at_the_radius_prompt_stores_no_radius_and_keeps_the_corners() {
        // The work-keeping tools now say it themselves (keep_work_on_escape),
        // with the host's old rule: out of a value prompt first, then Enter.
        let mut driver = ToolDriver::new();
        driver.start("modify.fillet");
        assert_eq!(driver.r#type("R").outcome, CONTINUE);
        assert_eq!(driver.expects(), ToolInput::Value);
        let step = driver.cancel();
        assert_eq!(step.outcome, DONE);
        assert!(step.command.is_none());
        assert_eq!(driver.executed(), 0);
    }
}
