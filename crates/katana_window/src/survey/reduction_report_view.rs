//! A reduction report on screen: the HTML survey's render_html writes, with a list of its sections
//! beside it to jump to, and what went wrong - flagged or rejected outliers, a failed global test,
//! a face pair or misclosure outside its tolerance - marked so it is seen first
//! (reduction_report_view.hpp/.cpp).
//!
//! The HTML is the report's own (the one a job keeps and exports); only its LOOK is changed here:
//! the report's print style sheet is taken out, every table given explicit borders, and an anchor
//! put before each section heading for the list to jump to (screen_html, as the C++). Nothing in
//! the report's content is changed.
//!
//! egui has no HTML browser where Qt had QTextBrowser, so the view shows the screen HTML as the
//! text a person reads in it - a block a line, a table's cells in their row, the marked cells in
//! the error colour - and that text is what the browser reports (QTextEdit::toPlainText's place).
//!
//! Object names: `<name>` (the view), `<name>Sections` (the list), `<name>Browser` (the text),
//! `<name>Summary` (what is marked, in a line).

use egui::{Align, Label, RichText, ScrollArea, Ui};
use katana_survey::reduction_report::{ReductionReport, ReportMessage};

use super::survey_dialogs::coloured_label;
use crate::drive::{self, ControlKind, ControlRecord};
use crate::theme::palette;

/// One section of a report as the view lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportSection {
    pub title: String,
    pub anchor: String,
    /// Cells marked as alarms in it.
    pub marked: usize,
}

/// The entities survey's render_html escapes, back to text for the list.
fn unescaped(text: &str) -> String {
    const ENTITIES: [(&str, char); 5] = [
        ("&amp;", '&'),
        ("&lt;", '<'),
        ("&gt;", '>'),
        ("&quot;", '"'),
        ("&#39;", '\''),
    ];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        if first == '&'
            && let Some((entity, character)) =
                ENTITIES.iter().find(|(entity, _)| rest.starts_with(entity))
        {
            out.push(*character);
            rest = &rest[entity.len()..];
            continue;
        }
        out.push(first);
        rest = &rest[first.len_utf8()..];
    }
    out
}

fn count(text: &str, what: &str) -> usize {
    text.matches(what).count()
}

/// The report's HTML made ready for the screen: its style sheet removed, every `<table>` given
/// explicit borders (QTextDocument drew no cell border from CSS alone), and an anchor before every
/// section heading. `sections` receives the sections in order.
pub fn screen_html(html: &str, mut sections: Option<&mut Vec<ReportSection>>) -> String {
    const HEADING: &str = "<h2>";
    let mut input = html;
    let mut out = String::with_capacity(html.len() + 4096);
    // The report's own style sheet is for paper; the view sets its own.
    if let Some(style) = input.find("<style>")
        && let Some(end) = input[style..].find("</style>")
    {
        out.push_str(&input[..style]);
        input = &input[style + end + "</style>".len()..];
    }
    // In `out`, for counting marks per section.
    let mut heading_at: Vec<usize> = Vec::new();
    let mut number = 0_usize;
    while !input.is_empty() {
        let table = input.find("<table");
        let heading = input.find(HEADING);
        let next = match (table, heading) {
            (None, None) => {
                out.push_str(input);
                break;
            }
            (Some(table), Some(heading)) => table.min(heading),
            (Some(table), None) => table,
            (None, Some(heading)) => heading,
        };
        out.push_str(&input[..next]);
        input = &input[next..];
        if Some(next) == heading {
            number += 1;
            let anchor = format!("section-{number}");
            let title = input
                .find("</h2>")
                .map_or("", |close| &input[HEADING.len()..close]);
            if let Some(list) = sections.as_mut() {
                list.push(ReportSection {
                    title: unescaped(title),
                    anchor: anchor.clone(),
                    marked: 0,
                });
            }
            heading_at.push(out.len());
            out.push_str(&format!("<a name=\"{anchor}\"></a>"));
            out.push_str(HEADING);
            input = &input[HEADING.len()..];
        } else {
            // Borders QTextDocument draws: it takes them from the table's attributes, not from a
            // CSS border on the cells.
            out.push_str("<table border=\"1\" cellspacing=\"0\" cellpadding=\"3\"");
            input = &input["<table".len()..];
        }
    }
    if let Some(list) = sections {
        for (at, section) in list.iter_mut().enumerate().take(heading_at.len()) {
            let start = heading_at[at];
            let end = heading_at.get(at + 1).copied().unwrap_or(out.len());
            section.marked = count(&out[start..end], "class=\"alarm");
        }
    }
    out
}

