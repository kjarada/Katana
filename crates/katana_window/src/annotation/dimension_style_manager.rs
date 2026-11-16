//! The Dimension Styles manager (Format > Dimension Styles..., docs/annotation.md "Dimension
//! styles"): the drawing's dimension styles, how many layers use each, and a form for the chosen
//! one with a preview of what a dimension reads as and a drawn sample. The port of
//! annotation/dimension_style_manager.
//!
//! It changes nothing itself. Every button builds the DIMSTYLE line a person would type - with the
//! field words of the cad layer's dimension_style_verbs, so it cannot write a line the verb reads
//! differently - and runs it through the window's one executor (host::Host::run): echoed in the
//! command log, one undo step, refused in the verb's own words, which the problem line shows.
//! Apply sends only the fields the form changed, as ONE line; with none changed it sends nothing
//! and makes no step. The dialog keeps no copy of the table: it reloads from the document on every
//! change, and keeps a form's unapplied edits only while the style they were made to is
//! unchanged.
//!
//! Object names, for the headless driver and the tests:
//!
//! ```text
//! dimensionStyleManagerDialog   the dialog
//! dimStyleList                  the styles: Style | Used by
//! dimStyleNewName               the name New and Duplicate give (blank: a fresh one, shown as
//!                               the placeholder)
//! dimStyleNew, dimStyleDuplicate, dimStyleDelete, dimStyleRevert, dimStyleApply
//! dimStyleText, dimStyleGap, dimStylePrefix, dimStyleSuffix           Text
//! dimStyleExtOff, dimStyleExtBeyond, dimStyleArrow, dimStyleHead      Lines and arrows
//! dimStyleScale, dimStyleDecimals, dimStyleRound, dimStyleTrim        Units
//! dimStylePaper                 paper-sized (drawn at the annotation scale)
//! dimStyleUnits                 what the sizes are in
//! dimStylePreview               "a dimension of 10 reads as ..."
//! dimStyleSample                the sample, drawn by the plan painter
//! dimStyleProblem               the last refusal, in the verb's words
//! ```
//!
//! What is the C++'s and not here: the Arrowhead choice's items carried each head's picture as an
//! icon; an egui choice lists text, so the chosen head's picture is drawn beside it (each head's
//! picture is made as the C++ made it, `head_picture`).

use std::sync::Arc;

use egui::{Context, Ui};
use katana_cad::annotation::command_words::command_word;
use katana_cad::annotation::dimension_style_verbs::{
    dimension_style_changes, layers_using_dimension_style,
};
use katana_cad::dimension_draw::{build_dimension, dimension_style_at_scale};
use katana_cad::{Document, DocumentChange};
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::annotation::DEFAULT_ANNOTATION_SCALE;
use katana_entity::dimension_text::format_measurement;
use katana_entity::entity::to_string;
use katana_entity::tables::{DEFAULT_DIMENSION_STYLE_NAME, arrow_head_from_string};
use katana_entity::{ArrowHead, DimensionGeometry, DimensionStyle, Geometry, validate};
use katana_geometry::Point2;

use super::dialog_support::{Followed, TableLook, show_dialog, table_look};
use super::form_widgets::{button, check, describe, edit, problem_line};
use crate::canvas::{Canvas, Color, Image, Pen, PenCap, PenJoin, PenStyle, RasterCanvas, RenderHint};
use crate::command_word::exact_number;
use crate::customisation::style_painter::CanvasRasterBackend;
use crate::dialog_layout as layout;
use crate::drive::controls::{self, Choice};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::host::Host;
use crate::plan_painter::{PlanFrame, PlanPaintCache, PlanPaintOptions, paint_plan_geometry};
use crate::shell::DialogFrame;
use crate::shell::dialog;
use crate::theme::palette;

/// The dialog's object name.
pub const DIALOG_NAME: &str = "dimensionStyleManagerDialog";

/// The sample's size, and the length it measures: ten units, the dimension the preview line reads
/// out, so the picture and the words agree.
const SAMPLE_WIDTH: u32 = 300;
const SAMPLE_HEIGHT: u32 = 110;
const SAMPLE_LENGTH: f64 = 10.0;

