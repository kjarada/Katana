//! tests/app/test_view_scale.cpp: a saved view's paper scale read, changed and
//! undone through katana_cli's session. (Its ViewScaleMcp case drives the MCP
//! server: wave 2, C.)

mod view_scale_session {
    use katana_app::{Captured, Session};
    use katana_cad::view_set::{ViewPaperScales, set_view_paper_scales};

    #[test]
    fn the_cli_session_reads_changes_and_undoes_a_saved_views_paper_scale() {
        let mut session = Session::new(None, &mut Captured::default());
        set_view_paper_scales(
            session.document_mut(),
            &ViewPaperScales::from([(4, Some(500.0))]),
        )
        .expect("the scales are set");
        let mut captured = Captured::default();
        assert!(session.run("VIEWS SCALE LIST", &mut captured));
        assert!(session.run("VIEWS SCALE 4 1:250", &mut captured));
        assert!(session.run("UNDO", &mut captured));
        assert!(session.run("VIEWS SCALE 4", &mut captured));
        assert!(captured.err.is_empty(), "{}", captured.err);
        let output = &captured.out;
        assert!(output.contains("view=4 kind=plan paper_scale=500\nviews=1"));
        assert!(output.contains("view=4 kind=plan paper_scale=250 changed=yes"));
        assert!(output.ends_with("view=4 kind=plan paper_scale=500\n"));
    }
}
