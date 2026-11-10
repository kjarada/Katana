// SURVEY IMPORT's reduction settings (katana_app's survey_verbs): SETTINGS, a
// file of the settings' text form, and SET, lines of it on the line - what
// they start from, what each changes, what the reply says of them and every
// refusal, with the drawing unchanged by one - and what the reply says of the
// control it held and of the adjustment. The cli.* tests run the same lines
// through katana_cli; these read the document the line leaves behind. Then,
// as SurveyVerbs, what the import does with the survey codes: CODES and
// LINEWORK, the drawing's own switches, and the two records of the reply.

use std::fmt;
use std::path::PathBuf;

use katana_app::import_records::path_text;
use katana_app::survey_verbs::{reduction_records, run_survey_line};
use katana_cad::survey_job::reduce_for_drawing;
use katana_cad::survey_points::drawing_survey_points;
use katana_cad::{
    CommandInterpreter, CustomisationOrigin, Document, DrawingSurveyPoint, SurveyImportOptions,
};
use katana_core::text::{parse_finite_double, parse_integer, read_reply_record, split_lines};
use katana_core::{ErrorCode, Result, Status};
use katana_entity::customisation::customisation_from_json;
use katana_entity::entity::heights_of;
use katana_entity::{
    Color, CustomisationAutomation, Entity, Geometry, Layer, PropertyValue, Style,
};
use katana_geometry::Point2;
use katana_survey::reduction_settings::serialise_reduction_settings;
use katana_survey::report_summary::rejected_observations;
use katana_survey::{
    AdjustmentMethod, AdjustmentReport, AtmosphericCorrection, ControlComponent, ControlConstraint,
    ControlOrigin, ControlPoint, ControlSelection, FaceHandling, NetworkDimension,
    ReductionContext, ReductionReport, ReductionSettings, ReportGlobalTest, ReportObservation,
};
use katana_surveyio::{ReadOptions, format_registry, read_survey};

// tests/surveyio/data, where the C++'s KATANA_SURVEYIO_DATA points.
fn fixture(relative: &str) -> String {
    format!(
        "{}/../../../tests/surveyio/data/{relative}",
        env!("CARGO_MANIFEST_DIR")
    )
}

// traverse.sdr less its two 08 coordinate records: nothing in it has a
// position until something holds a point.
fn bare_traverse() -> String {
    fixture("sdr/traverse_without_coordinates.sdr")
}

// A directory of the test's own, removed afterwards: the C++'s name, and the
// process id, since libtest runs the cases on threads of one process and
// another run of the binary may be going.
struct ScratchDirectory {
    path: PathBuf,
}

impl ScratchDirectory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("katana-survey-verbs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }

    // Writes `bytes` to `name` here and returns its path as a line quotes it.
    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let place = self.path.join(name);
        std::fs::write(&place, bytes).expect("a scratch file is written");
        format!("\"{}\"", path_text(&place))
    }

    // `name` here, as a line gives a path, unquoted.
    fn path_of(&self, name: &str) -> String {
        path_text(&self.path.join(name))
    }
}

impl Drop for ScratchDirectory {
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
fn must_be_ok(status: Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}

#[track_caller]
fn near(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: {actual} is not within {tolerance} of {expected}"
    );
}

// A survey point on the drawing, put there as katana_cli would: FORWARD
// from a coordinate one metre south of it, due north - level, or with no
// height ("-") when the start has none.
#[track_caller]
fn place_point(document: &mut Document, name: &str, south_of: &str, height_difference: &str) {
    let mut interpreter = CommandInterpreter::new();
    must(interpreter.run(
        document,
        &format!("FORWARD {south_of} 0 1 {height_difference} {name}"),
    ));
}

fn survey_points(document: &Document) -> Vec<DrawingSurveyPoint> {
    drawing_survey_points(document, &SurveyImportOptions::default())
}

fn point_named<'a>(points: &'a [DrawingSurveyPoint], id: &str) -> Option<&'a DrawingSurveyPoint> {
    points.iter().find(|point| point.id == id)
}

fn held(
    id: &str,
    origin: ControlOrigin,
    horizontal: ControlComponent,
    vertical: ControlComponent,
) -> ControlSelection {
    ControlSelection {
        point: ControlPoint {
            point_id: String::from(id),
            northing: horizontal,
            easting: horizontal,
            elevation: vertical,
        },
        origin,
    }
}

const FIXED: ControlComponent = ControlComponent {
    constraint: ControlConstraint::Fixed,
    sigma: 0.0,
};
const FREE: ControlComponent = ControlComponent {
    constraint: ControlConstraint::Free,
    sigma: 0.0,
};

// The lines of `text`, split as core splits them; every cut is at an ASCII
// line end, so each piece is UTF-8 still.
fn lines_of(text: &str) -> Vec<&str> {
    split_lines(text.as_bytes())
        .into_iter()
        .map(|line| std::str::from_utf8(line).unwrap_or_default())
        .collect()
}

mod survey_import_settings {
    use super::*;

