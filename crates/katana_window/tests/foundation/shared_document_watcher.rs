//! The twins of tests/qt_widgets/customisation/test_shared_document_watcher.cpp: a manager
//! dialog's one way of hearing from the Document - one delivery per frame, saying which of the
//! model, the library, the survey map, the selection and the current layer/style moved.
//!
//! The C++'s event-loop turn is a frame's delivery here: the dialog asks its watcher as it draws
//! (`DocumentWatcher::deliver`), and a notification only marks a delivery due.

use katana_cad::Document;
use katana_commands::{EntityAttributes, create_layer, create_point};
use katana_entity::tables::Layer;
use katana_entity::{StyleLibrary, SurveyMap};
use katana_geometry::Point2;
use katana_window::customisation::{DocumentChanges, DocumentWatcher};

use crate::window_support::Scratch;

mod document_watcher {
    use super::*;

    /// What a watcher delivered, in order.
    #[derive(Default)]
    struct Deliveries {
        calls: Vec<DocumentChanges>,
    }

    impl Deliveries {
        /// A frame's turn: the watcher's delivery, kept when there is one.
        fn turn(&mut self, watcher: &mut DocumentWatcher, document: &Document) {
            if let Some(changes) = watcher.deliver(document) {
                self.calls.push(changes);
            }
        }
    }

    fn layer_named(name: &str) -> Layer {
        Layer {
            name: String::from(name),
            ..Layer::default()
        }
    }

    #[test]
    fn three_notifications_in_one_turn_are_delivered_once_and_only_from_the_event_loop() {
        let mut document = Document::new(None);
        let mut seen = Deliveries::default();
        let mut watcher = DocumentWatcher::new(&document);

        document
            .execute(create_layer(layer_named("a")))
            .expect("a is made");
        document
            .execute(create_layer(layer_named("b")))
            .expect("b is made");
        document.notify_selection_changed();
        // Nothing yet: a notification only marks the delivery due; delivering inside it is what
        // a dialog must never do (docs/cad.md).
        assert!(seen.calls.is_empty());
        assert!(watcher.pending());

        seen.turn(&mut watcher, &document);
        assert_eq!(seen.calls.len(), 1);
        assert!(!watcher.pending());

        // And nothing more on the next turn: the three were one.
        seen.turn(&mut watcher, &document);
        assert_eq!(seen.calls.len(), 1);
    }

    #[test]
    fn each_flag_is_set_by_its_own_change_and_by_nothing_else() {
        let mut document = Document::new(None);
        let mut seen = Deliveries::default();
        let mut watcher = DocumentWatcher::new(&document);

        // A command: the model, and nothing else.
        document
            .execute(create_layer(layer_named("walls")))
            .expect("walls is made");
        seen.turn(&mut watcher, &document);
        assert_eq!(seen.calls.len(), 1);
        assert_eq!(
            seen.calls.last(),
            Some(&DocumentChanges {
                model: true,
                ..DocumentChanges::default()
            })
        );

        // A library: its generation moves; the model does not.
        document.set_style_library(StyleLibrary::default());
        seen.turn(&mut watcher, &document);
        assert_eq!(seen.calls.len(), 2);
        assert_eq!(
            seen.calls.last(),
            Some(&DocumentChanges {
                library: true,
                ..DocumentChanges::default()
            })
        );

        document.set_survey_map(SurveyMap::default());
        seen.turn(&mut watcher, &document);
        assert_eq!(seen.calls.len(), 3);
        assert_eq!(
            seen.calls.last(),
            Some(&DocumentChanges {
                survey_map: true,
                ..DocumentChanges::default()
            })
        );

        // A selection: its own flag, not the model's.
        document
            .execute(create_point(
                Point2::new(1.0, 2.0),
                EntityAttributes::default(),
            ))
            .expect("the point is made");
        seen.turn(&mut watcher, &document);
        assert_eq!(seen.calls.len(), 4);
        assert_eq!(
            seen.calls.last(),
            Some(&DocumentChanges {
                model: true,
                ..DocumentChanges::default()
            })
        );
        let created = document.last_created_entities();
        document.selection_mut().set(created);
        document.notify_selection_changed();
        seen.turn(&mut watcher, &document);
        assert_eq!(seen.calls.len(), 5);
        assert_eq!(
            seen.calls.last(),
            Some(&DocumentChanges {
                selection: true,
                ..DocumentChanges::default()
            })
        );

        // The current layer.
        document
            .set_current_layer("walls")
            .expect("walls is a layer");
        seen.turn(&mut watcher, &document);
        assert_eq!(seen.calls.len(), 6);
        assert_eq!(
            seen.calls.last(),
            Some(&DocumentChanges {
                current: true,
                ..DocumentChanges::default()
            })
        );

        // A notification that changed none of them is still delivered, flagless.
        document.notify_selection_changed();
        seen.turn(&mut watcher, &document);
        assert_eq!(seen.calls.len(), 7);
        assert!(!seen.calls.last().expect("a delivery").any());
    }

