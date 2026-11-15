//! tests/cad/test_document_changes.cpp: what a Document notification says
//! changed (DocumentChange).
//!
//! A listener told only "something changed" has to rebuild everything it
//! holds, and the 3D view did exactly that on every selection click: 217-768
//! ms on a real TIN archive to redraw a highlight. These tests pin down the
//! payload a listener can skip work by: which parts a notification names,
//! that a click names the selection ALONE, that an edit names its entities,
//! and that the listeners written before the payload existed still run for
//! everything.

mod document_changes {
    use std::sync::{Arc, Mutex};

    use katana_cad::{CommandInterpreter, Document, DocumentChange, ListenerHandle};
    use katana_commands::{
        EntityAttributes, create_circle, create_layer, create_point, delete_entities,
    };
    use katana_entity::{ChangeEvent, ChangeKind, EntityId, Layer, StyleLibrary, SurveyMap};
    use katana_geometry::Point2;

    use crate::{ScratchDirectory, must_create};

    // Everything one change listener was told, copied: the slice in a change
    // is lent only for the call.
    #[derive(Clone, Debug, Default)]
    struct Recorded {
        parts: u32,
        entities: Vec<ChangeEvent>,
    }

    struct Recorder {
        calls: Arc<Mutex<Vec<Recorded>>>,
        _handle: ListenerHandle,
    }

    impl Recorder {
        fn new(document: &Document) -> Self {
            let calls: Arc<Mutex<Vec<Recorded>>> = Arc::default();
            let sink = Arc::clone(&calls);
            let handle = document.add_change_listener(move |change: &DocumentChange<'_>| {
                sink.lock().unwrap().push(Recorded {
                    parts: change.parts,
                    entities: change.entities.to_vec(),
                });
            });
            Self {
                calls,
                _handle: handle,
            }
        }

