//! tests/app/test_session.cpp: what a session starts with. Each case sets the
//! customisation variables a program's session reads (start_environment), so
//! each holds the process lock first.

mod session {
    use std::path::Path;

    use katana_app::{Captured, Session};
    use katana_cad::CustomisationOrigin;
    use katana_cad::customisation_host::{
        BUILT_IN_CUSTOMISATION_VARIABLE, KEPT_CUSTOMISATION_VARIABLE, read_customisation_file,
    };
    use katana_core::path_text::path_from_utf8;

    use crate::app_support::{ScratchDirectory, contains, customisation_file, generic, run};
    use crate::start_environment::{ScopedVariable, StartEnvironment, TWO_SCRIPTS, process_lock};

    // The path inside the quotes `ScratchDirectory::file` gives for a line;
    // a variable takes it bare.
    fn bare(quoted: &str) -> &str {
        &quoted[1..quoted.len() - 1]
    }

    // ---- what a session starts with -----------------------------------------------------------

    #[test]
    fn a_programs_session_starts_with_the_built_in_of_the_run_and_says_which_on_stdout() {
        let _process = process_lock();
        let symbols = customisation_file("test_symbols.customisation.json");
        let _environment = StartEnvironment::new(Some(&symbols), None);
        let mut started = Captured::default();
        let mut session = Session::new(Some("katana_cli"), &mut started);
        // The line names what was installed, and holds no word a script takes
        // for a failure.
        assert_eq!(
            started.out,
            "Customisation: test_symbols, 4 linestyles and symbols and 0 survey code rules, \
             built in\n"
        );
        assert_eq!(started.err, "");
        assert_eq!(session.document().style_library().size(), 4);
        assert_eq!(
            session.document().customisation_state().origin,
            CustomisationOrigin::BuiltIn
        );
        assert_eq!(
            session.document().customisation_state().name,
            "test_symbols"
        );

        // The interpreter was handed the host: RESET has a built-in to go back
        // to, and KEEP is refused for the one thing this run was not given - a
        // kept file - naming the variable that would give it one.
        let reset = run(&mut session, "CUSTOMISE RESET");
        assert!(reset.ok, "{}", reset.err);
        assert_eq!(
            reset.out,
            "reset name=test_symbols definitions=4 codes=0 rules=0 kept=yes\n"
        );
        let keep = run(&mut session, "CUSTOMISE KEEP");
        assert!(!keep.ok);
        assert_eq!(keep.out, "");
        assert!(contains(&keep.err, "KATANA_CUSTOMISATION"), "{}", keep.err);
    }

    #[test]
    fn a_session_of_no_program_starts_empty_whatever_the_environment_names() {
        // Both variables name files that read: a program's session would start
        // with the kept one. A session given no program reads neither, so about
        // thirty suites that construct one see the same empty Document on every
        // machine.
        let _process = process_lock();
        let symbols = customisation_file("test_symbols.customisation.json");
        let survey = customisation_file("test_survey.customisation.json");
        let _environment = StartEnvironment::new(Some(&symbols), Some(&survey));
        let mut started = Captured::default();
        let mut session = Session::new(None, &mut started);
        assert_eq!(started.out, "");
        assert_eq!(started.err, "");
        assert!(session.document().style_library().empty());
        assert_eq!(session.document().survey_map().size(), 0);
        assert_eq!(
            session.document().customisation_state().origin,
            CustomisationOrigin::None
        );

        // And it was handed no host: the words that need one are refused by
        // name.
        for line in ["CUSTOMISE RESET", "CUSTOMISE KEEP", "CUSTOMISE REVERT"] {
            let refused = run(&mut session, line);
            assert!(!refused.ok, "{line}");
            assert_eq!(refused.out, "", "{line}");
            assert!(
                refused
                    .err
                    .starts_with(&format!("error: InvalidState: {line} needs ")),
                "{}",
                refused.err
            );
        }
        // The rest of the family is there all the same.
        let report = run(&mut session, "CUSTOMISE");
        assert!(report.ok, "{}", report.err);
        assert!(
            report.out.starts_with("No customisation is loaded.\n"),
            "{}",
            report.out
        );
    }