    // The owner's case, on a file of our own: a traverse that gives no
    // coordinates, its first station held where the drawing has it. Worked by
    // hand from CP1 (500000 E, 5000000 N, 100 Z) with the textbook forms -
    // H = S sin z, V = S cos z, less (1 - k/2) V H / R from H and plus
    // (1 - k) H^2 / 2R on V, k = 0.13 and R = 6 371 000 m, the reduction's own
    // constants - and Z = Z0 + instrument height + V - target height:
    //   CP2, the backsight, due north (the 07's azimuth 0 less the reading 0 on
    //   it): four pointings of 100.006 m at zenith 89.36977, H 99.9999502,
    //   V 1.1000027: N 5000099.9999340, Z 100 + 1.5 + 1.1006855 - 1.6 =
    //   101.0006855.
    //   T1 due east (circle 90): 150.0015 m, the mean of four, at zenith 87,
    //   H 149.7959282, V 7.8504756: E 500149.7957556, Z 100 + 1.5 + 7.8520040
    //   - 1.65 = 107.7020040.
    //   T2 from T1, oriented on CP1 at 270, circle 90, so due north: 80.001 m at
    //   zenith 91, H 79.9888155, V -1.3962069: N 5000079.9888319, Z 107.7020040
    //   + 1.55 - 1.3957731 - 1.7 = 106.1562309.
    // The working and the reduction evaluate the same closed forms in doubles,
    // differing only in the order of the means - well under a nanometre - so a
    // micrometre holds the result apart from every correction in it (the
    // smallest, CP2's curvature and refraction, is 16 um).
    #[test]
    fn a_file_with_no_coordinates_is_reduced_from_a_point_of_the_drawing_held_by_set() {
        let mut document = Document::default();
        place_point(&mut document, "CP1", "500000,4999999,100", "0");
        let reply = must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{}\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0",
                bare_traverse()
            ),
        ));
        assert!(
            reply.contains(
                "\nsettings file= set=1 differ=1\n\
                 setting key=control value=CP1;drawing;fixed;0;fixed;0;fixed;0\n\
                 imported job=job-2 entities=3 layer=survey/points reduction_warnings=3"
            ),
            "{reply}"
        );

        assert_eq!(document.survey_jobs().len(), 1);
        let job = &document.survey_jobs()[0];
        let expected = ReductionSettings {
            control: vec![held("CP1", ControlOrigin::Drawing, FIXED, FIXED)],
            ..ReductionSettings::default()
        };
        assert!(
            job.settings == expected,
            "{}",
            serialise_reduction_settings(&job.settings)
        );
        // CP1 is the drawing's: held, not drawn again.
        assert_eq!(job.placed_points.len(), 3);

        let points = survey_points(&document);
        assert_eq!(points.len(), 4);
        const MICROMETRE: f64 = 1e-6;
        let worked: [(&str, f64, f64, f64); 4] = [
            ("CP1", 500000.0, 5000000.0, 100.0),
            ("CP2", 500000.0, 5000099.9999340, 101.0006855),
            ("T1", 500149.7957556, 5000000.0, 107.7020040),
            ("T2", 500149.7957556, 5000079.9888319, 106.1562309),
        ];
        for (id, easting, northing, elevation) in worked {
            let drawn = point_named(&points, id).unwrap_or_else(|| panic!("{id} is drawn"));
            near(drawn.easting, easting, MICROMETRE, id);
            near(drawn.northing, northing, MICROMETRE, id);
            let height = drawn
                .elevation
                .unwrap_or_else(|| panic!("{id} has a height"));
            near(height, elevation, MICROMETRE, id);
        }

        // One undo step: the job and its three points go, the drawing's CP1 stays.
        must_be_ok(document.undo());
        assert!(document.survey_jobs().is_empty());
        let left = survey_points(&document);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, "CP1");
    }

    // Without SETTINGS the import starts where the wizard does: the defaults,
    // holding what the file declares as control - one_setup.jxl declares A,
    // with a height, so fixed in all three.
    #[test]
    fn without_settings_the_import_holds_the_control_the_file_declares() {
        let mut document = Document::default();
        let reply = must(run_survey_line(
            &mut document,
            &format!("SURVEY IMPORT \"{}\"", fixture("trimble_jxl/one_setup.jxl")),
        ));
        assert!(
            reply.contains(
                "\nsettings file= set=0 differ=1\n\
                 setting key=control value=A;file;fixed;0;fixed;0;fixed;0\n\
                 imported job="
            ),
            "{reply}"
        );
        assert_eq!(document.survey_jobs().len(), 1);
        let expected = ReductionSettings {
            control: vec![held("A", ControlOrigin::File, FIXED, FIXED)],
            ..ReductionSettings::default()
        };
        let settings = &document.survey_jobs()[0].settings;
        assert!(
            *settings == expected,
            "{}",
            serialise_reduction_settings(settings)
        );
    }

    // A SETTINGS file is the whole of the settings, as a job's stored settings
    // are: a file with no control line holds nothing, the control the survey
    // file declares included.
    #[test]
    fn a_settings_file_is_taken_whole_so_control_it_does_not_list_is_not_held() {
        let scratch = ScratchDirectory::new("whole");
        let settings = scratch.file(
            "atmosphere.txt",
            b"katana-reduction-settings=1\natmospheric=none\n",
        );
        let mut document = Document::default();
        let reply = must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{}\" SETTINGS {settings}",
                fixture("trimble_jxl/one_setup.jxl")
            ),
        ));
        assert!(
            reply.contains(
                "\nsettings file=atmosphere.txt set=0 differ=1\n\
                 setting key=atmospheric value=none\nimported job="
            ),
            "{reply}"
        );
        assert_eq!(document.survey_jobs().len(), 1);
        let expected = ReductionSettings {
            atmospheric: AtmosphericCorrection::None,
            ..ReductionSettings::default()
        };
        let used = &document.survey_jobs()[0].settings;
        assert!(*used == expected, "{}", serialise_reduction_settings(used));
    }

    // SET's control item holds its point as the wizard's Hold does: the entry of
    // the same id is replaced where it stands, and a point not yet held is added
    // after the others.
    #[test]
    fn a_set_control_item_replaces_the_point_of_its_id_and_adds_a_new_one_after() {
        let mut document = Document::default();
        place_point(&mut document, "Q", "30,39,5", "0");
        let reply = must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{}\" SET control=Q;drawing;fixed;0;fixed;0;free;0 \
                 control=A;file;weighted;0.01;weighted;0.01;free;0",
                fixture("trimble_jxl/one_setup.jxl")
            ),
        ));
        assert_eq!(document.survey_jobs().len(), 1);
        let weighted = ControlComponent {
            constraint: ControlConstraint::Weighted,
            sigma: 0.01,
        };
        let expected = ReductionSettings {
            control: vec![
                held("A", ControlOrigin::File, weighted, FREE),
                held("Q", ControlOrigin::Drawing, FIXED, FREE),
            ],
            ..ReductionSettings::default()
        };
        let used = &document.survey_jobs()[0].settings;
        assert!(*used == expected, "{}", serialise_reduction_settings(used));
        assert!(
            reply.contains(
                "\nsettings file= set=2 differ=2\n\
                 setting key=control value=A;file;weighted;0.01;weighted;0.01;free;0\n\
                 setting key=control value=Q;drawing;fixed;0;fixed;0;free;0\n"
            ),
            "{reply}"
        );
    }

    // SET over a SETTINGS file changes what it names and keeps the rest of the
    // file; the reply lists the lines that are not the defaults', in the text
    // form's order (the enumerations, then the numbers, then the switches, then
    // control) and its words, whatever order the items came in. A SET that
    // states a default is counted and differs from nothing.
    #[test]
    fn set_changes_the_keys_it_names_and_the_reply_says_what_is_not_the_default() {
        let scratch = ScratchDirectory::new("overlay");
        let settings = scratch.file(
            "held.txt",
            b"katana-reduction-settings=1\r\nrefraction.k=0.2\r\nfaces=separate\r\n\
              control=CP1;drawing;fixed;0;fixed;0;fixed;0\r\n",
        );
        let mut document = Document::default();
        place_point(&mut document, "CP1", "500000,4999999,100", "0");
        let reply = must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{}\" SETTINGS {settings} SET curvature_refraction=false \
                 refraction.k=0.13 atmospheric=auto faces=face-left-only",
                bare_traverse()
            ),
        ));
        assert!(
            reply.contains(
                "\nsettings file=held.txt set=4 differ=3\n\
                 setting key=faces value=face-left-only\n\
                 setting key=curvature_refraction value=false\n\
                 setting key=control value=CP1;drawing;fixed;0;fixed;0;fixed;0\n\
                 imported job="
            ),
            "{reply}"
        );
        assert_eq!(document.survey_jobs().len(), 1);
        let expected = ReductionSettings {
            faces: FaceHandling::FaceLeftOnly,
            curvature_and_refraction: false,
            control: vec![held("CP1", ControlOrigin::Drawing, FIXED, FIXED)],
            ..ReductionSettings::default()
        };
        let used = &document.survey_jobs()[0].settings;
        assert!(*used == expected, "{}", serialise_reduction_settings(used));
    }

    // A job's own settings, written in their text form, are a SETTINGS file:
    // importing with it again makes a job with the very same settings.
    #[test]
    fn a_jobs_settings_written_out_import_the_same_way_again() {
        let mut first = Document::default();
        place_point(&mut first, "CP1", "500000,4999999,100", "0");
        must(run_survey_line(
            &mut first,
            &format!(
                "SURVEY IMPORT \"{}\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0 \
                 atmospheric=none faces=separate",
                bare_traverse()
            ),
        ));
        assert_eq!(first.survey_jobs().len(), 1);
        let used = first.survey_jobs()[0].settings.clone();
        let scratch = ScratchDirectory::new("round-trip");
        let settings = scratch.file("job.txt", serialise_reduction_settings(&used).as_bytes());
        let mut second = Document::default();
        place_point(&mut second, "CP1", "500000,4999999,100", "0");
        must(run_survey_line(
            &mut second,
            &format!("SURVEY IMPORT \"{}\" SETTINGS {settings}", bare_traverse()),
        ));
        assert_eq!(second.survey_jobs().len(), 1);
        assert!(second.survey_jobs()[0].settings == used);
        assert_eq!(survey_points(&second), survey_points(&first));
    }

    // SET's items run to the next option word, so LAYER after them is LAYER.
    #[test]
    fn an_option_word_ends_sets_items() {
        let mut document = Document::default();
        place_point(&mut document, "CP1", "500000,4999999,100", "0");
        must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{}\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0 \
                 atmospheric=none LAYER survey/held",
                bare_traverse()
            ),
        ));
        assert_eq!(document.survey_jobs().len(), 1);
        assert_eq!(document.survey_jobs()[0].layer, "survey/held");
        assert_eq!(
            document.survey_jobs()[0].settings.atmospheric,
            AtmosphericCorrection::None
        );
    }

    // Every refusal, and the drawing as it was: no job, no point but the one
    // placed, nothing to undo but the placing.
    #[test]
    fn every_refusal_says_why_and_leaves_the_drawing_as_it_was() {
        let scratch = ScratchDirectory::new("refusals");
        let newer = scratch.file(
            "newer.txt",
            b"katana-reduction-settings=2\natmospheric=none\n",
        );
        let not_settings = scratch.file("notes.txt", b"atmospheric=none\n");
        let malformed = scratch.file(
            "malformed.txt",
            b"katana-reduction-settings=1\nfaces=sideways\n",
        );
        let twice = scratch.file(
            "twice.txt",
            b"katana-reduction-settings=1\natmospheric=none\natmospheric=auto\n",
        );
        let bare = format!("SURVEY IMPORT \"{}\" ", bare_traverse());
        let read = format!("SURVEY READ \"{}\" ", bare_traverse());
        let no_survey = format!("SURVEY IMPORT \"{}\" ", scratch.path_of("no survey.sdr"));
        let refusals: &[(String, ErrorCode, &str)] = &[
            // The line.
            (
                format!("{bare}SET"),
                ErrorCode::InvalidArgument,
                "SET needs at least one <key>=<value>",
            ),
            (
                format!("{bare}SET atmospheric=none SET faces=separate"),
                ErrorCode::InvalidArgument,
                "SET is given twice",
            ),
            (
                format!("{bare}SETTINGS a.txt SETTINGS b.txt"),
                ErrorCode::InvalidArgument,
                "SETTINGS is given twice",
            ),
            (
                format!("{bare}FORMAT sokkia-sdr FORMAT sokkia-sdr"),
                ErrorCode::InvalidArgument,
                "FORMAT is given twice",
            ),
            (
                format!("{bare}LAYER a LAYER b"),
                ErrorCode::InvalidArgument,
                "LAYER is given twice",
            ),
            (
                format!("{bare}SETTINGS"),
                ErrorCode::InvalidArgument,
                "SETTINGS needs a value",
            ),
            (
                format!("{read}SET atmospheric=none"),
                ErrorCode::InvalidArgument,
                "'SET' is not an option of SURVEY READ",
            ),
            (
                format!("{read}SETTINGS x.txt"),
                ErrorCode::InvalidArgument,
                "'SETTINGS' is not an option of SURVEY READ",
            ),
            // SET's items.
            (
                format!("{bare}SET bogus=1"),
                ErrorCode::InvalidArgument,
                "'bogus' is not a reduction setting; the settings are atmospheric, ",
            ),
            (
                format!("{bare}SET katana-reduction-settings=1"),
                ErrorCode::InvalidArgument,
                "'katana-reduction-settings' is not a reduction setting",
            ),
            (
                format!("{bare}SET refraction.k 0.2"),
                ErrorCode::InvalidArgument,
                "SET takes <key>=<value> items, and 'refraction.k' is not one",
            ),
            (
                format!("{bare}SET #atmospheric=none"),
                ErrorCode::InvalidArgument,
                "SET takes <key>=<value> items, and '#atmospheric=none' is not one",
            ),
            (
                format!("{bare}SET \"atmospheric=none\nfaces=separate\""),
                ErrorCode::InvalidArgument,
                "SET item 1 holds a line break",
            ),
            (
                format!("{bare}SET refraction.k=abc"),
                ErrorCode::ParseFailure,
                "'abc' is not a number (refraction.k)",
            ),
            (
                format!("{bare}SET faces=sideways"),
                ErrorCode::ParseFailure,
                "'sideways' is not a choice for faces",
            ),
            (
                format!("{bare}SET refraction.k=2"),
                ErrorCode::InvalidArgument,
                "the coefficient of refraction must lie between -1 and 1",
            ),
            (
                format!("{bare}SET iterations.max=0"),
                ErrorCode::InvalidArgument,
                "the adjustment needs at least one iteration",
            ),
            (
                format!("{bare}SET control=CP1;drawing"),
                ErrorCode::ParseFailure,
                "a control line needs id;origin;then constraint;sigma",
            ),
            (
                format!("{bare}SET control=CP1;drawing;weighted;0;weighted;0;free;0"),
                ErrorCode::InvalidArgument,
                "a weighted northing needs a positive standard deviation",
            ),
            (
                format!("{bare}SET atmospheric=none atmospheric=auto"),
                ErrorCode::InvalidArgument,
                "'atmospheric' is given twice",
            ),
            (
                format!(
                    "{bare}SET control=CP1;drawing;fixed;0;fixed;0;free;0 \
                     control=CP1;file;fixed;0;fixed;0;free;0"
                ),
                ErrorCode::InvalidArgument,
                "control point 'CP1' is listed twice",
            ),
            // A SETTINGS file.
            (
                format!("{bare}SETTINGS {newer}"),
                ErrorCode::Unsupported,
                "these reduction settings were written by a newer Katana (settings version 2)",
            ),
            (
                format!("{bare}SETTINGS {not_settings}"),
                ErrorCode::ParseFailure,
                "this text is not a set of Katana reduction settings",
            ),
            (
                format!("{bare}SETTINGS {malformed}"),
                ErrorCode::ParseFailure,
                "line 2 of the reduction settings: 'sideways' is not a choice for faces",
            ),
            (
                format!("{bare}SETTINGS {twice}"),
                ErrorCode::ParseFailure,
                "line 3 of the reduction settings: 'atmospheric' is given twice",
            ),
            (
                format!("{bare}SETTINGS \"{}\"", scratch.path_of("absent.txt")),
                ErrorCode::FileImportFailure,
                "the settings file cannot be read",
            ),
            // The settings before the survey file: a mistyped key costs no read
            // of the file, so one that is not there is not what is refused.
            (
                format!("{no_survey}SET bogus=1"),
                ErrorCode::InvalidArgument,
                "'bogus' is not a reduction setting",
            ),
            (
                format!("{no_survey}SETTINGS {newer}"),
                ErrorCode::Unsupported,
                "written by a newer Katana (settings version 2)",
            ),
            // The reduction, on control neither the file nor the drawing places.
            (
                format!("{bare}SET control=CP9;drawing;fixed;0;fixed;0;free;0"),
                ErrorCode::NotFound,
                "Control point CP9 is not on the drawing, so it cannot be held.",
            ),
            (
                format!("{bare}SET control=CP1;file;fixed;0;fixed;0;free;0"),
                ErrorCode::NotFound,
                "Control point CP1 is not a point with coordinates in the file, so it cannot \
                 be held.",
            ),
            (
                format!("{bare}SET control=CP0;drawing;fixed;0;fixed;0;fixed;0"),
                ErrorCode::InvalidArgument,
                "Control point CP0 is held in height but has no height; give it one or free its \
                 height.",
            ),
        ];
        // Every row reported, not the first alone.
        let mut failures: Vec<String> = Vec::new();
        for (line, code, said) in refusals {
            let mut document = Document::default();
            place_point(&mut document, "CP1", "500000,4999999,100", "0");
            // A point with no height: FORWARD from a start with none.
            place_point(&mut document, "CP0", "500010,4999999", "-");
            let error = match run_survey_line(&mut document, line) {
                Ok(reply) => {
                    failures.push(format!("{line}: not refused: {reply}"));
                    continue;
                }
                Err(error) => error,
            };
            if error.code != *code {
                failures.push(format!(
                    "{line}: code {:?} where {:?}: {}",
                    error.code,
                    code,
                    error.describe()
                ));
            }
            if !error.describe().contains(said) {
                failures.push(format!(
                    "{line}: does not say \"{said}\": {}",
                    error.describe()
                ));
            }
            if !document.survey_jobs().is_empty() {
                failures.push(format!("{line}: a job was kept"));
            }
            if survey_points(&document).len() != 2 {
                failures.push(format!("{line}: the drawing's points changed"));
            }
            // Nothing to undo but the two placings.
            if document.history().undo_count() != 2 {
                failures.push(format!(
                    "{line}: {} steps to undo",
                    document.history().undo_count()
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    // ---- Held control, and what the adjustment made of the data ------------------------

    // The reduction holds the first point of an id, and which is first is only the
    // order they were drawn in: CP1 drawn at two places 100 km apart, either
    // order, the line refuses to hold it and names both, in the drawing's order,
    // with the drawing as it was. Before, the job sat on whichever came first.
    #[test]
    fn an_id_the_drawing_has_at_two_places_is_not_held_and_both_are_named() {
        let orders: [(&str, &str, &str); 2] = [
            (
                "500000,4999999,100",
                "600000,4999999,100",
                "Control point CP1 is on the drawing 2 times, at E 500000.0000 N 5000000.0000 \
                 Z 100.0000 and E 600000.0000 N 5000000.0000 Z 100.0000, so which one to hold \
                 is not known; rename or delete all but one.",
            ),
            (
                "600000,4999999,100",
                "500000,4999999,100",
                "Control point CP1 is on the drawing 2 times, at E 600000.0000 N 5000000.0000 \
                 Z 100.0000 and E 500000.0000 N 5000000.0000 Z 100.0000, so which one to hold \
                 is not known; rename or delete all but one.",
            ),
        ];
        for (first, second, said) in orders {
            let mut document = Document::default();
            place_point(&mut document, "CP1", first, "0");
            place_point(&mut document, "CP1", second, "0");
            let error = run_survey_line(
                &mut document,
                &format!(
                    "SURVEY IMPORT \"{}\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0",
                    bare_traverse()
                ),
            )
            .expect_err(first);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{first}");
            assert_eq!(error.message, said, "{first}");
            assert!(document.survey_jobs().is_empty(), "{first}");
            assert_eq!(survey_points(&document).len(), 2, "{first}");
            assert_eq!(document.history().undo_count(), 2, "{first}");
        }
    }

    // Two points of one id at one mark - FORWARD twice from the same start - give
    // the same job whichever is held, so they are held: the positions worked by
    // hand at the top of this file, and the reply naming the first, as the
    // reduction takes it.
    #[test]
    fn points_of_one_id_at_one_mark_are_held_as_one() {
        let mut document = Document::default();
        place_point(&mut document, "CP1", "500000,4999999,100", "0");
        place_point(&mut document, "CP1", "500000,4999999,100", "0");
        let reply = must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{}\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0",
                bare_traverse()
            ),
        ));
        assert!(
            reply.contains("\nheld id=CP1 from=drawing entity=1 "),
            "{reply}"
        );
        let points = survey_points(&document);
        let t1 = point_named(&points, "T1").expect("T1 is drawn");
        near(t1.easting, 500149.7957556, 1e-6, "T1");
        near(t1.northing, 5000000.0, 1e-6, "T1");
    }

    // The reply says where each held point was held, as the reduction took it:
    // the drawing's first point of the id - FORWARD made CP1 the drawing's first
    // entity, 1 m north of 500000,4999999 and level at 100 - or the file's own
    // coordinates, which one_setup.jxl gives A as North 10, East 20, Elevation 3
    // (read from the fixture). A number is written as every record writes one,
    // the shortest text that reads back exactly: 5000000 is "5e+06".
    #[test]
    fn the_reply_says_where_each_held_point_was_held() {
        let mut document = Document::default();
        place_point(&mut document, "CP1", "500000,4999999,100", "0");
        let held_reply = must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{}\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0",
                bare_traverse()
            ),
        ));
        assert!(
            held_reply.contains(
                "\nimported job=job-2 entities=3 layer=survey/points reduction_warnings=3\n\
                 held id=CP1 from=drawing entity=1 northing=5e+06 easting=5e+05 height=100\n\
                 reduction method=radiation adjustments=0 rejected=0\n"
            ),
            "{held_reply}"
        );

        let mut other = Document::default();
        let declared = must(run_survey_line(
            &mut other,
            &format!("SURVEY IMPORT \"{}\"", fixture("trimble_jxl/one_setup.jxl")),
        ));
        assert!(
            declared.contains("\nheld id=A from=file northing=10 easting=20 height=3\n"),
            "{declared}"
        );
    }

    // Held from the drawing, a point the file never names changes nothing - the
    // file's own coordinates place its points, where cli.survey_import_sokkia_sdr
    // has them - and the reply's first reduction warning says so.
    #[test]
    fn a_held_point_the_file_never_names_is_said_to_change_nothing() {
        let mut document = Document::default();
        place_point(&mut document, "BM1", "700000,6999999,10", "0");
        let reply = must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{}\" SET control=BM1;drawing;fixed;0;fixed;0;fixed;0",
                fixture("sdr/traverse.sdr")
            ),
        ));
        assert!(
            reply.contains(
                "\nreduction method=radiation adjustments=0 rejected=0\n\
                 reduction_warning text=\"Control point BM1 is held from the drawing, but the \
                 file names no point BM1, so holding it changes nothing.\"\n"
            ),
            "{reply}"
        );
        let points = survey_points(&document);
        let t1 = point_named(&points, "T1").expect("T1 is drawn");
        near(t1.easting, 500149.7957556, 1e-6, "T1");
    }

    // Held from the drawing, a point the file gives other coordinates - CP1, which
    // traverse.sdr keys in at 500000 E 5000000 N 100 Z, drawn 1 000 km away - is
    // held where the drawing has it, and the first reduction warning gives both.
    #[test]
    fn a_held_point_the_file_gives_other_coordinates_is_said_with_both() {
        let mut document = Document::default();
        place_point(&mut document, "CP1", "600000,5999999,200", "0");
        let reply = must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{}\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0",
                fixture("sdr/traverse.sdr")
            ),
        ));
        assert!(
            reply.contains(
                "\nheld id=CP1 from=drawing entity=1 northing=6e+06 easting=6e+05 height=200\n"
            ),
            "{reply}"
        );
        assert!(
            reply.contains(
                "\nreduction_warning text=\"Control point CP1 is held where the drawing has it, \
                 E 600000.0000 N 6000000.0000 Z 200.0000, not where the file gives it, \
                 E 500000.0000 N 5000000.0000 Z 100.0000.\"\n"
            ),
            "{reply}"
        );
    }

    // The adjustment's outcome, record by record, from a report whose every value
    // is set here: the rows rejected before the adjustment count with the
    // outliers it rejected, a variance factor and a global test a run has not got
    // are "none", and the method is the settings' in words.
    #[test]
    fn each_adjustment_is_a_record_of_what_its_report_holds() {
        let kept = ReportObservation::default();
        let dropped = ReportObservation {
            rejected: true,
            ..ReportObservation::default()
        };
        let horizontal = AdjustmentReport {
            method: String::from("network least squares (horizontal)"),
            observations: 12,
            unknowns: 6,
            redundancy: 6,
            variance_factor: Some(0.25),
            global_test: Some(ReportGlobalTest {
                statistic: 1.5,
                lower_critical: 1.24,
                upper_critical: 14.45,
                significance_level: 0.05,
                passed: false,
            }),
            flagged_outliers: vec![
                String::from("direction A -> P"),
                String::from("distance A -> P"),
            ],
            rejected_outliers: vec![String::from("distance B -> P")],
            ..AdjustmentReport::default()
        };
        let levels = AdjustmentReport {
            method: String::from("network least squares (levels)"),
            observations: 4,
            unknowns: 2,
            redundancy: 2,
            variance_factor: Some(1.5),
            global_test: Some(ReportGlobalTest {
                statistic: 3.0,
                lower_critical: 0.05,
                upper_critical: 7.38,
                significance_level: 0.05,
                passed: true,
            }),
            ..AdjustmentReport::default()
        };
        let once = AdjustmentReport {
            method: String::from("network least squares (levels)"),
            observations: 2,
            unknowns: 2,
            ..AdjustmentReport::default()
        };
        let report = ReductionReport {
            settings: ReductionSettings {
                method: AdjustmentMethod::Network,
                network_dimension: NetworkDimension::HorizontalAndLevels,
                ..ReductionSettings::default()
            },
            observations: vec![kept, dropped.clone(), dropped],
            adjustments: vec![horizontal, levels, once],
            ..ReductionReport::default()
        };
        assert_eq!(
            reduction_records(&report),
            "\nreduction method=\"network, horizontal and levels\" adjustments=3 rejected=3\
             \nadjustment method=\"network least squares (horizontal)\" observations=12 \
             unknowns=6 redundancy=6 variance_factor=0.25 global_test=failed flagged=2 \
             rejected=1\
             \nadjustment method=\"network least squares (levels)\" observations=4 unknowns=2 \
             redundancy=2 variance_factor=1.5 global_test=passed flagged=0 rejected=0\
             \nadjustment method=\"network least squares (levels)\" observations=2 unknowns=2 \
             redundancy=0 variance_factor=none global_test=none flagged=0 rejected=0"
        );

        assert_eq!(
            reduction_records(&ReductionReport::default()),
            "\nreduction method=radiation adjustments=0 rejected=0"
        );
        let unrun = ReductionReport {
            settings: ReductionSettings {
                method: AdjustmentMethod::Traverse,
                ..ReductionSettings::default()
            },
            ..ReductionReport::default()
        };
        assert_eq!(
            reduction_records(&unrun),
            "\nreduction method=\"traverse, Bowditch\" adjustments=0 rejected=0"
        );
    }

    // A network through the line says what the adjustment made of it, as the
    // wizard's message does. network_gsi8.gsi (the wizard's own fixture) has two
    // setups, on A and on B, each observing the other and P and Q with a
    // direction and a distance. The reduction's network takes a setup's
    // directions as angles from its backsight (reduction_adjust), not as
    // directions with an orientation unknown each: at A, B to P and B to Q; at
    // B, A to P and A to Q - 4 angles - and the 6 distances: 10 observations.
    // A and B held in plan, the unknowns are P and Q, 2 each: 4. The redundancy
    // is 6, as the directions' form counts it too (12 directions and distances
    // less 4 coordinates and 2 orientations): a setup's n directions with an
    // orientation unknown are its n - 1 angles, one adjustment either way. The
    // variance factor and the global test are the adjustment's: the reply must
    // carry them as the reduction run on the same file and settings reports
    // them, to the last bit.
    #[test]
    fn a_network_through_the_line_says_what_its_adjustment_made_of_the_data() {
        // tests/qt_widgets/survey/data, where the C++'s KATANA_SURVEY_UI_DATA
        // points.
        let file = format!(
            "{}/../../../tests/qt_widgets/survey/data/network_gsi8.gsi",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut document = Document::default();
        let reply = must(run_survey_line(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{file}\" SET adjustment.method=network \
                 control=A;file;fixed;0;fixed;0;free;0 control=B;file;fixed;0;fixed;0;free;0"
            ),
        ));
        assert_eq!(document.survey_jobs().len(), 1);

        let settings = ReductionSettings {
            method: AdjustmentMethod::Network,
            control: vec![
                held("A", ControlOrigin::File, FIXED, FREE),
                held("B", ControlOrigin::File, FIXED, FREE),
            ],
            ..ReductionSettings::default()
        };
        let used = &document.survey_jobs()[0].settings;
        assert!(*used == settings, "{}", serialise_reduction_settings(used));
        let bytes = std::fs::read(&file).expect("the fixture is read");
        let read = must(read_survey(
            format_registry(),
            "leica-gsi",
            &bytes,
            "network_gsi8.gsi",
            &ReadOptions::default(),
        ));
        let direct = must(reduce_for_drawing(
            &read.project,
            &settings,
            &ReductionContext::default(),
        ));
        assert_eq!(direct.report.adjustments.len(), 1);
        let adjustment = &direct.report.adjustments[0];
        let variance_factor = adjustment
            .variance_factor
            .expect("the adjustment has a variance factor");
        let global_test = adjustment
            .global_test
            .expect("the adjustment has a global test");

        let at = reply
            .find("\nadjustment ")
            .unwrap_or_else(|| panic!("{reply}"));
        let line = reply[at + 1..].split('\n').next().unwrap_or_default();
        let record = read_reply_record(line).unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(
            record.value("method"),
            Some("network least squares (horizontal)")
        );
        assert_eq!(record.value("observations"), Some("10"));
        assert_eq!(record.value("unknowns"), Some("4"));
        assert_eq!(record.value("redundancy"), Some("6"));
        let variance = parse_finite_double(record.value("variance_factor").unwrap_or(""))
            .unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(variance, variance_factor);
        assert_eq!(
            record.value("global_test"),
            Some(if global_test.passed {
                "passed"
            } else {
                "failed"
            })
        );
        let flagged = format!("{}", adjustment.flagged_outliers.len());
        assert_eq!(record.value("flagged"), Some(flagged.as_str()));
        assert!(
            reply.contains(&format!(
                "\nreduction method=\"network, horizontal\" adjustments=1 rejected={}\n",
                rejected_observations(&direct.report)
            )),
            "{reply}"
        );
    }

    // A SETTINGS file is read as every text file is: saved with a byte order mark,
    // or as UTF-16, it is the same settings, where the parser alone called it
    // not settings at all. One that is not UTF-8 - a degree sign in Windows-1252
    // in a note - is read, and the guess is said.
    #[test]
    fn a_settings_file_is_decoded_as_any_text_file_is() {
        let scratch = ScratchDirectory::new("encodings");
        let lines: &[u8] =
            b"katana-reduction-settings=1\r\ncontrol=CP1;drawing;fixed;0;fixed;0;fixed;0\r\n";
        let mut utf16: Vec<u8> = vec![0xFF, 0xFE];
        for &byte in lines {
            utf16.push(byte);
            utf16.push(0);
        }
        let with_mark = [b"\xEF\xBB\xBF".as_slice(), lines].concat();
        let ansi = [b"# measured at 20\xB0C\r\n".as_slice(), lines].concat();
        let files: [(&str, Vec<u8>, Option<&str>); 3] = [
            ("bom.txt", with_mark, None),
            ("utf16.txt", utf16, None),
            (
                "ansi.txt",
                ansi,
                Some("\nsettings_warning text=\"the settings file is not UTF-8; it was read as "),
            ),
        ];
        for (name, bytes, warning) in &files {
            let mut document = Document::default();
            place_point(&mut document, "CP1", "500000,4999999,100", "0");
            let reply = must(run_survey_line(
                &mut document,
                &format!(
                    "SURVEY IMPORT \"{}\" SETTINGS {}",
                    bare_traverse(),
                    scratch.file(name, bytes)
                ),
            ));
            assert!(
                reply.contains(&format!(
                    "\nsettings file={name} set=0 differ=1\n\
                     setting key=control value=CP1;drawing;fixed;0;fixed;0;fixed;0\n"
                )),
                "{name}: {reply}"
            );
            assert_eq!(
                reply.contains("settings_warning"),
                warning.is_some(),
                "{name}: {reply}"
            );
            if let Some(warning) = warning {
                assert!(reply.contains(warning), "{name}: {reply}");
            }
            assert_eq!(document.survey_jobs().len(), 1, "{name}");
            assert_eq!(document.survey_jobs()[0].placed_points.len(), 3, "{name}");
        }
    }
}

