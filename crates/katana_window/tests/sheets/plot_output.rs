//! The twins of tests/qt_widgets/plotting/test_plot_output.cpp's PlotOutput suite (its TiffWriter
//! suite is C's, in tests/canvas): a sheet set plotted to one PDF, a PDF a sheet, PNG and TIFF
//! rasters and a printer's PDF (katana_window::plotting::plot_output) - what is written, how it is
//! named, what a cancel and a failure leave, and what is refused before anything is written.
//!
//! A PDF a case reads is written readable (PlotRequest::readable_pdf), as the C++ read
//! QPdfWriter's. A PNG's resolution is read from its pHYs chunk, where the C++ asked the QImage it
//! read back. The printer is the PDF the sheets are written to for the operating system
//! (PdfPrinter), where the C++ had a QPrinter printing to a PDF.

use std::collections::BTreeMap;
use std::path::Path;

use katana_cad::plot::{PaperSize, PlotColourMode};
use katana_cad::plotting::page_setup::PageSetup;
use katana_cad::plotting::sheet_set::{Sheet, SheetSet, Viewport, ViewportKind};
use katana_cad::plotting::sheet_verbs::parse_plot_sheets;
use katana_core::{ErrorCode, Result};
use katana_entity::{Color as EntityColor, Entity, Geometry, Model};
use katana_geometry::{Point2, Segment2};
use katana_update::inflate::inflate_raw_to_string;
use katana_window::canvas::{Image, ImageFormat, read_png};
use katana_window::plan_painter::PlanSource;
use katana_window::plotting::plot_output::{
    NoPrinters, PdfPrinter, PlotFormat, PlotProgress, PlotReport, PlotRequest, page_setup_for,
    planned_files, plot_format_extension, plot_format_from, plot_format_name, plot_request_for,
    plot_request_for_words, plot_sheets, plot_sheets_from, plots_file_per_sheet, print_sheets,
    print_sheets_on, validate_plot_request,
};
use katana_window::sheet_painter::{SheetPaintCache, SheetSource};

use crate::support::{bx, double_eq};
use crate::window_support::Scratch;

/// Three A3 sheets, ONE, TWO and THREE, of set C, each a plan of a red line.
struct Fixture {
    model: Model,
    set: SheetSet,
    cache: SheetPaintCache,
}

impl Fixture {
    fn new() -> Self {
        let mut model = Model::new();
        model
            .entities
            .add(Entity {
                geometry: Geometry::Line(Segment2::new(
                    Point2::new(-50.0, 0.0),
                    Point2::new(50.0, 20.0),
                )),
                color: Some(EntityColor {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                }),
                ..Entity::default()
            })
            .expect("the line is added");
        let mut set = SheetSet::default();
        set.defaults.set_number = String::from("C");
        for (i, name) in ["ONE", "TWO", "THREE"].iter().enumerate() {
            set.sheets.push(Sheet {
                id: format!("s{}", i + 1),
                name: String::from(*name),
                viewports: vec![Viewport {
                    id: format!("vp{}", i + 1),
                    kind: ViewportKind::Plan,
                    rect: bx(30.0, 40.0, 400.0, 280.0),
                    scale: 500.0,
                    ..Viewport::default()
                }],
                ..Sheet::default()
            });
        }
        Self {
            model,
            set,
            cache: SheetPaintCache::new(),
        }
    }

    fn plot(
        &mut self,
        request: &PlotRequest,
        progress: Option<PlotProgress<'_>>,
    ) -> Result<PlotReport> {
        let source = source_of(&self.model);
        plot_sheets_from(&self.set, request, &source, &mut self.cache, progress)
    }
}

fn source_of(model: &Model) -> SheetSource<'_> {
    SheetSource {
        plan: PlanSource {
            model: Some(model),
            ..PlanSource::default()
        },
        ..SheetSource::default()
    }
}

/// A path as the request holds it.
fn text_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A request for `format` to `destination`, its PDFs written readable.
fn request(format: PlotFormat, destination: &str, sheets: &str) -> PlotRequest {
    PlotRequest {
        format,
        destination: String::from(destination),
        sheets: String::from(sheets),
        readable_pdf: true,
        ..PlotRequest::default()
    }
}

