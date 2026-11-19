// SURVEY EXPORT (katana_app's survey_export_verb): the drawing's survey points
// written to a delimited point file - what Survey > Export Survey Points
// writes, from a line, so katana_cli, katana_mcp and the window's command line
// can do it. Each case reads the FILE the line wrote, with rows worked out by
// hand, and the cli.* tests run the same lines through katana_cli.

use std::path::PathBuf;

use katana_app::import_records::path_text;
use katana_app::survey_verbs::run_survey_line;
use katana_cad::{CommandInterpreter, Document};
use katana_commands::create_entities;
use katana_core::text::read_reply_record;
use katana_core::{ErrorCode, Result};
use katana_entity::entity::set_heights;
use katana_entity::{Entity, Geometry, PointGeometry, PropertyMap, PropertyValue};
use katana_geometry::{Point2, Polyline2};
use katana_survey::LinearUnit;
use katana_surveyio::{DelimitedImportOptions, parse_delimited_points, parse_layout_template};

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("katana-survey-export-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }

    // `name` here, as a line gives a path, quoted.
    fn quoted(&self, name: &str) -> String {
        format!("\"{}\"", path_text(&self.path.join(name)))
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path.join(name)).expect("the file was written")
    }

    fn exists(&self, name: &str) -> bool {
        self.path.join(name).exists()
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.path)
            .expect("the scratch folder is there")
            .map(|entry| entry.expect("an entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[track_caller]
fn must<T>(result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

#[track_caller]
fn refused(result: Result<String>, code: ErrorCode) -> String {
    match result {
        Ok(reply) => panic!("expected a refusal, got: {reply}"),
        Err(error) => {
            assert_eq!(error.code, code, "{}", error.describe());
            error.describe()
        }
    }
}

// A survey point as an import draws one: a point entity with a point number
// ("point"), a code and a description, and its height in the one height
// property (absent is not zero).
fn survey_point(
    layer: &str,
    id: &str,
    (easting, northing): (f64, f64),
    height: Option<f64>,
    code: &str,
    description: &str,
) -> Entity {
    let mut properties = PropertyMap::new();
    properties.insert(String::from("point"), PropertyValue::Text(String::from(id)));
    properties.insert(String::from("code"), PropertyValue::Text(String::from(code)));
    if !description.is_empty() {
        properties.insert(
            String::from("description"),
            PropertyValue::Text(String::from(description)),
        );
    }
    set_heights(&mut properties, &[height]);
    Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(easting, northing),
        }),
        layer: String::from(layer),
        properties,
        ..Entity::default()
    }
}

// Three survey points on layer survey - one with no height, one whose
// description holds a comma - a fourth on layer control, and on survey a point
// with no point number and a line, neither of which is a survey point.
fn surveyed_drawing() -> Document {
    let mut document = Document::default();
    let mut interpreter = CommandInterpreter::new();
    must(interpreter.run(&mut document, "LAYER NEW survey"));
    must(interpreter.run(&mut document, "LAYER NEW control"));
    let bare_point = Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(334005.0, 6250005.0),
        }),
        layer: String::from("survey"),
        ..Entity::default()
    };
    let line = Entity {
        geometry: Geometry::Polyline(Polyline2 {
            vertices: vec![Point2::new(334000.0, 6250000.0), Point2::new(334010.0, 6250000.0)],
            closed: false,
        }),
        layer: String::from("survey"),
        ..Entity::default()
    };
    must(document.execute(create_entities(vec![
        survey_point("survey", "101", (334000.001, 6250000.002), Some(20.125), "CP", "Control"),
        survey_point("survey", "102", (334010.5, 6250010.25), Some(20.25), "EP", ""),
        survey_point("survey", "103", (334020.75, 6250005.125), None, "EP", "no level, obscured"),
        survey_point("control", "900", (334100.0, 6250100.0), Some(30.0), "BM", "Benchmark"),
        bare_point,
        line,
    ])));
    document
}

// The file's lines, without the line ends (RFC 4180's CRLF, or LF).
fn lines_of(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| String::from(line.trim_end_matches('\r')))
        .collect()
}

fn record(reply: &str, head: &str) -> std::collections::BTreeMap<String, String> {
    let line = reply
        .lines()
        .find(|line| line.starts_with(&format!("{head} ")))
        .unwrap_or_else(|| panic!("no {head} record in:\n{reply}"));
    let read = read_reply_record(line).unwrap_or_else(|| panic!("{line} does not read"));
    read.fields.into_iter().collect()
}

mod survey_export_verb {
    use super::*;