// ---- SURVEY IMPORT codes and strings what it draws --------------------------------------
//
// The customisation is the hand-written fixture
// tests/data/field_codes/test_field_codes.customisation.json, installed on
// the Document as a loaded one (CUSTOMISE is the front ends' to run; what a
// drawing has loaded is the Document's). It has a folder of its own: the
// headless checks of the window load EVERY customisation file of
// tests/data/customisation, and pin what those three hold. Its rules, read by
// hand:
//   KB*   a LINE on FIELD KERB, linestyle "FIELD Kerb", colour "field kerb" -
//         a name only the fixture's own colour table knows: #C04000
//   EB*   a LINE on FIELD EDGE, a plain line, colour "blue" (a standard name)
//   CTRL  a POINT on FIELD CONTROL, a plain line, colour "red"
//
// The field file of most of these tests is tests/surveyio/data/fld/gnss.fld,
// an RTK job whose positions the file states, so nothing is reduced (the
// same five are worked beside cli.survey_import_gnss_field_file):
//   CM1   entered, no code            E 500000  N 6200000
//   R001  KB, string 1                E 500010  N 6200020  (its first position)
//   R002  KB, string 1                E 500020  N 6200020
//   R003  KB, string 1                E 500020  N 6200030
//   20    closes the string KB 1: R001, R002 and R003 are one closed line
//   R004  KB, string 1, begun again   E 500040  N 6200040  (alone: no line)
// So an import that codes and strings draws five points; codes the four KB
// ones by their string name KB1, which KB* answers, onto the one layer FIELD
// KERB in the one style "FIELD Kerb" (a style is named after its linestyle);
// draws one line, the closed KB 1; and leaves two things in no line - the
// second KB 1, of one point, and CM1, which has no code. The first of those
// is a fault and so a warning of the job's report; the second is not.

