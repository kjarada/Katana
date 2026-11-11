pub mod support;

#[allow(
    unsafe_code,
    reason = "Use the C runtime's narrow open independently of the path converter."
)]
mod narrow_file {
    use std::ffi::{CString, c_char, c_int, c_void};

    unsafe extern "C" {
        fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
        fn fwrite(data: *const c_void, size: usize, count: usize, file: *mut c_void) -> usize;
        fn fclose(file: *mut c_void) -> c_int;
    }

    pub fn write(path: &[u8]) {
        let path = CString::new(path).unwrap();
        let file = unsafe { fopen(path.as_ptr(), c"wb".as_ptr()) };
        assert!(
            !file.is_null(),
            "the narrow file must be created on the integration platform"
        );
        let written = unsafe { fwrite(b"narrow".as_ptr().cast(), 1, 6, file) };
        let closed = unsafe { fclose(file) };
        assert_eq!(written, 6);
        assert_eq!(closed, 0);
    }
}

mod path_text {
    use super::support::{Scratch, WorkingDirectory};
    use katana_core::ErrorCode;
    use katana_core::path_text::*;
    use std::path::Path;
    use std::process::Command;

    const ZURICH: &str = "Z\u{00fc}rich";
    const TWO_SCRIPTS: &str = "\u{65e5}\u{672c}-\u{05e2}\u{05d1}";
    const VARIABLE: &str = "KATANA_TEST_PATH_TEXT_VARIABLE";
    const STAGE: &str = "KATANA_CORE_PATH_TEST_STAGE";

    #[test]
    fn utf8_text_names_the_path_it_spells_and_is_said_back_as_the_same_text() {
        let path = path_from_utf8(format!("{ZURICH}/site plan.json"));
        assert_eq!(path.parent().unwrap().to_str().unwrap().chars().count(), 6);
        assert_eq!(path.file_name().unwrap(), "site plan.json");
        assert_eq!(
            path_to_utf8(Path::new(path.file_name().unwrap())),
            b"site plan.json"
        );
        assert_eq!(path_to_utf8(path.parent().unwrap()), ZURICH.as_bytes());
        assert_eq!(
            path_to_utf8(Path::new(
                path_from_utf8("plain/ascii.txt").file_name().unwrap()
            )),
            b"ascii.txt"
        );
        assert!(path_from_utf8("").as_os_str().is_empty());
    }

    #[test]
    fn bytes_that_are_not_utf8_are_taken_as_the_narrow_name_they_are_rather_than_thrown_on() {
        let ansi = b"caf\xe9.txt";
        let path = path_from_utf8(ansi);
        #[cfg(unix)]
        let native: Vec<u32> = {
            use std::os::unix::ffi::OsStrExt;
            path.as_os_str()
                .as_bytes()
                .iter()
                .map(|byte| u32::from(*byte))
                .collect()
        };
        #[cfg(windows)]
        let native: Vec<u32> = {
            use std::os::windows::ffi::OsStrExt;
            path.as_os_str().encode_wide().map(u32::from).collect()
        };
        assert!(native.len() >= 7);
        for (index, byte) in b"caf".iter().enumerate() {
            assert_eq!(native[index], u32::from(*byte), "{index}");
            assert_eq!(
                native[native.len() - 3 + index],
                u32::from(b"txt"[index]),
                "{index}"
            );
        }
        let scratch = Scratch::new("katana-core-tests-path-text-narrow-name");
        let directory = WorkingDirectory::enter(&scratch.root);
        super::narrow_file::write(ansi);
        let read = read_file_bytes(&path_from_utf8(ansi));
        drop(directory);
        assert_eq!(read.unwrap(), b"narrow");
    }

    #[test]
    fn an_environment_variable_is_read_as_the_text_it_holds_whatever_the_code_page() {
        if let Ok(stage) = std::env::var(STAGE) {
            match stage.as_str() {
                "unset" => {
                    assert_eq!(environment_variable(Some(VARIABLE)), b"");
                    assert_eq!(environment_variable(None), b"");
                    assert_eq!(environment_variable(Some("")), b"");
                }
                "plain" => assert_eq!(environment_variable(Some(VARIABLE)), b"plain value"),
                "scripts" => {
                    let read = environment_variable(Some(VARIABLE));
                    assert_eq!(read, TWO_SCRIPTS.as_bytes());
                    assert_eq!(
                        read.len(),
                        11,
                        "three bytes a kanji, one the hyphen, two a Hebrew letter"
                    );
                }
                "empty" => assert_eq!(environment_variable(Some(VARIABLE)), b""),
                _ => panic!("unknown child stage"),
            }
            return;
        }
        for (stage, value) in [
            ("unset", None),
            ("plain", Some("plain value")),
            ("scripts", Some(TWO_SCRIPTS)),
            ("empty", Some("")),
        ] {
            let mut child = Command::new(std::env::current_exe().unwrap());
            child.args(["--exact", "path_text::an_environment_variable_is_read_as_the_text_it_holds_whatever_the_code_page", "--test-threads=1"]);
            child.env(STAGE, stage).env_remove(VARIABLE);
            if let Some(value) = value {
                child.env(VARIABLE, value);
            }
            assert!(child.status().unwrap().success(), "{stage}");
        }
    }

