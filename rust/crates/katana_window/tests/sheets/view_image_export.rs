//! The twins of tests/qt_widgets/plotting/test_view_image_export.cpp's Snapshot and
//! ViewImageDialog suites: the SNAPSHOT line read and written, a picture's size and its file
//! (katana_window::plotting::view_image_export), and File > Export View as Image driven by its
//! controls' object names. Its ViewImage suite paints a plan view (the plan view's render to
//! image) and waits for VP's viewport_widget.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use katana_window::canvas::{Color, Image, read_image};
use katana_window::host::VerbOutcome;
use katana_window::plotting::view_image_export::{
    Background, SnapshotRequest, View, ViewImageDialog, parse_snapshot, snapshot_command_line,
    snapshot_size, write_snapshot,
};

use crate::support::Bench;
use crate::window_support::{Scratch, fixed_host, ok};

/// QDir::toNativeSeparators.
fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        String::from(path)
    }
}

/// The dialog, shown, on the harness; every line it runs answered `outcome`.
fn bench(dialog: ViewImageDialog, outcome: VerbOutcome) -> Bench<ViewImageDialog> {
    let mut dialog = dialog;
    dialog.show();
    Bench::new("viewImageDialog", dialog, fixed_host(outcome))
}

mod snapshot {
    use super::*;

    #[test]
    fn a_file_takes_the_defaults_and_its_extension_is_its_format() {
        let request = parse_snapshot("SNAPSHOT \"C:/out/site view.png\"")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(request.path, "C:/out/site view.png");
        assert!(!request.clipboard);
        assert_eq!(request.width, 0);
        assert_eq!(request.height, 0);
        assert_eq!(request.scale, 1.0);
        assert_eq!(request.background, Background::Theme);
        assert_eq!(request.view, View::Plan);
        for line in [
            "SNAPSHOT a.JPG",
            "SNAPSHOT a.jpeg",
            "SNAPSHOT a.tif",
            "SNAPSHOT a.TIFF",
        ] {
            assert!(parse_snapshot(line).is_ok(), "{line}");
        }
    }

    #[test]
    fn the_clipboard_and_every_option_are_read() {
        let request =
            parse_snapshot("snapshot clipboard WIDTH=640 height=480 scale=2 bg=none view=PLAN")
                .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(request.clipboard);
        assert!(request.path.is_empty());
        assert_eq!(request.width, 640);
        assert_eq!(request.height, 480);
        assert_eq!(request.scale, 2.0);
        assert_eq!(request.background, Background::None);
        assert_eq!(request.view, View::Plan);
        let model = parse_snapshot("snapshot clipboard view=3D bg=theme")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(model.view, View::Model3D);
    }

    // The 3D view is grabbed on its own ground, so a ground asked of it is refused, in either
    // order, rather than dropped.
    #[test]
    fn a_ground_asked_of_the3_d_view_is_refused_rather_than_dropped() {
        for line in [
            "SNAPSHOT a.png view=3d bg=none",
            "SNAPSHOT a.png bg=white view=3d",
            "SNAPSHOT CLIPBOARD bg=none view=3d",
        ] {
            let refused = parse_snapshot(line)
                .expect_err("a ground is not the 3D view's")
                .describe();
            assert!(refused.contains("bg= is the plan view's"), "{line}: {refused}");
        }
    }

    #[test]
    fn what_the_grammar_does_not_take_is_refused() {
        for line in [
            "SNAPSHOT",
            "SNAPSHOT a.bmp",
            "SNAPSHOT a",
            "SNAPSHOT a.png width=0",
            "SNAPSHOT a.png width=10001",
            "SNAPSHOT a.png height=tall",
            "SNAPSHOT a.png scale=0.1",
            "SNAPSHOT a.png scale=9",
            "SNAPSHOT a.png bg=red",
            "SNAPSHOT a.png view=side",
            "SNAPSHOT a.png size=2",
            "SNAPSHOT a.png big",
            "SNAPSHOT a.jpg bg=none",
            "SNAPSHOT a.tif bg=none",
        ] {
            assert!(parse_snapshot(line).is_err(), "{line}");
        }
        let jpeg = parse_snapshot("SNAPSHOT a.jpg bg=none")
            .expect_err("a JPEG has no transparency")
            .describe();
        assert!(jpeg.contains("bg=none needs a .png"), "{jpeg}");
        // The clipboard keeps transparency, as a PNG does.
        assert!(parse_snapshot("SNAPSHOT CLIPBOARD bg=none").is_ok());
    }

