//! tests/interop/test_archive12d.cpp, its ZipContainer cases: the ZIP
//! container a zipped archive file (.12daz) is read and written through. The
//! file's archive cases are katana_interop's.

use std::fs;
use std::path::{Path, PathBuf};

use katana_core::ErrorCode;
use katana_gis::zip_container::{list_zip, read_zip_member, write_zip};

use crate::{must, refused};

/// A folder of the test's own, named for the case and the process.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("katana-archive12d-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the scratch folder is made");
        Self { path }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn bytes_of(path: &Path) -> Vec<u8> {
    fs::read(path).expect("the file is read")
}

mod zip_container {
    use super::*;

    #[test]
    fn what_is_written_is_read_back_and_the_second_writing_replaces_the_first() {
        let dir = TempDir::new("zip");
        let archive = dir.file("a.12daz");
        let payload = vec![b'x'; 100_000];
        must(write_zip(&archive, "first.12da", b"one"));
        must(write_zip(&archive, "second.12da", &payload));

        let members = must(list_zip(&archive));
        // A directory cached from the first archive would still say "first".
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "second.12da");
        assert_eq!(members[0].size, 100_000);
        let bytes = must(read_zip_member(&archive, "second.12da", 1 << 20));
        assert_eq!(bytes, payload);
        // A hundred thousand identical bytes deflate to almost nothing; a
        // stored member would not.
        let size = fs::metadata(&archive).expect("the archive is there").len();
        assert!(size < 2000, "{size} bytes");
    }

    #[test]
    fn the_archive_is_the_kind_twelve_d_model_writes() {
        // ZIP APPNOTE 4.3.7, the local file header: signature 50 4B 03 04,
        // then version (2 bytes), flags (2), and the compression method (2) -
        // 8 is DEFLATE, which is what a .12daz written by the archive's own
        // program uses.
        let dir = TempDir::new("zip-header");
        let archive = dir.file("a.12daz");
        must(write_zip(&archive, "a.12da", b"model \"M\"\n"));
        let bytes = bytes_of(&archive);
        assert!(bytes.len() >= 10);
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        assert_eq!(bytes[8], 8);
        assert_eq!(bytes[9], 0);
    }

    #[test]
    fn failures_are_reported_not_guessed_around() {
        let dir = TempDir::new("zip-bad");
        assert_eq!(
            refused(list_zip(&dir.file("missing.12daz")), "a missing archive").code,
            ErrorCode::FileImportFailure
        );

        fs::write(dir.file("text.12daz"), "this is not a zip archive at all")
            .expect("the text file is written");
        assert!(list_zip(&dir.file("text.12daz")).is_err());

        let archive = dir.file("a.12daz");
        must(write_zip(&archive, "a.12da", b"0123456789"));
        assert_eq!(
            refused(
                read_zip_member(&archive, "other.12da", 100),
                "another member"
            )
            .code,
            ErrorCode::NotFound
        );
        // The limit is checked against the directory, before anything is
        // inflated.
        let too_big = refused(
            read_zip_member(&archive, "a.12da", 9),
            "a member over the limit",
        );
        assert!(too_big.message.contains("limit"), "{}", too_big.message);
        assert!(read_zip_member(&archive, "a.12da", 10).is_ok());

        for name in ["", "../escape.12da", "/rooted.12da", "C:\\drive.12da"] {
            assert_eq!(
                refused(write_zip(&dir.file("b.12daz"), name, b"x"), name).code,
                ErrorCode::InvalidArgument,
                "{name}"
            );
        }
    }

    #[test]
    fn an_empty_member_is_an_empty_string() {
        let dir = TempDir::new("zip-empty");
        let archive = dir.file("a.12daz");
        must(write_zip(&archive, "a.12da", b""));
        let bytes = must(read_zip_member(&archive, "a.12da", 100));
        assert!(bytes.is_empty());
    }
}
