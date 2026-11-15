//! What the import wizard and the Survey Jobs dialog share about survey jobs: the reader bound to
//! a job's stored bytes, the words for an adjustment's outcome, a job's last adjustment read back
//! from its stored report, the report written to a file, and the reduction run for a preview
//! (survey_job_support.hpp/.cpp).
//!
//! Here and not in cad because the reader is surveyio's, which cad may not see
//! (tools/check_layering.cmake): the window is where the two meet.

use std::io::Write;
use std::sync::Arc;

use katana_cad::survey_job::{ReductionFunction, SurveyJob, SurveyJobReader, reduce_for_drawing};
use katana_core::error::make_error;
use katana_core::path_text::path_from_utf8;
use katana_core::text::{split_lines, trimmed};
use katana_core::{ErrorCode, Result, Status};
use katana_survey::reduction::{ComputedPoint, ReductionContext, ReductionOutcome};
use katana_survey::{ReductionSettings, SurveyPoint, SurveyProject};
use katana_surveyio::{ReadOptions, SiblingFile, format_registry, read_survey, siblings_in_memory};

use crate::canvas::{
    Canvas, Color, Font, FontMetrics, FontSize, PdfCanvas, PdfDocument, Pen, PointF, RectF,
    TextMetrics,
};
use crate::theme::MONOSPACE_FAMILIES;

/// An outcome in a line, the rejected count and the method's words: in survey, where SURVEY
/// IMPORT's reply takes the same count.
pub use katana_survey::report_summary::{adjustment_summary, method_text, rejected_observations};
/// What the reduction's report input says about a file that was read: in surveyio, where
/// katana_cli's SURVEY IMPORT finds it too.
pub use katana_surveyio::report_input_for;

/// Inputs no larger than these are read and reduced on the interface thread, at once; larger ones
/// on a thread of their own with a Cancel button (SurveyTaskBar). Chosen so the interface thread
/// never spends more than a blink on one: a megabyte of field file reads in well under 0.1 s even
/// in a Debug build.
pub const BACKGROUND_READ_BYTES: u64 = 1 << 20;
pub const BACKGROUND_OBSERVATIONS: usize = 20_000;
/// Rows per table of a report on screen (display_report); the job keeps, and Export writes, every
/// row.
pub const REPORT_ROWS_ON_SCREEN: usize = 2_000;

/// Observations and points of a project: what decides whether its reduction runs in the
/// background.
pub fn survey_size(project: &SurveyProject) -> usize {
    let mut size = project.points.len() + project.observations.len();
    for station in &project.stations {
        size += station.observations.len();
    }
    size
}

/// The SurveyJobReader the window gives cad: the job's stored bytes read again with the reader
/// its format names, its sibling files served from memory - the folder they came from is not
/// needed, nor looked at.
pub fn stored_job_reader() -> SurveyJobReader {
    Box::new(|job: &SurveyJob| -> Result<SurveyProject> {
        let files: Vec<SiblingFile> = job
            .sibling_files
            .iter()
            .map(|file| SiblingFile {
                name: file.name.clone(),
                bytes: file.bytes.clone(),
            })
            .collect();
        let options = ReadOptions {
            siblings: Some(siblings_in_memory(files)),
            ..ReadOptions::default()
        };
        let read = read_survey(
            format_registry(),
            &job.format_id,
            &job.source_bytes,
            &job.source_file_name,
            &options,
        )?;
        Ok(read.project)
    })
}

/// A SurveyJobReader that answers for `job_id` with `project` - read once already, off the
/// interface thread - and for any other job reads the stored bytes (stored_job_reader). A job's
/// bytes never change, so its project read by the same reader never does either.
pub fn cached_job_reader(job_id: String, project: Option<Arc<SurveyProject>>) -> SurveyJobReader {
    let read = stored_job_reader();
    Box::new(move |job: &SurveyJob| -> Result<SurveyProject> {
        if let Some(project) = &project
            && job.id == job_id
        {
            return Ok(SurveyProject::clone(project));
        }
        read(job)
    })
}

/// The version of the reader a format has in this build, for SurveyJobReadjustment's
/// parser_version; empty for an unknown format.
pub fn parser_version_of(format_id: &str) -> String {
    format_registry()
        .find(format_id)
        .map(|descriptor| descriptor.parser_version)
        .unwrap_or_default()
}