    #[test]
    fn every_survey_point_is_written_in_the_dialogs_default_layout_and_nothing_else_is() {
        let scratch = Scratch::new("default");
        let mut document = surveyed_drawing();
        let reply = must(run_survey_line(
            &mut document,
            &format!("SURVEY EXPORT {}", scratch.quoted("points.csv")),
        ));
        // Worked by hand: point, easting, northing, height, code, description;
        // three places; an absent height an empty field, not 0; a field with
        // a comma quoted (RFC 4180); the bare point and the line not written.
        assert_eq!(
            lines_of(&scratch.read("points.csv")),
            [
                "Point,Easting,Northing,Elevation,Code,Description",
                "101,334000.001,6250000.002,20.125,CP,Control",
                "102,334010.500,6250010.250,20.250,EP,",
                "103,334020.750,6250005.125,,EP,\"no level, obscured\"",
                "900,334100.000,6250100.000,30.000,BM,Benchmark",
            ]
        );
        let exported = record(&reply, "exported");
        assert_eq!(exported["kind"], "survey-points");
        assert_eq!(exported["points"], "4");
        assert_eq!(exported["without_elevation"], "1");
        assert_eq!(exported["not_points"], "0");
        assert_eq!(exported["decimals"], "3");
        assert_eq!(exported["unit"], "metres");
        assert_eq!(
            exported["layout"],
            "P,E,N,Z,C,D;delimiter=comma;header=1;quote=double"
        );
        // No scope words: no scope record.
        assert!(!reply.contains("\nscope "), "{reply}");
        // A copy is written: the drawing is as it was.
        assert_eq!(document.model().entities.ids().len(), 6);
    }

    #[test]
    fn layout_decimals_and_unit_are_the_lines_words() {
        let scratch = Scratch::new("layout");
        let mut document = surveyed_drawing();
        must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY EXPORT {} LAYOUT \"P,N,E,Z;delimiter=tab;header=0;quote=none\" \
                 DECIMALS 2 UNIT feet",
                scratch.quoted("feet.txt")
            ),
        ));
        // The international foot is 0.3048 m exactly: 6250000.002 m is
        // 20505249.3504 ft, 334000.001 m 1095800.5282 ft, 20.125 m 66.0269 ft.
        let lines = lines_of(&scratch.read("feet.txt"));
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert_eq!(lines[0], "101\t20505249.35\t1095800.53\t66.03");
    }

    #[test]
    fn a_scope_writes_the_survey_points_among_what_it_took_and_says_what_it_took() {
        let scratch = Scratch::new("scope");
        let mut document = surveyed_drawing();
        let reply = must(run_survey_line(
            &mut document,
            &format!("SURVEY EXPORT {} LAYERS survey DECIMALS 3", scratch.quoted("survey.csv")),
        ));
        let lines = lines_of(&scratch.read("survey.csv"));
        // The three on survey, not the benchmark on control.
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(lines.iter().all(|line| !line.starts_with("900,")), "{lines:?}");
        let exported = record(&reply, "exported");
        assert_eq!(exported["points"], "3");
        // The bare point and the line the scope took are said, not written.
        assert_eq!(exported["not_points"], "2");
        let scope = record(&reply, "scope");
        assert_eq!(scope["scope"], "layers");
        assert_eq!(scope["matched"], "5");
    }

    #[test]
    fn a_scope_that_takes_nothing_is_reported_and_writes_no_file() {
        let scratch = Scratch::new("nothing");
        let mut document = surveyed_drawing();
        // Nothing is selected, so SELECTION takes nothing.
        let reply = must(run_survey_line(
            &mut document,
            &format!("SURVEY EXPORT {} SELECTION", scratch.quoted("none.csv")),
        ));
        let export = record(&reply, "export");
        assert_eq!(export["ran"], "no");
        assert_eq!(export["reason"], "the scope took nothing");
        assert_eq!(record(&reply, "scope")["matched"], "0");
        assert!(!scratch.exists("none.csv"));
    }

    #[test]
    fn the_file_reads_back_through_the_import_wizards_reader_as_the_same_points() {
        let scratch = Scratch::new("round-trip");
        let mut document = surveyed_drawing();
        let layout = "P,E,N,Z,C,D;delimiter=comma;header=1;quote=double";
        must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY EXPORT {} LAYOUT \"{layout}\" DECIMALS 4",
                scratch.quoted("back.csv")
            ),
        ));
        let read = must(parse_delimited_points(
            scratch.read("back.csv").as_bytes(),
            &must(parse_layout_template(layout)),
            "back.csv",
            &DelimitedImportOptions {
                unit: LinearUnit::Metres,
                encoding: None,
            },
        ));
        let points = &read.project.points;
        assert_eq!(points.len(), 4);
        assert_eq!(points[0].id, "101");
        assert!((points[0].easting - 334000.001).abs() < 1e-9);
        assert!((points[0].northing - 6250000.002).abs() < 1e-9);
        assert_eq!(points[0].elevation, Some(20.125));
        assert_eq!(points[2].elevation, None);
        assert_eq!(points[2].description, "no level, obscured");
    }

    #[test]
    fn a_drawing_with_no_survey_points_is_refused_and_no_file_is_written() {
        let scratch = Scratch::new("empty");
        let mut document = Document::default();
        let mut interpreter = CommandInterpreter::new();
        must(interpreter.run(&mut document, "POINT 1,2"));
        let said = refused(
            run_survey_line(
                &mut document,
                &format!("SURVEY EXPORT {}", scratch.quoted("none.csv")),
            ),
            ErrorCode::InvalidState,
        );
        assert!(said.contains("no survey points"), "{said}");
        assert!(scratch.names().is_empty(), "{:?}", scratch.names());
    }

    #[test]
    fn words_the_grammar_does_not_take_are_refused_by_name() {
        let scratch = Scratch::new("refusals");
        let mut document = surveyed_drawing();
        let file = scratch.quoted("x.csv");
        for (line, says) in [
            (String::from("SURVEY EXPORT"), "needs the file"),
            (format!("SURVEY EXPORT {file} DECIMALS 13"), "DECIMALS"),
            (format!("SURVEY EXPORT {file} DECIMALS -1"), "DECIMALS"),
            (format!("SURVEY EXPORT {file} DECIMALS"), "needs a value"),
            (format!("SURVEY EXPORT {file} UNIT parsecs"), "UNIT"),
            (format!("SURVEY EXPORT {file} UNIT feet UNIT metres"), "given twice"),
            (format!("SURVEY EXPORT {file} COLUMNS PNEZD"), "not an option"),
            (format!("SURVEY EXPORT {file} LAYOUT \"P,Q;delimiter=comma;header=1\""), "column letter"),
            (format!("SURVEY EXPORT {file} LAYOUT \"P,N,E\""), "delimiter"),
        ] {
            let said = refused(run_survey_line(&mut document, &line), ErrorCode::InvalidArgument);
            assert!(said.contains(says), "{line}: {said}");
        }
        // VIEW is the window's plan view; headless it is refused, naming AREA.
        let said = refused(
            run_survey_line(&mut document, &format!("SURVEY EXPORT {file} VIEW")),
            ErrorCode::InvalidState,
        );
        assert!(said.contains("AREA"), "{said}");
        assert!(scratch.names().is_empty(), "{:?}", scratch.names());
    }

    #[test]
    fn a_file_that_cannot_be_written_is_refused_and_leaves_nothing_beside_it() {
        let scratch = Scratch::new("unwritable");
        let mut document = surveyed_drawing();
        let said = refused(
            run_survey_line(
                &mut document,
                &format!("SURVEY EXPORT {}", scratch.quoted("no such folder/points.csv")),
            ),
            ErrorCode::FileExportFailure,
        );
        assert!(said.contains("could not be written"), "{said}");
        assert!(scratch.names().is_empty(), "{:?}", scratch.names());
    }
}