// The fixture, installed as the session's loaded customisation. Its folder is
// reached from the survey fixtures' own (tests/surveyio/data), two up.
#[track_caller]
fn install_field_codes(document: &mut Document) {
    let path = fixture("../../data/field_codes/test_field_codes.customisation.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("{path} is read"));
    let customisation = must(customisation_from_json(&bytes));
    must_be_ok(document.install_customisation(customisation, CustomisationOrigin::Loaded, false));
}

#[track_caller]
fn install_json(document: &mut Document, json: &str) {
    let customisation = must(customisation_from_json(json.as_bytes()));
    must_be_ok(document.install_customisation(customisation, CustomisationOrigin::Loaded, false));
}

// On a layer of its own at the top of the tree: an import onto survey/points
// creates "survey" with it, which its undo leaves behind (docs/survey.md, "Not
// done"), and these tests compare whole drawings.
fn import_gnss(options: &str) -> String {
    let mut line = format!(
        "SURVEY IMPORT \"{}\" LAYER fieldwork",
        fixture("fld/gnss.fld")
    );
    if !options.is_empty() {
        line.push(' ');
        line.push_str(options);
    }
    line
}

#[track_caller]
fn run(document: &mut Document, line: &str) -> String {
    match run_survey_line(document, line) {
        Ok(reply) => reply,
        Err(error) => panic!("{line}: {}", error.describe()),
    }
}

// The reply's records about the survey codes, in the order given: every line
// whose first word is coded, unmatched_code, unmatched_codes_more or linework.
fn finish_records(reply: &str) -> Vec<String> {
    const WORDS: [&str; 4] = [
        "coded ",
        "unmatched_code ",
        "unmatched_codes_more=",
        "linework ",
    ];
    lines_of(reply)
        .into_iter()
        .filter(|line| WORDS.iter().any(|word| line.starts_with(word)))
        .map(String::from)
        .collect()
}

// The reply with those records taken out: what it was before they existed.
fn without_finish_records(reply: &str) -> String {
    let records = finish_records(reply);
    let mut rest = String::new();
    for line in lines_of(reply) {
        if !records.iter().any(|record| record.as_str() == line) {
            rest.push_str(line);
            rest.push('\n');
        }
    }
    rest
}

// The value of `key` in the reply's `imported` record.
fn imported_value(reply: &str, key: &str) -> String {
    for line in lines_of(reply) {
        if line.starts_with("imported ") {
            return match read_reply_record(line) {
                Some(record) => String::from(record.value(key).unwrap_or("<absent>")),
                None => String::from("<not a record>"),
            };
        }
    }
    String::from("<no imported record>")
}

// The drawing's entities in id order, as the database visits them.
fn entities_of(document: &Document) -> Vec<&Entity> {
    let entities = &document.model().entities;
    entities
        .ids()
        .into_iter()
        .filter_map(|id| entities.find(id))
        .collect()
}

fn lines_drawn(document: &Document) -> Vec<&Entity> {
    entities_of(document)
        .into_iter()
        .filter(|entity| matches!(entity.geometry, Geometry::Polyline(_)))
        .collect()
}

fn text_of(entity: &Entity, key: &str) -> String {
    match entity.properties.get(key) {
        None => String::from("<absent>"),
        Some(PropertyValue::Text(text)) => text.clone(),
        Some(_) => String::from("<not text>"),
    }
}

// The last point entity numbered `number`, as the C++ finds it.
fn point_numbered<'a>(document: &'a Document, number: &str) -> Option<&'a Entity> {
    entities_of(document).into_iter().rfind(|entity| {
        matches!(entity.geometry, Geometry::Point(_)) && text_of(entity, "point") == number
    })
}

