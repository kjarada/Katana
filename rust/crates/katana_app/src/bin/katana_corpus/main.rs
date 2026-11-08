//! katana_corpus: the Rust readers and writers of step 3's formats - the
//! project (katana_storage), DXF, IFC, the .12da archive and the survey field
//! files (katana_surveyio) - driven from a command line, for the round-trip
//! gate (tests/round_trip.rs; docs/rust.md, "The order of the work"). Never
//! installed: there is no Rust katana_cli until step 6, and this program
//! stands in for the five lines of it the gate runs.
//!
//! ```text
//! katana_corpus [--application-version <v>] <step>...
//!
//!   open <project-directory>       OPEN <project-directory>
//!   import <file>                  IMPORT <file>: a .dxf, .12da or .ifc
//!   save                           SAVE
//!   save-as <project-directory>    SAVE <project-directory>
//!   export <file>                  EXPORT <file>: a .dxf, .12da or .ifc
//!   export-ifc <rest of the line>  EXPORT <file.ifc> [UTILITIES ...] [SCHEMA ...] [RULES ...] ...
//!   survey <file>                  SURVEY READ <file>
//!   survey-format <id> <file>      SURVEY READ <file> FORMAT <id>
//! ```
//!
//! The steps run in order on one drawing, as a katana_cli run's -c lines do,
//! each printing a line of what it did; the first that fails prints
//! "error: <what>" on standard error and ends the run with status 1, as
//! katana_cli's batch does. A usage mistake is status 2. SURVEY READ prints
//! the reply katana_cli prints, word for word. --application-version is the
//! version an IFC export names as the program that wrote it (the C++'s
//! KATANA_VERSION, its CMake project's version), which the gate passes.

mod drawing;
mod survey;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use drawing::Drawing;

const USAGE: &str = "usage: katana_corpus [--application-version <v>] <step>...
  open <project-directory> | import <file> | save | save-as <project-directory>
  export <file> | export-ifc <rest of an EXPORT line> | survey <file> | survey-format <id> <file>
";

enum Step {
    Open(PathBuf),
    Import(PathBuf),
    Save,
    SaveAs(PathBuf),
    Export(PathBuf),
    ExportIfc(String),
    Survey {
        file: PathBuf,
        format: Option<String>,
    },
}

struct Invocation {
    application_version: String,
    steps: Vec<Step>,
}

/// The word after a step, as a path.
fn path(word: Option<OsString>, step: &str) -> Result<PathBuf, String> {
    word.map(PathBuf::from)
        .ok_or_else(|| format!("{step} needs a path"))
}

/// The word after a step, as text.
fn text(word: Option<OsString>, step: &str) -> Result<String, String> {
    let word = word.ok_or_else(|| format!("{step} needs a value"))?;
    word.into_string()
        .map_err(|_| format!("the value of {step} is not UTF-8"))
}

fn parse(arguments: Vec<OsString>) -> Result<Invocation, String> {
    let mut invocation = Invocation {
        application_version: String::new(),
        steps: Vec::new(),
    };
    let mut words = arguments.into_iter();
    while let Some(word) = words.next() {
        let Some(name) = word.to_str() else {
            return Err(format!("'{}' is not a step", word.to_string_lossy()));
        };
        let step = match name {
            "--application-version" => {
                invocation.application_version = text(words.next(), name)?;
                continue;
            }
            "open" => Step::Open(path(words.next(), name)?),
            "import" => Step::Import(path(words.next(), name)?),
            "save" => Step::Save,
            "save-as" => Step::SaveAs(path(words.next(), name)?),
            "export" => Step::Export(path(words.next(), name)?),
            "export-ifc" => Step::ExportIfc(text(words.next(), name)?),
            "survey" => Step::Survey {
                file: path(words.next(), name)?,
                format: None,
            },
            "survey-format" => {
                let format = text(words.next(), name)?;
                Step::Survey {
                    file: path(words.next(), name)?,
                    format: Some(format),
                }
            }
            other => return Err(format!("'{other}' is not a step")),
        };
        invocation.steps.push(step);
    }
    Ok(invocation)
}

fn main() -> ExitCode {
    let invocation = match parse(std::env::args_os().skip(1).collect()) {
        Ok(invocation) if !invocation.steps.is_empty() => invocation,
        Ok(_) => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
        Err(why) => {
            eprintln!("error: {why}");
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let version = invocation.application_version.as_str();
    let mut drawing = Drawing::empty();
    for step in &invocation.steps {
        let reply = match step {
            Step::Open(directory) => drawing.open(directory),
            Step::Import(file) => drawing.import(file),
            Step::Save => drawing.save(),
            Step::SaveAs(directory) => drawing.save_as(directory),
            Step::Export(file) => drawing.export(file, version),
            Step::ExportIfc(argument) => drawing.export_ifc(argument, version),
            Step::Survey { file, format } => survey::read(file, format.as_deref()),
        };
        match reply {
            Ok(text) => println!("{text}"),
            Err(error) => {
                eprintln!("error: {}", error.describe());
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
