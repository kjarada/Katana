mod simd_level {
    use katana_core::ErrorCode;
    use katana_core::cpu_features::*;

    #[test]
    fn an_empty_request_keeps_the_detected_level() {
        assert_eq!(
            choose_simd_level("", SimdLevel::Avx2).unwrap(),
            SimdLevel::Avx2
        );
        assert_eq!(
            choose_simd_level("", SimdLevel::Scalar).unwrap(),
            SimdLevel::Scalar
        );
    }

    #[test]
    fn scalar_can_be_chosen_on_any_processor() {
        for detected in [SimdLevel::Scalar, SimdLevel::Avx2, SimdLevel::Neon] {
            assert_eq!(
                choose_simd_level("scalar", detected).unwrap(),
                SimdLevel::Scalar
            );
        }
    }

    #[test]
    fn avx2_is_chosen_where_the_processor_runs_it() {
        assert_eq!(
            choose_simd_level("avx2", SimdLevel::Avx2).unwrap(),
            SimdLevel::Avx2
        );
    }

    #[test]
    fn avx2_is_refused_on_a_processor_without_it_rather_than_faulting_or_running_scalar_quietly() {
        let error = choose_simd_level("avx2", SimdLevel::Scalar).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("avx2"), "{}", error.message);
    }

    #[test]
    fn neon_is_chosen_where_the_processor_runs_it() {
        assert_eq!(
            choose_simd_level("neon", SimdLevel::Neon).unwrap(),
            SimdLevel::Neon
        );
        assert_eq!(
            choose_simd_level("", SimdLevel::Neon).unwrap(),
            SimdLevel::Neon
        );
    }

    #[test]
    fn neon_is_refused_on_a_processor_without_it() {
        for detected in [SimdLevel::Scalar, SimdLevel::Avx2] {
            let error = choose_simd_level("neon", detected).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert!(error.message.contains("neon"), "{}", error.message);
        }
    }

    #[test]
    fn avx2_is_refused_on_a_neon_processor_although_it_comes_first_in_the_enumeration() {
        let error = choose_simd_level("avx2", SimdLevel::Neon).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("avx2"), "{}", error.message);
    }

    #[test]
    fn a_word_that_names_no_level_is_refused_naming_the_words_that_do() {
        for word in [
            "AVX2", "sse2", "avx512", "fast", " scalar", "NEON", "asimd", "neon ",
        ] {
            for detected in [SimdLevel::Avx2, SimdLevel::Neon] {
                let error = choose_simd_level(word, detected).unwrap_err();
                assert_eq!(error.code, ErrorCode::InvalidArgument, "{word}");
                assert!(
                    error.message.contains("use scalar, avx2 or neon"),
                    "{}",
                    error.message
                );
            }
        }
    }

    #[test]
    fn the_level_in_force_is_never_one_the_processor_cannot_run() {
        let detected = detected_simd_level();
        let active = active_simd_level();
        assert!(
            active == SimdLevel::Scalar || active == detected,
            "{active}"
        );
        let selection = simd_selection();
        assert!(selection.active == SimdLevel::Scalar || selection.active == selection.detected);
    }

    #[test]
    fn the_detected_level_belongs_to_this_processors_architecture() {
        #[cfg(target_arch = "aarch64")]
        {
            assert_ne!(detected_simd_level(), SimdLevel::Avx2);
            #[cfg(feature = "simd-kernels")]
            assert_eq!(detected_simd_level(), SimdLevel::Neon);
        }
        #[cfg(not(target_arch = "aarch64"))]
        assert_ne!(detected_simd_level(), SimdLevel::Neon);
    }

    #[test]
    fn the_environment_override_decides_the_starting_level() {
        let text = katana_core::text_encoding::utf8_or_windows1252(
            katana_core::path_text::environment_variable(Some("KATANA_SIMD")),
        );
        let selection = simd_selection();
        match choose_simd_level(&text, selection.detected) {
            Ok(expected) => {
                assert_eq!(selection.active, expected);
                assert_eq!(
                    selection.note.is_empty(),
                    expected == selection.detected,
                    "{}",
                    selection.note
                );
            }
            Err(_) => {
                assert_eq!(selection.active, selection.detected);
                assert!(selection.note.contains("ignored"), "{}", selection.note);
            }
        }
    }

    #[test]
    fn environment_overrides_are_checked_in_fresh_processes() {
        for requested in ["scalar", to_string(detected_simd_level()), "invalid-level"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "simd_level::the_environment_override_decides_the_starting_level",
                ])
                .env("KATANA_SIMD", requested)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success(),
                "KATANA_SIMD={requested}: {stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(stdout.contains("1 passed; 0 failed"), "{stdout}");
        }
    }

    #[test]
    fn setting_a_level_returns_the_previous_one_so_it_can_be_restored() {
        let before = active_simd_level();
        assert_eq!(set_simd_level(SimdLevel::Scalar).unwrap(), before);
        assert_eq!(active_simd_level(), SimdLevel::Scalar);
        set_simd_level(before).unwrap();
        assert_eq!(active_simd_level(), before);
    }

    #[test]
    fn levels_have_the_names_the_override_accepts() {
        assert_eq!(to_string(SimdLevel::Scalar), "scalar");
        assert_eq!(to_string(SimdLevel::Avx2), "avx2");
        assert_eq!(to_string(SimdLevel::Neon), "neon");
        for level in [SimdLevel::Scalar, SimdLevel::Avx2, SimdLevel::Neon] {
            assert_eq!(choose_simd_level(to_string(level), level).unwrap(), level);
        }
    }

    #[test]
    fn ordinary_code_is_compiled_for_the_baseline_so_the_program_starts_on_any_x64_machine() {
        assert!(
            !std::hint::black_box(cfg!(any(target_feature = "avx", target_feature = "avx2"))),
            "an ordinary source file was compiled with AVX enabled"
        );
    }
}