        fn count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        fn last(&self) -> Recorded {
            self.calls
                .lock()
                .unwrap()
                .last()
                .cloned()
                .expect("a notification")
        }
    }

    fn layer(name: &str) -> Layer {
        Layer {
            name: String::from(name),
            ..Layer::default()
        }
    }

    fn create_point_at(document: &mut Document, at: Point2) -> EntityId {
        must_create(document, create_point(at, EntityAttributes::default()))
    }

    #[test]
    fn a_selection_change_is_reported_as_the_selection_alone_and_changes_no_drawing() {
        let mut document = Document::default();
        let id = create_point_at(&mut document, Point2::new(0.0, 0.0));
        let recorder = Recorder::new(&document);

        document.selection_mut().set(vec![id]);
        document.notify_selection_changed();

        assert_eq!(recorder.count(), 1);
        let change = DocumentChange::of(recorder.last().parts);
        assert_eq!(change.parts, DocumentChange::SELECTION);
        assert!(change.selection_only());
        assert!(
            !change.changes_drawing(),
            "a click must not make a view rebuild its drawing"
        );
        assert!(recorder.last().entities.is_empty());
    }

    #[test]
    fn creating_an_entity_reports_its_id_and_the_history_but_no_table_or_selection() {
        let mut document = Document::default();
        let recorder = Recorder::new(&document);

        let id = create_point_at(&mut document, Point2::new(1.0, 2.0));

        assert_eq!(recorder.count(), 1);
        let last = recorder.last();
        assert_eq!(
            last.parts,
            DocumentChange::ENTITIES | DocumentChange::HISTORY
        );
        assert_eq!(last.entities.len(), 1);
        assert_eq!(last.entities[0].kind, ChangeKind::EntityAdded);
        assert_eq!(last.entities[0].id, id);
    }

    #[test]
    fn deleting_a_selected_entity_reports_the_entity_and_the_selection_it_left() {
        let mut document = Document::default();
        let kept = create_point_at(&mut document, Point2::new(0.0, 0.0));
        let deleted = create_point_at(&mut document, Point2::new(5.0, 0.0));
        document.selection_mut().set(vec![kept, deleted]);
        let recorder = Recorder::new(&document);

        assert!(document.execute(delete_entities(vec![deleted])).is_ok());

        assert_eq!(recorder.count(), 1);
        let last = recorder.last();
        assert_eq!(
            last.parts,
            DocumentChange::ENTITIES | DocumentChange::SELECTION | DocumentChange::HISTORY
        );
        assert_eq!(last.entities.len(), 1);
        assert_eq!(last.entities[0].kind, ChangeKind::EntityRemoved);
        assert_eq!(last.entities[0].id, deleted);
        assert!(document.selection().contains(kept));
        assert!(!document.selection().contains(deleted));
    }

    #[test]
    fn deleting_an_unselected_entity_does_not_report_the_selection() {
        let mut document = Document::default();
        let selected = create_point_at(&mut document, Point2::new(0.0, 0.0));
        let other = create_point_at(&mut document, Point2::new(5.0, 0.0));
        document.selection_mut().set(vec![selected]);
        let recorder = Recorder::new(&document);

        assert!(document.execute(delete_entities(vec![other])).is_ok());

        assert_eq!(recorder.count(), 1);
        assert!(!DocumentChange::of(recorder.last().parts).has(DocumentChange::SELECTION));
    }

    #[test]
    fn a_layer_command_reports_the_layers_and_no_entities() {
        let mut document = Document::default();
        let recorder = Recorder::new(&document);

        assert!(document.execute(create_layer(layer("Survey"))).is_ok());

        assert_eq!(recorder.count(), 1);
        let last = recorder.last();
        assert_eq!(last.parts, DocumentChange::LAYERS | DocumentChange::HISTORY);
        assert!(last.entities.is_empty());
        assert!(
            DocumentChange::of(last.parts).changes_drawing(),
            "a layer's colour is part of how the drawing looks"
        );
    }

    #[test]
    fn undoing_the_current_layers_creation_reports_the_layers_and_the_current_layer() {
        let mut document = Document::default();
        assert!(document.execute(create_layer(layer("Survey"))).is_ok());
        assert!(document.set_current_layer("Survey").is_ok());
        let recorder = Recorder::new(&document);

        assert!(document.undo().is_ok());

        assert_eq!(recorder.count(), 1);
        assert_eq!(
            recorder.last().parts,
            DocumentChange::LAYERS | DocumentChange::CURRENT_ATTRIBUTES | DocumentChange::HISTORY
        );
        assert_eq!(document.current_layer(), "0");
    }

    #[test]
    fn each_table_command_reports_its_own_table_only() {
        let mut document = Document::default();
        let mut interpreter = CommandInterpreter::default();
        let recorder = Recorder::new(&document);

        assert!(interpreter.run(&mut document, "STYLE NEW Kerb").is_ok());
        assert_eq!(recorder.count(), 1);
        assert_eq!(
            recorder.last().parts,
            DocumentChange::STYLES | DocumentChange::HISTORY
        );

        // Three PIs, no curves: the alignment table alone.
        assert!(
            interpreter
                .run(&mut document, "ALIGN NEW road 0,0 100,0 100,100")
                .is_ok()
        );
        assert_eq!(recorder.count(), 2);
        let last = recorder.last();
        assert_eq!(
            last.parts,
            DocumentChange::ALIGNMENTS | DocumentChange::HISTORY
        );
        assert!(last.entities.is_empty());
    }

    #[test]
    fn the_current_layer_and_style_are_reported_as_the_current_attributes_alone() {
        let mut document = Document::default();
        assert!(document.execute(create_layer(layer("Survey"))).is_ok());
        let recorder = Recorder::new(&document);

        assert!(document.set_current_layer("Survey").is_ok());

        assert_eq!(recorder.count(), 1);
        let parts = recorder.last().parts;
        assert_eq!(parts, DocumentChange::CURRENT_ATTRIBUTES);
        assert!(!DocumentChange::of(parts).changes_drawing());
    }

    #[test]
    fn metadata_library_and_survey_map_each_report_their_own_part() {
        let mut document = Document::default();
        let recorder = Recorder::new(&document);

        let mut metadata = document.metadata().clone();
        metadata.description = String::from("a corridor survey");
        document.set_metadata(metadata);
        assert_eq!(recorder.count(), 1);
        assert_eq!(recorder.last().parts, DocumentChange::METADATA);
        assert!(!DocumentChange::of(recorder.last().parts).changes_drawing());

        document.set_style_library(StyleLibrary::default());
        assert_eq!(recorder.count(), 2);
        assert_eq!(recorder.last().parts, DocumentChange::STYLE_LIBRARY);
        assert!(
            DocumentChange::of(recorder.last().parts).changes_drawing(),
            "library linestyles and symbols are what styled entities are drawn with"
        );

        document.set_survey_map(SurveyMap::default());
        assert_eq!(recorder.count(), 3);
        assert_eq!(recorder.last().parts, DocumentChange::SURVEY_MAP);
    }

    #[test]
    fn a_new_document_reports_every_drawing_part_so_an_entity_listener_rebuilds() {
        let mut document = Document::default();
        let id = create_point_at(&mut document, Point2::new(0.0, 0.0));
        document.selection_mut().set(vec![id]);
        let recorder = Recorder::new(&document);

        document.new_document();

        assert_eq!(recorder.count(), 1);
        let last = recorder.last();
        let change = DocumentChange::of(last.parts);
        assert!(change.has(DocumentChange::REPLACED));
        // A listener that knows nothing of REPLACED still sees what it tests
        // for.
        assert!(change.has(DocumentChange::ENTITIES));
        assert!(change.has(DocumentChange::SELECTION));
        assert!(change.has(DocumentChange::LAYERS));
        assert!(change.has(DocumentChange::ALIGNMENTS));
        assert!(change.has(DocumentChange::METADATA));
        assert!(change.has(DocumentChange::HISTORY));
        // The customisation is kept across a new drawing, so it is not named.
        assert!(!change.has(DocumentChange::STYLE_LIBRARY));
        assert!(!change.has(DocumentChange::SURVEY_MAP));
        assert!(!change.selection_only());
        assert!(
            last.entities.is_empty(),
            "a replaced drawing does not list its entities"
        );
    }

    #[test]
    fn an_open_reports_a_replaced_drawing_and_the_first_command_after_it_no_tables() {
        let scratch = ScratchDirectory::new("document-changes");
        let directory = scratch.path.join("opened.katana");
        {
            let mut saved = Document::default();
            assert!(saved.execute(create_layer(layer("Survey"))).is_ok());
            let _ = create_point_at(&mut saved, Point2::new(3.0, 4.0));
            assert!(saved.save_as(&directory).is_ok());
        }

        // Scoped: the open project holds its database file until the
        // Document goes, and Windows will not remove a file that is open.
        {
            let mut document = Document::default();
            let recorder = Recorder::new(&document);
            let opened = document.open(&directory);
            assert!(opened.is_ok(), "{opened:?}");
            assert_eq!(recorder.count(), 1);
            assert!(DocumentChange::of(recorder.last().parts).has(DocumentChange::REPLACED));

            // The tables loaded by the open are not a change the next command
            // made.
            let _ = create_point_at(&mut document, Point2::new(5.0, 6.0));
            assert_eq!(recorder.count(), 2);
            assert_eq!(
                recorder.last().parts,
                DocumentChange::ENTITIES | DocumentChange::HISTORY
            );

            assert!(document.save().is_ok());
            assert_eq!(recorder.count(), 3);
            assert_eq!(recorder.last().parts, DocumentChange::SAVED);
        }
    }

    #[test]
    fn a_refused_command_notifies_nobody() {
        let mut document = Document::default();
        let recorder = Recorder::new(&document);

        assert!(
            document
                .execute(create_circle(
                    Point2::new(0.0, 0.0),
                    -1.0,
                    EntityAttributes::default()
                ))
                .is_err()
        );

        assert_eq!(recorder.count(), 0);
    }

    #[test]
    fn plain_and_typed_listeners_are_both_called_in_the_order_they_were_added() {
        // The listeners written before the payload existed take no argument.
        // They must still run for every notification - a selection click
        // included - and in their place in the order.
        let mut document = Document::default();
        let order: Arc<Mutex<Vec<String>>> = Arc::default();
        let plain = Arc::clone(&order);
        let _first = document.add_listener(move || {
            plain.lock().unwrap().push(String::from("plain"));
        });
        let typed = Arc::clone(&order);
        let _second = document.add_change_listener(move |change: &DocumentChange<'_>| {
            typed
                .lock()
                .unwrap()
                .push(format!("typed {}", change.parts));
        });

        let _ = create_point_at(&mut document, Point2::new(0.0, 0.0));
        document.notify_selection_changed();

        // ENTITIES | HISTORY = 1 + 1024 = 1025; SELECTION = 2.
        let expected = vec![
            String::from("plain"),
            String::from("typed 1025"),
            String::from("plain"),
            String::from("typed 2"),
        ];
        assert_eq!(*order.lock().unwrap(), expected);
    }
}
