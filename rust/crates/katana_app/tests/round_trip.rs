//! Step 3's round-trip gate (docs/rust.md, "The order of the work"): every
//! file of the test data and the samples in a format step 3 ported - the
//! project (storage), DXF, IFC, the .12da archive and the survey field files
//! (surveyio) - is read by the C++ katana_cli and by the Rust katana_corpus,
//! written by both where a writer exists, and each program reads what the
//! other wrote.
//!
//! The corpus is the table CORPUS, one row per file, so that a new data file
//! is a decision rather than an accident: the first test fails for a file of
//! the test data or the samples that has no row, and a row says why a file is
//! left out. What each kind of row runs, in both programs:
//!
//! ```text
//! project   OPEN <copy>, SAVE, SAVE <new>    the copy (migrated, saved in place, its backups)
//!                                            and the new project compared
//! import    IMPORT <file>, SAVE <new>        the new project compared
//! survey    SURVEY READ <file>               the replies compared, word for word
//! ifc       EXPORT <file.ifc> UTILITIES ...  the files compared, then each program
//!                                            imports the other's
//! ```
//!
//! and after a project or an import row:
//!
//! ```text
//! the other's project   OPEN <the other's>, SAVE <new>    compared
//! each writer           OPEN <the C++'s project>, EXPORT <out.dxf | out.12da | out.ifc>
//!                                                         the files compared byte for byte
//! the other's file      IMPORT <the other's out>, SAVE <new>   compared
//! ```
//!
//! Both programs export from the same project - the C++'s - because an IFC's
//! GlobalIds are made from the project's name and the time it was created
//! (ifc::guidNamespaceFor), which two saves never share. Projects are
//! compared by tools/project_dump.py, a dump made with Python's own sqlite3,
//! independent of both programs, which normalises the values two saves of
//! the same content cannot share (the clock's stamps, the writer's version,
//! the backups' names, the line ends of the stored SQL) and nothing else.
//! The files a writer writes are compared byte for byte - the writers are
//! ports, line for line - with the one value an IFC takes from the clock,
//! the time stamp in its header, set aside; and an IFC is judged by
//! tools/check_ifc.py too, where the Python can import IfcOpenShell, both
//! programs' files alike.
//!
//! The C++ katana_cli is the one tools/build_visible.sh built, named by
//! KATANA_CPP_CLI as the number-format oracle is by its variable; without it
//! the gate fails rather than skipping. It runs with
//! KATANA_BUILTIN_CUSTOMISATION=none (the seam of cad/customisation_host.hpp),
//! because the Rust has no customisation yet: a session with the built-in
//! would record its name in every project it saves and resolve an archive's
//! colours by its palette. A difference is reported as the file, the format,
//! what was compared and the first difference.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The checkout's root, which the corpus paths are relative to.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../..");

// ---- the corpus -----------------------------------------------------------------------

/// What both programs do with a file.
#[derive(Clone, Copy)]
enum Read {
    /// A project's database: opened from a copy, saved in place and saved as
    /// a new project; then written by each writer.
    Project,
    /// A file IMPORT reads into a new drawing, saved as a project; then
    /// written by each writer.
    Import,
    /// A survey field file, read by SURVEY READ. The formats have no writer,
    /// and SURVEY IMPORT - the reduction, the coding and the drawing of the
    /// job - is the cad layer's, ported in step 4.
    Survey,
    /// An IFC export whose input the line's files are - a schedule, its
    /// delivery schema, a project's rules - from a new drawing or from a
    /// project of the corpus (`project`, a row of its own). `arguments` is
    /// the rest of the line after the file written, each {path} a file of
    /// the corpus.
    IfcExport {
        project: Option<&'static str>,
        arguments: &'static str,
    },
    /// Not in the gate, and why.
    Out(&'static str),
}

/// What the gate expects of one writer, given a row's drawing.
#[derive(Clone, Copy)]
enum Write {
    /// Both programs write it: the files are compared, and each program
    /// reads the other's.
    Compare,
    /// The Rust refuses it with these words in its error - what the C++
    /// writes needs a layer not ported yet - and both read the C++'s file.
    RustRefuses(&'static str),
}

#[derive(Clone, Copy)]
struct Row {
    /// From the checkout's root; for an Out row, a folder ending in '/' to
    /// leave out everything in it.
    path: &'static str,
    /// For the report.
    format: &'static str,
    read: Read,
    dxf: Write,
    archive: Write,
    ifc: Write,
    /// Metadata keys the row's project comparisons leave out; the row says why.
    ignore_metadata: &'static [&'static str],
}

const fn row(path: &'static str, format: &'static str, read: Read) -> Row {
    Row {
        path,
        format,
        read,
        dxf: Write::Compare,
        archive: Write::Compare,
        ifc: Write::Compare,
        ignore_metadata: &[],
    }
}

const fn project(path: &'static str) -> Row {
    row(path, "project", Read::Project)
}

const fn import(path: &'static str, format: &'static str) -> Row {
    row(path, format, Read::Import)
}

const fn survey(path: &'static str, format: &'static str) -> Row {
    row(path, format, Read::Survey)
}

const fn ifc_export(
    path: &'static str,
    project: Option<&'static str>,
    arguments: &'static str,
) -> Row {
    row(path, "ifc", Read::IfcExport { project, arguments })
}

const fn out(path: &'static str, format: &'static str, why: &'static str) -> Row {
    row(path, format, Read::Out(why))
}

impl Row {
    const fn dxf_refused(self, words: &'static str) -> Row {
        Row {
            dxf: Write::RustRefuses(words),
            ..self
        }
    }