fn contents(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

/// How often `needle` is in `haystack`, and where.
fn places(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if haystack.len() < needle.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at)
        .collect()
}

/// The /Page objects of a PDF, not counting the /Pages tree.
fn pdf_pages(pdf: &[u8]) -> usize {
    places(pdf, b"/Type /Page")
        .into_iter()
        .filter(|at| pdf.get(at + 11) != Some(&b's'))
        .count()
}

/// The files of a folder, by name.
fn files_in(folder: &Path) -> Vec<String> {
    entries_in(folder, true)
}

/// The folders in a folder, by name.
fn folders_in(folder: &Path) -> Vec<String> {
    entries_in(folder, false)
}

fn entries_in(folder: &Path, files: bool) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .filter_map(std::result::Result::ok)
                .filter(|entry| entry.path().is_file() == files)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// QDir::toNativeSeparators.
fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        String::from(path)
    }
}

/// The dots a metre a PNG says it prints at: its pHYs chunk's.
fn png_dots_per_metre(png: &[u8]) -> Option<u32> {
    let at = *places(png, b"pHYs").first()?;
    let x = png.get(at + 4..at + 8)?;
    Some(u32::from_be_bytes([x[0], x[1], x[2], x[3]]))
}

/// A baseline TIFF as tiff_writer writes it, read back: its tags' values (a rational as numerator
/// and denominator) and its pixels, the strips inflated and joined.
struct Tiff {
    tags: BTreeMap<u16, Vec<u32>>,
    software: String,
    pixels: Vec<u8>,
}

impl Tiff {
    fn tag(&self, id: u16) -> u32 {
        self.tags[&id][0]
    }
}

