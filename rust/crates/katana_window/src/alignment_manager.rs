//! Terrain > Alignment Manager... (docs/cad.md, "The Alignment Manager"): the drawing's
//! alignments, their PIs, their design profiles and their setting-out tables in one window, where
//! before there was only the ALIGN verb and a message telling a person to type it. The port of
//! alignment_manager.
//!
//! NOTHING IS CHANGED HERE. Every edit builds the ALIGN (or LABEL ALIGN) line a person would type
//! and hands it to the window's one executor (host::Host::run), so it is echoed in the command log,
//! kept in the history and undone as one step, exactly as if typed - and an agent does the same
//! through the verb. What the dialog shows is read from the model and from the functions ALIGN
//! STATIONS and ALIGN PROFILE print from (katana_cad::alignment_report), so the tables and the
//! verbs agree.
//!
//! NON-MODAL, kept by the window between uses. It hears the Document through a DocumentWatcher, so
//! an ALIGN typed on the command line, an undo or an import reloads it once, keeping the chosen
//! alignment - and a grid's unapplied edits, while what the grid was filled from is unchanged.
//!
//! The two grids - PIs and PVIs - are buffers, as the Linetypes tab's pattern grid is: edited in
//! place, applied by one line, one undo step, and never a line run from a table's own change. They
//! show the stored numbers exactly (shortest round trip), so applying an edit to one PI's easting
//! writes every other number back as it was.
//!
//! Only the chosen tab's page is drawn, as only it is seen: a driver chooses the tab
//! (alignmentTabs) before it acts on the page's controls, as a person does.
//!
//! Object names, which the widget tests and the headless --dialog, --fill and --press steps drive
//! it by:
//!
//! ```text
//! alignmentManagerDialog   the dialog (the action terrainAlignmentManager's data)
//! alignmentList            the alignments: name, PIs, start, end, length, profile
//! alignmentName            the name for New
//! alignmentPoints          the PIs for New, "x,y x,y ..."
//! alignmentUseSelection    fill alignmentPoints from the selected line or polyline
//! alignmentNew             ALIGN NEW "name" x,y x,y ...
//! alignmentDelete          ALIGN DELETE name
//! alignmentTabs            Horizontal, Vertical, Setting Out
//! Horizontal:
//!   alignmentPiTable       #, easting, northing, radius, spiral in, spiral out: edited in place,
//!                          then applied
//!   alignmentAddPi, alignmentRemovePi      a row more or less in the grid
//!   alignmentRevertPis     the grid as the drawing holds it again
//!   alignmentApplyPis      ALIGN PIS name x,y[,radius[,spiralIn[,spiralOut]]] ...
//!   alignmentStartStation, alignmentStart   ALIGN START name station
//! Vertical:
//!   alignmentPviTable      chainage, level, curve length: edited, then applied
//!   alignmentAddPvi, alignmentRemovePvi   a row more or less in the grid
//!   alignmentRevertProfile the grid as the drawing holds it again
//!   alignmentApplyProfile  ALIGN DESIGN name s,z[,L] ... from the grid
//!   alignmentClearProfile  ALIGN CLEARPROFILE name
//!   alignmentProfileElements  element, from, to, grade in, grade out, K (read only)
//!   alignmentHighLow       the high and low points
//!   alignmentProfilePreview   the profile drawn: grades, curves, high and low points
//! Setting Out:
//!   alignmentInterval      the chainage interval (20 by default), applied as typed
//!   alignmentStationsNote  how many stations and key stations, or why there are none
//!   alignmentStations      chainage, easting, northing, azimuth, radius, key
//!   alignmentStationsCopy  the table to the clipboard, tab separated
//!   alignmentStationsCsv   Save CSV... (no file dialog in a headless run)
//!   alignmentLabelStyle, alignmentLabelLayer, alignmentLabelChainages
//!                          LABEL ALIGN "name" style="..." [layer=...]
//! alignmentStatus          what the last line replied, or why it was refused
//! alignmentClose
//! ```

use std::path::Path as FilePath;

use egui::Ui;
use katana_cad::Document;
use katana_cad::alignment_report::{
    SettingOutStation, curve_k, format_azimuth, setting_out_csv, setting_out_stations,
};
use katana_core::error::make_error;
use katana_core::text::{parse_finite_double, trimmed};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::{Alignment, Geometry, LabelKind};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::profile::solve_profile;
use katana_geometry::{
    AlignmentPi, ProfileElementKind, ProfilePvi, SolvedProfile, VerticalAlignment,
};
use katana_math::cpp;

use crate::bim::road_dialog_support::{
    Combo, PreviewRaster, StatusLine, Table, button, cell, editable, field, note, preview,
    reply_lines, save_file_dialog,
};
use crate::canvas::{
    Brush, Canvas, Path, Pen, PenCap, PenJoin, PenStyle, PointF, RectF, RenderHint, TextFlags,
};
use crate::command_word::{command_word, exact_number};
use crate::customisation::document_watcher::DocumentWatcher;
use crate::drive::controls::tabs;
use crate::drive::registry::simplified;
use crate::format::qt_fixed;
use crate::host::Host;
use crate::shell::dialog::DialogFrame;
use crate::theme::palette;

/// The PI grid's columns.
pub const PI_INDEX: usize = 0;
pub const PI_EASTING: usize = 1;
pub const PI_NORTHING: usize = 2;
pub const PI_RADIUS: usize = 3;
pub const PI_SPIRAL_IN: usize = 4;
pub const PI_SPIRAL_OUT: usize = 5;

/// The tabs, in their order.
pub const TABS: [&str; 3] = ["Horizontal", "Vertical", "Setting Out"];

