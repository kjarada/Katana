//! tests/app/test_update_verbs.cpp: the cases that change what the process
//! shares - the running program's context, which reads KATANA_UPDATE_TOKEN
//! and KATANA_UPDATE_STATE_DIR, and katana_cli's session, which reads the
//! customisation variables as it starts. Each holds the process lock first.
//!
//! THE C++'S CliSession. A katana_cli session whose UPDATE lines run on the
//! rig instead of on this test program's own install (the C++'s
//! Session::setUpdateContextSource). The session owns its context source, so
//! the rig is shared with it through an Rc, and the source lends each line a
//! context borrowed from that rig (session.rs, UpdateContextSource). It
//! starts with no customisation (KATANA_BUILTIN_CUSTOMISATION=none, as the
//! C++'s does), and what it says as it starts is dropped.

mod update_verbs {
    use std::cell::RefCell;
    use std::rc::Rc;

    use katana_app::update_verbs::{make_update_context_for, run_update_line};
    use katana_app::{
        Captured, Session, UpdateContext, UpdateContextSource, UpdateSurface, make_update_context,
    };
    use katana_update::{
        InstallKind, UpdateState, parse_version, process_is_elevated, program_version,
        program_version_text, read_state, write_state,
    };

    use crate::start_environment::{ScopedVariable, StartEnvironment, process_lock};
    use crate::test_key::test_trust;
    use crate::update_rig::{
        AVAILABLE, FRESH_STATUS, NOW, Rig, Scratch, V1_SEQUENCE, V2_SEQUENCE, lines_of, must,
        write_bytes, write_install_records,
    };

    const TOKEN_VARIABLE: &str = "KATANA_UPDATE_TOKEN";
    const STATE_VARIABLE: &str = "KATANA_UPDATE_STATE_DIR";

    // The context source a CliSession hands its session: the rig's context for
    // the surface asked, each surface asked for kept.
    struct RigSource {
        rig: Rc<Rig>,
        asked: Rc<RefCell<Vec<UpdateSurface>>>,
    }

