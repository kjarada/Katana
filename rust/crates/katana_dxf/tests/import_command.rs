//! The twin of tests/dxf/test_import_command.cpp: the import as one undo step,
//! on layers the file locks and layers the model has locked.

pub mod support;

use katana_commands::CommandStack;
use katana_dxf::{DxfImport, ImportOptions, import_command, read_dxf_file};
use katana_entity::{Entity, Geometry, Layer, Model, PointGeometry};
use katana_geometry::Point2;
use support::data;

fn load(name: &str) -> DxfImport {
    match read_dxf_file(&data(name), &ImportOptions::default()) {
        Ok(imported) => imported,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn locked(model: &Model, name: &str) -> bool {
    model.layers.find(name).expect("the layer").locked
}

mod dxf_import_command {
    use super::*;

    #[test]
    fn the_import_is_one_undo_step_that_entities_on_a_locked_layer_do_not_break() {
        let mut model = Model::default();
        let mut stack = CommandStack::new(&mut model, None);
        let mut imported = load("r2000_site.dxf");
        let count = imported.entities.len();
        // The file locks layer Boundary, and its polyline is on it: a locked
        // layer refuses new entities, so the lock must come after them.
        let command = import_command(&mut imported, &model).expect("a command");
        if let Err(error) = stack.execute(&mut model, command) {
            panic!("{}", error.describe());
        }
        assert_eq!(model.entities.size(), count);
        assert!(model.layers.find("Boundary").is_some());
        assert!(locked(&model, "Boundary"));
        assert_eq!(
            model
                .linetypes
                .find("CENTER2")
                .expect("CENTER2")
                .pattern
                .len(),
            4
        );
        assert_eq!(stack.undo_count(), 1);
        assert!(stack.undo(&mut model).is_ok());
        assert_eq!(model.entities.size(), 0);
        assert!(model.layers.find("Boundary").is_none());
        assert!(model.linetypes.find("CENTER2").is_none());
    }

    #[test]
    fn importing_the_same_file_twice_adds_its_entities_and_no_layer_twice() {
        let mut model = Model::default();
        let mut stack = CommandStack::new(&mut model, None);
        for _ in 0..2 {
            let mut imported = load("r12_survey.dxf");
            let command = import_command(&mut imported, &model).expect("a command");
            assert!(stack.execute(&mut model, command).is_ok());
        }
        // 11 entities a time; layers 0, KERB, HIDDEN once each.
        assert_eq!(model.entities.size(), 22);
        assert_eq!(model.layers.size(), 3);
    }

    #[test]
    fn a_file_that_adds_nothing_makes_no_command() {
        let model = Model::default();
        let mut empty = DxfImport::default();
        empty.layers.push(Layer::default()); // layer 0, which the model has
        assert!(import_command(&mut empty, &model).is_none());
    }

    #[test]
    fn a_file_imported_a_second_time_comes_in_on_the_layer_the_first_time_locked() {
        // The first import locks Boundary, as the file says, with the site's
        // polyline on it. The second lands on that layer again: it must come in
        // too, and leave Boundary locked as it found it.
        let mut model = Model::default();
        let mut stack = CommandStack::new(&mut model, None);
        let mut count = 0;
        for time in 0..2 {
            let mut imported = load("r2000_site.dxf");
            count = imported.entities.len();
            let command = import_command(&mut imported, &model).expect("a command");
            if let Err(error) = stack.execute(&mut model, command) {
                panic!("import {}: {}", time + 1, error.describe());
            }
            assert!(model.layers.find("Boundary").is_some());
            assert!(locked(&model, "Boundary"), "import {}", time + 1);
        }
        assert_eq!(model.entities.size(), 2 * count);
        // The second import is one step too: undone, the first is what is left,
        // with its lock.
        assert!(stack.undo(&mut model).is_ok());
        assert_eq!(model.entities.size(), count);
        assert!(locked(&model, "Boundary"));
    }

    #[test]
    fn a_layer_locked_by_its_parent_in_the_model_takes_the_import_and_stays_locked() {
        // The model's "survey" is locked, which locks "survey/kerb" under it; the
        // file adds a point on "survey/kerb". The parent is opened for the point
        // and locked again after it; the new child is not locked of itself.
        let mut model = Model::default();
        let survey = Layer {
            name: String::from("survey"),
            locked: true,
            ..Layer::default()
        };
        assert!(model.layers.add(survey).is_ok());
        let mut imported = DxfImport::default();
        imported.layers.push(Layer {
            name: String::from("survey/kerb"),
            ..Layer::default()
        });
        imported.entities.push(Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(1.0, 2.0),
            }),
            layer: String::from("survey/kerb"),
            ..Entity::default()
        });
        let mut stack = CommandStack::new(&mut model, None);
        let command = import_command(&mut imported, &model).expect("a command");
        if let Err(error) = stack.execute(&mut model, command) {
            panic!("{}", error.describe());
        }
        assert_eq!(model.entities.size(), 1);
        assert!(locked(&model, "survey"));
        assert!(model.layers.find("survey/kerb").is_some());
        assert!(!locked(&model, "survey/kerb"));
    }
}