    #[test]
    fn the_line_it_writes_is_the_line_it_reads() {
        let request = SnapshotRequest {
            // The path as the platform's file dialog gives it: "C:\out\view.png" on Windows. On
            // POSIX '\' is part of a file name, not a separator (POSIX.1-2017, 3.170), so each
            // platform is given its own native form.
            path: native("C:/out/view.png"),
            width: 300,
            height: 150,
            background: Background::White,
            ..SnapshotRequest::default()
        };
        let line = snapshot_command_line(&request);
        assert_eq!(
            line,
            "SNAPSHOT \"C:/out/view.png\" width=300 height=150 bg=white"
        );
        let read = parse_snapshot(&line).expect("the line reads back");
        assert_eq!(read.width, 300);
        assert_eq!(read.background, Background::White);
        assert_eq!(read.view, View::Plan);
        let model = SnapshotRequest {
            path: String::from("view.png"),
            view: View::Model3D,
            ..SnapshotRequest::default()
        };
        assert_eq!(
            snapshot_command_line(&model),
            "SNAPSHOT \"view.png\" view=3d"
        );
        assert_eq!(
            parse_snapshot(&snapshot_command_line(&model))
                .expect("the line reads back")
                .view,
            View::Model3D
        );
        let clipboard = SnapshotRequest {
            clipboard: true,
            scale: 2.5,
            ..SnapshotRequest::default()
        };
        assert_eq!(
            snapshot_command_line(&clipboard),
            "SNAPSHOT CLIPBOARD scale=2.5"
        );
    }

    #[test]
    fn the_size_is_given_or_taken_from_the_views_proportions() {
        let view = (800, 600);
        let both = SnapshotRequest {
            width: 300,
            height: 100,
            ..SnapshotRequest::default()
        };
        assert_eq!(snapshot_size(&both, view), (300, 100));
        // 400 x 400 * 600 / 800
        let wide = SnapshotRequest {
            width: 400,
            ..SnapshotRequest::default()
        };
        assert_eq!(snapshot_size(&wide, view), (400, 300));
        // 150 * 800 / 600 x 150
        let tall = SnapshotRequest {
            height: 150,
            ..SnapshotRequest::default()
        };
        assert_eq!(snapshot_size(&tall, view), (200, 150));
        let twice = SnapshotRequest {
            scale: 2.0,
            ..SnapshotRequest::default()
        };
        assert_eq!(snapshot_size(&twice, view), (1600, 1200));
        // Never past the largest side, never below one pixel.
        let huge = SnapshotRequest {
            scale: 8.0,
            ..SnapshotRequest::default()
        };
        assert_eq!(snapshot_size(&huge, (4000, 10)), (10000, 80));
        assert_eq!(snapshot_size(&SnapshotRequest::default(), (0, 0)), (1, 1));
    }

    #[test]
    fn each_format_is_written_and_reads_back() {
        let scratch = Scratch::new("view_image_export_formats");
        let mut image = Image::new(40, 20);
        image.fill(Color::rgb(0, 0, 255));
        for name in ["view.png", "view.jpg"] {
            let path = scratch.path(name);
            assert!(write_snapshot(&image, &path).is_ok(), "{name}");
            let read = read_image(&path).expect("the picture reads back");
            assert_eq!((read.width(), read.height()), (40, 20), "{name}");
        }
        // A TIFF through the project's own writer (tiff_writer): little-endian.
        let tiff = scratch.path("view.tif");
        assert!(write_snapshot(&image, &tiff).is_ok());
        let bytes = std::fs::read(&tiff).expect("the TIFF is there");
        assert_eq!(&bytes[..4], b"II*\0");
        assert!(
            write_snapshot(
                &image,
                &scratch.path("missing").join("folder").join("view.png")
            )
            .is_err()
        );
    }
}

mod view_image_dialog {
    use super::*;