/// A head's picture's size (QComboBox::setIconSize(36, 14)).
const HEAD_WIDTH: u32 = 36;
const HEAD_HEIGHT: u32 = 14;

/// The heads, in the order the verb's help lists them, with the words a person reads.
const HEADS: [(ArrowHead, &str); 5] = [
    (ArrowHead::None, "None"),
    (ArrowHead::Tick, "Tick"),
    (ArrowHead::ClosedFilled, "Closed filled"),
    (ArrowHead::Open, "Open"),
    (ArrowHead::Dot, "Dot"),
];

/// The range of decimal places validate() allows (tables.rs: twelve is where a double stops
/// having digits to give).
const MOST_DECIMALS: i64 = 12;

/// A typed number, or the sentence that says which field did not read.
fn number_in(text: &str, what: &str) -> Result<f64> {
    let text = text.trim();
    parse_finite_double(text).ok_or_else(|| {
        Error::new(
            ErrorCode::ParseFailure,
            format!("{what} must be a number, not '{text}'"),
        )
    })
}

/// A dimension of `length` along x, `offset` below its points, drawn with `style` into `width` by
/// `height` pixels by the plan painter: the look of the style, not an imitation of it. Transparent
/// where nothing is drawn.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ helper's own parameters, in its order."
)]
fn drawn_dimension(
    style: &DimensionStyle,
    scale: f64,
    width: u32,
    height: u32,
    length: f64,
    offset: f64,
    text_override: &str,
    ink: Color,
) -> Image {
    let dimension = DimensionGeometry {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(length, 0.0),
        offset,
        text_override: String::from(text_override),
        ..DimensionGeometry::default()
    };
    let mut image = Image::new(width, height);
    let drawing = build_dimension(&dimension, style, scale);
    if drawing.empty() || drawing.extent.empty() {
        return image;
    }
    let mut frame = PlanFrame::default();
    frame
        .transform
        .resize(f64::from(width), f64::from(height));
    frame.transform.fit(&drawing.extent, 0.08);
    let options = PlanPaintOptions {
        annotation_scale: scale,
        raster: Some(Arc::new(CanvasRasterBackend)),
        ..PlanPaintOptions::default()
    };
    let mut cache = PlanPaintCache::new();
    let geometry = Geometry::Dimension(dimension);
    RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        canvas.set_pen(Pen::with(
            ink,
            1.2,
            PenStyle::Solid,
            PenCap::Square,
            PenJoin::Bevel,
        ));
        paint_plan_geometry(canvas, &frame, &options, &mut cache, &geometry, style, None);
    });
    image
}

/// Each head's picture, as a dimension with that head and no text is: a line with the head at
/// both ends.
fn head_picture(head: ArrowHead) -> Image {
    let look = DimensionStyle {
        arrow_head: head,
        arrow_size: 3.0,
        extension_offset: 0.0,
        extension_beyond: 0.0,
        ..DimensionStyle::default()
    };
    drawn_dimension(
        &look,
        DEFAULT_ANNOTATION_SCALE,
        HEAD_WIDTH,
        HEAD_HEIGHT,
        14.0,
        0.0,
        " ",
        palette::text(),
    )
}

/// What a click asked for, done after the frame drew it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pressed {
    New,
    Duplicate,
    Delete,
    Revert,
    Apply,
    Close,
}

/// The width of the arrowhead's choice (the look of 2026-11-16), so the three forms sit side by
/// side. A number, a prefix and a suffix are a number box's width (short_row).
const VALUE_WIDTH: f32 = 110.0;

/// The style list's column: the list, the name field and the buttons under it at one width, the
/// list's header and chosen row spanning it - the list was as wide as its two columns' words
/// (204) over a field of 232 (the third look audit's J). egui's own field width less a group's
/// margins, which the field had.
const LIST_WIDTH: f32 = 232.0;

/// The width the body is laid out at: the style list beside the three short forms, each a label
/// column and a value (VALUE_WIDTH), the arrowhead's picture beside its choice - about 220 for the
/// list and 230 for each form, and their gaps, at the Standard text size. At the frame's own
/// BODY_WIDTH (520) the second and third forms had no room left in the row, and their fields were
/// drawn nothing wide.
const BODY_WIDTH: f32 = 980.0;