// SURVEY READ and SURVEY IMPORT of a delimited point file through LAYOUT and
// UNIT: what the import wizard's Layout page and Import do, from a line. The
// fixture's facts, from its own text: twelve points, point 1 at N 6250000.000
// E 330000.000 Z 25.125 coded CP, point 4 with no level, point 9's description
// "Hydrant, fire" quoted for its comma.
mod survey_point_list {
    use super::*;
    use katana_cad::SurveyImportOptions;
    use katana_cad::survey_points::drawing_survey_points;

    const PNEZCD: &str = "P,N,E,Z,C,D;delimiter=comma;header=1;quote=double";

    fn fixture() -> String {
        format!(
            "\"{}/../../tests/surveyio/data/survey_points_pnezd.csv\"",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    fn points_of(document: &Document) -> Vec<katana_cad::DrawingSurveyPoint> {
        drawing_survey_points(document, &SurveyImportOptions::default())
    }

    #[test]
    fn a_point_file_is_imported_through_its_layout_as_one_undo_step() {
        let mut document = Document::default();
        let reply = must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT {} LAYOUT \"{PNEZCD}\" UNIT metres CODES off LINEWORK off",
                fixture()
            ),
        ));
        let points = points_of(&document);
        assert_eq!(points.len(), 12);
        let first = points.iter().find(|point| point.id == "1").expect("point 1");
        assert!((first.easting - 330000.0).abs() < 1e-9, "{}", first.easting);
        assert!((first.northing - 6250000.0).abs() < 1e-9, "{}", first.northing);
        assert_eq!(first.elevation, Some(25.125));
        assert_eq!(first.code, "CP");
        let fourth = points.iter().find(|point| point.id == "4").expect("point 4");
        assert_eq!(fourth.elevation, None);
        let ninth = points.iter().find(|point| point.id == "9").expect("point 9");
        assert_eq!(ninth.description, "Hydrant, fire");