fn append_clipped(target: &mut String, what: &str, shown: usize, total: usize) {
    if total <= shown {
        return;
    }
    if !target.is_empty() {
        target.push_str(", ");
    }
    target.push_str(&format!("{what} ({shown} of {total})"));
}

fn take_rows<T: Clone>(from: &[T], max_rows: usize, clipped: &mut String, what: &str) -> Vec<T> {
    let shown = from.len().min(max_rows);
    append_clipped(clipped, what, shown, from.len());
    from[..shown].to_vec()
}

/// A copy of `report` with each long table cut to `max_rows` rows and a warning saying so, for the
/// screen: a job of a million observations has a report of hundreds of megabytes, which no text
/// view lays out in reasonable time. The job keeps, and Export writes, the whole report.
pub fn display_report(report: &ReductionReport, max_rows: usize) -> ReductionReport {
    let mut clipped = String::new();
    let mut shown = ReductionReport {
        created_utc: report.created_utc.clone(),
        input: report.input.clone(),
        settings: report.settings.clone(),
        misclosures: report.misclosures.clone(),
        ..ReductionReport::default()
    };
    shown.input.warnings = take_rows(
        &report.input.warnings,
        max_rows,
        &mut clipped,
        "file warnings",
    );
    shown.setups = take_rows(&report.setups, max_rows, &mut clipped, "setups");
    shown.observations = take_rows(&report.observations, max_rows, &mut clipped, "observations");
    shown.face_pairs = take_rows(&report.face_pairs, max_rows, &mut clipped, "face pairs");
    // One per resected setup, each with a residual per pointing it used: a few dozen rows, where
    // a network's residuals can be thousands.
    shown.resections = take_rows(&report.resections, max_rows, &mut clipped, "resections");
    for adjustment in &report.adjustments {
        let mut cut = adjustment.clone();
        cut.residuals = take_rows(&adjustment.residuals, max_rows, &mut clipped, "residuals");
        cut.ellipses = take_rows(
            &adjustment.ellipses,
            max_rows,
            &mut clipped,
            "error ellipses",
        );
        shown.adjustments.push(cut);
    }
    shown.coordinates = take_rows(&report.coordinates, max_rows, &mut clipped, "coordinates");
    shown.warnings = take_rows(&report.warnings, max_rows, &mut clipped, "warnings");
    if !clipped.is_empty() {
        shown.warnings.insert(
            0,
            ReportMessage {
                text: format!(
                    "This view shows the first rows only: {clipped}. The whole report is kept \
                     with the survey job; export it (Survey > Survey Jobs, Export Report) to read \
                     every row."
                ),
                ..ReportMessage::default()
            },
        );
    }
    shown
}

/// One line of the text the view shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ViewLine {
    text: String,
    /// It holds a cell marked as an alarm.
    alarm: bool,
    /// It is a section's heading: the section's index.
    section: Option<usize>,
}