    #[test]
    fn a_file_an_environment_variable_names_outside_the_code_page_is_the_file_read() {
        if std::env::var_os(STAGE).is_some() {
            let file = Path::new(&std::env::var_os(VARIABLE).unwrap()).to_path_buf();
            let named = environment_variable(Some(VARIABLE));
            assert_eq!(named, path_to_utf8(&file));
            assert!(
                String::from_utf8(named.clone())
                    .unwrap()
                    .contains(TWO_SCRIPTS)
            );
            assert_eq!(read_file_bytes(&path_from_utf8(named)).unwrap(), b"kept");
            return;
        }
        let scratch = Scratch::new("katana-core-tests-path-text-variable-names-a-file");
        let folder = scratch.root.join(path_from_utf8(TWO_SCRIPTS));
        std::fs::create_dir_all(&folder).unwrap();
        let file = scratch.touch(folder.join("kept.txt"));
        std::fs::write(&file, "kept").unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "path_text::a_file_an_environment_variable_names_outside_the_code_page_is_the_file_read", "--test-threads=1"])
            .env(STAGE, "file")
            .env(VARIABLE, &file)
            .status().unwrap();
        assert!(status.success());
    }

    #[test]
    fn a_file_is_read_as_every_byte_it_holds() {
        let scratch = Scratch::new("katana-core-tests-path-text-every-byte");
        let bytes = b"a\r\nb\0c\xff";
        let path = scratch.root.join(path_from_utf8(format!("{ZURICH}.bin")));
        std::fs::write(&path, bytes).unwrap();
        let read = read_file_bytes(&path).unwrap();
        assert_eq!(read, bytes);
        assert_eq!(read.len(), 7);
        let empty = scratch.root.join("empty.bin");
        std::fs::write(&empty, []).unwrap();
        assert!(read_file_bytes(&empty).unwrap().is_empty());
    }

    #[test]
    fn a_file_that_is_not_there_and_a_directory_are_not_found_naming_the_path() {
        let scratch = Scratch::new("katana-core-tests-path-text-not-found");
        let missing = scratch.root.join("nowhere.bin");
        let absent = read_file_bytes(&missing).unwrap_err();
        assert_eq!(absent.code, ErrorCode::NotFound);
        assert_eq!(absent.context.as_bytes(), path_to_utf8(&missing));
        let directory = read_file_bytes(&scratch.root).unwrap_err();
        assert_eq!(directory.code, ErrorCode::NotFound);
        assert_eq!(directory.context.as_bytes(), path_to_utf8(&scratch.root));
    }

    // No C++ case: the C++ gets this from libstdc++, whose path::generic_string
    // rebuilds the text from the path's elements (bits/fs_path.h). The expected
    // values follow that rule, and the first is the C++ katana_cli's own reply
    // to cli.import_of_a_zipped_shapefile_by_vsizip, which types the doubled
    // slash and answers with one.
    #[test]
    fn the_generic_form_writes_a_run_of_separators_as_one_as_libstdcxx_rebuilds_it() {
        let generic = |text: &str| generic_path_utf8(Path::new(text));
        assert_eq!(
            generic("/vsizip//root/out/lots.shp.zip/katana.shp"),
            b"/vsizip/root/out/lots.shp.zip/katana.shp"
        );
        assert_eq!(generic("a//b///c"), b"a/b/c");
        assert_eq!(generic("///top"), b"/top");
        assert_eq!(generic("///"), b"/");
        assert_eq!(generic("a/b//"), b"a/b/");
        assert_eq!(generic("./a/../b"), b"./a/../b");
        assert_eq!(generic("plain.txt"), b"plain.txt");
        assert_eq!(generic(""), b"");
    }
}