/// A computed number, to the millimetre a table reads in (QString::number(value, 'f', 3)).
fn fixed3(value: f64) -> String {
    qt_fixed(value, 3)
}

/// The grids' cells, each one number: empty stays empty, a number is written back exactly, and
/// anything else is refused naming its row and column. The cells were once glued into the line as
/// they stood, so a decimal comma ("100,5") or an "x,y" pasted into one cell shifted every number
/// after it into the next field, and the verb took the line without a word.
pub fn grid_numbers(
    table: &Table,
    row: usize,
    first: usize,
    last: usize,
    row_name: &str,
) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for column in first..=last {
        let text = String::from(table.text(row, column).trim());
        if text.is_empty() {
            values.push(String::new());
            continue;
        }
        let Some(number) = parse_finite_double(&text) else {
            let header = table.headers().get(column).map_or("value", String::as_str);
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "{row_name}: the {header} '{text}' is not a number; type one number, with a \
                     decimal point"
                ),
            ));
        };
        values.push(exact_number(number));
    }
    Ok(values)
}

/// The design profile, drawn: the tangent polygon through the PVIs dashed, the solved grades and
/// curves over it, and the high and low points ringed. The vertical is exaggerated to fill the box,
/// as a long section's is, so that a 2% grade can be seen at all.
pub fn paint_profile_preview(
    canvas: &mut dyn Canvas,
    rect: RectF,
    pvis: &[ProfilePvi],
    solved: Option<&SolvedProfile>,
) {
    canvas.fill_rect(rect, &Brush::solid(palette::panel()));
    canvas.set_render_hint(RenderHint::Antialiasing, true);
    let (Some(first), Some(last)) = (pvis.first(), pvis.last()) else {
        canvas.set_pen(Pen::new(palette::text_muted()));
        canvas.draw_text_in(rect, TextFlags::CENTER, "No design profile");
        return;
    };
    if pvis.len() < 2 {
        canvas.set_pen(Pen::new(palette::text_muted()));
        canvas.draw_text_in(rect, TextFlags::CENTER, "No design profile");
        return;
    }
    let mut s0 = first.station;
    let mut s1 = last.station;
    let mut z0 = first.elevation;
    let mut z1 = z0;
    for pvi in pvis {
        s0 = cpp::min(s0, pvi.station);
        s1 = cpp::max(s1, pvi.station);
        z0 = cpp::min(z0, pvi.elevation);
        z1 = cpp::max(z1, pvi.elevation);
    }
    // A level profile still gets a height to be drawn in.
    if z1 - z0 < 1e-3 {
        z0 -= 0.5;
        z1 += 0.5;
    }
    if s1 - s0 < 1e-9 {
        return;
    }
    let frame = rect.adjusted(10.0, 10.0, -10.0, -10.0);
    let at = |station: f64, elevation: f64| {
        PointF::new(
            frame.left() + (station - s0) / (s1 - s0) * frame.width,
            frame.bottom() - (elevation - z0) / (z1 - z0) * frame.height,
        )
    };
    canvas.set_pen(Pen::with(
        palette::text_muted(),
        1.0,
        PenStyle::Dash,
        PenCap::Square,
        PenJoin::Bevel,
    ));
    for pair in pvis.windows(2) {
        canvas.draw_line(
            at(pair[0].station, pair[0].elevation),
            at(pair[1].station, pair[1].elevation),
        );
    }
    let Some(solved) = solved else {
        return;
    };
    let mut path = Path::new();
    let mut first_point = true;
    for element in solved.elements() {
        // A tangent is its two ends; a curve is sampled finely enough to look smooth at any width
        // the dialog is given.
        let steps: i32 = if element.kind == ProfileElementKind::Curve {
            32
        } else {
            1
        };
        for i in 0..=steps {
            let distance = element.length * f64::from(i) / f64::from(steps);
            let point = at(
                element.start_station + distance,
                element.elevation_at(distance),
            );
            if first_point {
                path.move_to(point);
                first_point = false;
            } else {
                path.line_to(point);
            }
        }
    }
    canvas.set_pen(Pen::with(
        palette::accent(),
        2.0,
        PenStyle::Solid,
        PenCap::Square,
        PenJoin::Bevel,
    ));
    canvas.draw_path(&path);
    canvas.set_pen(Pen::new(palette::text()));
    for point in solved.high_low_points() {
        let centre = at(point.station, point.elevation);
        canvas.draw_ellipse(RectF::new(centre.x - 4.0, centre.y - 4.0, 8.0, 8.0));
    }
}

/// Each grid is a buffer: edited in place, applied by one line. `edited` says it holds edits not
/// yet applied, made to what `alignment` held when the grid was filled. A reload that did not
/// change that keeps the edits; one that did refills the grid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Buffer {
    edited: bool,
    alignment: String,
}

struct Body {
    watcher: DocumentWatcher,
    executor: bool,
    list: Table,
    name: String,
    points: String,
    tab: usize,
    pi_table: Table,
    start_station: String,
    pvi_table: Table,
    elements: Table,
    high_low: String,
    preview_pvis: Vec<ProfilePvi>,
    preview_solved: Option<SolvedProfile>,
    raster: Option<PreviewRaster>,
    texture: Option<egui::TextureHandle>,
    interval: String,
    stations_note: StatusLine,
    stations: Table,
    label_style: Combo,
    label_layer: String,
    status: StatusLine,
    /// The buttons that need an alignment are enabled (needsAlignment).
    has_alignment: bool,
    /// True while a table is being refilled, when its change is the dialog's own doing.
    loading: bool,
    pis_buffer: Buffer,
    pis_from: Vec<AlignmentPi>,
    pvis_buffer: Buffer,
    pvis_from: Option<VerticalAlignment>,
    /// The start chainage the field was last filled with, so a reload that did not move it leaves
    /// what is being typed there alone.
    start_for: String,
    start_from: f64,
    close: bool,
}