/// The screen HTML as the lines a person reads in it: a block element ends a line, a table's
/// cells stand in their row " | " apart, a marked cell marks its line, an anchor names the heading
/// after it. Tags are dropped, entities read, runs of blanks one blank, the head left out.
fn view_lines(html: &str) -> Vec<ViewLine> {
    const BREAKS: [&str; 12] = [
        "br", "/p", "/h1", "/h2", "/h3", "/tr", "/li", "/div", "/table", "/section", "/caption",
        "/title",
    ];
    let mut lines = Vec::new();
    let mut current = ViewLine::default();
    let mut words = String::new();
    let mut in_head = false;
    let mut next_section: Option<usize> = None;
    let mut anchors = 0_usize;
    let mut rest = html;
    let flush_words = |words: &mut String, current: &mut ViewLine| {
        let collapsed = words.split_whitespace().collect::<Vec<_>>().join(" ");
        if !collapsed.is_empty() {
            if !current.text.is_empty() && !current.text.ends_with(' ') {
                current.text.push(' ');
            }
            current.text.push_str(&unescaped(&collapsed));
        }
        words.clear();
    };
    while !rest.is_empty() {
        let Some(open) = rest.find('<') else {
            if !in_head {
                words.push_str(rest);
            }
            break;
        };
        if !in_head {
            words.push_str(&rest[..open]);
        }
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        let tag = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        let name: String = tag
            .split(|character: char| character.is_whitespace() || character == '>')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match name.as_str() {
            "head" => in_head = true,
            "/head" => in_head = false,
            "a" if tag.contains("name=\"section-") => {
                next_section = Some(anchors);
                anchors += 1;
            }
            "td" | "th" => {
                flush_words(&mut words, &mut current);
                if tag.contains("class=\"alarm") {
                    current.alarm = true;
                }
                if !current.text.is_empty() {
                    current.text.push_str(" | ");
                }
            }
            _ if BREAKS.contains(&name.as_str()) => {
                flush_words(&mut words, &mut current);
                let text = current.text.trim_end_matches(" | ").trim().to_owned();
                if !text.is_empty() {
                    lines.push(ViewLine {
                        text,
                        alarm: current.alarm,
                        section: if name == "/h2" {
                            next_section.take()
                        } else {
                            None
                        },
                    });
                }
                current = ViewLine::default();
            }
            _ => {}
        }
    }
    flush_words(&mut words, &mut current);
    let text = current.text.trim().to_owned();
    if !text.is_empty() {
        lines.push(ViewLine {
            text,
            alarm: current.alarm,
            section: None,
        });
    }
    lines
}

/// The view (the C++ ReductionReportView).
#[derive(Clone, Debug, PartialEq)]
pub struct ReductionReportView {
    name: String,
    sections: Vec<ReportSection>,
    lines: Vec<ViewLine>,
    summary: String,
    summary_is_error: bool,
    current: Option<usize>,
    scroll_to: Option<usize>,
    above: bool,
}