fn position_of(point: &Entity) -> Point2 {
    match &point.geometry {
        Geometry::Point(shape) => shape.position,
        _ => panic!("entity {} is not a point", point.id),
    }
}

fn polyline_of(entity: &Entity) -> &katana_geometry::Polyline2 {
    match &entity.geometry {
        Geometry::Polyline(shape) => shape,
        _ => panic!("entity {} is not a polyline", entity.id),
    }
}

// Everything of a drawing a survey import may change.
#[derive(PartialEq)]
struct Drawing {
    entities: Vec<Entity>,
    layers: Vec<Layer>,
    styles: Vec<Style>,
}

// What a failed comparison prints: enough to see what is left over.
impl fmt::Debug for Drawing {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} entities; layers:", self.entities.len())?;
        for layer in &self.layers {
            write!(formatter, " [{}]", layer.name)?;
        }
        write!(formatter, " styles:")?;
        for style in &self.styles {
            write!(formatter, " [{}]", style.name)?;
        }
        Ok(())
    }
}

fn drawing_of(document: &Document) -> Drawing {
    Drawing {
        entities: entities_of(document).into_iter().cloned().collect(),
        layers: document.model().layers.all(),
        styles: document.model().styles.all(),
    }
}

// The text an import with every drawing option at its default has always
// kept with its job (cad's write_survey_job_options): what a job that was
// neither coded nor strung holds.
const PLAIN_JOB_OPTIONS: &str = "katana-survey-import-options=1\n\
                                 layer-per-code=false\n\
                                 create-layers=true\n\
                                 code-property=code\n\
                                 point-number-property=point\n\
                                 description-property=description\n\
                                 record-source=true\n";

fn records(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| String::from(*line)).collect()
}

mod survey_verbs {
    use super::*;

