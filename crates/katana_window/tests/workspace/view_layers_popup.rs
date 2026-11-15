//! The twins of tests/qt_widgets/test_view_layers_popup.cpp: a view's Layers popup sets nothing
//! itself. Every box and button builds the VIEWS line a person would type and hands it to the
//! window's executor, so a click is logged as a typed line is and is refused by the verb as a typed
//! one is. What the lines are is held without a window; what the popup does with them in a
//! workspace whose executor records every line and runs it on the drawing's interpreter.

mod view_layers_lines {
    use katana_core::ErrorCode;
    use katana_window::view_layers_popup::{view_layers_line, view_references_line};

    fn line(action: &str, view: u32, layers: &[&str]) -> katana_core::Result<String> {
        let layers: Vec<String> = layers.iter().map(|name| String::from(*name)).collect();
        view_layers_line(action, view, &layers)
    }

    fn ok(action: &str, view: u32, layers: &[&str]) -> String {
        line(action, view, layers).unwrap_or_else(|error| panic!("{}", error.describe()))
    }

    #[test]
    fn a_plain_layer_is_a_word_of_the_verbs_own_form() {
        assert_eq!(ok("HIDE", 2, &["design/road"]), "VIEWS HIDE 2 design/road");
        assert_eq!(ok("SHOW", 12, &["asbuilt"]), "VIEWS SHOW 12 asbuilt");
        assert_eq!(
            ok("ISOLATE", 3, &["design/surface"]),
            "VIEWS ISOLATE 3 design/surface"
        );
        // Several: a word each, which the verb reads as a comma list of one name apiece.
        assert_eq!(ok("HIDE", 1, &["a", "b", "c/d"]), "VIEWS HIDE 1 a b c/d");
    }

    #[test]
    fn a_name_with_a_blank_is_quoted_as_every_dialog_quotes_a_word() {
        assert_eq!(ok("HIDE", 2, &["as built"]), "VIEWS HIDE 2 \"as built\"");
        assert_eq!(
            ok("HIDE", 2, &["as built", "road"]),
            "VIEWS HIDE 2 \"as built\" road"
        );
    }

    #[test]
    fn a_name_with_a_comma_or_one_of_the_verbs_own_words_is_written_after_layer() {
        assert_eq!(ok("HIDE", 2, &["a,b"]), "VIEWS HIDE 2 LAYER a,b");
        assert_eq!(ok("SHOW", 2, &["ALL"]), "VIEWS SHOW 2 LAYER ALL");
        assert_eq!(ok("SHOW", 2, &["all"]), "VIEWS SHOW 2 LAYER all");
        assert_eq!(ok("HIDE", 2, &["Layer"]), "VIEWS HIDE 2 LAYER Layer");
        assert_eq!(ok("HIDE", 2, &["reference"]), "VIEWS HIDE 2 LAYER reference");
        assert_eq!(ok("ISOLATE", 2, &["x,y"]), "VIEWS ISOLATE 2 LAYER x,y");
        // One awkward name puts the whole list after LAYER, where each word is still one name.
        assert_eq!(
            ok("HIDE", 2, &["a", "b,c", "d e"]),
            "VIEWS HIDE 2 LAYER a b,c \"d e\""
        );
        // A name that merely contains such a word is plain.
        assert_eq!(
            ok("HIDE", 2, &["allotment", "layers"]),
            "VIEWS HIDE 2 allotment layers"
        );
    }

    #[test]
    fn a_name_no_line_can_carry_is_refused_naming_it() {
        for name in ["say \"hi\"", "two\nlines"] {
            let made = line("HIDE", 2, &[name]);
            let error = made.expect_err(name);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{name}");
            assert!(error.message.contains("layer name"), "{}", error.message);
        }
        // Refused whole: a good name beside it is not written alone.
        assert!(line("HIDE", 2, &["road", "say \"hi\""]).is_err());
    }

    #[test]
    fn no_layer_an_action_that_is_none_of_three_or_two_to_isolate_is_refused() {
        let code = |made: katana_core::Result<String>| made.expect_err("refused").code;
        assert_eq!(code(line("HIDE", 2, &[])), ErrorCode::InvalidArgument);
        assert_eq!(code(line("KEEP", 2, &["a"])), ErrorCode::InvalidArgument);
        assert_eq!(
            code(line("hide", 2, &["a"])),
            ErrorCode::InvalidArgument,
            "the actions are the verb's words, written as the verb does"
        );
        assert_eq!(code(line("ISOLATE", 2, &["a", "b"])), ErrorCode::InvalidArgument);
    }

    #[test]
    fn the_reference_layers_are_named_by_their_ids() {
        assert_eq!(view_references_line("HIDE", 3, &[7]), "VIEWS HIDE 3 REFERENCE 7");
        assert_eq!(
            view_references_line("SHOW", 3, &[7, 12]),
            "VIEWS SHOW 3 REFERENCE 7 12"
        );
    }
}

