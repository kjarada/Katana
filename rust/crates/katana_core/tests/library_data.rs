pub mod support;

mod library_data {
    use super::support::{Scratch, WorkingDirectory};
    use katana_core::library_data::*;
    use std::path::Path;

    fn equivalent(left: &Path, right: &Path) -> bool {
        std::fs::canonicalize(left).unwrap() == std::fs::canonicalize(right).unwrap()
    }

    #[test]
    fn a_loaded_library_is_found_by_the_start_of_its_file_name() {
        #[cfg(target_os = "linux")]
        {
            let libc = loaded_library_path("libc.so").unwrap();
            assert!(
                libc.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("libc.so")
            );
            assert!(libc.exists(), "{}", libc.display());
        }
        #[cfg(target_os = "macos")]
        {
            let system = loaded_library_path("libSystem.").unwrap();
            assert!(
                system
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("libSystem.")
            );
        }
        #[cfg(windows)]
        {
            let kernel = loaded_library_path("kernel32").unwrap();
            assert!(kernel.exists(), "{}", kernel.display());
            assert_eq!(loaded_library_path("KERNEL32"), Some(kernel));
            assert!(loaded_library_path("kernel33").is_none());
        }
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn data_beside_a_library_is_the_librarys_grandparent_joined_to_the_path_asked() {
        #[cfg(target_os = "linux")]
        let name = "libc.so";
        #[cfg(windows)]
        let name = "kernel32";
        let library = loaded_library_path(name).unwrap();
        let real = std::fs::canonicalize(library).unwrap();
        let relative =
            Path::new(real.parent().unwrap().file_name().unwrap()).join(real.file_name().unwrap());
        let found = data_beside_library(name, &relative).unwrap();
        assert!(equivalent(&found, &real), "{}", found.display());
        assert!(data_beside_library(name, Path::new("katana_no_such_directory/x")).is_none());
    }

    #[test]
    fn data_of_a_library_in_lib_is_in_the_shared_prefix() {
        let prefix = Scratch::new("katana_library_data_lib");
        let library = prefix.touch("lib/libproj.so.25");
        prefix.touch("share/proj/proj.db");
        let found = data_beside_library_file(&library, Path::new("share/proj/proj.db")).unwrap();
        assert!(equivalent(&found, &prefix.root.join("share/proj/proj.db")));
    }

    #[test]
    fn data_of_a_library_in_an_application_bundles_frameworks_is_in_its_resources() {
        let bundle = Scratch::new("katana_library_data_bundle");
        let library = bundle.touch("Contents/Frameworks/libproj.25.dylib");
        bundle.touch("Contents/Resources/share/proj/proj.db");
        bundle.touch("Contents/Resources/ssl/cacert.pem");
        let proj = data_beside_library_file(&library, Path::new("share/proj/proj.db")).unwrap();
        assert!(equivalent(
            &proj,
            &bundle.root.join("Contents/Resources/share/proj/proj.db")
        ));
        let certificates = data_beside_library_file(&library, Path::new("ssl/cacert.pem")).unwrap();
        assert!(equivalent(
            &certificates,
            &bundle.root.join("Contents/Resources/ssl/cacert.pem")
        ));
    }

    #[test]
    fn resources_are_looked_in_only_beside_a_frameworks_directory() {
        let prefix = Scratch::new("katana_library_data_not_bundle");
        let library = prefix.touch("lib/libproj.so.25");
        prefix.touch("Resources/share/proj/proj.db");
        assert!(data_beside_library_file(&library, Path::new("share/proj/proj.db")).is_none());
    }

    #[test]
    fn missing_data_beside_a_library_is_not_found() {
        let bundle = Scratch::new("katana_library_data_missing");
        let library = bundle.touch("Contents/Frameworks/libgdal.dylib");
        assert!(data_beside_library_file(&library, Path::new("share/gdal")).is_none());
    }

    #[test]
    fn an_empty_name_is_refused_wherever_the_working_directory_is() {
        let prefix = Scratch::new("katana_library_data_empty");
        prefix.touch("share/gdal/gdalvrt.xsd");
        let inside = prefix.root.join("lib/bin");
        std::fs::create_dir_all(&inside).unwrap();
        let directory = WorkingDirectory::enter(&inside);
        let found = data_beside_library_file(Path::new(""), Path::new("share/gdal"));
        drop(directory);
        assert!(found.is_none());
    }

    #[test]
    fn no_data_is_beside_a_library_that_is_not_loaded() {
        assert!(data_beside_library("libkatana_no_such_library.so", Path::new("share")).is_none());
    }

    #[test]
    fn a_library_that_is_not_loaded_is_not_found() {
        assert!(loaded_library_path("libkatana_no_such_library.so").is_none());
    }

    #[test]
    fn a_proj_data_directory_given_holds_proj_db() {
        if let Some(data) = proj_data_directory() {
            assert!(data.join("proj.db").is_file(), "{}", data.display());
            assert!(std::env::var_os("PROJ_DATA").is_none());
        }
    }
}