    const fn ignoring(self, keys: &'static [&'static str]) -> Row {
        Row {
            ignore_metadata: keys,
            ..self
        }
    }
}

/// The words of katana_corpus's refusal of what needs the cad layer.
const NEEDS_CAD: &str = "ported in step 4";

const LEGACY: &str = "a file of the retired style library and survey code file formats, whose legacy readers are not ported until the owner decides (docs/rust.md, \"Decisions waiting on the owner\")";
const SCRIPT: &str =
    "a katana_cli script of drawing verbs, the cad layer's (step 4), run by step 6's program";
const SETTINGS: &str = "reduction settings for SURVEY IMPORT, whose reduction, coding and drawing of the job are the cad layer's (step 4)";
const GIT: &str = "git's settings for the folder, not data";

const CORPUS: &[Row] = &[
    // ---- projects (storage) --------------------------------------------------------------
    // The sample project, at schema 12: opened, it is backed up and migrated.
    project("samples/site_plan/project.db"),
    // Its backups, each the sample as an earlier schema had it (4, 4, 6, 6, 7, 7
    // and 8), opened as a project: the migrations from each.
    project("samples/site_plan/backups/project-20260920T172742-596.db"),
    project("samples/site_plan/backups/project-20260920T185953-849.db"),
    project("samples/site_plan/backups/project-20260920T185953-896.db"),
    project("samples/site_plan/backups/project-20260921T012821-244.db"),
    project("samples/site_plan/backups/project-20260921T012821-262.db"),
    project("samples/site_plan/backups/project-20260921T014629-888.db"),
    project("samples/site_plan/backups/project-20260921T014629-905.db"),
    // A project main's katana_cli wrote before survey jobs, at schema 9.
    project("tests/storage/fixtures/schema9_main/project.db"),
    // ---- DXF -----------------------------------------------------------------------------
    import("tests/dxf/data/r12_survey.dxf", "dxf"),
    // Its DIMENSION is read as its picture block's lines and text, so nothing
    // in the drawing is the cad layer's to draw.
    import("tests/dxf/data/r2000_site.dxf", "dxf"),
    out("tests/dxf/data/.gitattributes", "git", GIT),
    // ---- the archive ---------------------------------------------------------------------
    // Its point cloud ("LiDAR Patch") is a reference layer of the C++ session,
    // as las_point_cloud.12da's is below: interop's reference data (step 5),
    // which the Rust has not yet, so reference_layers is left out here too.
    import("tests/archive12d/data/all_geometries.12da", "12da").ignoring(&["reference_layers"]),
    out(
        "tests/archive12d/data/all_geometries.12daz",
        "12daz",
        "a zip container, which the gis layer opens (step 5); the .12da it holds is the row above's file",
    ),
    out(
        "tests/archive12d/data/customisation/test_linestyles.4d",
        "style library",
        LEGACY,
    ),
    out(
        "tests/archive12d/data/customisation/test_survey.mapfile",
        "survey code file",
        LEGACY,
    ),
    out(
        "tests/archive12d/data/customisation/test_symbols.4d",
        "style library",
        LEGACY,
    ),
    // Its two drafting leaders are drawn into a DXF by the cad layer's export
    // annotation (cad/annotation/export_annotation.cpp), so the Rust writes no
    // DXF of it yet, and both read the C++'s.
    import("tests/archive12d/data/drafting_leaders.12da", "12da").dxf_refused(NEEDS_CAD),
    out(
        "tests/archive12d/data/drafting_palette.customisation.json",
        "customisation",
        "a Katana customisation: the entity layer's format (step 2), which only the cad layer's CUSTOMISE puts in a drawing (step 4)",
    ),
    import("tests/archive12d/data/drainage_strings.12da", "12da"),
    // Its LAS cloud is a reference layer of the C++ session (interop's
    // reference data, step 5), which a SAVE records in the project's
    // reference_layers; the Rust has no reference data yet, so that one
    // record is left out of this row's comparisons.
    import("tests/archive12d/data/las_point_cloud.12da", "12da").ignoring(&["reference_layers"]),
    import("tests/archive12d/data/multiple_tins.12da", "12da"),
    import("tests/archive12d/data/super_tin.12da", "12da"),
    import("tests/archive12d/data/symbols.12da", "12da"),
    // ---- IFC -----------------------------------------------------------------------------
    // The corpus holds no .ifc: every row above writes one and reads the
    // other program's back, and these write the files an IFC export takes.
    ifc_export(
        "samples/utilities/schedule.csv",
        None,
        "UTILITIES {samples/utilities/schedule.csv}",
    ),
    // The schedule written to a delivery schema, first by its own columns...
    ifc_export(
        "samples/utilities/schedule_tfnsw.csv",
        None,
        "UTILITIES {samples/utilities/schedule_tfnsw.csv}",
    ),
    // ... then with the schema, and nothing of the drawing.
    ifc_export(
        "tests/ifc/data/delivery_schema.csv",
        None,
        "UTILITIES {samples/utilities/schedule_tfnsw.csv} SCHEMA {tests/ifc/data/delivery_schema.csv} NODRAWING",
    ),
    // A project's own classification rules, tried before the defaults.
    ifc_export(
        "samples/ifc/classification_rules.csv",
        Some("samples/site_plan/project.db"),
        "RULES {samples/ifc/classification_rules.csv}",
    ),
    out("samples/ifc/scenario.txt", "script", SCRIPT),
    out("samples/site_plan.kcs", "script", SCRIPT),
    out(
        "samples/utilities/design.csv",
        "csv",
        "the design of UTILITY CLEARANCE, a verb of the cad layer (step 4)",
    ),
    out(
        "samples/utilities/located_services.geojson",
        "geojson",
        "vector data, read through GDAL by the gis layer (step 5)",
    ),
    // ---- the gis layer's and the window's --------------------------------------------------
    out(
        "samples/gis/parcels.geojson",
        "geojson",
        "vector data, read through GDAL by the gis layer (step 5)",
    ),
    out(
        "samples/gis/survey_scan.las",
        "las",
        "a point cloud, read through PDAL by the pointcloud layer (step 5)",
    ),
    out(
        "samples/gis/terrain.asc",
        "asc",
        "a raster, read through GDAL by the gis layer (step 5)",
    ),
    out(
        "samples/gis/terrain.prj",
        "prj",
        "the raster's coordinate system, read with it by the gis layer (step 5)",
    ),
    out(
        "tests/geo/data/",
        "gis",
        "the geoprocessing suite's rasters and vectors, the gis layer's (step 5)",
    ),
    out(
        "tests/interop/data/online/",
        "online",
        "replies of online catalogues and services, the interop layer's (step 5)",
    ),
    out(
        "tests/qt_widgets/data/",
        "script",
        "the window's scripts (step 7)",
    ),
    out(
        "tests/app/data/survey_settings/hold_cp1.txt",
        "settings",
        SETTINGS,
    ),
    out(
        "tests/app/data/survey_settings/newer_version.txt",
        "settings",
        SETTINGS,
    ),
    // ---- survey field files (surveyio): read only --------------------------------------------
    survey("samples/Survey_File/Example field file1.fld", "fld"),
    survey("samples/Survey_File/Example field file2.fld", "fld"),
    survey("samples/Survey_File/Example field file3.fld", "fld"),
    // Prose about the field file format: whatever detection makes of it, both
    // programs must make the same.
    survey(
        "samples/Survey_File/Structure of the 12dfield. or fld .txt",
        "txt",
    ),
    survey("samples/Survey_File/example sdr.sdr", "sdr"),
    // GSI-16 records under a .txt name.
    survey("samples/Survey_File/example.txt", "txt"),
    out("tests/surveyio/data/fld/.gitattributes", "git", GIT),
    survey("tests/surveyio/data/fld/free_station.fld", "fld"),
    survey("tests/surveyio/data/fld/gnss.fld", "fld"),
    survey("tests/surveyio/data/fld/resection.fld", "fld"),
    survey("tests/surveyio/data/fld/rtk_setup.fld", "fld"),
    survey("tests/surveyio/data/fld/setup.fld", "fld"),
    survey("tests/surveyio/data/fld/two_backsights.fld", "fld"),
    survey("tests/surveyio/data/gts/setup.gt7", "gt7"),
    survey("tests/surveyio/data/leica/codes_after_gsi8.gsi", "gsi"),
    survey("tests/surveyio/data/leica/codes_before_gsi8.gsi", "gsi"),
    survey("tests/surveyio/data/leica/coordinates_gsi8.gsi", "gsi"),
    survey("tests/surveyio/data/leica/damaged_gsi8.gsi", "gsi"),
    survey(
        "tests/surveyio/data/leica/derived_distances_gsi8.gsi",
        "gsi",
    ),
    survey("tests/surveyio/data/leica/feet_mil_gsi8.gsi", "gsi"),
    survey(
        "tests/surveyio/data/leica/negative_vertical_gsi8.gsi",
        "gsi",
    ),
    survey("tests/surveyio/data/leica/rounded_seconds_gsi16.gsi", "gsi"),
    survey("tests/surveyio/data/leica/tps_gsi16.gsi", "gsi"),
    survey("tests/surveyio/data/leica/tps_gsi8.gsi", "gsi"),
    // A DBX job is a folder of files that name each other: each read alone,
    // with its folder for the others.
    survey("tests/surveyio/data/leica/dbx/JOB1.X01", "dbx"),
    survey("tests/surveyio/data/leica/dbx/JOB1.XCF", "dbx"),
    // RINEX observations, compact and plain, with their navigation files
    // beside them, and the navigation files read alone.
    survey(
        "tests/surveyio/data/rinex/BASE00AUS_R_20242561200_01H_30S_MO.crx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/BASE00AUS_R_20242561200_01H_30S_MO.rnx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/BASE00AUS_R_20242561200_01H_MN.rnx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/ROVR00AUS_R_20242561100_01H_01S_GO.crx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/ROVR00AUS_R_20242561100_01H_01S_GO.rnx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/TEST00AUS_R_20242561000_01H_30S_MO.crx",
        "rinex",
    ),
    survey(
        "tests/surveyio/data/rinex/TEST00AUS_R_20242561000_01H_30S_MO.rnx",
        "rinex",
    ),
    survey("tests/surveyio/data/rinex/rinex305_example1.crx", "rinex"),
    survey("tests/surveyio/data/rinex/rinex305_example1.rnx", "rinex"),
    survey("tests/surveyio/data/rinex/test2560.24d", "rinex"),
    survey("tests/surveyio/data/rinex/test2560.24n", "rinex"),
    survey("tests/surveyio/data/rinex/test2560.24o", "rinex"),
    survey("tests/surveyio/data/rw5/feet_grads.rw5", "rw5"),
    survey("tests/surveyio/data/rw5/gnss.rw5", "rw5"),
    survey("tests/surveyio/data/rw5/journal.rw5", "rw5"),
    survey("tests/surveyio/data/rw5/setup_metres.rw5", "rw5"),
    survey("tests/surveyio/data/rw5/survce250_us_feet.rw5", "rw5"),
    survey("tests/surveyio/data/sdr/traverse.sdr", "sdr"),
    survey(
        "tests/surveyio/data/sdr/traverse_without_coordinates.sdr",
        "sdr",
    ),
    // Coordinate files: SURVEY READ refuses one - it is read through its
    // column layout - and both programs must refuse it alike.
    survey("tests/surveyio/data/survey_points_headerless.txt", "points"),
    survey(
        "tests/surveyio/data/survey_points_headerless_penz.txt",
        "points",
    ),
    survey("tests/surveyio/data/survey_points_pnezd.csv", "points"),
    survey(
        "tests/surveyio/data/survey_points_pnezd_utm30s.csv",
        "points",
    ),
    survey("tests/surveyio/data/trimble_dc/synthetic_job.dc", "dc"),
    survey("tests/surveyio/data/trimble_jxl/one_setup.jxl", "jxl"),
    survey("tests/surveyio/data/trimble_jxl/tps_gnss_job.jxl", "jxl"),
];

