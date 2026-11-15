//! The twin of tests/commands/test_command_stack_names.cpp:
//! CommandStack::undo_names and redo_names, the history the Edit toolbar's
//! Undo and Redo lists show, newest first, where the k-th name is the step
//! UNDO k or REDO k takes back or puts back last. The names are the commands'
//! own: create_point's is CREATE_POINT, create_line's CREATE_LINE.

use katana_commands::{CommandStack, EntityAttributes, create_circle, create_line, create_point};
use katana_entity::Model;
use katana_geometry::Point2;

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn none() -> EntityAttributes {
    EntityAttributes::default()
}

mod command_stack_names {
    use super::*;

    #[test]
    fn an_empty_history_has_no_names() {
        let mut model = Model::new();
        let stack = CommandStack::new(&mut model, None);
        assert!(stack.undo_names().is_empty());
        assert!(stack.redo_names().is_empty());
    }

    #[test]
    fn the_undo_history_is_newest_first_and_begins_with_undo_name() {
        let mut model = Model::new();
        let mut stack = CommandStack::new(&mut model, None);
        assert!(
            stack
                .execute(&mut model, create_point(p(1.0, 1.0), none()))
                .is_ok()
        );
        assert!(
            stack
                .execute(&mut model, create_line(p(0.0, 0.0), p(5.0, 0.0), none()))
                .is_ok()
        );
        assert!(
            stack
                .execute(&mut model, create_circle(p(0.0, 0.0), 2.0, none()))
                .is_ok()
        );
        assert_eq!(
            stack.undo_names(),
            vec!["CREATE_CIRCLE", "CREATE_LINE", "CREATE_POINT"]
        );
        assert_eq!(stack.undo_names()[0], stack.undo_name());
        assert!(stack.redo_names().is_empty());
    }

    #[test]
    fn undoing_moves_the_names_to_the_redo_history_next_first() {
        let mut model = Model::new();
        let mut stack = CommandStack::new(&mut model, None);
        assert!(
            stack
                .execute(&mut model, create_point(p(1.0, 1.0), none()))
                .is_ok()
        );
        assert!(
            stack
                .execute(&mut model, create_line(p(0.0, 0.0), p(5.0, 0.0), none()))
                .is_ok()
        );
        assert!(
            stack
                .execute(&mut model, create_circle(p(0.0, 0.0), 2.0, none()))
                .is_ok()
        );
        assert!(stack.undo(&mut model).is_ok());
        assert!(stack.undo(&mut model).is_ok());
        assert_eq!(stack.undo_names(), vec!["CREATE_POINT"]);
        // The line comes back first, then the circle.
        assert_eq!(stack.redo_names(), vec!["CREATE_LINE", "CREATE_CIRCLE"]);
        assert_eq!(stack.redo_names()[0], stack.redo_name());
        assert!(stack.redo(&mut model).is_ok());
        assert_eq!(stack.undo_names(), vec!["CREATE_LINE", "CREATE_POINT"]);
        assert_eq!(stack.redo_names(), vec!["CREATE_CIRCLE"]);
    }

    #[test]
    fn a_new_step_clears_the_redo_history() {
        let mut model = Model::new();
        let mut stack = CommandStack::new(&mut model, None);
        assert!(
            stack
                .execute(&mut model, create_point(p(1.0, 1.0), none()))
                .is_ok()
        );
        assert!(stack.undo(&mut model).is_ok());
        assert_eq!(stack.redo_names().len(), 1);
        assert!(
            stack
                .execute(&mut model, create_line(p(0.0, 0.0), p(5.0, 0.0), none()))
                .is_ok()
        );
        assert!(stack.redo_names().is_empty());
        assert_eq!(stack.undo_names(), vec!["CREATE_LINE"]);
    }
}