impl Body {
    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    /// The chosen alignment's name: the first selected row's (QTableWidget::selectedItems).
    fn current_alignment(&self) -> String {
        self.list
            .selected_rows()
            .first()
            .map(|&row| self.list.text(row, 0))
            .unwrap_or_default()
    }

    fn current<'d>(&self, document: &'d Document) -> Option<&'d Alignment> {
        if !self.alive() {
            return None;
        }
        let chosen = self.current_alignment();
        if chosen.is_empty() {
            return None;
        }
        document.model().alignments.find(&chosen)
    }

    fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status.set(text, is_error);
    }

    /// Runs one line through the window's executor and shows what it said. True when it was
    /// carried out. The dialog reloads at once, rather than waiting for the watcher, so what the
    /// line did is on screen when this returns; the watcher's own delivery then finds nothing new.
    fn run(&mut self, host: &mut dyn Host, line: &str) -> bool {
        // The drawing closed under the dialog: there is nothing to change.
        if !self.alive() {
            self.set_status("The drawing this dialog edited is gone.", true);
            return false;
        }
        if !self.executor {
            self.set_status(
                format!("Nothing here can run {line}: the dialog has no command line."),
                true,
            );
            return false;
        }
        let outcome = host.run(line);
        if outcome.ok {
            let lines = reply_lines(&outcome.reply);
            let said = String::from(lines.last().copied().unwrap_or(line));
            self.set_status(said, false);
        } else if outcome.error.is_empty() {
            self.set_status(format!("{line} was refused."), true);
        } else {
            self.set_status(outcome.error.as_str(), true);
        }
        self.reload(host.document());
        outcome.ok
    }

    /// "`verb` <the chosen alignment> `rest`", or None, said on the status line, when there is no
    /// alignment to name.
    fn named(&mut self, verb: &str, rest: &str) -> Option<String> {
        let chosen = self.current_alignment();
        if chosen.is_empty() {
            self.set_status("Choose an alignment first, or make one with New.", true);
            return None;
        }
        match command_word(&chosen, "the alignment's name") {
            Ok(quoted) => Some(if rest.is_empty() {
                format!("{verb} {quoted}")
            } else {
                format!("{verb} {quoted} {rest}")
            }),
            Err(error) => {
                self.set_status(error.message, true);
                None
            }
        }
    }

    fn select_named(&mut self, document: &Document, name: &str) -> bool {
        let Some(row) = (0..self.list.row_count()).find(|&row| self.list.text(row, 0) == name)
        else {
            return false;
        };
        if self.list.select_row(row) && !self.loading {
            self.load_alignment(document);
        }
        true
    }

    // ---- loading ------------------------------------------------------------------------------

    fn reload(&mut self, document: &Document) {
        if !self.alive() {
            return;
        }
        let keep = self.current_alignment();
        self.load_list(document, &keep);
        self.load_label_styles(document);
        self.load_alignment(document);
    }

    fn load_list(&mut self, document: &Document, keep: &str) {
        self.loading = true;
        self.list.clear();
        let mut keep_row = None;
        let list = &mut self.list;
        document.model().alignments.for_each(|alignment| {
            let pis = format!("{}", alignment.horizontal.pis.len());
            // Stored alignments always solve - the model refuses one that does not - so a failure
            // is a defect worth showing, not hiding.
            let (start, end, length) = match solve_alignment(&alignment.horizontal) {
                Ok(solved) => (
                    fixed3(solved.start_station()),
                    fixed3(solved.end_station()),
                    fixed3(solved.length()),
                ),
                Err(_) => (String::from("does not solve"), String::new(), String::new()),
            };
            let profile = match &alignment.vertical {
                Some(vertical) => format!("{} PVIs", vertical.pvis.len()),
                None => String::from("none"),
            };
            let row = list.push_row(vec![
                cell(alignment.name.as_str()),
                cell(pis),
                cell(start),
                cell(end),
                cell(length),
                cell(profile),
            ]);
            if alignment.name == keep {
                keep_row = Some(row);
            }
        });
        if keep_row.is_none() && self.list.row_count() > 0 {
            keep_row = Some(0);
        }
        if let Some(row) = keep_row {
            self.list.select_row(row);
        }
        self.loading = false;
    }

    fn load_alignment(&mut self, document: &Document) {
        let alignment = self.current(document);
        self.has_alignment = alignment.is_some();
        self.load_horizontal(alignment);
        self.load_vertical(alignment);
        self.load_setting_out(document);
    }

    fn renumber_pis(&mut self) {
        for row in 0..self.pi_table.row_count() {
            self.pi_table.set_text(row, PI_INDEX, &format!("{row}"));
        }
    }

    fn load_horizontal(&mut self, alignment: Option<&Alignment>) {
        let alignment_name = alignment.map(|each| each.name.clone()).unwrap_or_default();
        let pis: Vec<AlignmentPi> = alignment
            .map(|each| each.horizontal.pis.clone())
            .unwrap_or_default();
        let keep_grid = self.pis_buffer.edited
            && alignment_name == self.pis_buffer.alignment
            && pis == self.pis_from;
        if !keep_grid {
            self.loading = true;
            self.pi_table.clear();
            for pi in &pis {
                self.pi_table.push_row(vec![
                    cell(""),
                    editable(exact_number(pi.point.x)),
                    editable(exact_number(pi.point.y)),
                    editable(exact_number(pi.radius)),
                    editable(exact_number(pi.spiral_in)),
                    editable(exact_number(pi.spiral_out)),
                ]);
            }
            self.renumber_pis();
            self.loading = false;
            self.pis_buffer = Buffer {
                edited: false,
                alignment: alignment_name.clone(),
            };
            self.pis_from = pis;
        }
        let start = alignment.map_or(0.0, |each| each.horizontal.start_station);
        if alignment_name != self.start_for || start != self.start_from {
            self.start_station = if alignment.is_some() {
                exact_number(start)
            } else {
                String::new()
            };
            self.start_for = alignment_name;
            self.start_from = start;
        }
    }

    fn load_vertical(&mut self, alignment: Option<&Alignment>) {
        let alignment_name = alignment.map(|each| each.name.clone()).unwrap_or_default();
        let profile: Option<VerticalAlignment> = alignment.and_then(|each| each.vertical.clone());
        // Unapplied edits survive a reload that did not touch what they were made to: another
        // alignment's edit, a label, an undo elsewhere.
        let keep_grid = self.pvis_buffer.edited
            && alignment_name == self.pvis_buffer.alignment
            && profile == self.pvis_from;
        if !keep_grid {
            self.loading = true;
            self.pvi_table.clear();
            if let Some(profile) = &profile {
                for pvi in &profile.pvis {
                    self.pvi_table.push_row(vec![
                        editable(exact_number(pvi.station)),
                        editable(exact_number(pvi.elevation)),
                        editable(exact_number(pvi.curve_length)),
                    ]);
                }
            }
            self.loading = false;
            self.pvis_buffer = Buffer {
                edited: false,
                alignment: alignment_name,
            };
            self.pvis_from = profile.clone();
        }

        self.elements.clear();
        let solved = profile
            .as_ref()
            .and_then(|definition| solve_profile(definition).ok());
        let mut extremes = Vec::new();
        if let Some(solved) = &solved {
            for element in solved.elements() {
                let curve = element.kind == ProfileElementKind::Curve;
                let k = curve_k(element).map(fixed3).unwrap_or_default();
                self.elements.push_row(vec![
                    cell(if curve { "curve" } else { "tangent" }),
                    cell(fixed3(element.start_station)),
                    cell(fixed3(element.start_station + element.length)),
                    cell(fixed3(element.start_grade * 100.0)),
                    cell(fixed3(element.end_grade * 100.0)),
                    cell(k),
                ]);
            }
            for point in solved.high_low_points() {
                extremes.push(format!(
                    "{} point at chainage {}, level {}",
                    if point.high { "High" } else { "Low" },
                    fixed3(point.station),
                    fixed3(point.elevation)
                ));
            }
        }
        self.high_low = if profile.is_none() {
            String::from("No design profile: fill the grid and Apply.")
        } else if extremes.is_empty() {
            String::from("No high or low point: no curve turns the grade through level.")
        } else {
            format!("{}.", extremes.join("; "))
        };
        self.preview_pvis = profile.map(|each| each.pvis).unwrap_or_default();
        self.preview_solved = solved;
    }

    fn setting_out(&self, document: &Document) -> Result<Vec<SettingOutStation>> {
        let Some(alignment) = self.current(document) else {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "no alignment is chosen",
            ));
        };
        let Some(spacing) = parse_finite_double(trimmed(self.interval.as_bytes())) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the interval is not a number",
                self.interval.as_str(),
            ));
        };
        let solved = solve_alignment(&alignment.horizontal)?;
        setting_out_stations(&solved, spacing)
    }

    fn load_setting_out(&mut self, document: &Document) {
        self.stations.clear();
        self.stations_note.set("", false);
        if self.current(document).is_none() {
            return;
        }
        let rows = match self.setting_out(document) {
            Ok(rows) => rows,
            Err(error) => {
                self.stations_note.set(error.describe(), true);
                return;
            }
        };
        let keys = rows.iter().filter(|row| !row.key.is_empty()).count();
        self.stations_note.set(
            format!("{} stations, {keys} of them key stations.", rows.len()),
            false,
        );
        for station in &rows {
            let radius = if station.curvature == 0.0 {
                String::from("straight")
            } else {
                format!(
                    "{}{}",
                    fixed3(1.0 / station.curvature.abs()),
                    if station.curvature > 0.0 { " L" } else { " R" }
                )
            };
            self.stations.push_row(vec![
                cell(fixed3(station.station)),
                cell(fixed3(station.point.x)),
                cell(fixed3(station.point.y)),
                cell(format_azimuth(station.azimuth)),
                cell(radius),
                cell(station.key.as_str()),
            ]);
        }
    }

    fn load_label_styles(&mut self, document: &Document) {
        let keep = self.label_style.current_text();
        self.label_style.clear();
        let label_style = &mut self.label_style;
        document.model().label_styles.for_each(|style| {
            if style.kind == LabelKind::Chainage {
                label_style.add_item(&style.name);
            }
        });
        if let Some(found) = self.label_style.find_text(&keep) {
            self.label_style.set_current_index(Some(found));
        }
    }

    // ---- acting -------------------------------------------------------------------------------

    fn use_selection(&mut self, document: &Document) {
        if !self.alive() {
            return;
        }
        let ids = document.selection().ids();
        let entity = if ids.len() == 1 {
            document.model().entities.find(ids[0])
        } else {
            None
        };
        let mut vertices = Vec::new();
        if let Some(entity) = entity {
            match &entity.geometry {
                Geometry::Line(line) => vertices = vec![line.start, line.end],
                Geometry::Polyline(polyline) => vertices = polyline.vertices.clone(),
                Geometry::CurvePolyline(_) => {
                    // Its vertices are the arcs' tangent points, not intersection points: taken as
                    // PIs they would lay a different road.
                    self.set_status(
                        "The selected polyline has arcs: its vertices are tangent points, not \
                         PIs. Type the PIs, or straighten it first.",
                        true,
                    );
                    return;
                }
                _ => {}
            }
        }
        if vertices.len() < 2 {
            self.set_status(
                format!(
                    "Select one line or polyline to take the PIs from ({} selected).",
                    ids.len()
                ),
                true,
            );
            return;
        }
        let words: Vec<String> = vertices
            .iter()
            .map(|vertex| format!("{},{}", exact_number(vertex.x), exact_number(vertex.y)))
            .collect();
        self.points = words.join(" ");
        self.set_status(
            format!("{} PIs from the selection's vertices.", vertices.len()),
            false,
        );
    }

    fn create_alignment(&mut self, host: &mut dyn Host) {
        let typed = String::from(self.name.trim());
        if typed.is_empty() {
            self.set_status("Type a name for the new alignment.", true);
            return;
        }
        let quoted = match command_word(&typed, "the name") {
            Ok(quoted) => quoted,
            Err(error) => {
                self.set_status(error.message, true);
                return;
            }
        };
        let pis = simplified(&self.points);
        if pis.is_empty() {
            self.set_status(
                "Type the PIs (x,y x,y ...) or take them From Selection.",
                true,
            );
            return;
        }
        if self.run(host, &format!("ALIGN NEW {quoted} {pis}")) {
            self.select_named(host.document(), &typed);
            self.name.clear();
            self.points.clear();
        }
    }

    fn apply_pis(&mut self, host: &mut dyn Host) {
        let mut pis = Vec::new();
        for row in 0..self.pi_table.row_count() {
            let values = match grid_numbers(
                &self.pi_table,
                row,
                PI_EASTING,
                PI_SPIRAL_OUT,
                &format!("PI {row}"),
            ) {
                Ok(values) => values,
                Err(error) => {
                    self.set_status(error.message, true);
                    return;
                }
            };
            if values.iter().all(String::is_empty) {
                continue; // a row added and never filled
            }
            if values[0].is_empty() || values[1].is_empty() {
                self.set_status(format!("PI {row} needs an easting and a northing."), true);
                return;
            }
            // The curve's numbers as far as the last one given, an empty one before it 0; a corner
            // with no curve at all is just x,y.
            let mut curve: Vec<String> = values[2..].to_vec();
            while curve
                .last()
                .is_some_and(|last| last.is_empty() || last == "0")
            {
                curve.pop();
            }
            for value in &mut curve {
                if value.is_empty() {
                    *value = String::from("0");
                }
            }
            if curve.is_empty() {
                pis.push(format!("{},{}", values[0], values[1]));
            } else {
                pis.push(format!("{},{},{}", values[0], values[1], curve.join(",")));
            }
        }
        if pis.len() < 2 {
            self.set_status("An alignment needs at least two PIs.", true);
            return;
        }
        // Applied, the reload finds PIs other than the grid was filled from and refills it from
        // the model; refused, the model is unchanged and the grid is kept for correcting.
        if let Some(line) = self.named("ALIGN PIS", &pis.join(" ")) {
            self.run(host, &line);
        }
    }

    fn apply_profile(&mut self, host: &mut dyn Host) {
        let mut pvis = Vec::new();
        for row in 0..self.pvi_table.row_count() {
            let values = match grid_numbers(
                &self.pvi_table,
                row,
                0,
                2,
                &format!("Profile row {}", row + 1),
            ) {
                Ok(values) => values,
                Err(error) => {
                    self.set_status(error.message, true);
                    return;
                }
            };
            let (station, level, length) = (&values[0], &values[1], &values[2]);
            if station.is_empty() && level.is_empty() && length.is_empty() {
                continue; // a row added and never filled
            }
            if station.is_empty() || level.is_empty() {
                self.set_status(
                    format!("Profile row {} needs a chainage and a level.", row + 1),
                    true,
                );
                return;
            }
            if length.is_empty() || length == "0" {
                pvis.push(format!("{station},{level}"));
            } else {
                pvis.push(format!("{station},{level},{length}"));
            }
        }
        if pvis.len() < 2 {
            self.set_status("A design profile needs at least two PVIs.", true);
            return;
        }
        // Applied, the reload finds a profile other than the one the grid was loaded from and
        // refills it from the model; refused, the model is unchanged and the grid is kept.
        if let Some(line) = self.named("ALIGN DESIGN", &pvis.join(" ")) {
            self.run(host, &line);
        }
    }

    fn copy_stations(&mut self, host: &mut dyn Host) {
        let text = format!("{}\n", self.stations.tab_separated().join("\n"));
        // QClipboard::setText cannot fail; a clipboard here can (the desktop's), and says why.
        if let Err(error) = host.clipboard().set_text(&text) {
            self.set_status(error.describe(), true);
            return;
        }
        self.set_status(
            format!("{} stations copied.", self.stations.row_count()),
            false,
        );
    }

    fn save_stations_csv(&self, document: &Document, path: &FilePath) -> Status {
        let rows = self.setting_out(document)?;
        let text = setting_out_csv(&rows);
        std::fs::write(path, text.as_bytes()).map_err(|error| {
            make_error(
                ErrorCode::FileExportFailure,
                format!("cannot write {}", path.display()),
                format!("{error}"),
            )
        })
    }

    fn save_stations(&mut self, host: &mut dyn Host) {
        if host.headless() {
            self.set_status(
                "A headless session opens no file dialog; ALIGN STATIONS name interval gives the \
                 same table.",
                true,
            );
            return;
        }
        let chosen = self.current_alignment();
        let Some(path) = save_file_dialog(
            "Save Setting-Out Table",
            &format!("{chosen}_setting_out.csv"),
            "CSV",
            "csv",
        ) else {
            return;
        };
        if let Err(error) = self.save_stations_csv(host.document(), FilePath::new(&path)) {
            self.set_status(error.describe(), true);
            return;
        }
        // QDir::toNativeSeparators.
        let shown = if cfg!(windows) {
            path.replace('/', "\\")
        } else {
            path
        };
        self.set_status(format!("Saved to {shown}."), false);
    }

    fn label_chainages(&mut self, host: &mut dyn Host) {
        let style = self.label_style.current_text();
        if style.is_empty() {
            self.set_status(
                "No label style labels chainages. Add one in Format > Label Styles (LABELSTYLE \
                 DEFAULTS adds the standard set).",
                true,
            );
            return;
        }
        let quoted_style = match command_word(&style, "the label style's name") {
            Ok(quoted) => quoted,
            Err(error) => {
                self.set_status(error.message, true);
                return;
            }
        };
        let mut options = format!("style={quoted_style}");
        let layer = String::from(self.label_layer.trim());
        if !layer.is_empty() {
            match command_word(&layer, "the layer") {
                Ok(quoted) => options.push_str(&format!(" layer={quoted}")),
                Err(error) => {
                    self.set_status(error.message, true);
                    return;
                }
            }
        }
        if let Some(line) = self.named("LABEL ALIGN", &options) {
            self.run(host, &line);
        }
    }

    /// A cell of a grid typed into: an edit marks the grid and nothing more - a line is run by
    /// Apply, never from the table's own change (docs/desktop.md, "The rules a dialog or panel
    /// follows").
    fn cell_edited(&mut self, table: &str) {
        if self.loading {
            return;
        }
        match table {
            "alignmentPiTable" => self.pis_buffer.edited = true,
            "alignmentPviTable" => self.pvis_buffer.edited = true,
            _ => {}
        }
    }

    // ---- drawing ------------------------------------------------------------------------------

    fn list_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if self.list.ui(ui).selection_changed && !self.loading {
            self.load_alignment(host.document());
        }
        egui::Grid::new("alignmentNewForm").show(ui, |ui| {
            ui.label("Name:");
            field(
                ui,
                "alignmentName",
                &mut self.name,
                "name",
                "The new alignment's name",
            );
            ui.end_row();
            ui.label("PIs:");
            field(
                ui,
                "alignmentPoints",
                &mut self.points,
                "x,y x,y ...",
                "The new alignment's PIs, easting first, at least two",
            );
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if button(
                ui,
                "alignmentUseSelection",
                "From Selection",
                "Take the PIs from the selected line or polyline's vertices",
            )
            .clicked()
            {
                self.use_selection(host.document());
            }
            if button(ui, "alignmentNew", "New", "ALIGN NEW name x,y x,y ...").clicked() {
                self.create_alignment(host);
            }
            let enabled = self.has_alignment;
            ui.add_enabled_ui(enabled, |ui| {
                if button(
                    ui,
                    "alignmentDelete",
                    "Delete",
                    "ALIGN DELETE name: the chosen alignment (one undo step)",
                )
                .clicked()
                    && let Some(line) = self.named("ALIGN DELETE", "")
                {
                    self.run(host, &line);
                }
            });
        });
    }

    fn horizontal_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        note(
            ui,
            "The PIs in order, counted from 0: edit the grid, then Apply - every PI is one ALIGN \
             PIS line, one undo step. Radius 0 is a corner with no curve; the first and last PI \
             carry none.",
        );
        let events = self.pi_table.ui(ui);
        if !events.edited.is_empty() {
            self.cell_edited("alignmentPiTable");
        }
        let enabled = self.has_alignment;
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                if button(
                    ui,
                    "alignmentAddPi",
                    "Add Row",
                    "A row for another PI, at the end",
                )
                .clicked()
                {
                    self.loading = true;
                    let mut cells = vec![cell("")];
                    cells.extend((PI_EASTING..=PI_SPIRAL_OUT).map(|_| editable("")));
                    let row = self.pi_table.push_row(cells);
                    self.renumber_pis();
                    self.loading = false;
                    self.pis_buffer.edited = true;
                    self.pi_table.select_row(row);
                }
                if button(
                    ui,
                    "alignmentRemovePi",
                    "Remove Row",
                    "Drop the chosen row's PI",
                )
                .clicked()
                {
                    match self.pi_table.current_row() {
                        None => self.set_status("Choose the PI to remove in the grid first.", true),
                        Some(row) => {
                            self.loading = true;
                            self.pi_table.remove_row(row);
                            self.renumber_pis();
                            self.loading = false;
                            self.pis_buffer.edited = true;
                        }
                    }
                }
                if button(
                    ui,
                    "alignmentRevertPis",
                    "Revert",
                    "Put back the PIs the drawing holds",
                )
                .clicked()
                {
                    self.pis_buffer.edited = false;
                    let alignment = self.current(host.document());
                    self.load_horizontal(alignment);
                }
                if button(
                    ui,
                    "alignmentApplyPis",
                    "Apply PIs",
                    "ALIGN PIS name x,y[,radius[,spiralIn[,spiralOut]]] ...: the grid as the PIs",
                )
                .clicked()
                {
                    self.apply_pis(host);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Start chainage:");
            field(
                ui,
                "alignmentStartStation",
                &mut self.start_station,
                "0",
                "The chainage the alignment starts at",
            );
            ui.add_enabled_ui(enabled, |ui| {
                if button(
                    ui,
                    "alignmentStart",
                    "Set Start Chainage",
                    "ALIGN START name chainage",
                )
                .clicked()
                {
                    let chainage = String::from(self.start_station.trim());
                    if chainage.is_empty() {
                        self.set_status("Type the chainage the alignment starts at.", true);
                    } else if let Some(line) = self.named("ALIGN START", &chainage) {
                        self.run(host, &line);
                    }
                }
            });
        });
    }

    fn vertical_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        note(
            ui,
            "The PVIs, by chainage: edit the grid, then Apply - the whole profile is one ALIGN \
             DESIGN line, one undo step. The first and last PVI carry no curve.",
        );
        let events = self.pvi_table.ui(ui);
        if !events.edited.is_empty() {
            self.cell_edited("alignmentPviTable");
        }
        let enabled = self.has_alignment;
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                if button(ui, "alignmentAddPvi", "Add Row", "A row for another PVI").clicked() {
                    self.loading = true;
                    let row =
                        self.pvi_table
                            .push_row(vec![editable(""), editable(""), editable("")]);
                    self.loading = false;
                    self.pvis_buffer.edited = true;
                    self.pvi_table.select_row(row);
                }
                if button(
                    ui,
                    "alignmentRemovePvi",
                    "Remove Row",
                    "Drop the chosen row",
                )
                .clicked()
                {
                    match self.pvi_table.current_row() {
                        None => self.set_status("Choose the row to remove first.", true),
                        Some(row) => {
                            self.pvi_table.remove_row(row);
                            self.pvis_buffer.edited = true;
                        }
                    }
                }
                if button(
                    ui,
                    "alignmentRevertProfile",
                    "Revert",
                    "Put back the profile the drawing holds",
                )
                .clicked()
                {
                    self.pvis_buffer.edited = false;
                    let alignment = self.current(host.document());
                    self.load_vertical(alignment);
                }
                if button(
                    ui,
                    "alignmentApplyProfile",
                    "Apply Profile",
                    "ALIGN DESIGN name s,z[,L] ...: the grid as the design profile",
                )
                .clicked()
                {
                    self.apply_profile(host);
                }
                if button(
                    ui,
                    "alignmentClearProfile",
                    "Clear Profile",
                    "ALIGN CLEARPROFILE name: remove the design profile",
                )
                .clicked()
                    && let Some(line) = self.named("ALIGN CLEARPROFILE", "")
                {
                    // What the grid held goes with the profile.
                    self.pvis_buffer.edited = false;
                    self.run(host, &line);
                }
            });
        });
        self.elements.ui(ui);
        crate::drive::controls::label(ui, "alignmentHighLow", &self.high_low);
        let width = ui.available_width().max(200.0);
        let pvis = self.preview_pvis.as_slice();
        let solved = self.preview_solved.as_ref();
        preview(
            ui,
            "alignmentProfilePreview",
            egui::vec2(width, 120.0),
            self.raster.as_ref(),
            &mut self.texture,
            &mut |canvas: &mut dyn Canvas| {
                paint_profile_preview(
                    canvas,
                    RectF::new(0.0, 0.0, f64::from(width), 120.0),
                    pvis,
                    solved,
                );
            },
        );
    }

    fn setting_out_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        note(
            ui,
            "Every station on the interval and every key station - TS, SC, CS, ST, TC, CT, a PI \
             with no curve - as ALIGN STATIONS prints them. Azimuths are clockwise from grid \
             north.",
        );
        ui.horizontal(|ui| {
            ui.label("Interval:");
            // As it is typed: the table is cheap to make (setting_out_stations caps it), and a
            // headless --fill sets the text without finishing an edit.
            if field(
                ui,
                "alignmentInterval",
                &mut self.interval,
                "20",
                "The chainage interval, in model units",
            )
            .changed()
            {
                self.load_setting_out(host.document());
            }
            // What the table holds, or why it holds nothing: its own line, so a half-typed
            // interval never overwrites what the last line replied.
            self.stations_note.ui(ui, "alignmentStationsNote");
        });
        self.stations.ui(ui);
        let enabled = self.has_alignment;
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                if button(
                    ui,
                    "alignmentStationsCopy",
                    "Copy",
                    "The table to the clipboard, tab separated, for a spreadsheet",
                )
                .clicked()
                {
                    self.copy_stations(host);
                }
                if button(
                    ui,
                    "alignmentStationsCsv",
                    "Save CSV...",
                    "The table as a CSV file",
                )
                .clicked()
                {
                    self.save_stations(host);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Label style:");
            self.label_style.ui(ui, "alignmentLabelStyle");
            ui.label("Layer:");
            field(
                ui,
                "alignmentLabelLayer",
                &mut self.label_layer,
                "layer 0",
                "The layer the labels go on, which must exist; empty for layer 0, as LABEL ALIGN \
                 puts them",
            );
            ui.add_enabled_ui(enabled, |ui| {
                if button(
                    ui,
                    "alignmentLabelChainages",
                    "Label Chainages",
                    "LABEL ALIGN name style=...: chainage labels along the alignment, one undo \
                     step",
                )
                .clicked()
                {
                    self.label_chainages(host);
                }
            });
        });
    }

    fn body_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        note(
            ui,
            "An alignment is defined by its PIs, each with the radius and spirals of the curve \
             that rounds it; its design profile by its PVIs. Every change runs an ALIGN line on \
             the command line - it is echoed there and is one undo step.",
        );
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| self.list_ui(ui, host));
            ui.vertical(|ui| {
                tabs(ui, "alignmentTabs", &TABS, &mut self.tab);
                match self.tab {
                    0 => self.horizontal_ui(ui, host),
                    1 => self.vertical_ui(ui, host),
                    _ => self.setting_out_ui(ui, host),
                }
            });
        });
    }

    fn footer_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            self.status.ui(ui, "alignmentStatus");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button(ui, "alignmentClose", "Close", "").clicked() {
                    self.close = true;
                }
            });
        });
    }
}