    #[test]
    fn an_undo_followed_by_a_new_command_in_one_turn_is_a_model_change() {
        // The case a per-turn comparison misses. Worked by hand: after creating layer "a" the
        // history is (undo 1, redo 0) and there are two layers ("0" and "a"). Undo gives (0, 1)
        // and one layer; creating "b" gives (1, 0) and two layers again - every count where it
        // started, and a different drawing ("b", not "a").
        let mut document = Document::new(None);
        document
            .execute(create_layer(layer_named("a")))
            .expect("a is made");
        let mut seen = Deliveries::default();
        let mut watcher = DocumentWatcher::new(&document);

        document.undo().expect("the layer is undone");
        document
            .execute(create_layer(layer_named("b")))
            .expect("b is made");
        assert_eq!(document.history().undo_count(), 1);
        assert_eq!(document.history().redo_count(), 0);
        seen.turn(&mut watcher, &document);

        assert_eq!(seen.calls.len(), 1);
        assert!(seen.calls.last().expect("a delivery").model);
    }

    #[test]
    fn reopening_an_unchanged_project_from_a_fresh_drawing_is_a_model_change() {
        // The fingerprint's blind spot, gone with Document::model_revision: an empty drawing saved
        // as a project and reopened has no history before or after, the same size of every table
        // and the same project directory - and it is still a new drawing, whose views must
        // reload.
        let scratch = Scratch::new("watcher-reopen");
        let directory = scratch.path("same.katana");
        let mut seen = Deliveries::default();
        {
            // Its own scope: Windows will not delete the project's open database.
            let mut document = Document::new(None);
            document
                .save_as(&directory)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            let mut watcher = DocumentWatcher::new(&document);

            document
                .open(&directory)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            seen.turn(&mut watcher, &document);
        }
        assert_eq!(seen.calls.len(), 1);
        assert!(seen.calls.last().expect("a delivery").model);
    }

    #[test]
    fn a_document_destroyed_with_a_delivery_queued_is_never_read_and_nothing_is_delivered() {
        let mut seen = Deliveries::default();
        let mut watcher;
        {
            let mut document = Document::new(None);
            watcher = DocumentWatcher::new(&document);
            assert!(watcher.document_alive());
            document
                .execute(create_layer(layer_named("a")))
                .expect("a is made");
            assert!(watcher.pending());
        }
        assert!(!watcher.document_alive());
        // The queued delivery's turn, with another drawing at hand: the one the watcher was made
        // with is gone, and no drawing is read.
        let other = Document::new(None);
        seen.turn(&mut watcher, &other);
        assert!(seen.calls.is_empty());
        drop(watcher); // and the watcher then goes quietly
    }

    #[test]
    fn a_watcher_destroyed_with_a_delivery_queued_takes_the_delivery_with_it() {
        let mut document = Document::new(None);
        let seen = Deliveries::default();
        let watcher = DocumentWatcher::new(&document);
        document
            .execute(create_layer(layer_named("a")))
            .expect("a is made");
        assert!(watcher.pending());
        drop(watcher);
        // Nothing is queued anywhere but in the watcher, which has gone; and its registration
        // went with it: the Document notifies nobody, and a command after runs as before.
        document
            .execute(create_layer(layer_named("b")))
            .expect("b is made");
        assert!(seen.calls.is_empty());
    }
}