/// A row of one of the three forms: `label` in its form's label column, and what `add` draws
/// beside it VALUE_WIDTH wide rather than filling the row, so the three forms sit side by side.
fn value_row(form: &mut dialog::Form<'_>, label: &str, add: impl FnOnce(&mut Ui)) {
    form.row(label, |ui| {
        dialog::fixed_fields(ui, VALUE_WIDTH);
        add(ui);
    });
}

/// A row of one of the three forms holding a number, or a short word (a prefix, a suffix): its
/// field a number box's width (controls::NUMBER_FIELD_WIDTH), as the Text Styles' sizes are - at
/// VALUE_WIDTH the sizes here were 110 wide against 65 there (the third look audit's C).
fn short_row(form: &mut dialog::Form<'_>, label: &str, add: impl FnOnce(&mut Ui)) {
    form.row(label, |ui| {
        dialog::fixed_fields(ui, controls::NUMBER_FIELD_WIDTH);
        add(ui);
    });
}

/// Format > Dimension Styles... (see the module comment).
pub struct DimensionStyleManagerDialog {
    frame: DialogFrame,
    /// False for a dialog given no command line (the C++'s empty runner).
    command_line: bool,
    followed: Followed,
    /// The list's rows: the style, and what uses it.
    rows: Vec<Vec<String>>,
    current: Option<usize>,
    new_name: String,
    new_name_placeholder: String,
    text: String,
    gap: String,
    prefix: String,
    suffix: String,
    ext_off: String,
    ext_beyond: String,
    arrow: String,
    head: Choice,
    scale: String,
    decimals: i64,
    round: String,
    trim: bool,
    paper: bool,
    units: String,
    preview: String,
    sample: Image,
    /// What the sample was last drawn for, so a frame that changed neither draws nothing again.
    sampled: Option<(DimensionStyle, u64)>,
    problem: String,
    shown: String,
    /// The style as the form was last loaded with it: a refresh keeps the form's edits while the
    /// stored style still equals this.
    loaded: DimensionStyle,
    /// The drawing's annotation scale at the last look: what a paper-sized sample is drawn at.
    annotation_scale: f64,
    head_pictures: Vec<Image>,
    sample_texture: Option<egui::TextureHandle>,
    head_textures: Vec<egui::TextureHandle>,
    pressed: Option<Pressed>,
}

impl DimensionStyleManagerDialog {
    /// The manager over `document`, showing Standard; its lines run through the host it is drawn
    /// with.
    pub fn new(document: &Document) -> Self {
        let mut dialog = Self {
            frame: DialogFrame::new(DIALOG_NAME, "Dimension Styles").width(BODY_WIDTH),
            command_line: true,
            followed: Followed::new(document),
            rows: Vec::new(),
            current: None,
            new_name: String::new(),
            new_name_placeholder: String::new(),
            text: String::new(),
            gap: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            ext_off: String::new(),
            ext_beyond: String::new(),
            arrow: String::new(),
            head: Choice::at(0),
            scale: String::new(),
            decimals: 0,
            round: String::new(),
            trim: false,
            paper: false,
            units: String::new(),
            preview: String::new(),
            sample: Image::default(),
            sampled: None,
            problem: String::new(),
            shown: String::new(),
            loaded: DimensionStyle::default(),
            annotation_scale: document.annotation_scale(),
            head_pictures: HEADS.iter().map(|&(head, _)| head_picture(head)).collect(),
            sample_texture: None,
            head_textures: Vec::new(),
            pressed: None,
        };
        dialog.refresh(document);
        dialog
    }

    /// The same manager given no command line to run on.
    pub fn without_command_line(mut self) -> Self {
        self.command_line = false;
        self
    }