/// The Alignment Manager (see the module comment).
pub struct AlignmentManagerDialog {
    frame: DialogFrame,
    open: bool,
    body: Body,
}

impl AlignmentManagerDialog {
    /// The manager over `document`, shown, its first alignment chosen.
    pub fn new(document: &Document) -> Self {
        let mut body = Body {
            watcher: DocumentWatcher::new(document),
            executor: true,
            list: Table::new(
                "alignmentList",
                &["Name", "PIs", "Start", "End", "Length", "Profile"],
            ),
            name: String::new(),
            points: String::new(),
            tab: 0,
            pi_table: Table::new(
                "alignmentPiTable",
                &[
                    "#",
                    "Easting",
                    "Northing",
                    "Radius",
                    "Spiral in",
                    "Spiral out",
                ],
            )
            .editing(),
            start_station: String::new(),
            pvi_table: Table::new("alignmentPviTable", &["Chainage", "Level", "Curve length"])
                .editing(),
            elements: Table::new(
                "alignmentProfileElements",
                &["Element", "From", "To", "Grade in %", "Grade out %", "K"],
            ),
            high_low: String::new(),
            preview_pvis: Vec::new(),
            preview_solved: None,
            raster: None,
            texture: None,
            interval: String::from("20"),
            stations_note: StatusLine::default(),
            stations: Table::new(
                "alignmentStations",
                &[
                    "Chainage", "Easting", "Northing", "Azimuth", "Radius", "Key",
                ],
            )
            .max_height(260.0),
            label_style: Combo::new(),
            label_layer: String::new(),
            status: StatusLine::default(),
            has_alignment: false,
            loading: false,
            pis_buffer: Buffer::default(),
            pis_from: Vec::new(),
            pvis_buffer: Buffer::default(),
            pvis_from: None,
            start_for: String::new(),
            start_from: 0.0,
            close: false,
        };
        body.reload(document);
        Self {
            // Non-modal: kept open beside the drawing while PIs are picked off it.
            frame: DialogFrame::new("alignmentManagerDialog", "Alignment Manager"),
            open: true,
            body,
        }
    }