    #[test]
    fn an_import_codes_its_points_and_strings_the_files_lines_and_one_undo_takes_it_all_back() {
        let mut document = Document::default();
        install_field_codes(&mut document);
        let before = drawing_of(&document);

        let reply = run(&mut document, &import_gnss(""));

        // entities= is the five POINTS, as it always was; the line has its record.
        assert_eq!(imported_value(&reply, "job"), "job-1");
        assert_eq!(imported_value(&reply, "entities"), "5");
        assert_eq!(imported_value(&reply, "layer"), "fieldwork");
        // The line made no layer and no style of its own: it goes on the layer
        // and wears the style the coding had just made for its points.
        assert_eq!(
            finish_records(&reply),
            records(&[
                "coded points=4 matched=4 unmatched_codes=0 layers=1 styles=1",
                "linework lines=1 unplaced=2 layers=0 styles=0",
            ])
        );

        // The drawing: five points and the one line.
        assert_eq!(document.model().entities.size(), 6);
        for number in ["R001", "R002", "R003", "R004"] {
            let point = point_numbered(&document, number).expect(number);
            assert_eq!(point.layer, "FIELD KERB", "{number}");
            assert_eq!(point.style, "FIELD Kerb", "{number}");
            assert_eq!(text_of(point, "code"), "KB", "{number}");
            assert_eq!(text_of(point, "string"), "1", "{number}");
        }
        let uncoded = point_numbered(&document, "CM1").expect("CM1");
        assert_eq!(uncoded.layer, "fieldwork");
        assert_eq!(uncoded.style, "");

        let lines = lines_drawn(&document);
        assert_eq!(lines.len(), 1);
        let kerb = lines[0];
        let shape = polyline_of(kerb);
        assert!(shape.closed, "the file closed the string");
        assert_eq!(
            shape.vertices,
            vec![
                Point2::new(500010.0, 6200020.0),
                Point2::new(500020.0, 6200020.0),
                Point2::new(500020.0, 6200030.0),
            ]
        );
        assert_eq!(kerb.layer, "FIELD KERB");
        assert_eq!(kerb.style, "FIELD Kerb");
        assert_eq!(text_of(kerb, "code"), "KB");
        assert_eq!(text_of(kerb, "string"), "1");
        // A line runs through its points where they stand, in height as in plan.
        let heights = heights_of(&kerb.properties, 3);
        for (i, number) in ["R001", "R002", "R003"].into_iter().enumerate() {
            let point = point_numbered(&document, number).expect(number);
            assert!(heights[i].is_some(), "{number}");
            assert_eq!(heights[i], heights_of(&point.properties, 1)[0], "{number}");
        }

        // The style: the rule's linestyle, and its colour as the Document's
        // resolver reads the name - the fixture's own table, #C04000.
        let style = document
            .model()
            .styles
            .find("FIELD Kerb")
            .expect("the style FIELD Kerb");
        assert_eq!(style.linetype, "FIELD Kerb");
        assert_eq!(
            style.color,
            Some(Color {
                r: 0xC0,
                g: 0x40,
                b: 0x00,
                a: 255
            })
        );

        // The job owns the points and the line, and remembers how it was imported.
        assert_eq!(document.survey_jobs().len(), 1);
        let job = &document.survey_jobs()[0];
        assert_eq!(job.placed_points.len(), 5);
        assert_eq!(job.created_entities.len(), 6);
        assert_eq!(
            job.import_options,
            format!("{PLAIN_JOB_OPTIONS}apply-codes=true\ndraw-linework=true\n")
        );

        // ONE undo: the points, their layers and style, the line and the job.
        assert_eq!(document.history().undo_count(), 1);
        must_be_ok(document.undo());
        assert!(document.survey_jobs().is_empty());
        assert_eq!(drawing_of(&document), before);
        assert!(document.undo().is_err(), "it was one step");
    }

    // What the finish left over is a warning of the job's report, and so of the
    // reply: here the second string KB 1, which is one point. CM1, with no code,
    // is in no line for being what it is, and is not one.
    #[test]
    fn what_the_finish_leaves_over_is_counted_and_listed_with_the_reductions_warnings() {
        let mut plain = Document::default();
        install_field_codes(&mut plain);
        let bare = run(&mut plain, &import_gnss("CODES off LINEWORK off"));
        let mut document = Document::default();
        install_field_codes(&mut document);
        let reply = run(&mut document, &import_gnss(""));

        let count = |text: &str| parse_integer(imported_value(text, "reduction_warnings"));
        let bare_count = count(&bare).unwrap_or_else(|| panic!("{bare}"));
        let reply_count = count(&reply).unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(reply_count, bare_count + 1);
        // The last of the warnings, straight before the coded record.
        assert!(
            reply.contains(
                "\nreduction_warning text=\"1 string(s) of the file are in no line (fewer than \
                 two of its points have a position: 1).\"\ncoded points=4 "
            ),
            "{reply}"
        );
        assert!(!bare.contains("in no line"), "{bare}");
    }