/// The folders whose every file is the corpus's: the test data of each
/// suite, the samples and the storage fixtures.
fn corpus_folders(root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![root.join("samples"), root.join("tests/storage/fixtures")];
    let suites = std::fs::read_dir(root.join("tests")).expect("the checkout's tests/ folder");
    for suite in suites {
        let data = suite.expect("an entry of tests/").path().join("data");
        if data.is_dir() {
            folders.push(data);
        }
    }
    folders
}

fn files_under(folder: &Path, into: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(folder)
        .unwrap_or_else(|error| panic!("{} cannot be listed: {error}", folder.display()));
    for entry in entries {
        let path = entry.expect("a folder's entry").path();
        if path.is_dir() {
            files_under(&path, into);
        } else {
            into.push(path);
        }
    }
}

/// A path under the checkout as the table spells it.
fn relative(root: &Path, path: &Path) -> String {
    let inside = path.strip_prefix(root).unwrap_or(path);
    inside
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// git ignores samples/*/backups/ (.gitignore): running the program on a
/// sample writes a backup there, and such a backup is no corpus file. The
/// backups git tracks have rows of their own.
fn is_written_by_a_run(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    parts.len() == 4 && parts[0] == "samples" && parts[2] == "backups"
}

/// The {path}s an IFC export row's line names.
fn named_files(arguments: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = arguments;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        found.push(&rest[open + 1..open + close]);
        rest = &rest[open + close + 1..];
    }
    found
}