    #[test]
    fn a_kept_file_named_by_the_variable_starts_the_session_in_the_built_ins_place() {
        let _process = process_lock();
        // A copy, so that nothing a session does can touch the committed file.
        let scratch = ScratchDirectory::new("kept");
        let kept = scratch.path.join("kept.customisation.json");
        std::fs::copy(customisation_file("test_survey.customisation.json"), &kept)
            .expect("the survey customisation is copied");
        let symbols = customisation_file("test_symbols.customisation.json");
        let _environment = StartEnvironment::new(Some(&symbols), Some(&generic(&kept)));
        let mut started = Captured::default();
        let session = Session::new(Some("katana_cli"), &mut started);
        assert_eq!(
            started.out,
            "Customisation: test_survey, 0 linestyles and symbols and 11 survey code rules, \
             kept\n"
        );
        // The file says nothing of what it was made from, so nothing is said of
        // another built-in.
        assert_eq!(started.err, "");
        assert_eq!(
            session.document().customisation_state().origin,
            CustomisationOrigin::Kept
        );
        assert_eq!(session.document().survey_map().size(), 11);
        assert!(session.document().style_library().empty());
    }

    #[test]
    fn a_kept_customisation_made_from_another_built_in_starts_and_says_so_on_stderr() {
        // It names what it was made from, and that is not this run's built-in
        // (test_symbols). It is the user's, so it is what starts; the warning
        // is for them to know the program's own has moved on.
        let _process = process_lock();
        let scratch = ScratchDirectory::new("kept-from-another");
        let kept = scratch.file(
            "kept.customisation.json",
            r#"{"format": "katana-customisation", "version": 1, "name": "Mine",
 "basedOn": {"name": "Some Other", "digest": "0123456789abcdef"},
 "codes": [{"key": "MN*", "sets": "feature", "layer": "MINE"}]}"#,
        );
        let symbols = customisation_file("test_symbols.customisation.json");
        let _environment = StartEnvironment::new(Some(&symbols), Some(bare(&kept)));
        let mut started = Captured::default();
        let session = Session::new(Some("katana_cli"), &mut started);
        // ONE rule, so the noun is in the singular: the line said "1 survey code
        // rules", and this expected it, until 2026-10-07.
        assert_eq!(
            started.out,
            "Customisation: Mine, 0 linestyles and symbols and 1 survey code rule, kept\n"
        );
        assert_eq!(
            started.err,
            "warning: the kept customisation was made from another built-in customisation than \
             this program has; CUSTOMISE RESET gives this program's and CUSTOMISE BUILTIN lists \
             the built-in customisations it has\n"
        );
        assert_eq!(session.document().customisation_state().name, "Mine");
    }

    #[test]
    fn the_start_up_line_counts_one_definition_and_one_rule_in_the_singular() {
        // The kept file written here holds ONE symbol and ONE rule, and there
        // is no built-in for the run. The line gave every count the plural - "1
        // linestyles and symbols and 1 survey code rules" - where the window
        // says "1 definition (1 symbol) and 1 survey code rule" of the same
        // start. One definition is a linestyle OR a symbol; the line does not
        // say which.
        let _process = process_lock();
        let scratch = ScratchDirectory::new("kept-one-of-each");
        let kept = scratch.file(
            "kept.customisation.json",
            r#"{"format": "katana-customisation", "version": 1, "name": "One",
 "symbols": [{"name": "ONE Mark", "atVertices": true, "strokes": [["move", 0, 0], ["circle", 0.5]]}],
 "codes": [{"key": "MK*", "sets": "symbol", "symbol": {"name": "ONE Mark"}}]}"#,
        );
        let _environment = StartEnvironment::new(Some("none"), Some(bare(&kept)));
        let mut started = Captured::default();
        let session = Session::new(Some("katana_cli"), &mut started);
        assert_eq!(
            started.out,
            "Customisation: One, 1 linestyle or symbol and 1 survey code rule, kept\n"
        );
        assert_eq!(started.err, "");
        assert_eq!(session.document().style_library().size(), 1);
        assert_eq!(session.document().survey_map().size(), 1);
    }

    #[test]
    fn a_start_up_problem_is_said_on_stderr_and_the_session_starts_all_the_same() {
        // The seam names a file that is not there: there is no built-in for the
        // run, and that is said - one line, where errors go, naming the
        // variable and the file - rather than the session starting with
        // whatever this build compiled in, or failing to start.
        let _process = process_lock();
        let scratch = ScratchDirectory::new("no-built-in");
        let absent = generic(&scratch.path.join("absent.customisation.json"));
        let _environment = StartEnvironment::new(Some(&absent), None);
        let mut started = Captured::default();
        let mut session = Session::new(Some("katana_cli"), &mut started);
        // Nothing was installed, so no line says anything was.
        assert_eq!(started.out, "");
        assert!(started.err.starts_with("error: "), "{}", started.err);
        assert!(
            contains(&started.err, BUILT_IN_CUSTOMISATION_VARIABLE),
            "{}",
            started.err
        );
        assert!(
            contains(&started.err, "absent.customisation.json"),
            "{}",
            started.err
        );
        assert_eq!(started.err.matches('\n').count(), 1, "{}", started.err);
        assert!(session.document().style_library().empty());
        assert_eq!(
            session.document().customisation_state().origin,
            CustomisationOrigin::None
        );
        assert!(run(&mut session, "POINT 1,1").ok);
    }

    // The two variables name FILES, and a file may sit under a folder named in
    // any script. They were read with getenv, which on Windows gives the ANSI
    // code page's bytes: a name the code page cannot spell arrived as '?', the
    // kept file was "not there" - the ordinary case, so nothing was said and
    // the built-in started in its place - and CUSTOMISE KEEP then failed to
    // write it. The folder here is named in two scripts no one code page holds.

    #[test]
    fn a_kept_file_under_a_folder_no_code_page_spells_starts_the_session_and_is_where_keep_writes()
    {
        let _process = process_lock();
        let scratch = ScratchDirectory::new("kept-under-two-scripts");
        let folder = scratch
            .path
            .join(path_from_utf8(format!("kept-{TWO_SCRIPTS}")));
        std::fs::create_dir_all(&folder).expect("the folder is made");
        let kept = folder.join("k.customisation.json");
        std::fs::copy(customisation_file("test_survey.customisation.json"), &kept)
            .expect("the survey customisation is copied");
        let symbols = customisation_file("test_symbols.customisation.json");
        let _built_in = ScopedVariable::set(BUILT_IN_CUSTOMISATION_VARIABLE, symbols);
        let _kept = ScopedVariable::set(KEPT_CUSTOMISATION_VARIABLE, &kept);

        let mut started = Captured::default();
        let mut session = Session::new(Some("katana_cli"), &mut started);
        // The kept file starts the session: the survey fixture's eleven rules
        // and no definition, not the built-in's four symbols.
        assert_eq!(
            started.out,
            "Customisation: test_survey, 0 linestyles and symbols and 11 survey code rules, \
             kept\n"
        );
        assert_eq!(started.err, "");
        assert_eq!(
            session.document().customisation_state().origin,
            CustomisationOrigin::Kept
        );
        assert_eq!(session.document().survey_map().size(), 11);

        // And it is where KEEP writes. An edit first, so that the session is no
        // longer the file: coding on a survey import is switched off.
        let set = run(&mut session, "CUSTOMISE SET auto.codes=off");
        assert!(set.ok, "{}", set.err);
        let keep = run(&mut session, "CUSTOMISE KEEP");
        assert!(keep.ok, "{}", keep.err);
        assert_eq!(keep.err, "");
        let written =
            read_customisation_file(&kept).unwrap_or_else(|error| panic!("{}", error.describe()));
        let automation = written
            .customisation
            .automation
            .expect("the kept file says what is automatic");
        assert!(!automation.codes_on_survey_import);
        assert!(automation.linework_on_survey_import);
        assert_eq!(written.customisation.map.size(), 11);
        // The file that was there stays beside it.
        let mut backup = kept.into_os_string();
        backup.push(".bak");
        assert!(Path::new(&backup).exists());
    }

    #[test]
    fn a_built_in_the_seam_names_under_such_a_folder_starts_the_session() {
        let _process = process_lock();
        let scratch = ScratchDirectory::new("built-in-under-two-scripts");
        let folder = scratch
            .path
            .join(path_from_utf8(format!("built-in-{TWO_SCRIPTS}")));
        std::fs::create_dir_all(&folder).expect("the folder is made");
        let built_in = folder.join("b.customisation.json");
        std::fs::copy(
            customisation_file("test_symbols.customisation.json"),
            &built_in,
        )
        .expect("the symbols customisation is copied");
        let _built_in = ScopedVariable::set(BUILT_IN_CUSTOMISATION_VARIABLE, built_in);
        let _kept = ScopedVariable::unset(KEPT_CUSTOMISATION_VARIABLE);

        let mut started = Captured::default();
        let session = Session::new(Some("katana_cli"), &mut started);
        // Read through the code page the seam named a file that is not there,
        // which is a problem said on stderr, and no built-in.
        assert_eq!(
            started.out,
            "Customisation: test_symbols, 4 linestyles and symbols and 0 survey code rules, \
             built in\n"
        );
        assert_eq!(started.err, "");
        assert_eq!(session.document().style_library().size(), 4);
    }
}