    impl UpdateContextSource for RigSource {
        fn context(&self, surface: UpdateSurface) -> UpdateContext<'_> {
            self.asked.borrow_mut().push(surface);
            self.rig.context(surface)
        }
    }

    // The C++'s CliSession (the module's head). Its fields drop in their order:
    // the session ends before the environment is put back, as the C++'s
    // members are destroyed.
    struct CliSession {
        session: Session,
        // The surface each context was asked for.
        asked: Rc<RefCell<Vec<UpdateSurface>>>,
        _environment: StartEnvironment,
    }

    impl CliSession {
        fn new(rig: &Rc<Rig>) -> Self {
            let environment = StartEnvironment::new(Some("none"), None);
            let mut session = Session::new(Some("katana_cli"), &mut Captured::default());
            let asked = Rc::new(RefCell::new(Vec::new()));
            session.set_update_context_source(Some(Box::new(RigSource {
                rig: Rc::clone(rig),
                asked: Rc::clone(&asked),
            })));
            Self {
                session,
                asked,
                _environment: environment,
            }
        }
    }

    #[test]
    fn katana_clis_session_runs_blocked_clear_and_prints_its_record() {
        let _process = process_lock();
        let rig = Rc::new(Rig::new());
        let state = UpdateState {
            blocked_sequences: vec![V2_SEQUENCE],
            ..UpdateState::default()
        };
        write_state(&rig.state, &state).expect("the state is written");
        let mut cli = CliSession::new(&rig);
        {
            let mut captured = Captured::default();
            assert!(cli.session.run("UPDATE BLOCKED CLEAR", &mut captured));
            assert_eq!(captured.out, "blocked=none\n");
            assert!(captured.err.is_empty(), "{}", captured.err);
        }
        let after = read_state(&rig.state).expect("the state reads");
        assert!(after.blocked_sequences.is_empty());
        {
            // A word that is not the line's: the refusal on `err`, the run
            // failed.
            let mut captured = Captured::default();
            assert!(!cli.session.run("UPDATE BLOCKED", &mut captured));
            assert!(captured.out.is_empty(), "{}", captured.out);
            assert!(
                captured
                    .err
                    .starts_with("error: InvalidArgument: UPDATE BLOCKED takes CLEAR"),
                "{}",
                captured.err
            );
        }
        assert!(cli.session.take_pending_apply().is_none());
    }

    // ---- the running program's context --------------------------------------------------------

    #[test]
    fn the_context_of_a_program_in_no_install_is_a_development_copys() {
        // This suite's executable is in no install: nothing packaging wrote lies
        // beside it. The token comes from the variable alone here, so that the
        // credential store of whoever runs the suite is not read.
        let _process = process_lock();
        let _token = ScopedVariable::set(TOKEN_VARIABLE, "ghp_from_the_environment");
        let scratch = Scratch::new();
        let _folder = ScopedVariable::set(STATE_VARIABLE, scratch.path().join("U"));

        for surface in [
            UpdateSurface::Cli,
            UpdateSurface::Window,
            UpdateSurface::HeadlessWindow,
            UpdateSurface::Mcp,
        ] {
            let context = make_update_context(surface);
            assert_eq!(context.surface, surface);
            assert_eq!(context.install.kind, InstallKind::Development);
            assert!(context.install.target.is_empty());
            assert_eq!(
                katana_update::version::to_string(&context.install.version),
                program_version_text()
            );
            assert!(
                context.install_problem.is_empty(),
                "{}",
                context.install_problem
            );
            assert!(context.fetch.is_some());
            assert!(context.credentials.is_some());
            assert_eq!(context.feed.token, "ghp_from_the_environment");
            assert_eq!(context.pid, 0, "left for the caller");
            assert!(context.relaunch.as_os_str().is_empty());
            // How this process was started, as the update library answers it:
            // whichever that is for this run of the suite, the context says it.
            // WHAT THIS LINE CANNOT SHOW in a suite started the ordinary way:
            // both sides are then "no", and a context that was never filled is
            // "no" as well. It fails for one only in a suite that itself holds
            // the elevated half of a split token (one started as
            // administrator). No test can make the system answer "yes", and the
            // member is set by the tests for that reason: what a "yes" refuses
            // is shown with it set (the tests under "a Katana that was started
            // as administrator"), and which token is a "yes" in
            // katana_update's own tests.
            assert_eq!(context.elevated, process_is_elevated());
            let keeps = matches!(surface, UpdateSurface::Cli | UpdateSurface::Window);
            assert_eq!(context.state_directory.as_os_str().is_empty(), !keeps);
            if keeps {
                assert_eq!(context.state_directory, scratch.path().join("U"));
            }

            // And the lines on it: a development copy, asked about by nobody.
            let checked = must(run_update_line(&context, "UPDATE CHECK"));
            assert!(
                checked.text.contains("update=unsupported"),
                "{}",
                checked.text
            );
            assert!(
                checked.text.contains("reason=development_tree"),
                "{}",
                checked.text
            );
            assert!(!checked.text.contains("ghp_from_the_environment"));
            let status = must(run_update_line(&context, "UPDATE STATUS"));
            assert!(status.text.contains("kind=development"), "{}", status.text);
            assert!(status.text.contains("token=present"), "{}", status.text);
        }
        assert!(
            !scratch.path().join("U").exists(),
            "nothing was kept by any of them"
        );
    }

    #[test]
    fn a_headless_context_has_no_token_without_the_variable() {
        let _process = process_lock();
        let _token = ScopedVariable::unset(TOKEN_VARIABLE);
        for surface in [UpdateSurface::HeadlessWindow, UpdateSurface::Mcp] {
            let context = make_update_context(surface);
            assert!(context.feed.token.is_empty());
            let status = must(run_update_line(&context, "UPDATE STATUS"));
            assert!(status.text.contains("token=absent"), "{}", status.text);
        }
    }

    #[test]
    fn the_context_of_a_program_in_an_install_is_that_installs_own_records() {
        // A scratch install with the two records an installed copy has, and the
        // context of the program that lies in its bin/. The token comes from
        // the variable and the state folder is the test's own, so that neither
        // the credential store nor the settings of whoever runs the suite is
        // read.
        let _process = process_lock();
        let _token = ScopedVariable::set(TOKEN_VARIABLE, "ghp_from_the_environment");
        let rig = Rig::new();
        let _folder = ScopedVariable::set(STATE_VARIABLE, &rig.state);
        write_install_records(&rig.root);
        let program = rig.root.join("bin").join("katana_cli.exe");
        assert!(program.exists());
        // What is installed is the higher of the two builds the records name:
        // the helper's 0.9.0, above the package's own 0.8.0 (9 > 8). The
        // package names its version, so the program that reads it does not
        // stand for the copy (katana_update's install, read_install), and
        // nothing below depends on the version this project has reached -
        // which it did while install.json said the target and no more. That
        // reading has its own test, next.
        let mut context = make_update_context_for(UpdateSurface::Cli, &program);
        assert_eq!(context.surface, UpdateSurface::Cli);
        assert!(
            context.install_problem.is_empty(),
            "{}",
            context.install_problem
        );
        assert_eq!(context.install.root, rig.root);
        assert_eq!(context.install.target, "win64");
        // No installer recorded this scratch folder as the place it installed
        // to.
        assert_eq!(context.install.kind, InstallKind::Archive);
        assert_eq!(
            katana_update::version::to_string(&context.install.version),
            "0.9.0"
        );
        assert_eq!(context.install.sequence, V1_SEQUENCE);
        assert!(context.install.has_installed_record);
        assert_eq!(
            context.feed.target, "win64",
            "the feed is asked for the copy's own target"
        );
        assert_eq!(context.feed.token, "ghp_from_the_environment");
        assert_eq!(context.state_directory, rig.state);
        assert_eq!(context.pid, 0, "left for the caller");
        assert!(context.relaunch.as_os_str().is_empty());
        // As in the test above: "no" on both sides unless the suite itself
        // holds the elevated half of a split token, so only such a run sees
        // this form ask.
        assert_eq!(
            context.elevated,
            process_is_elevated(),
            "this form asks as well"
        );

        // The lines on it, with the rig's feed and the fixtures' keys where the
        // network and the release keys are: an installed copy, so it IS asked
        // about. (The address is the rig's, whatever this build was configured
        // to ask.)
        context.trust = Some(test_trust());
        context.feed = rig.feed_config.clone();
        context.fetch = rig.feed.fetch();
        context.now_epoch = NOW;
        rig.serve_v2();
        let status = must(run_update_line(&context, "UPDATE STATUS"));
        assert_eq!(
            status.text,
            "installed=0.9.0 target=win64 kind=archive staged=none skipped=none auto=on \
             last_check=never token=present last=none"
        );
        let checked = must(run_update_line(&context, "UPDATE CHECK"));
        assert_eq!(checked.text, AVAILABLE);
    }

    #[test]
    fn the_context_of_an_install_whose_record_is_below_this_program_says_this_programs_version() {
        // What a newer zip unpacked by hand over a copy that had updated itself
        // leaves: the helper's record of the older version beside the programs
        // of the newer one. This zip's install.json says its target and no
        // more, so the program that reads it stands for the copy, and the
        // installed version is the higher of the record's and this program's
        // own (katana_update's install, read_install): the record cannot lower
        // what a release must be above. 0.1.0 is a version this project left
        // behind.
        let _process = process_lock();
        let _token = ScopedVariable::set(TOKEN_VARIABLE, "ghp_from_the_environment");
        let rig = Rig::new();
        let _folder = ScopedVariable::set(STATE_VARIABLE, &rig.state);
        let records = rig.root.join("share").join("katana");
        write_bytes(
            &records.join("install.json"),
            "{\"schema\":1,\"target\":\"win64\"}\n",
        );
        write_bytes(
            &records.join("installed.json"),
            "{\"schema\":1,\"product\":\"katana\",\"version\":\"0.1.0\",\"sequence\":1700000000,\
             \"tag\":\"v0.1.0\",\"target\":\"win64\"}\n",
        );
        let own = program_version().expect("this build has a version");
        assert!(parse_version("0.1.0").expect("0.1.0 is a version") < own);

        let context = make_update_context_for(
            UpdateSurface::Cli,
            &rig.root.join("bin").join("katana_cli.exe"),
        );
        assert!(
            context.install_problem.is_empty(),
            "{}",
            context.install_problem
        );
        assert_eq!(context.install.version, own);
        assert_eq!(
            katana_update::version::to_string(&context.install.version),
            program_version_text()
        );
        // The record is still the record: its sequence, and that there is one.
        assert_eq!(context.install.sequence, 1700000000);
        assert!(context.install.has_installed_record);

        // And it is what the lines say is installed.
        let status = must(run_update_line(&context, "UPDATE STATUS"));
        assert!(
            status.text.starts_with(&format!(
                "installed={} target=win64 kind=archive ",
                program_version_text()
            )),
            "{}",
            status.text
        );
    }

    #[test]
    fn the_context_of_a_program_whose_install_record_does_not_read_says_so_and_is_asked_about_by_nobody()
     {
        let _process = process_lock();
        let _token = ScopedVariable::set(TOKEN_VARIABLE, "ghp_from_the_environment");
        let rig = Rig::new();
        let _folder = ScopedVariable::set(STATE_VARIABLE, &rig.state);
        write_bytes(
            &rig.root.join("share").join("katana").join("install.json"),
            "this is not a record",
        );

        let mut context = make_update_context_for(
            UpdateSurface::Cli,
            &rig.root.join("bin").join("katana_cli.exe"),
        );
        // Why, and of which file: the error of the reading names it
        // (katana_update's install).
        assert!(
            context.install_problem.contains("install.json"),
            "{}",
            context.install_problem
        );
        assert_eq!(context.install.root, rig.root);
        assert!(context.install.target.is_empty());
        assert_eq!(
            katana_update::version::to_string(&context.install.version),
            program_version_text()
        );

        context.trust = Some(test_trust());
        context.feed = rig.feed_config.clone();
        context.fetch = rig.feed.fetch();
        context.now_epoch = NOW;
        rig.serve_v2();
        let version = program_version_text();
        let checked = must(run_update_line(&context, "UPDATE CHECK"));
        assert!(
            checked.text.starts_with(&format!(
                "update=unsupported installed={version} reason=install_record detail="
            )),
            "{}",
            checked.text
        );
        let status = must(run_update_line(&context, "UPDATE STATUS"));
        let lines = lines_of(&status.text);
        assert_eq!(lines.len(), 2, "{}", status.text);
        assert_eq!(
            lines[0],
            format!(
                "installed={version} target=none kind=development staged=none skipped=none \
                 auto=on last_check=never token=present last=none"
            )
        );
        assert!(
            lines[1].starts_with("warning text=\"this copy's install record could not be read"),
            "{}",
            lines[1]
        );
        assert!(
            rig.feed.requests.borrow().is_empty(),
            "nothing is asked about a copy that is not known"
        );
        assert!(!rig.state.exists());
    }

    // ---- the help, and the session ------------------------------------------------------------

    #[test]
    fn a_programs_session_runs_its_update_lines_on_the_context_it_is_handed_and_prints_the_reply() {
        let _process = process_lock();
        let rig = Rc::new(Rig::new());
        rig.serve_v2();
        let mut cli = CliSession::new(&rig);
        {
            let mut captured = Captured::default();
            assert!(cli.session.run("UPDATE STATUS", &mut captured));
            assert_eq!(captured.out, format!("{FRESH_STATUS}\n"));
            assert!(captured.err.is_empty(), "{}", captured.err);
        }
        {
            let mut captured = Captured::default();
            assert!(cli.session.run("update check", &mut captured));
            assert_eq!(captured.out, format!("{AVAILABLE}\n"));
            assert!(captured.err.is_empty(), "{}", captured.err);
        }
        {
            // A refusal is the line's failure: said on `err`, nothing on `out`.
            let mut captured = Captured::default();
            assert!(!cli.session.run("UPDATE DOWNLOAD", &mut captured));
            assert!(captured.out.is_empty(), "{}", captured.out);
            assert!(
                captured
                    .err
                    .starts_with("error: InvalidArgument: UPDATE DOWNLOAD needs the word CONFIRM"),
                "{}",
                captured.err
            );
        }
        // The context is asked for as katana_cli's, anew for each line.
        {
            let asked = cli.asked.borrow();
            assert!(asked.len() >= 2);
            for surface in asked.iter() {
                assert_eq!(*surface, UpdateSurface::Cli);
            }
        }
        assert!(cli.session.take_pending_apply().is_none());

        // Told that it is katana_mcp's, the session refuses the line before any
        // context is made: nothing of the install or the user is read for it.
        cli.session.set_update_surface(UpdateSurface::Mcp);
        cli.asked.borrow_mut().clear();
        {
            let mut captured = Captured::default();
            assert!(!cli.session.run("UPDATE CHECK", &mut captured));
            assert!(
                captured.err.contains("katana_check_update"),
                "{}",
                captured.err
            );
        }
        assert!(cli.asked.borrow().is_empty());
    }

    // ---- katana_cli's own path: the session -----------------------------------------------------
    //
    // The cases of the C++'s Windows branch, where the system's file system
    // the download and the apply go through is written. In a module of the
    // suite's own name, so that the parity checker reads them as the suite's.
    // Written, not compiled until step 8.
    #[cfg(windows)]
    mod update_verbs {
        use std::rc::Rc;

        use katana_app::Captured;
        use katana_update::is_valid_transaction_id;

        use super::CliSession;
        use crate::start_environment::process_lock;
        use crate::update_rig::{
            Rig, lines_of, listed_for_the_sweep, put_a_helper_in, transaction_of,
        };

        #[test]
        fn katana_clis_session_prints_a_downloads_progress_once_and_keeps_the_prepared_apply_to_be_taken()
         {
            let _process = process_lock();
            let rig = Rc::new(Rig::new());
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let request = {
                let mut cli = CliSession::new(&rig);
                let downloaded = {
                    let mut captured = Captured::default();
                    assert!(
                        cli.session.run("UPDATE DOWNLOAD CONFIRM", &mut captured),
                        "{}",
                        captured.err
                    );
                    assert!(captured.err.is_empty(), "{}", captured.err);
                    captured.out
                };
                // Progress lines, then what is staged: the verbs' reply, a line
                // each.
                let lines = lines_of(&downloaded);
                assert!(lines.len() >= 2, "{downloaded}");
                assert_eq!(lines[lines.len() - 1], "staged=0.10.0");
                assert_eq!(lines[lines.len() - 2], "progress bytes=2949/2949 files=5/5");
                for line in &lines[..lines.len() - 1] {
                    assert!(line.starts_with("progress bytes="), "{line}");
                }
                // Each once. A progress line is printed as it happens AND is in
                // the reply, which is printed when the line has run; and no two
                // lines of one download are the same, for one is said only when
                // the bytes or the files have moved on. So a line seen twice was
                // printed twice.
                let mut sorted = lines.clone();
                sorted.sort();
                assert!(
                    sorted.windows(2).all(|pair| pair[0] != pair[1]),
                    "{downloaded}"
                );

                let applied = {
                    let mut captured = Captured::default();
                    assert!(
                        cli.session.run("UPDATE APPLY CONFIRM", &mut captured),
                        "{}",
                        captured.err
                    );
                    assert!(captured.err.is_empty(), "{}", captured.err);
                    captured.out
                };
                let taken = cli
                    .session
                    .take_pending_apply()
                    .expect("the line that succeeded left its request");
                assert_eq!(
                    applied,
                    format!("ready=0.10.0 transaction={}\n", taken.transaction)
                );
                assert!(
                    cli.session.take_pending_apply().is_none(),
                    "handed over once"
                );
                taken
            };

            // What katana_cli's request tells the helper: wait for THIS process
            // - the one that ran the line - and start nothing afterwards.
            assert_eq!(request.wait_pid, u64::from(std::process::id()));
            assert!(request.relaunch.as_os_str().is_empty());
            assert_eq!(request.root, rig.root);
            assert_eq!(request.stage_directory, rig.stage_directory());
            assert_eq!(request.state_directory, rig.state);
            assert!(
                is_valid_transaction_id(&request.transaction),
                "{}",
                request.transaction
            );

            // The session started nothing: its front end does, once the session
            // has ended. And a request that was taken is the front end's to
            // launch - the session that ended did not give its helper copy up.
            assert!(rig.launcher.detached.borrow().is_empty());
            assert!(request.helper.exists());
            assert!(listed_for_the_sweep(&rig.state).is_empty());
        }

        #[test]
        fn a_session_whose_apply_is_refused_leaves_nothing_to_launch() {
            let _process = process_lock();
            let rig = Rc::new(Rig::new());
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let mut cli = CliSession::new(&rig);
            {
                // Nothing is downloaded yet.
                let mut captured = Captured::default();
                assert!(!cli.session.run("UPDATE APPLY CONFIRM", &mut captured));
                assert!(
                    captured.err.contains("reason=not_staged"),
                    "{}",
                    captured.err
                );
                assert!(captured.out.is_empty(), "{}", captured.out);
            }
            assert!(cli.session.take_pending_apply().is_none());

            {
                let mut captured = Captured::default();
                assert!(
                    cli.session.run("UPDATE DOWNLOAD CONFIRM", &mut captured),
                    "{}",
                    captured.err
                );
            }
            {
                // Downloaded, and the helper does not prove that it starts.
                rig.launcher.exit_code.set(3);
                let mut captured = Captured::default();
                assert!(!cli.session.run("UPDATE APPLY CONFIRM", &mut captured));
                assert!(
                    captured.err.contains("reason=helper_failed"),
                    "{}",
                    captured.err
                );
                assert!(!captured.out.contains("ready="), "{}", captured.out);
            }
            assert!(cli.session.take_pending_apply().is_none());
            assert!(rig.launcher.detached.borrow().is_empty());
        }

        #[test]
        fn a_session_gives_up_a_prepared_apply_that_a_later_one_replaces_or_that_nobody_took() {
            let _process = process_lock();
            let rig = Rc::new(Rig::new());
            rig.serve_v2();
            put_a_helper_in(&rig.root);
            let (first, second) = {
                let mut cli = CliSession::new(&rig);
                {
                    let mut captured = Captured::default();
                    assert!(
                        cli.session.run("UPDATE DOWNLOAD CONFIRM", &mut captured),
                        "{}",
                        captured.err
                    );
                }
                let first = {
                    let mut captured = Captured::default();
                    assert!(
                        cli.session.run("UPDATE APPLY CONFIRM", &mut captured),
                        "{}",
                        captured.err
                    );
                    transaction_of(&captured.out)
                };
                assert!(is_valid_transaction_id(&first), "{first}");
                assert!(
                    listed_for_the_sweep(&rig.state).is_empty(),
                    "it may yet be taken and launched"
                );

                // A second apply is prepared before anybody took the first: one
                // is launched at most, so the first's helper copy is nobody's.
                let second = {
                    let mut captured = Captured::default();
                    assert!(
                        cli.session.run("UPDATE APPLY CONFIRM", &mut captured),
                        "{}",
                        captured.err
                    );
                    transaction_of(&captured.out)
                };
                assert!(is_valid_transaction_id(&second), "{second}");
                assert_ne!(first, second);
                assert_eq!(
                    listed_for_the_sweep(&rig.state),
                    vec![rig.state.join(format!("run-{first}"))]
                );
                (first, second)
            };
            // The session ended and nobody took the second: that one is given up
            // too.
            assert_eq!(
                listed_for_the_sweep(&rig.state),
                vec![
                    rig.state.join(format!("run-{first}")),
                    rig.state.join(format!("run-{second}"))
                ]
            );
            assert!(rig.launcher.detached.borrow().is_empty());
        }
    }
}