    #[test]
    fn codes_off_strings_the_lines_and_leaves_the_points_as_the_import_drew_them() {
        let mut document = Document::default();
        install_field_codes(&mut document);
        let reply = run(&mut document, &import_gnss("CODES off"));

        // The layer FIELD KERB and the style "FIELD Kerb" are then the LINE's to
        // make, and its record says so: nothing else would account for them.
        assert_eq!(
            finish_records(&reply),
            records(&[
                "coded none reason=off",
                "linework lines=1 unplaced=2 layers=1 styles=1",
            ])
        );
        assert_eq!(imported_value(&reply, "entities"), "5");
        for number in ["CM1", "R001", "R002", "R003", "R004"] {
            let point = point_numbered(&document, number).expect(number);
            assert_eq!(point.layer, "fieldwork", "{number}");
            assert_eq!(point.style, "", "{number}");
        }
        // The line is styled all the same: its rule is how it is known to be one.
        let lines = lines_drawn(&document);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].layer, "FIELD KERB");
        assert_eq!(lines[0].style, "FIELD Kerb");
        assert_eq!(
            document.survey_jobs()[0].import_options,
            format!("{PLAIN_JOB_OPTIONS}draw-linework=true\n")
        );
        must_be_ok(document.undo());
        assert_eq!(document.model().entities.size(), 0);
    }

    #[test]
    fn linework_off_codes_the_points_and_draws_no_line() {
        let mut document = Document::default();
        install_field_codes(&mut document);
        let reply = run(&mut document, &import_gnss("LINEWORK off"));

        assert_eq!(
            finish_records(&reply),
            records(&[
                "coded points=4 matched=4 unmatched_codes=0 layers=1 styles=1",
                "linework none reason=off",
            ])
        );
        assert!(lines_drawn(&document).is_empty());
        assert_eq!(document.model().entities.size(), 5);
        let coded = point_numbered(&document, "R002").expect("R002");
        assert_eq!(coded.layer, "FIELD KERB");
        assert_eq!(coded.style, "FIELD Kerb");
        assert_eq!(
            document.survey_jobs()[0].import_options,
            format!("{PLAIN_JOB_OPTIONS}apply-codes=true\n")
        );
    }

    // Both off is the import of before: the points on LAYER, no layer or style of
    // a rule, no string number, and a job stored as one that was not finished.
    // The words are read in any case.
    #[test]
    fn both_off_imports_the_points_alone_though_survey_codes_are_loaded() {
        let mut document = Document::default();
        install_field_codes(&mut document);
        let before = drawing_of(&document);
        let reply = run(&mut document, &import_gnss("codes OFF Linework Off"));

        assert_eq!(
            finish_records(&reply),
            records(&["coded none reason=off", "linework none reason=off"])
        );
        assert_eq!(document.model().entities.size(), 5);
        for number in ["CM1", "R001", "R002", "R003", "R004"] {
            let point = point_numbered(&document, number).expect(number);
            assert_eq!(point.layer, "fieldwork", "{number}");
            assert_eq!(point.style, "", "{number}");
            assert_eq!(text_of(point, "string"), "<absent>", "{number}");
        }
        assert!(!document.model().layers.contains("FIELD KERB"));
        assert!(document.model().styles.all() == before.styles);
        assert_eq!(document.survey_jobs()[0].import_options, PLAIN_JOB_OPTIONS);
    }

    // CODES and LINEWORK unsaid, the drawing's customisation decides: its two
    // automation switches, which a customisation file or CUSTOMISE SET auto.*
    // sets and which are both on until one does. A word on the line overrides.
    #[test]
    fn without_the_words_the_documents_automation_switches_decide() {
        let mut document = Document::default();
        install_field_codes(&mut document);
        document.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: true,
        });
        assert_eq!(
            finish_records(&run(&mut document, &import_gnss(""))),
            records(&[
                "coded none reason=off",
                "linework lines=1 unplaced=2 layers=1 styles=1",
            ])
        );
        let uncoded = point_numbered(&document, "R001").expect("R001");
        assert_eq!(uncoded.layer, "fieldwork");

        let mut neither = Document::default();
        install_field_codes(&mut neither);
        neither.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: false,
        });
        assert_eq!(
            finish_records(&run(&mut neither, &import_gnss(""))),
            records(&["coded none reason=off", "linework none reason=off"])
        );
        assert_eq!(neither.survey_jobs()[0].import_options, PLAIN_JOB_OPTIONS);

        // Said on the line, CODES on codes whatever the switch says.
        let mut said = Document::default();
        install_field_codes(&mut said);
        said.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: false,
        });
        assert_eq!(
            finish_records(&run(&mut said, &import_gnss("CODES on"))),
            records(&[
                "coded points=4 matched=4 unmatched_codes=0 layers=1 styles=1",
                "linework none reason=off",
            ])
        );
        let coded = point_numbered(&said, "R001").expect("R001");
        assert_eq!(coded.layer, "FIELD KERB");

        // And LINEWORK on strings whatever ITS switch says, the codes' switch
        // staying as it is: the points are left where the import drew them, and
        // the file's closed string is drawn on its rule's layer.
        let mut strung = Document::default();
        install_field_codes(&mut strung);
        strung.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: false,
        });
        assert_eq!(
            finish_records(&run(&mut strung, &import_gnss("LINEWORK on"))),
            records(&[
                "coded none reason=off",
                "linework lines=1 unplaced=2 layers=1 styles=1",
            ])
        );
        let left = point_numbered(&strung, "R001").expect("R001");
        assert_eq!(left.layer, "fieldwork");
        assert_eq!(left.style, "");
        let lines = lines_drawn(&strung);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].layer, "FIELD KERB");
        assert!(polyline_of(lines[0]).closed);
        assert_eq!(
            strung.survey_jobs()[0].import_options,
            format!("{PLAIN_JOB_OPTIONS}draw-linework=true\n")
        );
    }

    // The promise every earlier test of this verb rests on: a drawing with no
    // survey codes is imported EXACTLY as it was before the verb coded anything -
    // the same entities, the same job, the same lines of the reply - whatever
    // the line asks, and only the two records are added.
    #[test]
    fn with_no_survey_codes_loaded_the_import_is_the_plain_one_and_the_reply_only_gains_its_two_records()
     {
        let mut unsaid = Document::default();
        let mut asked = Document::default();
        let mut refused = Document::default();
        let by_default = run(&mut unsaid, &import_gnss(""));
        let on = run(&mut asked, &import_gnss("CODES on LINEWORK on"));
        let off = run(&mut refused, &import_gnss("CODES off LINEWORK off"));

        let no_codes = records(&[
            "coded none reason=no-survey-codes",
            "linework none reason=no-survey-codes",
        ]);
        assert_eq!(finish_records(&by_default), no_codes);
        assert_eq!(finish_records(&on), no_codes);
        assert_eq!(
            finish_records(&off),
            records(&["coded none reason=off", "linework none reason=off"])
        );
        // Every other line of the three replies is the same line.
        assert_eq!(
            without_finish_records(&by_default),
            without_finish_records(&off)
        );
        assert_eq!(without_finish_records(&on), without_finish_records(&off));

        for document in [&unsaid, &asked] {
            assert_eq!(drawing_of(document), drawing_of(&refused));
            assert_eq!(document.survey_jobs().len(), 1);
            let job = &document.survey_jobs()[0];
            let plain = &refused.survey_jobs()[0];
            // Not stored as a job to finish: a re-adjustment after codes are
            // loaded would code only the points it drew for the first time.
            assert_eq!(job.import_options, PLAIN_JOB_OPTIONS);
            assert_eq!(job.created_entities, plain.created_entities);
            assert_eq!(job.placed_points.len(), plain.placed_points.len());
            let point = point_numbered(document, "R001").expect("R001");
            assert_eq!(text_of(point, "string"), "<absent>");
            assert!(!job.report_text.contains("survey codes"));
        }
    }

    // A step with nothing to go on says which nothing: no rule for the file's
    // codes, no code in the file, no point drawn.
    #[test]
    fn a_step_with_nothing_to_go_on_says_why_and_is_no_failure() {
        // Rules, and none for KB: the one code the file has is named.
        let mut other_codes = Document::default();
        install_json(
            &mut other_codes,
            r#"{"format": "katana-customisation", "version": 1, "name": "other codes",
                "codes": [{"key": "ZZ*", "sets": "feature", "layer": "ELSEWHERE", "draw": "line"}]}"#,
        );
        assert_eq!(
            finish_records(&run(&mut other_codes, &import_gnss(""))),
            records(&[
                "coded none reason=no-rule-matches",
                "unmatched_code text=KB",
                "linework none reason=no-rule-matches",
            ])
        );
        assert_eq!(other_codes.model().entities.size(), 5);
        let point = point_numbered(&other_codes, "R001").expect("R001");
        assert_eq!(point.layer, "fieldwork");
        assert!(!other_codes.model().layers.contains("ELSEWHERE"));

        // rounded_seconds_gsi16.gsi carries no code at all (its two shots and two
        // marks: cli.survey_import_gsi_radiates_the_shots_whose_angles_wrote_
        // sixty_seconds).
        let mut no_codes = Document::default();
        install_field_codes(&mut no_codes);
        let uncoded = run(
            &mut no_codes,
            &format!(
                "SURVEY IMPORT \"{}\"",
                fixture("leica/rounded_seconds_gsi16.gsi")
            ),
        );
        assert_eq!(imported_value(&uncoded, "entities"), "4");
        assert_eq!(
            finish_records(&uncoded),
            records(&[
                "coded none reason=no-codes-in-file",
                "linework none reason=no-codes-in-file",
            ])
        );

        // The traverse with no coordinates places nothing without control.
        let mut no_points = Document::default();
        install_field_codes(&mut no_points);
        let nothing = run(
            &mut no_points,
            &format!("SURVEY IMPORT \"{}\"", bare_traverse()),
        );
        assert_eq!(imported_value(&nothing, "entities"), "0");
        assert_eq!(
            finish_records(&nothing),
            records(&[
                "coded none reason=no-points",
                "linework none reason=no-points"
            ])
        );
    }

    // traverse.sdr, whose points are coded CTRL (CP1 and CP2, keyed in) and TRAV
    // (T1 and T2, observed) and whose format has no string numbers. The fixture
    // makes CTRL a point code and has no rule for TRAV: four points carry a code,
    // two are matched onto FIELD CONTROL in the one style a plain red line makes
    // ("Plain"), one code is unmatched and named, and nothing is a line - two
    // points of a point code and two of a code with no rule.
    #[test]
    fn codes_with_no_rule_are_counted_and_named_and_a_point_code_strings_nothing() {
        let mut document = Document::default();
        install_field_codes(&mut document);
        let reply = run(
            &mut document,
            &format!(
                "SURVEY IMPORT \"{}\" LAYER traverse",
                fixture("sdr/traverse.sdr")
            ),
        );

        assert_eq!(imported_value(&reply, "entities"), "4");
        assert_eq!(
            finish_records(&reply),
            records(&[
                "coded points=4 matched=2 unmatched_codes=1 layers=1 styles=1",
                "unmatched_code text=TRAV",
                "linework lines=0 unplaced=4 layers=0 styles=0",
            ])
        );
        for number in ["CP1", "CP2"] {
            let mark = point_numbered(&document, number).expect(number);
            assert_eq!(mark.layer, "FIELD CONTROL", "{number}");
            assert_eq!(mark.style, "Plain", "{number}");
        }
        for number in ["T1", "T2"] {
            let station = point_numbered(&document, number).expect(number);
            assert_eq!(station.layer, "traverse", "{number}");
            assert_eq!(station.style, "", "{number}");
        }
        assert!(lines_drawn(&document).is_empty());
        // Both are for a person to act on, and are the job's warnings.
        assert!(
            reply.contains("\nreduction_warning text=\"No rule for the code(s): TRAV.\"\n"),
            "{reply}"
        );
        assert!(
            reply.contains(
                "\nreduction_warning text=\"4 point(s) are in no line (no rule for its code and \
                 no control code: 2; its code is a point code: 2).\"\n"
            ),
            "{reply}"
        );
    }

    // setup.fld: CP1 and CP2 entered with no code, 101 and 102 shot as string 01
    // of EB (the third shot of it is the record the reader skips). EB* is a plain
    // line in "blue", a name the fixture's table does not have: the standard
    // names answer after it, and blue is #0000FF.
    #[test]
    fn a_standard_colour_name_is_answered_after_the_customisations_own_table() {
        let mut document = Document::default();
        install_field_codes(&mut document);
        let reply = run(
            &mut document,
            &format!("SURVEY IMPORT \"{}\" LAYER setup", fixture("fld/setup.fld")),
        );

        assert_eq!(imported_value(&reply, "entities"), "4");
        assert_eq!(
            finish_records(&reply),
            records(&[
                "coded points=2 matched=2 unmatched_codes=0 layers=1 styles=1",
                "linework lines=1 unplaced=2 layers=0 styles=0",
            ])
        );
        let style = document
            .model()
            .styles
            .find("Plain")
            .expect("the style Plain");
        assert_eq!(
            style.color,
            Some(Color {
                r: 0,
                g: 0,
                b: 255,
                a: 255
            })
        );

        let first = point_numbered(&document, "101").expect("101");
        let second = point_numbered(&document, "102").expect("102");
        assert_eq!(first.layer, "FIELD EDGE");
        assert_eq!(first.style, "Plain");
        let lines = lines_drawn(&document);
        assert_eq!(lines.len(), 1);
        // Through the two shots where they were drawn, in the order shot.
        assert_eq!(
            polyline_of(lines[0]).vertices,
            vec![position_of(first), position_of(second)]
        );
        assert!(!polyline_of(lines[0]).closed);
        assert_eq!(lines[0].layer, "FIELD EDGE");
        assert_eq!(lines[0].style, "Plain");
    }

    // Where the two records stand: after the reduction's warnings and before the
    // resections', which end the reply as they did before the records existed
    // (cli.survey_import_resection_field_file holds the LIST that follows them).
    #[test]
    fn the_coded_and_linework_records_follow_the_warnings_and_come_before_the_resections() {
        let mut document = Document::default();
        install_field_codes(&mut document);
        let reply = run(
            &mut document,
            &format!("SURVEY IMPORT \"{}\"", fixture("fld/resection.fld")),
        );

        let at = |found: Option<usize>| found.unwrap_or_else(|| panic!("{reply}"));
        let imported = at(reply.find("\nimported job="));
        let reduction = at(reply.find("\nreduction method="));
        let last_warning = at(reply.rfind("\nreduction_warning text="));
        let coded = at(reply.find("\ncoded "));
        let linework = at(reply.find("\nlinework "));
        let resection = at(reply.find("\nresection setup=S1 "));
        assert!(imported < reduction);
        assert!(reduction < last_warning);
        assert!(last_warning < coded);
        assert!(coded < linework);
        assert!(linework < resection);
        // The reply ends with a resection's record.
        assert_eq!(reply.rfind('\n'), reply.rfind("\nresection"), "{reply}");
    }

    // Every refusal of the two words, and the drawing as it was.
    #[test]
    fn codes_and_linework_take_on_or_off_once_each_and_only_on_an_import() {
        let bare = format!("SURVEY IMPORT \"{}\" ", fixture("fld/gnss.fld"));
        let read = format!("SURVEY READ \"{}\" ", fixture("fld/gnss.fld"));
        let refusals: [(String, &str); 8] = [
            (format!("{bare}CODES"), "CODES needs a value"),
            (format!("{bare}LINEWORK"), "LINEWORK needs a value"),
            (
                format!("{bare}CODES maybe"),
                "CODES takes on or off, and 'maybe' is neither",
            ),
            (
                format!("{bare}LINEWORK yes"),
                "LINEWORK takes on or off, and 'yes' is neither",
            ),
            (format!("{bare}CODES on CODES off"), "CODES is given twice"),
            (
                format!("{bare}LINEWORK off LINEWORK off"),
                "LINEWORK is given twice",
            ),
            (
                format!("{read}CODES on"),
                "'CODES' is not an option of SURVEY READ",
            ),
            (
                format!("{read}LINEWORK off"),
                "'LINEWORK' is not an option of SURVEY READ",
            ),
        ];
        let mut failures: Vec<String> = Vec::new();
        for (line, said) in &refusals {
            let mut document = Document::default();
            install_field_codes(&mut document);
            let error = match run_survey_line(&mut document, line) {
                Ok(reply) => {
                    failures.push(format!("{line}: not refused: {reply}"));
                    continue;
                }
                Err(error) => error,
            };
            if error.code != ErrorCode::InvalidArgument {
                failures.push(format!("{line}: {}", error.describe()));
            }
            if !error.message.contains(said) {
                failures.push(format!(
                    "{line}: does not say \"{said}\": {}",
                    error.describe()
                ));
            }
            // The usage names both words.
            if !error.message.contains("[CODES on|off] [LINEWORK on|off]") {
                failures.push(format!("{line}: no usage: {}", error.describe()));
            }
            if !document.survey_jobs().is_empty()
                || document.model().entities.size() != 0
                || document.history().undo_count() != 0
            {
                failures.push(format!("{line}: the drawing changed"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));

        // SET's items end at either word, as at any other option.
        let mut document = Document::default();
        install_field_codes(&mut document);
        let reply = run(
            &mut document,
            &format!("{bare}SET atmospheric=none CODES off"),
        );
        assert!(
            reply.contains("\nsetting key=atmospheric value=none\n"),
            "{reply}"
        );
        let finished = finish_records(&reply);
        assert!(!finished.is_empty());
        assert_eq!(finished[0], "coded none reason=off");

        // SURVEY READ is as it was: it says nothing of codes.
        let mut reader = Document::default();
        install_field_codes(&mut reader);
        assert!(finish_records(&run(&mut reader, &read)).is_empty());
    }

    // layers= and styles= are each record's OWN step's, and are two counts. The
    // one rule here makes a kerb a line on a layer and says nothing of how it
    // looks - no linestyle, no colour, no symbol - so coding it makes a layer and
    // no style (cad's survey_coding: a code whose rules give no appearance keeps
    // its style). With the codes on, the layer is the coding's and the line
    // makes nothing; with CODES off the line makes the layer itself.
    #[test]
    fn each_record_counts_the_layers_and_the_styles_its_own_step_made_and_the_two_are_not_one_number()
     {
        let install_bare_kerb = |document: &mut Document| {
            install_json(
                document,
                r#"{"format": "katana-customisation", "version": 1, "name": "bare kerb",
                    "codes": [{"key": "KB*", "sets": "feature", "layer": "BARE KERB",
                               "draw": "line"}]}"#,
            );
        };

        let mut coded = Document::default();
        install_bare_kerb(&mut coded);
        let before = drawing_of(&coded);
        assert_eq!(
            finish_records(&run(&mut coded, &import_gnss(""))),
            records(&[
                "coded points=4 matched=4 unmatched_codes=0 layers=1 styles=0",
                "linework lines=1 unplaced=2 layers=0 styles=0",
            ])
        );
        assert!(coded.model().layers.contains("BARE KERB"));
        assert!(
            coded.model().styles.all() == before.styles,
            "no style was made"
        );
        let point = point_numbered(&coded, "R002").expect("R002");
        assert_eq!(point.layer, "BARE KERB");
        assert_eq!(point.style, "");
        assert_eq!(lines_drawn(&coded).len(), 1);
        assert_eq!(lines_drawn(&coded)[0].layer, "BARE KERB");
        assert_eq!(lines_drawn(&coded)[0].style, "");

        let mut uncoded = Document::default();
        install_bare_kerb(&mut uncoded);
        assert_eq!(
            finish_records(&run(&mut uncoded, &import_gnss("CODES off"))),
            records(&[
                "coded none reason=off",
                "linework lines=1 unplaced=2 layers=1 styles=0",
            ])
        );
        assert!(uncoded.model().layers.contains("BARE KERB"));
        assert!(uncoded.model().styles.all() == before.styles);
        let left = point_numbered(&uncoded, "R002").expect("R002");
        assert_eq!(left.layer, "fieldwork");
        assert_eq!(lines_drawn(&uncoded).len(), 1);
        assert_eq!(lines_drawn(&uncoded)[0].layer, "BARE KERB");
    }

    // Ten codes with no rule are named and the rest are counted. The file is
    // written here: thirteen entered coordinates of the opcode field file (02,
    // then the blank column, the feature code, no string number, the point id, no
    // name and no comment, then X, Y and Z - docs/survey.md, "The opcode field
    // file (.fld)"), coded ZL back to ZA - twelve codes the fixture has no rule
    // for - and then CTRL, its point code. The codes are named in NAME order,
    // which is the reverse of the file's: ZA to ZJ, and two more.
    #[test]
    fn ten_codes_with_no_rule_are_named_in_name_order_and_the_rest_are_counted() {
        let mut text = String::from("{Version 6.0}\r\n");
        for i in 0_u8..13 {
            let code = if i < 12 {
                format!("Z{}", char::from(b'L' - i))
            } else {
                String::from("CTRL")
            };
            text.push_str(&format!(
                "02\t\t{code}\t\tP{}\t\t\t{}.000\t2000.000\t10.000\r\n",
                i + 1,
                1000 + 10 * u32::from(i)
            ));
        }
        let scratch = ScratchDirectory::new("many-codes");
        let file = scratch.file("many_codes.fld", text.as_bytes());
        let mut document = Document::default();
        install_field_codes(&mut document);
        let reply = run(
            &mut document,
            &format!("SURVEY IMPORT {file} FORMAT opcode-field-file LAYER many"),
        );

        assert_eq!(imported_value(&reply, "entities"), "13");
        // Thirteen points carry a code and one, CTRL, has a rule: its layer and
        // its plain red style. None is a line: twelve have no rule and CTRL is a
        // point code.
        assert_eq!(
            finish_records(&reply),
            records(&[
                "coded points=13 matched=1 unmatched_codes=12 layers=1 styles=1",
                "unmatched_code text=ZA",
                "unmatched_code text=ZB",
                "unmatched_code text=ZC",
                "unmatched_code text=ZD",
                "unmatched_code text=ZE",
                "unmatched_code text=ZF",
                "unmatched_code text=ZG",
                "unmatched_code text=ZH",
                "unmatched_code text=ZI",
                "unmatched_code text=ZJ",
                "unmatched_codes_more=2",
                "linework lines=0 unplaced=13 layers=0 styles=0",
            ])
        );
        assert!(lines_drawn(&document).is_empty());
    }

    // What HELP LINEWORK says of a job imported with LINEWORK off: the verb
    // strings its points BY THEIR CODES - the four shots of KB 1 in point order,
    // one open line - and not as the file strung them, which closed R001 to R003
    // and began the string again at R004. And having drawn that line it does not
    // draw it again: the drawing holds it.
    #[test]
    fn a_job_imported_with_linework_off_is_strung_by_the_verb_by_code_and_only_once() {
        let mut document = Document::default();
        install_field_codes(&mut document);
        run(&mut document, &import_gnss("LINEWORK off"));
        assert!(lines_drawn(&document).is_empty());
        assert_eq!(document.model().entities.size(), 5);

        // The job took the first entity id for its number, so the points are
        // entities 2 to 6 and the line is entity 7. CM1 has no code.
        let mut interpreter = CommandInterpreter::new();
        let first = must(interpreter.run(&mut document, "LINEWORK"));
        assert_eq!(
            first,
            "linework scope=drawing matched=5 considered=5 lines=1 unplaced=1 notes=0\n\
             string name=KB1 key=KB* number=1 points=4 vertices=4 closed=no \
             layer=\"FIELD KERB\" entity=7\n\
             unplaced reason=\"no code\" points=1"
        );
        assert_eq!(lines_drawn(&document).len(), 1);
        let shape = polyline_of(lines_drawn(&document)[0]).clone();
        assert!(!shape.closed);
        assert_eq!(
            shape.vertices,
            vec![
                Point2::new(500010.0, 6200020.0),
                Point2::new(500020.0, 6200020.0),
                Point2::new(500020.0, 6200030.0),
                Point2::new(500040.0, 6200040.0),
            ]
        );

        let second = must(interpreter.run(&mut document, "LINEWORK"));
        assert_eq!(
            second,
            "linework scope=drawing matched=6 considered=5 lines=0 unplaced=1 notes=0\n\
             left_out=4 reason=already-drawn lines=1\n\
             unplaced reason=\"no code\" points=1"
        );
        assert_eq!(lines_drawn(&document).len(), 1);
        assert_eq!(document.model().entities.size(), 6);
        assert_eq!(
            document.history().undo_count(),
            2,
            "the import, and the one run that drew"
        );
    }
}