    /// The dialog's object name.
    pub fn name(&self) -> &'static str {
        DIALOG_NAME
    }

    /// What the drawing did since the dialog last looked, seen now (dialog_support): styles and the
    /// layers that use them, and the scale a paper-sized sample is drawn at.
    fn look(&mut self, document: &Document) {
        if self.followed.take().has(
            DocumentChange::DIMENSION_STYLES
                | DocumentChange::LAYERS
                | DocumentChange::METADATA
                | DocumentChange::REPLACED,
        ) {
            self.refresh(document);
        }
    }

    /// The name New and Duplicate give: what is typed, else `base`, else `base 2`, `base 3`...
    fn name_for_new(&self, document: &Document, base: &str) -> String {
        let typed = self.new_name.trim();
        if !typed.is_empty() {
            return String::from(typed);
        }
        let styles = &document.model().dimension_styles;
        let mut name = String::from(base);
        let mut n = 2;
        while styles.contains(&name) {
            name = format!("{base} {n}");
            n += 1;
        }
        name
    }

    fn refresh(&mut self, document: &Document) {
        let model = document.model();
        self.annotation_scale = document.annotation_scale();
        self.rows.clear();
        model.dimension_styles.for_each(|style| {
            let users = layers_using_dimension_style(model, &style.name).len();
            let mut used_by = match users {
                0 => String::from("no layer"),
                1 => String::from("1 layer"),
                n => format!("{n} layers"),
            };
            if style.name == DEFAULT_DIMENSION_STYLE_NAME {
                used_by.push_str(" (the default)");
            }
            self.rows.push(vec![style.name.clone(), used_by]);
        });
        self.new_name_placeholder = self.name_for_new(document, "Dimension Style");

        let keep = if self.shown.is_empty() {
            String::from(DEFAULT_DIMENSION_STYLE_NAME)
        } else {
            self.shown.clone()
        };
        let stored = model.dimension_styles.find(&keep);
        if stored.is_some_and(|stored| *stored == self.loaded) && !self.shown.is_empty() {
            // The style is as the form was loaded with it: keep what is being typed, and only put
            // the list's mark back on it.
            self.current = self.rows.iter().position(|row| row[0] == keep);
            self.show_preview(); // the scale may have moved
            return;
        }
        self.select_style(document, &keep);
    }

    /// Shows the style `name` (Standard when there is none of that name).
    pub fn select(&mut self, document: &Document, name: &str) {
        self.look(document);
        self.select_style(document, name);
    }

    fn select_style(&mut self, document: &Document, name: &str) {
        let styles = &document.model().dimension_styles;
        let target = if styles.contains(name) {
            String::from(name)
        } else {
            String::from(DEFAULT_DIMENSION_STYLE_NAME)
        };
        if let Some(at) = self.rows.iter().position(|row| row[0] == target) {
            self.current = Some(at);
        }
        if let Some(style) = styles.find(&target) {
            let style = style.clone();
            self.show_style(&style);
        }
    }

    fn show_style(&mut self, style: &DimensionStyle) {
        self.shown.clone_from(&style.name);
        self.loaded = style.clone();
        self.text = exact_number(style.text_height);
        self.gap = exact_number(style.text_gap);
        self.prefix.clone_from(&style.prefix);
        self.suffix.clone_from(&style.suffix);
        self.ext_off = exact_number(style.extension_offset);
        self.ext_beyond = exact_number(style.extension_beyond);
        self.arrow = exact_number(style.arrow_size);
        self.head = Choice {
            index: HEADS.iter().position(|&(head, _)| head == style.arrow_head),
            ..Choice::default()
        };
        self.scale = exact_number(style.unit_scale);
        self.decimals = i64::from(style.decimals).clamp(0, MOST_DECIMALS);
        self.round = exact_number(style.round_to);
        self.trim = style.suppress_trailing_zeros;
        self.paper = style.paper_sized;
        self.show_preview();
    }

    /// What the sizes are in, said under the form.
    fn units_text(&self) -> String {
        if self.paper {
            format!(
                "Sizes are millimetres on paper, drawn at 1:{} here (the drawing's annotation \
                 scale).",
                exact_number(self.annotation_scale)
            )
        } else {
            String::from(
                "Sizes are model units: the dimension is drawn at its size on the ground.",
            )
        }
    }

    /// The preview line and the sample, from the form as it stands.
    fn show_preview(&mut self) {
        self.units = self.units_text();
        let style = match self.form_style() {
            Ok(style) => style,
            Err(error) => {
                self.preview = describe(&Err(error));
                self.sample = Image::default();
                self.sampled = None;
                return;
            }
        };
        if let Err(error) = validate(&style) {
            self.preview = describe(&Err(error));
            self.sample = Image::default();
            self.sampled = None;
            return;
        }
        // The formatter DIMSTYLE LIST and INFO read it out with.
        let reads = format_measurement(SAMPLE_LENGTH, &style);
        self.preview = format!(
            "a dimension of {} reads as {reads}",
            exact_number(SAMPLE_LENGTH)
        );
        let scale = self.annotation_scale;
        let key = (style.clone(), scale.to_bits());
        if self.sampled.as_ref() == Some(&key) {
            return;
        }
        // The dimension line above its points by two and a half texts: room for the text above the
        // line whatever the sizes.
        let text_model = dimension_style_at_scale(&style, scale).text_height;
        self.sample = drawn_dimension(
            &style,
            scale,
            SAMPLE_WIDTH,
            SAMPLE_HEIGHT,
            SAMPLE_LENGTH,
            2.5 * text_model,
            "",
            palette::text(),
        );
        self.sampled = Some(key);
        self.sample_texture = None;
    }

    /// The style the form shows.
    pub fn current(&self) -> &str {
        &self.shown
    }

    /// What dimStyleNewName shows while it is empty: the name New would give.
    pub fn new_name_placeholder(&self) -> &str {
        &self.new_name_placeholder
    }

    /// The form as a style; an error naming the field that does not read.
    pub fn form_style(&self) -> Result<DimensionStyle> {
        let mut style = self.loaded.clone();
        style.name.clone_from(&self.shown);
        style.text_height = number_in(&self.text, "the text height")?;
        style.text_gap = number_in(&self.gap, "the gap")?;
        style.extension_offset = number_in(&self.ext_off, "the extension offset")?;
        style.extension_beyond = number_in(&self.ext_beyond, "the extension beyond")?;
        style.arrow_size = number_in(&self.arrow, "the arrow size")?;
        style.unit_scale = number_in(&self.scale, "the unit scale")?;
        style.round_to = number_in(&self.round, "the rounding")?;
        style.prefix.clone_from(&self.prefix);
        style.suffix.clone_from(&self.suffix);
        if let Some(&(head, _)) = self.head.index.and_then(|at| HEADS.get(at))
            && let Ok(read) = arrow_head_from_string(to_string(head))
        {
            style.arrow_head = read;
        }
        style.decimals = i32::try_from(self.decimals).unwrap_or(0);
        style.suppress_trailing_zeros = self.trim;
        style.paper_sized = self.paper;
        Ok(style)
    }

    /// The line Apply runs: empty when the form changes nothing.
    pub fn apply_line(&self, document: &Document) -> Result<String> {
        let Some(stored) = document.model().dimension_styles.find(&self.shown) else {
            return Err(Error::new(
                ErrorCode::NotFound,
                "choose a dimension style first",
            ));
        };
        let form = self.form_style()?;
        let changes = dimension_style_changes(stored, &form)?;
        if changes.is_empty() {
            return Ok(String::new());
        }
        let name = command_word(&stored.name)?;
        Ok(format!("DIMSTYLE SET {name} {changes}"))
    }

    /// What the last refused action said; empty after one that worked.
    pub fn problem(&self) -> &str {
        &self.problem
    }

    /// "a dimension of 10 reads as ...", or why the form does not read.
    pub fn preview(&self) -> &str {
        &self.preview
    }

    /// What the sizes are in.
    pub fn units(&self) -> &str {
        &self.units
    }

    /// The sample as last drawn: transparent where the dimension is not; null when the form does
    /// not read.
    pub fn sample(&self) -> &Image {
        &self.sample
    }

    /// Each head's picture, in the choice's order.
    pub fn head_pictures(&self) -> &[Image] {
        &self.head_pictures
    }

    /// Runs `line`; its refusal, or the reason it could not be built, goes to the problem line.
    fn run(&mut self, host: &mut dyn Host, line: Result<String>) -> bool {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                self.problem = describe(&Err(error));
                return false;
            }
        };
        if !self.command_line {
            self.problem =
                String::from("there is no command line to run the dimension style line on");
            return false;
        }
        let outcome = host.run(&line);
        self.problem = if outcome.ok {
            String::new()
        } else if outcome.error.is_empty() {
            format!("refused: {line}")
        } else {
            outcome.error.clone()
        };
        self.look(host.document());
        outcome.ok
    }

    /// Apply: the fields the form changed stored as one line, one undo step; nothing, and true,
    /// when it changed none.
    pub fn apply(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let line = self.apply_line(host.document());
        if let Ok(line) = &line
            && line.is_empty()
        {
            self.problem.clear();
            return true; // nothing changed: no step
        }
        self.run(host, line)
    }

    /// New: a style with the standard sizes, named from dimStyleNewName or the fresh name its
    /// placeholder shows.
    pub fn new_style(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let name = self.name_for_new(host.document(), "Dimension Style");
        let word = match command_word(&name) {
            Ok(word) => word,
            Err(error) => return self.run(host, Err(error)),
        };
        if !self.run(host, Ok(format!("DIMSTYLE NEW {word}"))) {
            return false;
        }
        self.new_name.clear();
        self.select_style(host.document(), &name);
        true
    }

    /// Duplicate: what the form shows, edits and all, made a new style in ONE step: NEW with every
    /// field that differs from a new style's.
    pub fn duplicate_style(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let name = self.name_for_new(host.document(), &format!("{} copy", self.shown));
        let form = match self.form_style() {
            Ok(form) => form,
            Err(error) => return self.run(host, Err(error)),
        };
        let changes = match dimension_style_changes(&DimensionStyle::default(), &form) {
            Ok(changes) => changes,
            Err(error) => return self.run(host, Err(error)),
        };
        let word = match command_word(&name) {
            Ok(word) => word,
            Err(error) => return self.run(host, Err(error)),
        };
        let mut line = format!("DIMSTYLE NEW {word}");
        if !changes.is_empty() {
            line.push(' ');
            line.push_str(&changes);
        }
        if !self.run(host, Ok(line)) {
            return false;
        }
        self.new_name.clear();
        self.select_style(host.document(), &name);
        true
    }

    /// Delete: the style shown, refused while a layer uses it.
    pub fn delete_style(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let word = match command_word(&self.shown) {
            Ok(word) => word,
            Err(error) => return self.run(host, Err(error)),
        };
        if !self.run(host, Ok(format!("DIMSTYLE DELETE {word}"))) {
            return false;
        }
        self.select_style(host.document(), DEFAULT_DIMENSION_STYLE_NAME);
        true
    }

    /// Revert: the form's edits thrown away.
    pub fn revert(&mut self, document: &Document) {
        self.look(document);
        self.problem.clear();
        let shown = self.shown.clone();
        self.select_style(document, &shown);
    }

    // ---- egui ---------------------------------------------------------------------------------

    /// Draws the dialog for this frame; false once its title bar's close button is clicked.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let mut frame =
            std::mem::replace(&mut self.frame, DialogFrame::new(DIALOG_NAME, "Dimension Styles"));
        let mut closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        let pressed = self.pressed.take();
        match pressed {
            Some(Pressed::New) => {
                self.new_style(host);
            }
            Some(Pressed::Duplicate) => {
                self.duplicate_style(host);
            }
            Some(Pressed::Delete) => {
                self.delete_style(host);
            }
            Some(Pressed::Revert) => self.revert(host.document()),
            Some(Pressed::Apply) => {
                self.apply(host);
            }
            Some(Pressed::Close) => closed = true,
            None => {}
        }
        // The preview follows the form as it is typed in: worked out after the frame took its
        // edits, and shown at the next, which is asked for.
        let before = (self.preview.clone(), self.units.clone());
        self.show_preview();
        if pressed.is_some() || before != (self.preview.clone(), self.units.clone()) {
            ctx.request_repaint();
        }
        !closed
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(LIST_WIDTH);
                dialog::fill_fields(ui);
                let shown = table_look(
                    ui,
                    "dimStyleList",
                    &["Style", "Used by"],
                    &self.rows,
                    self.current,
                    &[],
                    None,
                    TableLook {
                        fill: true,
                        ..TableLook::default()
                    },
                );
                if let Some(at) = shown.chosen
                    && let Some(row) = self.rows.get(at)
                {
                    let name = row[0].clone();
                    self.select_style(host.document(), &name);
                }
                edit(
                    ui,
                    "dimStyleNewName",
                    &mut self.new_name,
                    &self.new_name_placeholder,
                )
                .on_hover_text("The name New and Duplicate give; blank for the one shown");
                ui.horizontal(|ui| {
                    if button(ui, "dimStyleNew", "New")
                        .on_hover_text("A new style with the standard sizes (DIMSTYLE NEW)")
                        .clicked()
                    {
                        self.pressed = Some(Pressed::New);
                    }
                    if button(ui, "dimStyleDuplicate", "Duplicate")
                        .on_hover_text(
                            "A new style of what the form shows, in one step (DIMSTYLE NEW name \
                             field value ...)",
                        )
                        .clicked()
                    {
                        self.pressed = Some(Pressed::Duplicate);
                    }
                    if button(ui, "dimStyleDelete", "Delete")
                        .on_hover_text(
                            "Delete the style; refused while a layer uses it (DIMSTYLE DELETE)",
                        )
                        .clicked()
                    {
                        self.pressed = Some(Pressed::Delete);
                    }
                });
            });
            ui.add_space(layout::SECTION_GAP);
            ui.vertical(|ui| {
                // Three short forms side by side, their values a number's width (short_row, value_row); each
                // has its own label column, as a column of a body does.
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = layout::SECTION_GAP * 2.0;
                    self.text_group(ui);
                    self.lines_group(ui);
                    self.units_group(ui);
                });
                ui.add_space(layout::GAP);
                check(
                    ui,
                    "dimStylePaper",
                    "Paper-sized (drawn at the annotation scale)",
                    &mut self.paper,
                )
                .on_hover_text(
                    "The sizes are millimetres on paper, drawn at size x scale / 1000 in the view \
                     or sheet showing the dimension, so it reads the same size on every sheet",
                );
                controls::label_as(
                    ui,
                    "dimStyleUnits",
                    &self.units,
                    egui::Label::new(
                        egui::RichText::new(self.units.as_str())
                            .color(palette::text_muted().to_egui()),
                    ),
                );
                controls::label(ui, "dimStylePreview", &self.preview);
                self.sample_ui(ui);
            });
        });
    }

    fn text_group(&mut self, ui: &mut Ui) {
        let size_tip =
            " (model units, or millimetres on paper when the style is paper-sized)";
        ui.vertical(|ui| {
            dialog::section(ui, "Text");
            dialog::form(ui, |form| {
                short_row(form, "Height", |ui| {
                    edit(ui, "dimStyleText", &mut self.text, "")
                        .on_hover_text(format!("Text height{size_tip}"));
                });
                short_row(form, "Gap to the line", |ui| {
                    edit(ui, "dimStyleGap", &mut self.gap, "")
                        .on_hover_text(format!("From the dimension line to its text{size_tip}"));
                });
                short_row(form, "Prefix", |ui| {
                    edit(ui, "dimStylePrefix", &mut self.prefix, "")
                        .on_hover_text("Written before the number");
                });
                short_row(form, "Suffix", |ui| {
                    edit(ui, "dimStyleSuffix", &mut self.suffix, "")
                        .on_hover_text("Written after the number, e.g. mm");
                });
            });
        });
    }

    fn lines_group(&mut self, ui: &mut Ui) {
        let size_tip =
            " (model units, or millimetres on paper when the style is paper-sized)";
        ui.vertical(|ui| {
            dialog::section(ui, "Lines and arrows");
            dialog::form(ui, |form| {
                short_row(form, "Extension offset", |ui| {
                    edit(ui, "dimStyleExtOff", &mut self.ext_off, "").on_hover_text(format!(
                        "The gap between a measured point and its extension line{size_tip}"
                    ));
                });
                short_row(form, "Extension beyond", |ui| {
                    edit(ui, "dimStyleExtBeyond", &mut self.ext_beyond, "").on_hover_text(
                        format!(
                            "How far an extension line runs past the dimension line{size_tip}"
                        ),
                    );
                });
                short_row(form, "Arrow size", |ui| {
                    edit(ui, "dimStyleArrow", &mut self.arrow, "")
                        .on_hover_text(format!("Arrowhead size{size_tip}"));
                });
                value_row(form, "Arrowhead", |ui| {
                    let labels: Vec<String> =
                        HEADS.iter().map(|&(_, label)| String::from(label)).collect();
                    controls::choice(ui, "dimStyleHead", &labels, &mut self.head)
                        .on_hover_text("How each end of the dimension line is drawn");
                    self.head_picture_ui(ui);
                });
            });
        });
    }

    fn units_group(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            dialog::section(ui, "Units");
            dialog::form(ui, |form| {
                short_row(form, "Unit scale", |ui| {
                    edit(ui, "dimStyleScale", &mut self.scale, "").on_hover_text(
                        "Multiplies the measurement before it is written: 1000 writes a drawing \
                         in metres in millimetres",
                    );
                });
                short_row(form, "Decimal places", |ui| {
                    controls::whole(ui, "dimStyleDecimals", &mut self.decimals, 0..=MOST_DECIMALS)
                        .on_hover_text("Places after the decimal point");
                });
                short_row(form, "Round to", |ui| {
                    edit(ui, "dimStyleRound", &mut self.round, "").on_hover_text(
                        "Rounds the measurement to a multiple of this; 0 for none",
                    );
                });
                form.check("dimStyleTrim", "Drop trailing zeros", &mut self.trim);
            });
        });
    }

    /// The chosen head's picture, beside the choice (see the module comment).
    fn head_picture_ui(&mut self, ui: &mut Ui) {
        if self.head_textures.is_empty() {
            self.head_textures = self
                .head_pictures
                .iter()
                .enumerate()
                .map(|(at, picture)| {
                    ui.ctx().load_texture(
                        format!("dimStyleHead{at}"),
                        picture.to_color_image(),
                        egui::TextureOptions::LINEAR,
                    )
                })
                .collect();
        }
        if let Some(texture) = self.head.index.and_then(|at| self.head_textures.get(at)) {
            ui.image((
                texture.id(),
                egui::vec2(HEAD_WIDTH as f32, HEAD_HEIGHT as f32),
            ));
        }
    }

    /// The sample, on the view's ground with a border, as the C++ label's stylesheet drew it.
    fn sample_ui(&mut self, ui: &mut Ui) {
        let size = egui::vec2(SAMPLE_WIDTH as f32, SAMPLE_HEIGHT as f32);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, palette::viewport().to_egui());
            if !self.sample.is_null() {
                let texture = self.sample_texture.get_or_insert_with(|| {
                    ui.ctx().load_texture(
                        "dimStyleSample",
                        self.sample.to_color_image(),
                        egui::TextureOptions::LINEAR,
                    )
                });
                painter.image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, palette::border().to_egui()),
                egui::StrokeKind::Inside,
            );
        }
        controls::custom(
            ui,
            &response,
            ControlRecord::new("dimStyleSample", ControlKind::Label),
        );
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        let problem = self.problem.clone();
        // The one footer (shell::dialog::button_row): the last refusal at the left; Revert, Close,
        // then Apply, the primary, last at the right. Close is the footer's own: the dialog had
        // only its title bar's cross to dismiss it.
        let (_, (closed, reverted, applied)) = dialog::button_row(
            ui,
            |ui| {
                problem_line(ui, "dimStyleProblem", &problem);
            },
            |ui| {
                let reverted = controls::button(ui, "dimStyleRevert", "Revert")
                    .on_hover_text("Throw the form's edits away")
                    .clicked();
                let closed = controls::button(ui, "dimStyleClose", "Close")
                    .on_hover_text("Close the dialog; edits not applied are dropped")
                    .clicked();
                let applied = controls::primary(ui, "dimStyleApply", "Apply")
                    .on_hover_text(
                        "Store the fields the form changed, as one undo step (DIMSTYLE SET)",
                    )
                    .clicked();
                (closed, reverted, applied)
            },
        );
        if closed {
            self.pressed = Some(Pressed::Close);
        }
        if reverted {
            self.pressed = Some(Pressed::Revert);
        }
        if applied {
            self.pressed = Some(Pressed::Apply);
        }
    }
}