/// The last adjustment of a job as its stored plain-text report states it. The job keeps the
/// report as text, not as values (storage's SurveyJob), so the list of jobs reads these two lines
/// of it back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobAdjustmentLine {
    /// "1.023"
    pub variance_factor: Option<String>,
    /// "passed" or "FAILED"
    pub global_test: Option<String>,
}

fn add_value(target: &mut Option<String>, value: &str) {
    *target = Some(match target.take() {
        Some(before) => format!("{before} / {value}"),
        None => String::from(value),
    });
}

/// The text renderer's key-value rows: two spaces, the key padded, the value (survey's
/// render_text). A report with two adjustments (horizontal and levels) has each row twice; both
/// are given, " / " between.
pub fn adjustment_from_report_text(report_text: &str) -> JobAdjustmentLine {
    const VARIANCE: &str = "Variance factor";
    const GLOBAL: &str = "Chi-square global test";
    let mut line = JobAdjustmentLine::default();
    for row in split_lines(report_text.as_bytes()) {
        // Cut at ASCII bytes only, so what is left is still UTF-8.
        let Ok(row) = std::str::from_utf8(trimmed(row)) else {
            continue;
        };
        if let Some(rest) = row.strip_prefix(VARIANCE) {
            let value = std::str::from_utf8(trimmed(rest.as_bytes())).unwrap_or_default();
            let value = value.find(" (").map_or(value, |at| &value[..at]);
            add_value(&mut line.variance_factor, value);
        } else if row.starts_with(GLOBAL)
            && let Some(colon) = row.rfind(": ")
        {
            add_value(&mut line.global_test, &row[colon + 2..]);
        }
    }
    line
}

/// How a job's report is written: .html as the job keeps it, .txt its plain text, .pdf laid out
/// on A4 pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReportFileFormat {
    Html,
    Text,
    Pdf,
}

/// The format a path's extension names; None for another.
pub fn report_format_of(path: &str) -> Option<ReportFileFormat> {
    let path = path_from_utf8(path);
    let suffix = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match suffix.as_str() {
        "html" | "htm" => Some(ReportFileFormat::Html),
        "txt" | "text" => Some(ReportFileFormat::Text),
        "pdf" => Some(ReportFileFormat::Pdf),
        _ => None,
    }
}

/// Writes a job's report to `path`.
///
/// The C++ printed the report's HTML onto A4 pages through QTextDocument and QPdfWriter. The PDF
/// here is drawn on the window's one PDF canvas (canvas::PdfCanvas, as a plot is), on the same A4
/// pages between the same 15 mm margins, with the report's plain text: the window has no HTML
/// layout to print the HTML through, and the plain text is the same report in the same rows
/// (survey's render_text). REJECTED: a PDF writer of the report's own, which made two writers of
/// one file format. The C++'s title and creator are not in the file: PdfDocument writes no
/// document information yet.
pub fn export_report(job: &SurveyJob, path: &str, format: ReportFileFormat) -> Status {
    if path.trim().is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "choose a file to export the report to",
            "",
        ));
    }
    if job.report_html.is_empty() && job.report_text.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidState,
            "the survey job has no reduction report",
            job.id.as_str(),
        ));
    }
    let file = path_from_utf8(path);
    let bytes: Vec<u8> = match format {
        ReportFileFormat::Html => job.report_html.as_bytes().to_vec(),
        ReportFileFormat::Text => job.report_text.as_bytes().to_vec(),
        ReportFileFormat::Pdf => pdf_of_text(&job.report_text).map_err(|error| {
            let context = format!("{path}: {}", error.context);
            error.with_context(context)
        })?,
    };
    let mut written = std::fs::File::create(&file).map_err(|error| {
        make_error(
            ErrorCode::FileExportFailure,
            "the report cannot be written",
            format!("{path}: {error}"),
        )
    })?;
    written.write_all(&bytes).map_err(|error| {
        make_error(
            ErrorCode::FileExportFailure,
            "the report was not written whole",
            format!("{path}: {error}"),
        )
    })?;
    drop(written);
    // As the C++ checked what QPdfWriter left: whether a file is there.
    if std::fs::metadata(file).map_or(0, |metadata| metadata.len()) == 0 {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the report file is empty",
            path,
        ));
    }
    Ok(())
}