fn checkout_root() -> PathBuf {
    std::fs::canonicalize(ROOT).expect("the checkout's root")
}

#[test]
fn the_corpus_table_has_a_row_for_every_file_of_the_test_data_and_the_samples() {
    let root = checkout_root();
    let mut files = Vec::new();
    for folder in corpus_folders(&root) {
        files_under(&folder, &mut files);
    }
    let mut problems = Vec::new();
    for file in &files {
        let name = relative(&root, file);
        let listed = CORPUS
            .iter()
            .any(|row| row.path == name || (row.path.ends_with('/') && name.starts_with(row.path)));
        if !listed && !is_written_by_a_run(&name) {
            problems.push(format!(
                "{name} has no row: add one that reads it, or says why it is left out"
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for row in CORPUS {
        if !seen.insert(row.path) {
            problems.push(format!("{} has two rows", row.path));
        }
        if !root.join(row.path).exists() {
            problems.push(format!("{} has a row and is not there", row.path));
        }
        if let Read::Out(why) = row.read
            && why.trim().is_empty()
        {
            problems.push(format!("{} is left out without saying why", row.path));
        }
        if let Read::IfcExport { project, arguments } = row.read {
            for named in named_files(arguments).into_iter().chain(project) {
                if !CORPUS
                    .iter()
                    .any(|other| other.path == named && !matches!(other.read, Read::Out(_)))
                {
                    problems.push(format!(
                        "{}'s export names {named}, which is no file of the corpus read by a row of its own",
                        row.path
                    ));
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "the corpus table is out of step with the corpus ({} files):\n{}",
        files.len(),
        problems.join("\n")
    );
}

// ---- the programs ---------------------------------------------------------------------

/// What a program did: whether it succeeded, and what it wrote.
struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Run {
    /// The line saying why it failed.
    fn error(&self) -> String {
        self.stderr
            .lines()
            .find(|line| line.starts_with("error: "))
            .map_or_else(
                || format!("(no error line; standard error: {:?})", self.stderr.trim()),
                String::from,
            )
    }
}

fn run(command: &mut Command) -> Run {
    match command.output() {
        Ok(output) => Run {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(error) => Run {
            ok: false,
            stdout: String::new(),
            stderr: format!("error: the program could not be started: {error}"),
        },
    }
}

/// A Rust run's steps.
#[derive(Default)]
struct Steps(Vec<OsString>);

impl Steps {
    fn word(mut self, word: &str) -> Self {
        self.0.push(OsString::from(word));
        self
    }

    fn path(mut self, path: &Path) -> Self {
        self.0.push(path.as_os_str().to_owned());
        self
    }
}

/// A katana_cli line of a verb and a path, quoted.
fn line(verb: &str, path: &Path) -> String {
    format!("{verb} \"{}\"", path.display())
}

struct Gate {
    root: PathBuf,
    cpp: PathBuf,
    rust: PathBuf,
    python: OsString,
    /// The C++'s KATANA_VERSION: the CMake project's version.
    version: String,
    ifcopenshell: bool,
    scratch: PathBuf,
}

impl Gate {
    fn cpp(&self, lines: &[String]) -> Run {
        let mut command = Command::new(&self.cpp);
        for each in lines {
            command.arg("-c").arg(each);
        }
        command
            .env("KATANA_BUILTIN_CUSTOMISATION", "none")
            .env_remove("KATANA_CUSTOMISATION")
            .current_dir(&self.scratch);
        run(&mut command)
    }

    fn rust(&self, steps: Steps) -> Run {
        let mut command = Command::new(&self.rust);
        command
            .arg("--application-version")
            .arg(&self.version)
            .args(steps.0)
            .current_dir(&self.scratch);
        run(&mut command)
    }

    fn tool(&self, name: &str) -> PathBuf {
        self.root.join("tools").join(name)
    }

    fn corpus(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    /// tools/project_dump.py's dump of a project.
    fn dump(&self, project: &Path, ignore: &[&str]) -> Result<String, String> {
        let mut command = Command::new(&self.python);
        command.arg(self.tool("project_dump.py")).arg(project);
        for key in ignore {
            command.arg("--ignore-metadata").arg(key);
        }
        let ran = run(&mut command);
        if ran.ok {
            Ok(ran.stdout)
        } else {
            Err(format!(
                "tools/project_dump.py fails on {}: {}",
                project.display(),
                ran.stderr.trim()
            ))
        }
    }

    /// tools/check_ifc.py's judgement of a file: its status and its words,
    /// the file's own path taken out of them.
    fn judge_ifc(&self, file: &Path) -> (bool, String) {
        let ran = run(Command::new(&self.python)
            .arg(self.tool("check_ifc.py"))
            .arg(file));
        let named = format!("{}", file.display());
        (ran.ok, ran.stdout.replace(named.as_str(), "<file>"))
    }

    /// The {path}s of an IFC export row's line, each the corpus file, quoted.
    fn with_corpus_paths(&self, arguments: &str) -> String {
        let mut text = String::from(arguments);
        for named in named_files(arguments) {
            text = text.replace(
                &format!("{{{named}}}"),
                &format!("\"{}\"", self.corpus(named).display()),
            );
        }
        text
    }
}

/// The CMake project's version (the root CMakeLists.txt), which the C++ builds
/// into katana_cli as KATANA_VERSION and an IFC names as the program that
/// wrote it.
fn katana_version(root: &Path) -> String {
    let text = std::fs::read_to_string(root.join("CMakeLists.txt"))
        .expect("the checkout's CMakeLists.txt");
    let (_, after) = text
        .split_once("project(Katana VERSION ")
        .expect("CMakeLists.txt names Katana's version in its project()");
    after
        .split_whitespace()
        .next()
        .expect("a version after VERSION")
        .to_owned()
}

/// A copy of a project's database as the project.db of `directory`.
fn place_project(database: &Path, directory: &Path) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .and_then(|()| std::fs::copy(database, directory.join("project.db")))
        .map(|_| ())
        .map_err(|error| {
            format!(
                "{} cannot be copied to {}: {error}",
                database.display(),
                directory.display()
            )
        })
}

// ---- comparing ------------------------------------------------------------------------

/// What one row found, each a line naming the file, its format, what was
/// compared and the first difference.
struct Findings<'a> {
    row: &'a Row,
    list: Vec<String>,
}

impl Findings<'_> {
    fn add(&mut self, what: &str, difference: &str) {
        self.list.push(format!(
            "{} ({}): {what}: {difference}",
            self.row.path, self.row.format
        ));
    }
}

fn shown(line: Option<&str>) -> String {
    match line {
        None => String::from("(the end)"),
        Some(line) => {
            let cut: String = line.chars().take(300).collect();
            format!("{cut:?}")
        }
    }
}

/// The first line two texts differ at; None when they are the same.
fn first_difference(cpp: &str, rust: &str) -> Option<String> {
    if cpp == rust {
        return None;
    }
    let mut cpp_lines = cpp.lines();
    let mut rust_lines = rust.lines();
    let mut number = 1;
    loop {
        match (cpp_lines.next(), rust_lines.next()) {
            (Some(a), Some(b)) if a == b => number += 1,
            (None, None) => {
                return Some(format!(
                    "the same lines, ended differently ({} and {} bytes)",
                    cpp.len(),
                    rust.len()
                ));
            }
            (a, b) => {
                return Some(format!(
                    "line {number}: the C++ has {}, the Rust {}",
                    shown(a),
                    shown(b)
                ));
            }
        }
    }
}

/// Whether both runs succeeded. Both failing alike - the same error line - is
/// an answer, not a difference; anything else is a finding.
fn both_succeeded(findings: &mut Findings<'_>, what: &str, cpp: &Run, rust: &Run) -> bool {
    match (cpp.ok, rust.ok) {
        (true, true) => true,
        (false, false) => {
            if cpp.error() != rust.error() {
                findings.add(
                    what,
                    &format!(
                        "both fail, differently: the C++ {:?}, the Rust {:?}",
                        cpp.error(),
                        rust.error()
                    ),
                );
            }
            false
        }
        (true, false) => {
            findings.add(
                what,
                &format!("the C++ succeeds and the Rust fails: {}", rust.error()),
            );
            false
        }
        (false, true) => {
            findings.add(
                what,
                &format!("the Rust succeeds and the C++ fails: {}", cpp.error()),
            );
            false
        }
    }
}

fn same_project(gate: &Gate, findings: &mut Findings<'_>, what: &str, cpp: &Path, rust: &Path) {
    let ignore = findings.row.ignore_metadata;
    match (gate.dump(cpp, ignore), gate.dump(rust, ignore)) {
        (Ok(a), Ok(b)) => {
            if let Some(difference) = first_difference(&a, &b) {
                findings.add(what, &difference);
            }
        }
        (Err(why), _) | (_, Err(why)) => findings.add(what, &why),
    }
}

/// An IFC header's time stamp - FILE_NAME's second value, which each program
/// takes from its own clock (ifc_verbs.cpp: headerTimestamp of now) - set to
/// zeros; any other file as it is.
fn without_header_time(bytes: &[u8]) -> Vec<u8> {
    const MARKER: &[u8] = b"FILE_NAME('";
    const STAMP: &[u8] = b"0000-00-00T00:00:00";
    let mut out = bytes.to_vec();
    let Some(start) = bytes
        .windows(MARKER.len())
        .position(|window| window == MARKER)
    else {
        return out;
    };
    // Past the file's name, a STEP string in which '' is one quote.
    let mut at = start + MARKER.len();
    while at < bytes.len() {
        if bytes[at] == b'\'' {
            if bytes.get(at + 1) == Some(&b'\'') {
                at += 2;
                continue;
            }
            break;
        }
        at += 1;
    }
    let stamp = at + 3;
    if bytes.get(at..stamp) == Some(b"','".as_slice())
        && bytes.get(stamp + STAMP.len()) == Some(&b'\'')
    {
        out[stamp..stamp + STAMP.len()].copy_from_slice(STAMP);
    }
    out
}

/// A file's text for the report: UTF-16 little-endian behind its byte order
/// mark (a .12da), else UTF-8.
fn text_of_file(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xff\xfe") {
        let (pairs, _) = rest.as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn same_file(findings: &mut Findings<'_>, what: &str, cpp: &Path, rust: &Path) {
    let (Ok(a), Ok(b)) = (std::fs::read(cpp), std::fs::read(rust)) else {
        findings.add(what, "a file to compare cannot be read");
        return;
    };
    let (a, b) = (without_header_time(&a), without_header_time(&b));
    if a != b {
        let difference = first_difference(&text_of_file(&a), &text_of_file(&b))
            .unwrap_or_else(|| String::from("the bytes differ where their text does not"));
        findings.add(what, &difference);
    }
}

/// tools/check_ifc.py on both files, where IfcOpenShell is importable: the
/// two must be judged alike - not necessarily valid, which is the C++ suite's
/// to hold, but the same.
fn same_ifc_judgement(gate: &Gate, findings: &mut Findings<'_>, cpp: &Path, rust: &Path) {
    if !gate.ifcopenshell {
        return;
    }
    let (cpp_ok, cpp_words) = gate.judge_ifc(cpp);
    let (rust_ok, rust_words) = gate.judge_ifc(rust);
    if cpp_ok != rust_ok || cpp_words != rust_words {
        let difference = first_difference(&cpp_words, &rust_words)
            .unwrap_or_else(|| format!("the C++'s passes: {cpp_ok}, the Rust's: {rust_ok}"));
        findings.add(
            "tools/check_ifc.py's judgement of the two .ifc",
            &difference,
        );
    }
}

// ---- the rows -------------------------------------------------------------------------

/// Each program imports a file into a new drawing and saves it as a project:
/// the C++ `cpp_reads`, the Rust `rust_reads`.
fn each_reads(
    gate: &Gate,
    findings: &mut Findings<'_>,
    what: &str,
    folder: &Path,
    cpp_reads: &Path,
    rust_reads: &Path,
) {
    let (cpp_saved, rust_saved) = (folder.join("cpp/read"), folder.join("rust/read"));
    let cpp = gate.cpp(&[line("IMPORT", cpp_reads), line("SAVE", &cpp_saved)]);
    let rust = gate.rust(
        Steps::default()
            .word("import")
            .path(rust_reads)
            .word("save-as")
            .path(&rust_saved),
    );
    if both_succeeded(findings, &format!("IMPORT of {what}"), &cpp, &rust) {
        same_project(
            gate,
            findings,
            &format!("the project each program saves of {what}"),
            &cpp_saved,
            &rust_saved,
        );
    }
}

/// One writer, from the C++'s project, which both programs open; then each
/// reads the other's file, or both the C++'s where the Rust wrote none.
fn check_writer(
    gate: &Gate,
    findings: &mut Findings<'_>,
    work: &Path,
    extension: &str,
    expected: Write,
    project: &Path,
) {
    let folder = work.join(extension);
    let (cpp_source, rust_source) = (folder.join("cpp/source"), folder.join("rust/source"));
    let file = format!("out.{extension}");
    let (cpp_file, rust_file) = (
        folder.join("cpp").join(file.as_str()),
        folder.join("rust").join(file.as_str()),
    );
    let database = project.join("project.db");
    if let Err(why) =
        place_project(&database, &cpp_source).and_then(|()| place_project(&database, &rust_source))
    {
        findings.add("the copies", &why);
        return;
    }
    let what = format!("EXPORT .{extension}");
    let cpp = gate.cpp(&[line("OPEN", &cpp_source), line("EXPORT", &cpp_file)]);
    let rust = gate.rust(
        Steps::default()
            .word("open")
            .path(&rust_source)
            .word("export")
            .path(&rust_file),
    );
    let rust_wrote = match expected {
        Write::Compare => {
            if !both_succeeded(findings, &what, &cpp, &rust) {
                return;
            }
            same_file(
                findings,
                &format!("the .{extension} both write"),
                &cpp_file,
                &rust_file,
            );
            if extension == "ifc" {
                same_ifc_judgement(gate, findings, &cpp_file, &rust_file);
            }
            true
        }
        Write::RustRefuses(words) => {
            if !cpp.ok {
                findings.add(&what, &format!("the C++ fails: {}", cpp.error()));
                return;
            }
            if rust.ok {
                findings.add(
                    &what,
                    &format!(
                        "the row expects the Rust to refuse ({words}), and it wrote the file: the row is out of date"
                    ),
                );
            } else if !rust.error().contains(words) {
                findings.add(
                    &what,
                    &format!(
                        "the Rust fails otherwise than the row expects ({words}): {}",
                        rust.error()
                    ),
                );
                return;
            }
            rust.ok
        }
    };
    let cpp_reads = if rust_wrote { &rust_file } else { &cpp_file };
    let other = if rust_wrote {
        format!("the other's .{extension}")
    } else {
        format!("the C++'s .{extension}")
    };
    each_reads(gate, findings, &other, &folder, cpp_reads, &cpp_file);
}

/// After a project or an import row: each program opens the other's project
/// and saves it as a new one, then each writer.
fn check_writers(
    gate: &Gate,
    findings: &mut Findings<'_>,
    work: &Path,
    cpp_project: &Path,
    rust_project: &Path,
) {
    let (cpp_reads, rust_reads) = (work.join("cpp/reads-rust"), work.join("rust/reads-cpp"));
    match place_project(&rust_project.join("project.db"), &cpp_reads.join("opened"))
        .and_then(|()| place_project(&cpp_project.join("project.db"), &rust_reads.join("opened")))
    {
        Err(why) => findings.add("the copies", &why),
        Ok(()) => {
            let cpp = gate.cpp(&[
                line("OPEN", &cpp_reads.join("opened")),
                line("SAVE", &cpp_reads.join("saved")),
            ]);
            let rust = gate.rust(
                Steps::default()
                    .word("open")
                    .path(&rust_reads.join("opened"))
                    .word("save-as")
                    .path(&rust_reads.join("saved")),
            );
            if both_succeeded(
                findings,
                "OPEN of the other's project and SAVE <new>",
                &cpp,
                &rust,
            ) {
                same_project(
                    gate,
                    findings,
                    "the project each program saves of the other's",
                    &cpp_reads.join("saved"),
                    &rust_reads.join("saved"),
                );
            }
        }
    }
    let row = findings.row;
    for (extension, expected) in [("dxf", row.dxf), ("12da", row.archive), ("ifc", row.ifc)] {
        check_writer(gate, findings, work, extension, expected, cpp_project);
    }
}

fn check_project(gate: &Gate, findings: &mut Findings<'_>, work: &Path) {
    let database = gate.corpus(findings.row.path);
    let (cpp_opened, rust_opened) = (work.join("cpp/opened"), work.join("rust/opened"));
    let (cpp_saved, rust_saved) = (work.join("cpp/saved"), work.join("rust/saved"));
    if let Err(why) =
        place_project(&database, &cpp_opened).and_then(|()| place_project(&database, &rust_opened))
    {
        findings.add("the copies", &why);
        return;
    }
    let cpp = gate.cpp(&[
        line("OPEN", &cpp_opened),
        String::from("SAVE"),
        line("SAVE", &cpp_saved),
    ]);
    let rust = gate.rust(
        Steps::default()
            .word("open")
            .path(&rust_opened)
            .word("save")
            .word("save-as")
            .path(&rust_saved),
    );
    if !both_succeeded(findings, "OPEN, SAVE and SAVE <new>", &cpp, &rust) {
        return;
    }
    same_project(
        gate,
        findings,
        "the project opened (migrated where it is older) and saved in place, with its backups",
        &cpp_opened,
        &rust_opened,
    );
    same_project(
        gate,
        findings,
        "the project saved as a new one",
        &cpp_saved,
        &rust_saved,
    );
    check_writers(gate, findings, work, &cpp_saved, &rust_saved);
}

fn check_import(gate: &Gate, findings: &mut Findings<'_>, work: &Path) {
    let file = gate.corpus(findings.row.path);
    let (cpp_saved, rust_saved) = (work.join("cpp/saved"), work.join("rust/saved"));
    if let Err(error) = std::fs::create_dir_all(work) {
        findings.add("the work folder", &format!("{error}"));
        return;
    }
    let cpp = gate.cpp(&[line("IMPORT", &file), line("SAVE", &cpp_saved)]);
    let rust = gate.rust(
        Steps::default()
            .word("import")
            .path(&file)
            .word("save-as")
            .path(&rust_saved),
    );
    if !both_succeeded(findings, "IMPORT and SAVE <new>", &cpp, &rust) {
        return;
    }
    same_project(
        gate,
        findings,
        "the file read and saved as a project",
        &cpp_saved,
        &rust_saved,
    );
    check_writers(gate, findings, work, &cpp_saved, &rust_saved);
}

fn check_survey(gate: &Gate, findings: &mut Findings<'_>) {
    let file = gate.corpus(findings.row.path);
    let cpp = gate.cpp(&[line("SURVEY READ", &file)]);
    let rust = gate.rust(Steps::default().word("survey").path(&file));
    if both_succeeded(findings, "SURVEY READ", &cpp, &rust)
        && let Some(difference) = first_difference(&cpp.stdout, &rust.stdout)
    {
        findings.add("SURVEY READ's reply", &difference);
    }
}

fn check_ifc_export(
    gate: &Gate,
    findings: &mut Findings<'_>,
    work: &Path,
    project: Option<&str>,
    arguments: &str,
) {
    let folder = work.join("ifc");
    let (cpp_file, rust_file) = (folder.join("cpp/out.ifc"), folder.join("rust/out.ifc"));
    let mut cpp_lines = Vec::new();
    let mut rust_steps = Steps::default();
    if let Some(project) = project {
        let database = gate.corpus(project);
        let (cpp_source, rust_source) = (folder.join("cpp/source"), folder.join("rust/source"));
        if let Err(why) = place_project(&database, &cpp_source)
            .and_then(|()| place_project(&database, &rust_source))
        {
            findings.add("the copies", &why);
            return;
        }
        cpp_lines.push(line("OPEN", &cpp_source));
        rust_steps = rust_steps.word("open").path(&rust_source);
    }
    for side in ["cpp", "rust"] {
        if let Err(error) = std::fs::create_dir_all(folder.join(side)) {
            findings.add("the work folder", &format!("{error}"));
            return;
        }
    }
    let arguments = gate.with_corpus_paths(arguments);
    cpp_lines.push(format!("{} {arguments}", line("EXPORT", &cpp_file)));
    let rust_steps = rust_steps
        .word("export-ifc")
        .word(&format!(" \"{}\" {arguments}", rust_file.display()));
    let cpp = gate.cpp(&cpp_lines);
    let rust = gate.rust(rust_steps);
    if !both_succeeded(findings, "EXPORT .ifc", &cpp, &rust) {
        return;
    }
    same_file(findings, "the .ifc both write", &cpp_file, &rust_file);
    same_ifc_judgement(gate, findings, &cpp_file, &rust_file);
    each_reads(
        gate,
        findings,
        "the other's .ifc",
        &folder,
        &rust_file,
        &cpp_file,
    );
}

/// One row through both programs.
fn check_row(gate: &Gate, index: usize, row: &Row) -> Vec<String> {
    let work = gate.scratch.join(format!("{index:03}"));
    let mut findings = Findings {
        row,
        list: Vec::new(),
    };
    match row.read {
        Read::Out(_) => {}
        Read::Project => check_project(gate, &mut findings, &work),
        Read::Import => check_import(gate, &mut findings, &work),
        Read::Survey => check_survey(gate, &mut findings),
        Read::IfcExport { project, arguments } => {
            check_ifc_export(gate, &mut findings, &work, project, arguments);
        }
    }
    findings.list
}

#[test]
fn every_file_of_the_corpus_is_read_and_written_alike_by_the_cpp_and_the_rust() {
    let cpp = std::env::var_os("KATANA_CPP_CLI").expect(
        "KATANA_CPP_CLI names the C++ katana_cli: run tools/build_visible.sh wsl, which builds it and sets it",
    );
    let root = checkout_root();
    let python = std::env::var_os("KATANA_PYTHON").unwrap_or_else(|| OsString::from("python3"));
    let ifcopenshell = run(Command::new(&python).args(["-c", "import ifcopenshell"])).ok;
    if !ifcopenshell {
        eprintln!(
            "note: {} cannot import ifcopenshell, so tools/check_ifc.py does not judge the .ifc files; \
             they are compared byte for byte (the C++ suite registers its own IFC checks only where it can)",
            python.to_string_lossy()
        );
    }
    let scratch =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("round_trip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("the gate's scratch folder");
    let gate = Gate {
        version: katana_version(&root),
        root,
        cpp: PathBuf::from(cpp),
        rust: PathBuf::from(env!("CARGO_BIN_EXE_katana_corpus")),
        python,
        ifcopenshell,
        scratch: scratch.clone(),
    };

    // The rows are independent, each in its own folder, so they run side by
    // side; the findings are reported in the table's order.
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<(usize, Vec<String>)>> = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |count| count.get().min(8));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(row) = CORPUS.get(index) else {
                        break;
                    };
                    let findings = check_row(&gate, index, row);
                    found
                        .lock()
                        .expect("no worker panics holding the findings")
                        .push((index, findings));
                }
            });
        }
    });
    let mut found = found.into_inner().expect("the findings");
    found.sort_by_key(|(index, _)| *index);
    let differences: Vec<String> = found.into_iter().flat_map(|(_, list)| list).collect();
    let compared = CORPUS
        .iter()
        .filter(|row| !matches!(row.read, Read::Out(_)))
        .count();
    if differences.is_empty() {
        let _ = std::fs::remove_dir_all(&scratch);
        eprintln!(
            "round trip: {compared} files of the corpus read and written alike by both programs ({} left out, each saying why)",
            CORPUS.len() - compared
        );
    }
    assert!(
        differences.is_empty(),
        "{} differences between the C++ and the Rust over {compared} files of the corpus (the work is kept in {}):\n{}",
        differences.len(),
        scratch.display(),
        differences.join("\n")
    );
}