    /// A manager with no executor (the C++'s empty CommandRunner, a dialog built without a
    /// window): it says it cannot run the line.
    pub fn without_executor(mut self) -> Self {
        self.body.executor = false;
        self
    }

    /// The window's raster canvas, for the profile preview to be painted on screen.
    pub fn set_raster(&mut self, raster: PreviewRaster) {
        self.body.raster = Some(raster);
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn hide(&mut self) {
        self.open = false;
    }

    /// Chooses the alignment `name` in the list; false when there is none.
    pub fn select_alignment(&mut self, document: &Document, name: &str) -> bool {
        self.body.select_named(document, name)
    }

    /// The chosen alignment's name; empty when there is none.
    pub fn current_alignment(&self) -> String {
        self.body.current_alignment()
    }

    /// What Save CSV writes, to `path`, without asking where: the setting-out table of the chosen
    /// alignment at the interval shown.
    pub fn save_stations_csv(&self, document: &Document, path: &FilePath) -> Status {
        self.body.save_stations_csv(document, path)
    }

    /// The tables, by their object names.
    pub fn table(&self, name: &str) -> Option<&Table> {
        let body = &self.body;
        [
            &body.list,
            &body.pi_table,
            &body.pvi_table,
            &body.elements,
            &body.stations,
        ]
        .into_iter()
        .find(|table| table.name() == name)
    }

    /// A cell of the grid `table` typed into, as a person commits an edit
    /// (QTableWidgetItem::setText): it marks the grid for Apply and runs nothing. False when there
    /// is no such cell.
    pub fn edit_cell(&mut self, table: &str, row: usize, column: usize, text: &str) -> bool {
        let body = &mut self.body;
        let grid = match table {
            "alignmentPiTable" => &mut body.pi_table,
            "alignmentPviTable" => &mut body.pvi_table,
            _ => return false,
        };
        if grid.cell(row, column).is_none() {
            return false;
        }
        if grid.set_text(row, column, text) {
            body.cell_edited(table);
        }
        true
    }

    /// QTableWidget::setCurrentCell on a grid: its row chosen. False when there is no such row.
    pub fn set_current_cell(&mut self, table: &str, row: usize) -> bool {
        let body = &mut self.body;
        let grid = match table {
            "alignmentPiTable" => &mut body.pi_table,
            "alignmentPviTable" => &mut body.pvi_table,
            _ => return false,
        };
        if row >= grid.row_count() {
            return false;
        }
        grid.select_row(row);
        true
    }

    /// The profile preview painted into `canvas` over `rect`, as it is on screen.
    pub fn paint_preview(&self, canvas: &mut dyn Canvas, rect: RectF) {
        paint_profile_preview(
            canvas,
            rect,
            &self.body.preview_pvis,
            self.body.preview_solved.as_ref(),
        );
    }

    /// Draws the manager for this frame, when it is shown; then the watcher's delivery, as the
    /// C++'s event loop delivered it after a click (bim::corridor_dialog's `ui` says why).
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if self.open {
            let ctx = ui.ctx().clone();
            let mut lent = (&mut self.body, &mut *host);
            let shown = self.frame.show(
                &ctx,
                &mut lent,
                |ui, lent| lent.0.body_ui(ui, &mut *lent.1),
                |ui, lent| lent.0.footer_ui(ui),
            );
            if shown.closed || std::mem::take(&mut self.body.close) {
                self.open = false;
            }
        }
        if let Some(changes) = self.body.watcher.deliver(host.document())
            && changes.model
        {
            self.body.reload(host.document());
            ui.ctx().request_repaint();
        }
    }
}