        assert_eq!(record(&reply, "survey")["format"], "delimited-points");
        assert_eq!(record(&reply, "content")["points"], "12");
        assert_eq!(record(&reply, "layout")["unit"], "metres");
        let imported = record(&reply, "imported");
        assert_eq!(imported["entities"], "12");
        assert_eq!(imported["without_elevation"], "1");
        assert_eq!(imported["layer"], SurveyImportOptions::default().layer);
        assert_eq!(record(&reply, "coded")["reason"], "off");

        // One undo takes the whole import back.
        must(document.undo());
        assert!(points_of(&document).is_empty());
    }

    #[test]
    fn the_unit_said_scales_the_numbers_into_metres() {
        let mut document = Document::default();
        must(run_survey_line(
            &mut document,
            &format!("SURVEY IMPORT {} LAYOUT \"{PNEZCD}\" UNIT feet CODES off", fixture()),
        ));
        // The international foot is 0.3048 m exactly.
        let first = points_of(&document)
            .into_iter()
            .find(|point| point.id == "1")
            .expect("point 1");
        assert!((first.easting - 100584.0).abs() < 1e-6, "{}", first.easting);
        assert!((first.northing - 1905000.0).abs() < 1e-6, "{}", first.northing);
        assert!((first.elevation.expect("a height") - 7.6581).abs() < 1e-9);
    }

    #[test]
    fn read_with_a_layout_says_what_the_file_holds_and_changes_nothing() {
        let mut document = Document::default();
        let reply = must(run_survey_line(
            &mut document,
            &format!("SURVEY READ {} LAYOUT \"{PNEZCD}\" UNIT metres", fixture()),
        ));
        assert_eq!(record(&reply, "content")["points"], "12");
        assert!(!reply.contains("\nimported "), "{reply}");
        assert!(document.model().entities.ids().is_empty());
    }

    #[test]
    fn what_the_drawing_writes_the_same_layout_reads_back_as_the_same_points() {
        let scratch = Scratch::new("point-list-round-trip");
        let mut source = surveyed_drawing();
        must(run_survey_line(
            &mut source,
            &format!("SURVEY EXPORT {} DECIMALS 4", scratch.quoted("all.csv")),
        ));
        let mut copy = Document::default();
        must(run_survey_line(
            &mut copy,
            &format!(
                "SURVEY IMPORT {} LAYOUT \"P,E,N,Z,C,D;delimiter=comma;header=1\" UNIT metres \
                 CODES off LINEWORK off",
                scratch.quoted("all.csv")
            ),
        ));
        let (before, after) = (points_of(&source), points_of(&copy));
        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(&after) {
            assert_eq!(a.id, b.id);
            assert!((a.easting - b.easting).abs() < 1e-9);
            assert!((a.northing - b.northing).abs() < 1e-9);
            assert_eq!(a.elevation, b.elevation);
            assert_eq!(a.code, b.code);
            assert_eq!(a.description, b.description);
        }
    }

    #[test]
    fn a_point_file_line_that_cannot_be_honoured_is_refused_with_the_drawing_unchanged() {
        let mut document = Document::default();
        let file = fixture();
        for (line, says) in [
            (format!("SURVEY IMPORT {file} LAYOUT \"{PNEZCD}\""), "UNIT"),
            (format!("SURVEY IMPORT {file} UNIT metres"), "LAYOUT"),
            (
                format!("SURVEY IMPORT {file} LAYOUT \"{PNEZCD}\" UNIT parsecs"),
                "UNIT is metres",
            ),
            (
                format!("SURVEY IMPORT {file} LAYOUT \"{PNEZCD}\" UNIT metres SET method=radiation"),
                "no observations",
            ),
            (
                format!("SURVEY IMPORT {file} FORMAT sokkia-sdr LAYOUT \"{PNEZCD}\" UNIT metres"),
                "delimited",
            ),
            (format!("SURVEY IMPORT {file} LAYOUT \"P,N\" UNIT metres"), "delimiter"),
        ] {
            let said = refused(run_survey_line(&mut document, &line), ErrorCode::InvalidArgument);
            assert!(said.contains(says), "{line}: {said}");
        }
        assert!(document.model().entities.ids().is_empty());

        // Imported once, the same ids again are refused (the wizard's default
        // policy), and nothing is added.
        must(run_survey_line(
            &mut document,
            &format!("SURVEY IMPORT {file} LAYOUT \"{PNEZCD}\" UNIT metres CODES off LINEWORK off"),
        ));
        let count = document.model().entities.ids().len();
        assert!(
            run_survey_line(
                &mut document,
                &format!("SURVEY IMPORT {file} LAYOUT \"{PNEZCD}\" UNIT metres CODES off"),
            )
            .is_err()
        );
        assert_eq!(document.model().entities.ids().len(), count);
    }
}