mod layers_popup {
    use std::collections::BTreeSet;

    use katana_core::ErrorCode;
    use katana_interop::{RasterOverlay, ReferenceData};
    use katana_window::view_layers_popup::ViewLayersPopup;
    use katana_window::view_workspace::ViewWorkspace;
    use katana_window::host::RecordingHost;

    use crate::support::Site;

    fn hidden(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| String::from(*name)).collect()
    }

    /// `f` with the plan view's popup, opened over the drawing, and the host lent.
    fn with_popup<R>(
        w: &mut Site,
        f: impl FnOnce(&mut ViewLayersPopup, &ViewWorkspace, &mut RecordingHost) -> R,
    ) -> R {
        let plan = w.active();
        if !w.views.layers_popup_open() {
            let reference = w.host.parts.reference.clone();
            assert!(w.views.show_layers_popup(w.host.document(), Some(&reference), plan));
        }
        let Site { views, host, .. } = w;
        views
            .with_layers_popup(|popup, views| f(popup, views, host))
            .expect("the popup is open")
    }

    #[test]
    fn ticking_a_box_runs_the_views_line_and_the_line_sets_the_view() {
        let mut w = Site::new();
        w.add_layers(&["BUILDING", "ROAD"]);
        let plan = w.active();
        let modified = w.document().is_modified();
        let undoable = w.document().history().undo_count();

        let checked = with_popup(&mut w, |popup, views, host| {
            assert!(popup.item_for("BUILDING").is_some());
            popup.set_layer_checked(views, host, "BUILDING", false);
            popup.item_for("BUILDING").map(|row| row.checked)
        });
        assert_eq!(w.ran().len(), 1);
        assert_eq!(w.ran().last().cloned(), Some(format!("VIEWS HIDE {plan} BUILDING")));
        assert!(w.state(plan).layers.hides("BUILDING"));
        assert!(!w.state(plan).layers.hides("ROAD"));
        assert_eq!(checked, Some(false));

        with_popup(&mut w, |popup, views, host| {
            popup.set_layer_checked(views, host, "BUILDING", true);
        });
        assert_eq!(w.ran().len(), 2);
        assert_eq!(w.ran().last().cloned(), Some(format!("VIEWS SHOW {plan} BUILDING")));
        assert!(!w.state(plan).layers.hides("BUILDING"));

        // A view's own filter, still: a line that is no edit of the drawing.
        assert_eq!(w.document().is_modified(), modified);
        assert_eq!(w.document().history().undo_count(), undoable);
    }

    #[test]
    fn a_line_the_runner_refuses_leaves_the_box_where_the_view_is() {
        let mut w = Site::new();
        w.add_layers(&["BUILDING"]);
        let plan = w.active();
        w.refuse.set(true);
        let checked = with_popup(&mut w, |popup, views, host| {
            popup.set_layer_checked(views, host, "BUILDING", false);
            popup.item_for("BUILDING").map(|row| row.checked)
        });
        assert_eq!(w.ran().len(), 1, "the line was handed over");
        assert!(
            !w.state(plan).layers.hides("BUILDING"),
            "and refused: the view is as it was"
        );
        assert_eq!(checked, Some(true), "so the box says what the view does");
    }

    #[test]
    fn a_layer_whose_name_holds_a_comma_is_hidden_alone_by_a_line_that_can_say_it() {
        let mut w = Site::new();
        w.add_layers(&["a,b", "a", "b"]);
        let plan = w.active();
        with_popup(&mut w, |popup, views, host| {
            assert!(popup.item_for("a,b").is_some());
            popup.set_layer_checked(views, host, "a,b", false);
        });
        assert_eq!(w.ran().len(), 1);
        assert_eq!(w.ran().last().cloned(), Some(format!("VIEWS HIDE {plan} LAYER a,b")));
        // The layer called a,b, and neither a nor b, which a comma list would have hidden.
        assert_eq!(*w.state(plan).layers.hidden(), hidden(&["a,b"]));
    }

    #[test]
    fn show_all_runs_one_line_when_something_is_hidden_and_none_when_nothing_is() {
        let mut w = Site::new();
        w.add_layers(&["BUILDING", "ROAD"]);
        let plan = w.active();
        with_popup(&mut w, |popup, views, host| popup.show_all(views, host));
        assert!(w.ran().is_empty(), "nothing hidden, nothing to say");

        with_popup(&mut w, |popup, views, host| {
            popup.set_layer_checked(views, host, "BUILDING", false);
            popup.set_layer_checked(views, host, "ROAD", false);
        });
        assert_eq!(w.ran().len(), 2);
        with_popup(&mut w, |popup, views, host| popup.show_all(views, host));
        assert_eq!(w.ran().len(), 3);
        assert_eq!(w.ran().last().cloned(), Some(format!("VIEWS SHOW {plan} ALL")));
        assert!(w.state(plan).layers.empty());
    }

    #[test]
    fn hide_others_isolates_the_layer_and_then_hides_again_what_this_view_hid_beneath_it() {
        let mut w = Site::new();
        w.add_layers(&["ANNOT", "design/road", "design/surface/tin1", "survey/points"]);
        let plan = w.active();
        // The view hides tin1 already.
        with_popup(&mut w, |popup, views, host| {
            popup.set_layer_checked(views, host, "design/surface/tin1", false);
        });
        assert_eq!(w.ran().len(), 1);

        // design/surface is a node of the tree only. Isolated, the view hides its siblings at each
        // level; Hide Others then hides tin1 again, which isolating had shown.
        let others = with_popup(&mut w, |popup, views, host| {
            popup.set_current(Some("design/surface"));
            popup.hide_others(views, host)
        });
        assert!(others.is_ok());
        let ran = w.ran();
        assert_eq!(ran.len(), 3);
        assert_eq!(ran[1], format!("VIEWS ISOLATE {plan} design/surface"));
        assert_eq!(ran[2], format!("VIEWS HIDE {plan} design/surface/tin1"));
        assert_eq!(
            *w.state(plan).layers.hidden(),
            hidden(&["0", "ANNOT", "design/road", "survey", "design/surface/tin1"])
        );

        // Isolate alone replaces what the view hid: tin1 is shown again.
        let isolated = with_popup(&mut w, |popup, views, host| {
            popup.set_current(Some("design/surface"));
            popup.isolate_selected(views, host)
        });
        assert!(isolated.is_ok());
        assert_eq!(
            w.ran().last().cloned(),
            Some(format!("VIEWS ISOLATE {plan} design/surface"))
        );
        assert_eq!(
            *w.state(plan).layers.hidden(),
            hidden(&["0", "ANNOT", "design/road", "survey"])
        );
    }

    #[test]
    fn isolate_or_hide_others_with_nothing_selected_runs_no_line() {
        let mut w = Site::new();
        w.add_layers(&["BUILDING"]);
        let (isolated, others) = with_popup(&mut w, |popup, views, host| {
            popup.set_current(None);
            (popup.isolate_selected(views, host), popup.hide_others(views, host))
        });
        assert_eq!(isolated.expect_err("nothing selected").code, ErrorCode::NotFound);
        assert_eq!(others.expect_err("nothing selected").code, ErrorCode::NotFound);
        assert!(w.ran().is_empty());
    }

    #[test]
    fn an_isolate_the_runner_refuses_is_a_status_carrying_its_reason() {
        let mut w = Site::new();
        w.add_layers(&["BUILDING", "ROAD"]);
        let plan = w.active();
        w.refuse.set(true);
        let status = with_popup(&mut w, |popup, views, host| {
            popup.set_current(Some("ROAD"));
            popup.isolate_selected(views, host)
        });
        let error = status.expect_err("refused");
        assert!(error.message.contains("refused by the test"), "{}", error.message);
        assert!(w.state(plan).layers.empty());
    }

    #[test]
    fn a_reference_layers_box_runs_the_reference_line() {
        let mut w = Site::new();
        let mut reference = ReferenceData::default();
        let image = RasterOverlay {
            name: String::from("ortho"),
            width: 2,
            height: 2,
            rgba: vec![255; 2 * 2 * 4],
            ..RasterOverlay::default()
        };
        let id = reference.add(image);
        w.host.parts.reference = reference.clone();
        w.views.invalidate_reference_cache(Some(&reference));
        let plan = w.active();

        let rows = with_popup(&mut w, |popup, views, host| {
            let rows = popup.reference_rows().len();
            popup.set_reference_checked(views, host, id, false);
            rows
        });
        assert_eq!(rows, 1);
        assert_eq!(w.ran().len(), 1);
        assert_eq!(
            w.ran().last().cloned(),
            Some(format!("VIEWS HIDE {plan} REFERENCE {id}"))
        );
        assert!(w.state(plan).hidden_references.contains(&id));
        assert_eq!(w.views.hidden_count(plan), 1, "its Layers button says so");

        with_popup(&mut w, |popup, views, host| {
            popup.set_reference_checked(views, host, id, true);
        });
        assert_eq!(
            w.ran().last().cloned(),
            Some(format!("VIEWS SHOW {plan} REFERENCE {id}"))
        );
        assert!(w.state(plan).hidden_references.is_empty());

        // Show All takes the reference layers back as well as the layers.
        with_popup(&mut w, |popup, views, host| {
            popup.set_reference_checked(views, host, id, false);
            popup.show_all(views, host);
        });
        assert_eq!(w.ran().last().cloned(), Some(format!("VIEWS SHOW {plan} ALL")));
        assert!(w.state(plan).hidden_references.is_empty());
    }
}