impl ReductionReportView {
    /// A view named `name`, showing "No report yet."
    pub fn new(name: &str) -> Self {
        let mut view = Self {
            name: String::from(name),
            sections: Vec::new(),
            lines: Vec::new(),
            summary: String::new(),
            summary_is_error: false,
            current: None,
            scroll_to: None,
            above: false,
        };
        view.show_note("No report yet.");
        view
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Shows a report rendered by survey's render_html.
    pub fn set_report_html(&mut self, html: &str) {
        self.sections.clear();
        let screen = screen_html(html, Some(&mut self.sections));
        self.lines = view_lines(&screen);
        self.current = None;
        self.scroll_to = None;
        let total = self.marked();
        self.summary = if total == 0 {
            format!(
                "{} sections; nothing is flagged, rejected or outside a tolerance.",
                self.sections.len()
            )
        } else {
            format!(
                "{} sections; {total} value(s) flagged, rejected, failed or outside a tolerance - \
                 marked in red.",
                self.sections.len()
            )
        };
        self.summary_is_error = total != 0;
    }

    /// Shows a sentence instead of a report ("No preview yet").
    pub fn show_note(&mut self, note: &str) {
        self.sections.clear();
        self.current = None;
        self.scroll_to = None;
        self.lines = vec![ViewLine {
            text: String::from(note),
            ..ViewLine::default()
        }];
        self.summary.clear();
        self.summary_is_error = false;
    }

    pub fn sections(&self) -> &[ReportSection] {
        &self.sections
    }

    pub fn marked(&self) -> usize {
        self.sections.iter().map(|section| section.marked).sum()
    }

    /// Scrolls to the section at `index` of sections().
    pub fn show_section(&mut self, index: usize) {
        if index < self.sections.len() {
            self.scroll_to = Some(index);
        }
    }

    /// True when the sections are listed above the text rather than beside it.
    pub fn sections_above(&self) -> bool {
        self.above
    }

    /// What the text shows, a line each.
    pub fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Draws the view.
    pub fn ui(&mut self, ui: &mut Ui) {
        // Below this width a list beside the text leaves the text too narrow for the report's
        // tables; above it the list costs little. Two reports side by side (Survey Jobs' preview)
        // would otherwise give half of each to a list of headings.
        const SIDE_BY_SIDE: f32 = 620.0;
        self.above = ui.available_width() < SIDE_BY_SIDE;
        let shown = ui.vertical(|ui| {
            let summary_colour = if self.summary_is_error {
                palette::error().to_egui()
            } else {
                palette::text_muted().to_egui()
            };
            coloured_label(
                ui,
                &format!("{}Summary", self.name),
                &self.summary,
                summary_colour,
            );
            if self.above {
                self.sections_ui(ui, 96.0);
                self.browser_ui(ui);
            } else {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(190.0);
                        self.sections_ui(ui, 320.0);
                    });
                    self.browser_ui(ui);
                });
            }
        });
        drive::custom(
            ui,
            &shown.response,
            ControlRecord::new(&self.name, ControlKind::Other),
        );
    }

    fn sections_ui(&mut self, ui: &mut Ui, height: f32) {
        // The count first, so a narrow list still shows that something in the section is marked.
        let rows: Vec<Vec<String>> = self
            .sections
            .iter()
            .map(|section| {
                vec![if section.marked == 0 {
                    section.title.clone()
                } else {
                    format!("[{} marked] {}", section.marked, section.title)
                }]
            })
            .collect();
        let name = format!("{}Sections", self.name);
        let mut current = self.current;
        let changed = ScrollArea::vertical()
            .id_salt((self.name.as_str(), "sections"))
            .max_height(height)
            .show(ui, |ui| {
                drive::rows(ui, &name, &rows, &mut current).changed()
            })
            .inner;
        self.current = current;
        if changed && let Some(row) = current {
            self.show_section(row);
        }
    }

    fn browser_ui(&mut self, ui: &mut Ui) {
        let error = palette::error().to_egui();
        let scroll_to = self.scroll_to.take();
        let lines = &self.lines;
        let shown = ScrollArea::both()
            .id_salt((self.name.as_str(), "browser"))
            .max_height(360.0)
            .show(ui, |ui| {
                for line in lines {
                    let mut text = RichText::new(line.text.as_str()).monospace();
                    if line.alarm {
                        text = text.color(error).strong();
                    } else if line.section.is_some() {
                        text = text.strong();
                    }
                    let response = ui.add(Label::new(text).extend());
                    if line.section.is_some() && line.section == scroll_to {
                        response.scroll_to_me(Some(Align::TOP));
                    }
                }
            });
        let mut record = ControlRecord::new(&format!("{}Browser", self.name), ControlKind::Text);
        record.value = self.plain_text();
        let response = ui.interact(
            shown.inner_rect,
            ui.id().with((self.name.as_str(), "browserRecord")),
            egui::Sense::hover(),
        );
        drive::custom(ui, &response, record);
    }
}

#[cfg(test)]
mod tests {
    use super::{ReportSection, screen_html, unescaped, view_lines};

    #[test]
    fn the_screen_html_anchors_each_heading_and_counts_its_marks() {
        let html = "<html><style>p{}</style><body><h2>A &amp; B</h2><table><tr><td \
                    class=\"alarm\">1</td></tr></table><h2>C</h2><p>x</p></body></html>";
        let mut sections = Vec::new();
        let screen = screen_html(html, Some(&mut sections));
        assert!(!screen.contains("<style>"));
        assert!(screen.contains("<a name=\"section-1\"></a><h2>A &amp; B</h2>"));
        assert!(screen.contains("<table border=\"1\" cellspacing=\"0\" cellpadding=\"3\">"));
        assert_eq!(
            sections,
            vec![
                ReportSection {
                    title: String::from("A & B"),
                    anchor: String::from("section-1"),
                    marked: 1,
                },
                ReportSection {
                    title: String::from("C"),
                    anchor: String::from("section-2"),
                    marked: 0,
                },
            ]
        );
        let lines = view_lines(&screen);
        assert_eq!(lines[0].text, "A & B");
        assert_eq!(lines[0].section, Some(0));
        assert!(lines[1].alarm);
        assert_eq!(unescaped("&lt;&#39;&gt;"), "<'>");
    }
}