fn read_tiff(file: &[u8]) -> Option<Tiff> {
    if file.len() < 8 || !file.starts_with(b"II*\0") {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([file[at], file[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]);
    let mut tiff = Tiff {
        tags: BTreeMap::new(),
        software: String::new(),
        pixels: Vec::new(),
    };
    let directory = u32_at(4) as usize;
    let entries = u16_at(directory);
    for i in 0..usize::from(entries) {
        let entry = directory + 2 + 12 * i;
        let id = u16_at(entry);
        let kind = u16_at(entry + 2);
        let count = u32_at(entry + 4) as usize;
        let size = match kind {
            3 => 2,
            4 => 4,
            5 => 8,
            _ => 1,
        };
        let at = if size * count <= 4 {
            entry + 8
        } else {
            u32_at(entry + 8) as usize
        };
        let mut values = Vec::new();
        for n in 0..count {
            match kind {
                3 => values.push(u32::from(u16_at(at + 2 * n))),
                4 => values.push(u32_at(at + 4 * n)),
                5 => {
                    values.push(u32_at(at + 8 * n));
                    values.push(u32_at(at + 8 * n + 4));
                }
                _ => {}
            }
        }
        if kind == 2 {
            tiff.software = String::from_utf8_lossy(&file[at..at + count - 1]).into_owned();
        }
        tiff.tags.insert(id, values);
    }
    let width = tiff.tag(256);
    let height = tiff.tag(257);
    let row_bytes = width * tiff.tag(277);
    let rows_per_strip = tiff.tag(278);
    let offsets = tiff.tags[&273].clone();
    let counts = tiff.tags[&279].clone();
    for (strip, (&offset, &count)) in offsets.iter().zip(&counts).enumerate() {
        let rows = rows_per_strip.min(height - strip as u32 * rows_per_strip);
        let expected = rows * row_bytes;
        let packed = &file[offset as usize..(offset + count) as usize];
        // A zlib stream: two bytes of header, the deflate data, and four of its Adler-32.
        if packed.len() < 6 {
            return None;
        }
        let inflated =
            inflate_raw_to_string(&packed[2..packed.len() - 4], u64::from(expected)).ok()?;
        if inflated.len() != expected as usize {
            return None;
        }
        tiff.pixels.extend_from_slice(&inflated);
    }
    Some(tiff)
}

/// The pixels of `image` as a TIFF holds them: rows of RGB, or of grey.
fn tiff_pixels_of(image: &Image) -> Vec<u8> {
    match image.format() {
        ImageFormat::Grey8 => image.bytes().to_vec(),
        ImageFormat::Rgba8Premultiplied => image
            .to_straight_rgba8()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect(),
    }
}

mod plot_output {
    use super::*;

    #[test]
    fn formats_are_named_read_and_given_their_extension() {
        for format in [
            PlotFormat::Pdf,
            PlotFormat::PdfPerSheet,
            PlotFormat::Png,
            PlotFormat::Tiff,
        ] {
            assert_eq!(plot_format_from(plot_format_name(format)), Some(format));
        }
        assert_eq!(plot_format_from("TIF"), Some(PlotFormat::Tiff));
        assert_eq!(
            plot_format_from("pdf-per-sheet"),
            Some(PlotFormat::PdfPerSheet)
        );
        assert!(plot_format_from("jpeg").is_none());
        assert_eq!(plot_format_extension(PlotFormat::Tiff), ".tif");
        assert_eq!(plot_format_extension(PlotFormat::PdfPerSheet), ".pdf");
        assert!(!plots_file_per_sheet(PlotFormat::Pdf));
        assert!(plots_file_per_sheet(PlotFormat::Png));
    }

    #[test]
    fn a_request_comes_from_the_page_setup_and_goes_back_to_it() {
        let setup = PageSetup {
            colour_mode: PlotColourMode::Monochrome,
            line_weight_scale: 0.7,
            dpi: 150.0,
            file_name_pattern: String::from("{number} {name}"),
            file_per_sheet: true,
        };
        let asked = plot_request_for(&setup, "C:/out", "2-3");
        assert_eq!(asked.format, PlotFormat::PdfPerSheet);
        assert_eq!(asked.colour_mode, PlotColourMode::Monochrome);
        assert_eq!(asked.line_weight_scale, 0.7);
        assert_eq!(asked.dpi, 150.0);
        assert_eq!(asked.file_name_pattern, "{number} {name}");
        assert_eq!(asked.sheets, "2-3");
        assert_eq!(asked.destination, "C:/out");
        // And back: the same setup; one PDF turns the file per sheet off; a raster leaves it as it
        // was.
        assert_eq!(page_setup_for(&asked, &PageSetup::default()), setup);
        let single = PlotRequest {
            format: PlotFormat::Pdf,
            ..asked.clone()
        };
        assert!(!page_setup_for(&single, &setup).file_per_sheet);
        let raster = PlotRequest {
            format: PlotFormat::Png,
            ..asked
        };
        assert!(page_setup_for(&raster, &setup).file_per_sheet);
        assert!(!page_setup_for(&raster, &PageSetup::default()).file_per_sheet);
    }

    // The PLOTSHEETS verb's words make the request the Plot dialog would make: the page setup's,
    // each word given in its place, one PDF unless a format is named.
    #[test]
    fn the_plot_sheets_verbs_words_make_a_request_over_the_page_setup() {
        let mut set = SheetSet::default();
        for id in ["s1", "s2", "s3"] {
            set.sheets.push(Sheet {
                id: String::from(id),
                ..Sheet::default()
            });
        }
        let setup = PageSetup {
            colour_mode: PlotColourMode::Greyscale,
            dpi: 200.0,
            line_weight_scale: 0.8,
            file_per_sheet: true,
            file_name_pattern: String::from("{n} {name}"),
        };
        let words_of = |words: &[&str]| -> Vec<String> {
            words.iter().map(|word| String::from(*word)).collect()
        };

        let bare = parse_plot_sheets(&set, &words_of(&["out.pdf"])).expect("the words read");
        let kept = plot_request_for_words(&setup, &bare);
        assert_eq!(
            kept.format,
            PlotFormat::Pdf,
            "one PDF unless a format is named"
        );
        assert_eq!(kept.colour_mode, PlotColourMode::Greyscale);
        assert!(double_eq(kept.dpi, 200.0));
        assert!(double_eq(kept.line_weight_scale, 0.8));
        assert_eq!(kept.file_name_pattern, "{n} {name}");
        assert_eq!(kept.destination, "out.pdf");
        assert!(kept.sheets.is_empty());

        let words = parse_plot_sheets(
            &set,
            &words_of(&[
                "format=tiff",
                "folder=plots",
                "style=mono",
                "dpi=90",
                "lineweight=1.5",
                "pattern={id}",
                "sheets=3,1",
            ]),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let given = plot_request_for_words(&setup, &words);
        assert_eq!(given.format, PlotFormat::Tiff);
        assert_eq!(given.destination, "plots");
        assert_eq!(given.colour_mode, PlotColourMode::Monochrome);
        assert!(double_eq(given.dpi, 90.0));
        assert!(double_eq(given.line_weight_scale, 1.5));
        assert_eq!(given.file_name_pattern, "{id}");
        assert_eq!(given.sheets, "3,1");
        assert!(validate_plot_request(&set, &given).is_ok());
        let files = planned_files(&set, &given).expect("the files are planned");
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("s3.tif"), "{}", files[0]);
    }

    #[test]
    fn one_pdf_holds_the_chosen_sheets_in_the_order_chosen() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_one_pdf");
        // Made when missing.
        let path = text_of(&scratch.path("nested").join("folder").join("set.pdf"));
        let report = fixture
            .plot(&request(PlotFormat::Pdf, &path, "3,1"), None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(report.files, vec![path.clone()]);
        assert_eq!(report.sheets, vec![2, 0]);
        assert_eq!(report.sheets_asked, 2);
        assert!(!report.cancelled);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        let pdf = contents(&path);
        assert!(pdf.starts_with(b"%PDF"));
        assert_eq!(pdf_pages(&pdf), 2);
        assert_eq!(
            report.summary(&request(PlotFormat::Pdf, &path, "")),
            format!("Plotted 2 sheets to {}.", native(&path))
        );

        // Everything: three pages.
        assert!(
            fixture
                .plot(&request(PlotFormat::Pdf, &path, ""), None)
                .is_ok()
        );
        assert_eq!(pdf_pages(&contents(&path)), 3);
        // No temporary file is left beside it.
        assert_eq!(
            files_in(&scratch.path("nested").join("folder")),
            vec![String::from("set.pdf")]
        );
    }

    #[test]
    fn a_pdf_a_sheet_is_named_by_the_pattern_in_the_folder() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_pdf_a_sheet");
        let folder = text_of(&scratch.path("sheets"));
        let asked = request(PlotFormat::PdfPerSheet, &folder, "");
        let planned = planned_files(&fixture.set, &asked)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let report = fixture
            .plot(&asked, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(report.files, planned);
        // The default pattern: the set number, the sheet number to two places, the sheet's name.
        assert_eq!(
            files_in(&scratch.path("sheets")),
            ["C01 ONE.pdf", "C02 TWO.pdf", "C03 THREE.pdf"]
        );
        for file in &report.files {
            assert_eq!(pdf_pages(&contents(file)), 1, "{file}");
        }
        assert_eq!(
            report.summary(&asked),
            format!("Plotted 3 sheets to 3 PDF files in {}.", native(&folder))
        );

        // Another pattern, some of the sheets.
        let mut named = request(
            PlotFormat::PdfPerSheet,
            &text_of(&scratch.path("named")),
            "s2,3",
        );
        named.file_name_pattern = String::from("{id} of {N} - {name}");
        assert!(fixture.plot(&named, None).is_ok());
        assert_eq!(
            files_in(&scratch.path("named")),
            ["s2 of 3 - TWO.pdf", "s3 of 3 - THREE.pdf"]
        );
    }

    #[test]
    fn rasters_are_a_file_a_sheet_at_the_chosen_resolution() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_rasters");
        let mut png = request(PlotFormat::Png, &text_of(&scratch.path("png")), "2");
        png.dpi = 50.0;
        let report = fixture
            .plot(&png, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(report.files.len(), 1);
        assert_eq!(
            Path::new(&report.files[0])
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            Some(String::from("C02 TWO.png"))
        );
        let image = read_png(Path::new(&report.files[0])).expect("the PNG reads back");
        // A3 at 50 dpi: 420 and 297 mm are 826.8 and 584.6 pixels.
        assert_eq!((image.width(), image.height()), (827, 585));
        // 50 / 0.0254
        assert_eq!(png_dots_per_metre(&contents(&report.files[0])), Some(1969));
        assert_ne!(image.format(), ImageFormat::Grey8);
        // The red line is in it, red: at 50 dpi a thin line is under a pixel wide and antialiased
        // to a pink, so red is what stands out.
        let mut red = 0;
        for y in 0..image.height() {
            for x in 0..image.width() {
                let pixel = image.pixel(x, y);
                if i32::from(pixel.r) - i32::from(pixel.g) > 60 && pixel.g == pixel.b {
                    red += 1;
                }
            }
        }
        assert!(red > 50, "{red}");

        // The TIFF of the same sheet holds the same pixels, at the same resolution, deflated, and
        // says what wrote it.
        let tiff = PlotRequest {
            format: PlotFormat::Tiff,
            destination: text_of(&scratch.path("tiff")),
            ..png
        };
        let tiff_report = fixture
            .plot(&tiff, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(tiff_report.files.len(), 1);
        assert_eq!(
            Path::new(&tiff_report.files[0])
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            Some(String::from("C02 TWO.tif"))
        );
        let read = read_tiff(&contents(&tiff_report.files[0])).expect("the TIFF reads back");
        assert_eq!(read.tag(256), 827);
        assert_eq!(read.tag(257), 585);
        assert_eq!(read.tags[&258], vec![8, 8, 8]);
        assert_eq!(read.tag(259), 8); // Deflate
        assert_eq!(read.tag(262), 2); // RGB
        assert_eq!(read.tags[&282], vec![50, 1]);
        assert_eq!(read.tag(296), 2); // inches
        assert_eq!(read.software, "Katana");
        assert!(read.pixels == tiff_pixels_of(&image));
    }

    #[test]
    fn a_greyscale_or_monochrome_raster_is_one_grey_channel() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_grey_rasters");
        for (mode, name) in [
            (PlotColourMode::Greyscale, "grey"),
            (PlotColourMode::Monochrome, "mono"),
        ] {
            let folder = text_of(&scratch.path(name));
            let mut png = request(PlotFormat::Png, &folder, "1");
            png.dpi = 50.0;
            png.colour_mode = mode;
            let report = fixture
                .plot(&png, None)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            let image = read_png(Path::new(&report.files[0])).expect("the PNG reads back");
            assert_eq!(image.format(), ImageFormat::Grey8, "{name}");

            let tiff = PlotRequest {
                format: PlotFormat::Tiff,
                ..png
            };
            let tiff_report = fixture
                .plot(&tiff, None)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            let read = read_tiff(&contents(&tiff_report.files[0])).expect("the TIFF reads back");
            assert_eq!(read.tags[&258], vec![8]);
            assert_eq!(read.tag(262), 1); // BlackIsZero
            assert_eq!(read.tag(277), 1);
            assert!(read.pixels == tiff_pixels_of(&image));
        }
    }

    #[test]
    fn the_same_request_writes_the_same_bytes() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_same_bytes");
        for format in [PlotFormat::Png, PlotFormat::Tiff] {
            let mut first = request(format, &text_of(&scratch.path("a")), "1");
            first.dpi = 60.0;
            first.colour_mode = PlotColourMode::Greyscale;
            let second = PlotRequest {
                destination: text_of(&scratch.path("b")),
                ..first.clone()
            };
            let a = fixture.plot(&first, None).expect("the first is plotted");
            let b = fixture.plot(&second, None).expect("the second is plotted");
            assert!(
                contents(&a.files[0]) == contents(&b.files[0]),
                "{}",
                plot_format_name(format)
            );
        }
    }

    #[test]
    fn progress_is_told_of_every_sheet_and_the_end() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_progress");
        let mut calls: Vec<(usize, usize, String)> = Vec::new();
        let mut told = |done: usize, total: usize, name: &str| {
            calls.push((done, total, String::from(name)));
            true
        };
        let report = fixture.plot(
            &request(
                PlotFormat::Pdf,
                &text_of(&scratch.path("set.pdf")),
                "2-3",
            ),
            Some(&mut told),
        );
        assert!(report.is_ok(), "{:?}", report.err().map(|e| e.describe()));
        assert_eq!(
            calls,
            vec![
                (0, 2, String::from("TWO")),
                (1, 2, String::from("THREE")),
                (2, 2, String::new()),
            ]
        );
    }

    #[test]
    fn the_source_is_asked_for_again_before_each_sheet() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_source_asked");
        let mut asked = 0;
        let model = &fixture.model;
        let report = plot_sheets(
            &fixture.set,
            &request(PlotFormat::Pdf, &text_of(&scratch.path("set.pdf")), ""),
            &mut || {
                asked += 1;
                source_of(model)
            },
            &mut fixture.cache,
            None,
        );
        assert!(report.is_ok(), "{:?}", report.err().map(|e| e.describe()));
        assert_eq!(asked, 3);
    }

    #[test]
    fn a_cancelled_pdf_is_not_written_and_leaves_an_earlier_one_as_it_was() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_cancelled_pdf");
        let path = text_of(&scratch.file("set.pdf", "an earlier plot"));
        let asked = request(PlotFormat::Pdf, &path, "");
        // Stop before the second sheet.
        let mut stop = |done: usize, _: usize, _: &str| done < 1;
        let report = fixture
            .plot(&asked, Some(&mut stop))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(report.cancelled);
        assert_eq!(report.sheets, vec![0]);
        assert!(report.files.is_empty());
        assert_eq!(contents(&path), b"an earlier plot");
        assert_eq!(files_in(&scratch.root), vec![String::from("set.pdf")]);
        assert_eq!(report.summary(&asked), "Cancelled after 1 of 3 sheets.");
    }

    #[test]
    fn a_cancelled_file_per_sheet_plot_keeps_the_files_it_finished() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_cancelled_per_sheet");
        for format in [PlotFormat::PdfPerSheet, PlotFormat::Png] {
            let folder = text_of(&scratch.path(plot_format_name(format)));
            let mut asked = request(format, &folder, "");
            asked.dpi = 50.0;
            let mut stop = |done: usize, _: usize, _: &str| done < 2;
            let report = fixture
                .plot(&asked, Some(&mut stop))
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            assert!(report.cancelled);
            assert_eq!(report.sheets, vec![0, 1]);
            assert_eq!(report.files.len(), 2);
            let extension = plot_format_extension(format);
            assert_eq!(
                files_in(Path::new(&folder)),
                [
                    format!("C01 ONE{extension}"),
                    format!("C02 TWO{extension}")
                ]
            );
            assert_eq!(
                report.summary(&asked),
                format!(
                    "Cancelled after 2 of 3 sheets; 2 files kept in {}.",
                    native(&folder)
                )
            );
        }
    }

    #[test]
    fn a_file_that_cannot_be_written_stops_the_plot_and_the_error_names_the_files_kept() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_cannot_be_written");
        for format in [PlotFormat::PdfPerSheet, PlotFormat::Png] {
            let folder = scratch.path(plot_format_name(format));
            let extension = plot_format_extension(format);
            // A folder where the third sheet's file goes: no file can take its name.
            std::fs::create_dir_all(folder.join(format!("C03 THREE{extension}")))
                .expect("the folder in the way is made");
            let mut asked = request(format, &text_of(&folder), "");
            asked.dpi = 50.0;
            let error = fixture
                .plot(&asked, None)
                .expect_err("the third file cannot be written");
            assert_eq!(error.code, ErrorCode::FileExportFailure);
            let message = error.message;
            assert!(
                message.contains("; 2 files written before it were kept: "),
                "{message}"
            );
            assert!(
                message.contains(&format!("C01 ONE{extension},")),
                "{message}"
            );
            assert!(
                message.contains(&format!("C02 TWO{extension}")),
                "{message}"
            );
            assert!(folder.join(format!("C02 TWO{extension}")).is_file());
        }
    }

    #[test]
    fn a_request_is_checked_before_anything_is_written() {
        let mut fixture = Fixture::new();
        let scratch = Scratch::new("plot_output_checked_first");
        let refused = |fixture: &mut Fixture, asked: &PlotRequest| {
            let report = fixture.plot(asked, None);
            assert!(report.is_err());
            assert!(files_in(&scratch.root).is_empty());
            assert!(folders_in(&scratch.root).is_empty());
            report.err().map(|error| error.message).unwrap_or_default()
        };
        let in_scratch = |name: &str| text_of(&scratch.path(name));
        assert_eq!(
            refused(
                &mut fixture,
                &request(PlotFormat::Pdf, &in_scratch("set.pdf"), "4")
            ),
            "there is no sheet 4: the set has 3 sheets"
        );
        assert_eq!(
            refused(&mut fixture, &request(PlotFormat::Pdf, "  ", "")),
            "choose the PDF file the plot goes in"
        );
        assert_eq!(
            refused(&mut fixture, &request(PlotFormat::Png, "", "")),
            "choose the folder the plot's files go in"
        );
        let mut coarse = request(PlotFormat::Png, &in_scratch("out"), "");
        coarse.dpi = 20.0;
        assert_eq!(
            refused(&mut fixture, &coarse),
            "the resolution must be 50 to 1200 dpi"
        );
        let mut thin = request(PlotFormat::Pdf, &in_scratch("set.pdf"), "");
        thin.line_weight_scale = 0.0;
        assert_eq!(
            refused(&mut fixture, &thin),
            "the line weight scale must be 0.1 to 5"
        );
        let mut bad_pattern = request(PlotFormat::PdfPerSheet, &in_scratch("out"), "");
        bad_pattern.file_name_pattern = String::from("{sheet}");
        assert!(refused(&mut fixture, &bad_pattern).contains("no token {sheet}"));
        // One PDF names no file by the pattern, so its pattern is not asked about.
        let mut single = request(PlotFormat::Pdf, &in_scratch("single.pdf"), "");
        single.file_name_pattern = String::from("{sheet}");
        assert!(validate_plot_request(&fixture.set, &single).is_ok());

        // A raster too large to hold is refused with the resolution that fits.
        fixture.set.sheets[1].paper = PaperSize::A0;
        // Sheet 2 alone: the A3 sheets are too large at 1200 dpi as well.
        let mut huge = request(PlotFormat::Tiff, &in_scratch("out"), "2");
        huge.dpi = 1200.0;
        assert_eq!(
            refused(&mut fixture, &huge),
            "sheet 2 (TWO) at 1200 dpi is 2232 megapixels, more than the 200 a raster may be: \
             plot it at 359 dpi or less"
        );
        // 359 dpi is 199.8 megapixels; 360 would be 200.9.
        huge.dpi = 359.0;
        assert!(validate_plot_request(&fixture.set, &huge).is_ok());
        huge.dpi = 360.0;
        assert!(validate_plot_request(&fixture.set, &huge).is_err());
    }

    #[test]
    fn printing_puts_each_sheet_on_a_page_of_the_printer() {
        let mut fixture = Fixture::new();
        fixture.set.sheets[1].paper = PaperSize::A1;
        let scratch = Scratch::new("plot_output_printing");
        // A printer that prints to a PDF: the pages it is given can be counted.
        let path = scratch.path("printed.pdf");
        let printer = PdfPrinter {
            readable: true,
            ..PdfPrinter::for_file(&path)
        };
        let asked = PlotRequest {
            sheets: String::from("1-2"),
            colour_mode: PlotColourMode::Monochrome,
            ..PlotRequest::default()
        };
        let mut told = 0;
        let mut count = |_: usize, _: usize, _: &str| {
            told += 1;
            true
        };
        let model = &fixture.model;
        let report = print_sheets(
            &printer,
            &fixture.set,
            &asked,
            &mut || source_of(model),
            &mut fixture.cache,
            Some(&mut count),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(report.sheets, vec![0, 1]);
        assert!(report.files.is_empty());
        assert_eq!(report.printer, native(&text_of(&path)));
        // A PDF printer takes any paper, so nothing was made smaller.
        assert!(report.shrunk.is_empty());
        assert_eq!(told, 3);
        let pdf = contents(&text_of(&path));
        assert_eq!(pdf_pages(&pdf), 2);
        // The A1 page is 841 x 594 mm: 2384 x 1684 points.
        assert!(!places(&pdf, b"2383.9").is_empty() || !places(&pdf, b"2384").is_empty());
        assert_eq!(
            report.summary(&asked),
            format!("Printed 2 sheets on {}.", native(&text_of(&path)))
        );

        // A printer the system does not have is named, not replaced by another.
        let missing = print_sheets_on(
            &NoPrinters,
            "No Printer By This Name",
            &scratch.path("spool.pdf"),
            true,
            &fixture.set,
            &asked,
            &mut || source_of(model),
            &mut fixture.cache,
            None,
        )
        .expect_err("there is no such printer");
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert_eq!(missing.message, "there is no printer of that name");
    }
}