/// `text` on A4 pages (QPageSize::A4, 210 x 297 mm) between the C++'s 15 mm margins, drawn on the
/// PDF canvas: a line of the report a line of the page, in an 8 pt fixed-pitch face, as the
/// report's tables are read in columns; a line too long for the page is cut at the right margin
/// rather than wrapped, for the same reason. FileExportFailure when the PDF cannot be written
/// (PdfDocument::finish).
fn pdf_of_text(text: &str) -> Result<Vec<u8>> {
    // QPdfWriter's own resolution (canvas::pdf), so the page is laid out in the pixels the C++'s
    // was.
    const RESOLUTION: f64 = 1200.0;
    const POINT_SIZE: f64 = 8.0;
    const PAGE_MM: (f64, f64) = (210.0, 297.0);
    const MARGIN_MM: f64 = 15.0;
    let points = |mm: f64| mm * 72.0 / 25.4;
    let pixels = |mm: f64| mm * RESOLUTION / 25.4;
    let font = Font {
        families: MONOSPACE_FAMILIES
            .iter()
            .map(|&family| String::from(family))
            .collect(),
        size: FontSize::Points(POINT_SIZE),
        fixed_pitch: true,
        ..Font::default()
    };
    let area = RectF::new(
        pixels(MARGIN_MM),
        pixels(MARGIN_MM),
        pixels(PAGE_MM.0 - 2.0 * MARGIN_MM),
        pixels(PAGE_MM.1 - 2.0 * MARGIN_MM),
    );
    let metrics = TextMetrics::new(RESOLUTION);
    let ascent = metrics.ascent(&font);
    let step = metrics.line_spacing(&font);
    // A face with no height (none resolved) still gives one page, with every line on it.
    let per_page = if step > 0.0 {
        ((area.height / step).floor() as usize).max(1)
    } else {
        usize::MAX
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut pages: Vec<&[&str]> = lines.chunks(per_page).collect();
    if pages.is_empty() {
        pages.push(&[]);
    }
    let mut document = PdfDocument::new();
    for page_lines in pages {
        let mut page = PdfCanvas::new(points(PAGE_MM.0), points(PAGE_MM.1), RESOLUTION);
        page.set_pen(Pen::new(Color::BLACK));
        page.set_font(font.clone());
        page.set_clip_rect(area, false);
        for (row, line) in page_lines.iter().enumerate() {
            let baseline = area.y + ascent + row as f64 * step;
            page.draw_text(PointF::new(area.x, baseline), line);
        }
        document.add_page(page)?;
    }
    document.finish()
}

/// A reduction that has already run (on its own thread, for a preview), handed to a command as
/// its ReductionFunction so the command does not run it a second time on the interface thread.
/// Used only when the command asks with the very settings, previous coordinates and drawing points
/// the outcome was made with; anything else runs reduce_for_drawing. The raw project is the
/// caller's to keep the same: the one the outcome was made from.
#[derive(Clone, Debug, Default)]
pub struct PrecomputedReduction {
    pub outcome: Option<Arc<ReductionOutcome>>,
    pub settings: ReductionSettings,
    pub previous: Vec<ComputedPoint>,
    pub drawing_points: Vec<SurveyPoint>,
}

pub fn precomputed_reduction(done: PrecomputedReduction) -> ReductionFunction {
    Box::new(
        move |raw: &SurveyProject,
              settings: &ReductionSettings,
              context: &ReductionContext|
              -> Result<ReductionOutcome> {
            if let Some(outcome) = &done.outcome
                && *settings == done.settings
                && context.previous == done.previous
                && context.drawing_points == done.drawing_points
            {
                let mut outcome = ReductionOutcome::clone(outcome);
                // The command's own time and input, as a fresh run would have.
                outcome.report.created_utc.clone_from(&context.created_utc);
                outcome.report.input = context.input.clone();
                return Ok(outcome);
            }
            reduce_for_drawing(raw, settings, context)
        },
    )
}