    #[test]
    fn the_fields_write_the_line_and_export_runs_it() {
        let mut bench = bench(
            ViewImageDialog::new(""),
            ok("file=\"out.png\" view=plan width=300 height=150"),
        );
        assert!(bench.harness.target_record("viewImageDialog").is_some());
        assert!(!bench.enabled("viewImageExport"));
        bench.fill("viewImagePath", "out.png");
        assert_eq!(bench.value("viewImageCommand"), "SNAPSHOT \"out.png\"");
        bench.fill("viewImageSize", "2x the view");
        assert_eq!(
            bench.value("viewImageCommand"),
            "SNAPSHOT \"out.png\" scale=2"
        );
        assert!(!bench.enabled("viewImageWidth"));
        bench.fill("viewImageSize", "Custom size");
        bench.fill("viewImageWidth", "300");
        bench.fill("viewImageHeight", "150");
        bench.fill("viewImageBackground", "White");
        assert_eq!(
            bench.value("viewImageCommand"),
            "SNAPSHOT \"out.png\" width=300 height=150 bg=white"
        );
        // The ground is the plan view's: for the 3D view it is greyed out and left off the line,
        // which the verb would refuse.
        bench.fill("viewImageView", "3D view");
        assert!(!bench.enabled("viewImageBackground"));
        let expected = "SNAPSHOT \"out.png\" width=300 height=150 view=3d";
        assert_eq!(bench.value("viewImageCommand"), expected);
        bench.press("viewImageExport");
        assert_eq!(bench.host.lines(), [expected]);
        assert_eq!(
            bench.text("viewImageStatus"),
            "Exported: file=\"out.png\" view=plan width=300 height=150"
        );
    }

    #[test]
    fn the_format_sets_the_extension_and_a_transparent_jpeg_is_refused() {
        let mut bench = bench(ViewImageDialog::new(""), ok(""));
        bench.fill("viewImagePath", "C:/out/site.png");
        bench.fill("viewImageFormat", "JPEG");
        assert_eq!(bench.value("viewImagePath"), "C:/out/site.jpg");
        bench.fill("viewImageBackground", "Transparent");
        assert!(!bench.enabled("viewImageExport"));
        assert!(bench.value("viewImageCommand").is_empty());
        // The field's placeholder: why there is no line yet.
        let waiting = bench.control("viewImageCommand").text;
        assert!(waiting.contains("bg=none needs a .png"), "{waiting}");
        bench.fill("viewImageFormat", "PNG");
        assert!(bench.enabled("viewImageExport"));
    }

    // The file follows the drawing (suggest_path), in the format chosen, until one is typed; and a
    // file that is there is written over only when the person says so.
    #[test]
    fn the_suggested_file_follows_the_drawing_until_one_is_typed_and_a_replace_is_asked() {
        let scratch = Scratch::new("view_image_dialog_replace");
        let there = scratch
            .file("there.png", "")
            .to_string_lossy()
            .into_owned();

        let asked: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let replace = Rc::new(Cell::new(false));
        let mut dialog = ViewImageDialog::new("C:/a/a.png");
        {
            let asked = Rc::clone(&asked);
            let replace = Rc::clone(&replace);
            dialog.set_confirm_replace(Box::new(move |path: &str| {
                asked.borrow_mut().push(String::from(path));
                replace.get()
            }));
        }
        let mut bench = bench(dialog, ok("file=\"there.png\""));
        assert_eq!(bench.value("viewImagePath"), "C:/a/a.png");
        bench.fill("viewImageFormat", "JPEG");
        bench.dialog.suggest_path("C:/b/b.png");
        bench.draw();
        assert_eq!(
            bench.value("viewImagePath"),
            "C:/b/b.jpg",
            "another drawing's, in the format chosen"
        );

        bench.fill("viewImagePath", &there);
        bench.fill("viewImageFormat", "PNG");
        bench.dialog.suggest_path("C:/c/c.png");
        bench.draw();
        assert_eq!(bench.value("viewImagePath"), there, "a file typed is kept");

        bench.dialog.export_image(&mut bench.host);
        bench.draw();
        assert_eq!(*asked.borrow(), vec![there.clone()]);
        assert!(
            bench.host.lines().is_empty(),
            "kept when the person says no"
        );
        assert!(bench.text("viewImageStatus").starts_with("Not exported:"));
        replace.set(true);
        bench.dialog.export_image(&mut bench.host);
        assert_eq!(bench.host.lines().len(), 1);
    }
}
