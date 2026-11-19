//! The main window's frame (main_window::window's FRAME ORDER): the menu bar, the toolbars, the
//! four panels and the docks beside them, the views, the status bar, and every dialog - and the
//! panels' own parts, ported from buildDocks, buildReferenceDock and buildStatusBar: the Layers
//! tree with its tools, the Reference Data table with its tools and display choices, the
//! Properties panel with its Style row, the command line with its log, history and paste, and the
//! status bar's labels.
//!
//! Each panel is a dock of the window's own chrome (dock_chrome, a `DockChrome` of its own, as the
//! C++ installed every panel under the window's): its title bar with Minimise, Float and Close, the
//! dock a drive target named as the C++ object (LayersDock, PropertiesDock, CommandLineDock,
//! ReferenceDataDock, SurfacesDock, VerticesDock, SurveyPointsDock). The panels are laid out to the
//! C++'s geometry (main_window::layout), not by egui_dock: a Minimise or a Close hides the panel,
//! which View > Panels brings back; Float and Maximise are a view's, not a panel's, here
//! (docs/rust.md records the difference).
//!
//! The layer tree, which the C++ left unnamed, is drawn as rows a person clicks; `--toggle-layer`
//! reaches it through `toggle_layer_through_panel`, which flips a layer's box as a click does.

use std::rc::Rc;

use egui::{Align, Context, Key, Layout, Pos2, Rect, RichText, Sense, Ui, UiBuilder, Vec2};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Status};
use katana_entity::{EnumName, Layer};

use super::actions::{BarPlace, Composed};
use super::docking::{DropZones, Side};
use super::docking_ui::arrange;
use super::layout::{self, Geometry, natural_width, row_rooms};
use super::window::{ImportOptions, MainWindow};
use crate::dock_chrome::{ChromeIcons, ChromeRequest, DockChrome, DockRole, DockTab};
use crate::drive::action::{Action, Menu, ToolBar, ToolBarItem};
use crate::drive::controls::{self, Choice, LineOptions};
use crate::drive::registry::{TargetKind, TargetSpec, WINDOW, begin_target, end_target};
use crate::host::Host;
use crate::import_placement::DialogResult;
use crate::shell::{DialogFrame, Workbench};
use crate::theme::palette;
use crate::tools::flyout_button::{Strip, TOOL_BAR_PADDING_ALONG, tool_bar_strip};
use crate::view_workspace::Scenery;

// ---- the panels' state ----------------------------------------------------------------------

/// How a toolbar's strip is laid out (`MainWindow::strip_drawn`): along a docked line at `at`,
/// `room` long, or wrapped in rows of a floating bar's `width`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StripShape {
    Along { at: Rect, room: f32 },
    Wrapped { width: f32 },
}

/// A panel of the window's chrome, by its dock's object name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelTab(pub String);

impl DockTab for PanelTab {
    fn dock_name(&self) -> &str {
        &self.0
    }
}

/// The panels' own state: the command line's text and history, the layer and reference layer
/// picked, the style rows and the status bar's lines.
pub struct Panels {
    pub chrome: DockChrome<PanelTab>,
    pub command_text: String,
    pub placeholder: String,
    pub layer_selected: Option<String>,
    pub reference_selected: Option<u64>,
    /// The Reference Data panel's referenceOpacity and referenceColour: the selected layer's,
    /// or the last shown while none of its kind is selected.
    pub reference_opacity: Choice,
    pub reference_colour: Choice,
    pub current_style: Choice,
    pub property_style: Choice,
    /// The selection's styles the Style row last showed: the row is emptied when they change to
    /// none or to several, and left alone while a person picks from it.
    pub property_styles_seen: Vec<String>,
    /// The status bar's message: a running tool's prompt, a view's status, an error.
    pub status: String,
    pub coordinates: String,
    pub snap: String,
    pub frame_stats: String,
    /// What the command log was last laid out for - its blocks, its line's height and its own
    /// height - so a change of any scrolls it to its last line (command_line_ui).
    pub log_laid_out: (usize, f32, f32),
}

impl Default for Panels {
    fn default() -> Self {
        let mut chrome = DockChrome::new(chrome_icons());
        for (name, icon, title) in [
            ("LayersDock", "Layers", "Layers"),
            ("PropertiesDock", "Properties", "Properties"),
            ("CommandLineDock", "CommandLine", "Command Line"),
            ("ReferenceDataDock", "ReferenceData", "Reference Data"),
            ("SurfacesDock", "SurfacesPanel", "Surfaces"),
            ("VerticesDock", "draw.vertex.edit", "Vertices"),
            // The Point Manager's own icon: "SurveyPoints" named none, and the bar drew no icon.
            ("SurveyPointsDock", "SurveyPointManager", "Survey Points"),
        ] {
            chrome.install(name, icon, DockRole::Panel).set_title(title);
        }
        Self {
            chrome,
            command_text: String::new(),
            placeholder: String::from(super::executor::COMMAND_PLACEHOLDER),
            layer_selected: None,
            reference_selected: None,
            reference_opacity: Choice::at(0),
            reference_colour: Choice::at(0),
            current_style: Choice::at(0),
            property_style: Choice::editable(""),
            property_styles_seen: Vec::new(),
            status: String::new(),
            coordinates: String::from("0.0000, 0.0000"),
            snap: String::new(),
            frame_stats: String::new(),
            log_laid_out: (0, 0.0, 0.0),
        }
    }
}

/// The icons the chrome draws: the window's set, painted by the canvas.
pub fn chrome_icons() -> ChromeIcons {
    // The size in points and the screen's ratio, so an icon's strokes are weighted for the 14
    // points it is shown at, not for its device pixels (at 200 % it was drawn as a 28 point
    // icon scaled down: thin).
    ChromeIcons::scaled(|name: &str, pixels: u32, ratio: f32| {
        let ratio = if ratio > 0.0 { f64::from(ratio) } else { 1.0 };
        crate::icons::icon_image(
            &crate::icons::CanvasRasteriser,
            name,
            f64::from(pixels) / ratio,
            ratio,
            crate::icons::IconMode::Normal,
        )
    })
}

/// What a small question the window asks is for (QInputDialog's callers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputPurpose {
    /// View > Vertical Exaggeration: the factor.
    Exaggeration,
    /// Terrain > Cut Section Along Alignment: the alignment.
    SectionAlignment,
    /// The Layers panel's New Layer: its name.
    NewLayer,
    /// New Child Layer, under the layer named.
    NewChildLayer(String),
    /// Rename Layer: the layer named.
    RenameLayer(String),
}

/// A question asked in a small dialog of the window's own (QInputDialog::getText, getDouble,
/// getItem): "inputDialog", its field "inputValue", "inputOk" and "inputCancel".
pub struct InputDialog {
    frame: DialogFrame,
    state: InputState,
    pub purpose: InputPurpose,
}

/// What the question's frame lends its body and footer.
struct InputState {
    label: String,
    value: String,
    items: Vec<String>,
    chosen: Choice,
    answer: Option<bool>,
}

impl InputDialog {
    pub fn text(title: &str, label: &str, value: &str, purpose: InputPurpose) -> Self {
        Self {
            frame: DialogFrame::new("inputDialog", title).modal(true),
            state: InputState {
                label: String::from(label),
                value: String::from(value),
                items: Vec::new(),
                chosen: Choice::default(),
                answer: None,
            },
            purpose,
        }
    }

    pub fn choice(title: &str, label: &str, items: Vec<String>, purpose: InputPurpose) -> Self {
        let mut dialog = Self::text(title, label, "", purpose);
        dialog.state.chosen = Choice::at(0);
        dialog.state.items = items;
        dialog
    }

    /// The answer typed or chosen.
    pub fn value(&self) -> String {
        if self.state.items.is_empty() {
            self.state.value.trim().to_owned()
        } else {
            self.state.chosen.current_text(&self.state.items)
        }
    }

    /// One frame; Some(true) for OK, Some(false) for Cancel.
    pub fn ui(&mut self, ctx: &Context) -> Option<bool> {
        let shown = self.frame.show(
            ctx,
            &mut self.state,
            |ui, state| {
                ui.label(state.label.as_str());
                if state.items.is_empty() {
                    let _ = controls::line(ui, "inputValue", &mut state.value);
                } else {
                    let items = state.items.clone();
                    let _ = controls::choice(ui, "inputValue", &items, &mut state.chosen);
                }
            },
            |ui, state| {
                ui.horizontal(|ui| {
                    if controls::button(ui, "inputOk", "OK").clicked() {
                        state.answer = Some(true);
                    }
                    if controls::button(ui, "inputCancel", "Cancel").clicked() {
                        state.answer = Some(false);
                    }
                });
            },
        );
        if shown.closed {
            return Some(false);
        }
        self.state.answer.take()
    }
}

/// Help > About Katana ("aboutDialog"): the logo, the version and what the program is, and Close
/// ("aboutClose"). Non-modal, as the start screen beside it is; it asks nothing.
pub struct AboutDialog {
    frame: DialogFrame,
    state: AboutState,
}

/// What the About dialog's frame lends its body and footer.
struct AboutState {
    open: bool,
    logo: Option<egui::TextureHandle>,
}

/// The side of the About dialog's logo: the start screen's, so the two show the program alike.
const ABOUT_LOGO: f32 = 64.0;

impl Default for AboutDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl AboutDialog {
    pub fn new() -> Self {
        Self {
            // As narrow as the logo and two lines of words need: the body's 520 left the words
            // one long line with the dialog mostly empty.
            frame: DialogFrame::new("aboutDialog", "About Katana").width(400.0),
            state: AboutState {
                open: false,
                logo: None,
            },
        }
    }

    pub fn show(&mut self) {
        self.state.open = true;
    }

    pub fn is_open(&self) -> bool {
        self.state.open
    }

    /// The words the dialog shows under its title: the version first.
    pub fn text() -> String {
        format!(
            "Version {}\nSurvey and CAD, for a person at the window and an agent at the \
             command line alike.",
            katana_update::version::program_version_text()
        )
    }

    pub fn ui(&mut self, ctx: &Context) {
        if !self.state.open {
            return;
        }
        let shown = self.frame.show(
            ctx,
            &mut self.state,
            |ui, state| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = crate::shell::dialog::SECTION_GAP;
                    let logo = state
                        .logo
                        .get_or_insert_with(|| {
                            let image = crate::icons::application_icon_image(
                                &crate::icons::CanvasRasteriser,
                                ABOUT_LOGO as u32,
                            );
                            ui.ctx().load_texture(
                                "katanaAboutLogo",
                                image.to_color_image(),
                                egui::TextureOptions::LINEAR,
                            )
                        })
                        .id();
                    let _ = ui.image((logo, Vec2::splat(ABOUT_LOGO)));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Katana").heading().strong());
                        let text = Self::text();
                        let _ = controls::label_as(
                            ui,
                            "aboutText",
                            &text,
                            egui::Label::new(
                                RichText::new(text.as_str()).color(palette::text_muted().to_egui()),
                            )
                            .wrap(),
                        );
                    });
                });
            },
            |ui, state| {
                let ((), close) = crate::shell::dialog::button_row(
                    ui,
                    |_| {},
                    |ui| controls::button(ui, "aboutClose", "Close").clicked(),
                );
                if close {
                    state.open = false;
                }
            },
        );
        if shown.closed {
            self.state.open = false;
        }
    }
}

// ---- the frame ------------------------------------------------------------------------------

impl MainWindow {
    /// One frame of the window (see main_window::window, THE FRAME ORDER).
    pub(crate) fn frame(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if !self.ui.themed {
            // The theme's palette and text styles, and the icons the toolbars paint with.
            crate::theme::apply(&ctx);
            if let Some(raster) = crate::icons::default_rasteriser() {
                crate::icons::install(&ctx, raster);
            }
            self.ui.themed = true;
        }
        if let Some(size) = self.ui.text_size_asked.take() {
            crate::theme::set_text_size(&ctx, size);
        }
        // 1-2: the jobs' ends, and what the drawing did.
        self.exec.deliver_jobs(&ctx);
        let heard = self.exec.follow_drawing();
        // A new, opened or saved drawing, or its name changed: the Plot and Export View as Image
        // dialogs' files follow it (the C++'s fileListener_, suggestPlotFiles).
        if katana_cad::DocumentChange::of(heard).has(
            katana_cad::DocumentChange::REPLACED
                | katana_cad::DocumentChange::SAVED
                | katana_cad::DocumentChange::METADATA,
        ) {
            self.suggest_plot_files();
        }
        if !self.exec.workspace.has_chrome() {
            self.exec.workspace.set_chrome(chrome_icons());
        }
        // The running tool's action checked, and Select while none runs.
        let running = self.exec.workspace.active_tool_id();
        self.ui.tools.set_active(&running);
        if running.is_empty() {
            self.ui.panels.placeholder = String::from(super::executor::COMMAND_PLACEHOLDER);
        }

        // 3: the window target and its parts.
        let screen = ui.max_rect();
        let title = window_title(&self.exec.document);
        begin_target(ui, TargetSpec::new(WINDOW, TargetKind::Window, &title));
        ui.painter()
            .rect_filled(screen, 0.0, crate::theme::palette::window().to_egui());
        let composed = self.composed();
        let right_extra = self.ui.drawing.vertices_shown() || self.ui.survey.point_manager_shown();
        // The toolbars' lines first: what they take of each side is the room the rest is laid
        // out in (main_window::docking).
        let shown = |name: &str| self.ui.bars.shown(name);
        let lines = self.bar_lines(&arrange(&composed.bars, &shown));
        let geometry = layout::geometry(
            screen,
            &self.ui.docks,
            right_extra,
            &lines,
            self.ui.docking.panels.band_breadths(),
        );
        self.ui.docking.screen = screen;
        self.ui.docking.views = geometry.views_and_bands;
        self.ui.docking.drawing = geometry.views;
        self.ui.docking.drawn.clear();
        self.menu_bar_ui(ui, &geometry, &composed);
        self.tool_bars_ui(ui, &geometry, &composed);
        self.panels_ui(ui, &geometry);
        self.views_ui(ui, &geometry);
        self.menu_panels_ui(ui, &geometry, &composed);
        self.floating_bars_ui(ui, &composed);
        self.status_bar_ui(ui, &geometry);
        self.step_menu_ui(ui, &composed);
        self.drags_ui(ui, &geometry);
        end_target(ui, screen);

        // 4: the dialogs, each in its frame.
        self.dialogs_ui(ui);
        let missing = self.ui.dialogs.missing_asked.take();
        if let Some(name) = missing {
            self.show_missing_in_styles(&name);
        }

        // 5: what the clicks asked.
        for name in std::mem::take(&mut self.ui.triggered) {
            if let Err(error) = self.trigger(&name) {
                self.exec.log_message(&error.describe(), true);
            }
        }
        self.workspace_events();
        let requests = self.ui.panels.chrome.take_requests();
        for request in requests {
            self.panel_request(request);
        }
        // 6: what the lines asked of the window.
        self.after_call();
        if self.closed() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// The menu bar: each menu a button that opens its items, as a person clicks it, with a
    /// tear-off line at the top of every menu and submenu (main_window::docking_ui).
    fn menu_bar_ui(&mut self, ui: &mut Ui, geometry: &Geometry, composed: &Composed) {
        let triggered = &mut self.ui.triggered;
        let mut torn: Option<(String, Pos2)> = None;
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(geometry.menu_bar)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    for menu in &composed.menus {
                        menu_ui(ui, menu, triggered, &mut torn);
                    }
                });
            },
        );
        if let Some((menu, at)) = torn
            && let Err(error) = self.tear_off_menu(&menu, Some(at))
        {
            self.exec.log_message(&error.describe(), true);
        }
    }

    /// The toolbars docked along the window's four sides, each side's lines from its edge inwards
    /// (main_window::docking): along the top and the bottom in rows that show their names, down
    /// the left and the right in columns; then the Drafting bar over the status bar. Every docked
    /// bar's grip is its handle, and the lines drawn are where a drag drops a bar.
    fn tool_bars_ui(&mut self, ui: &mut Ui, geometry: &Geometry, composed: &Composed) {
        let icon_size = self.ui.bars.icon_size;
        let names_shown = self.ui.bars.names_shown;
        let shown = |name: &str| self.ui.bars.shown(name);
        let arranged = arrange(&composed.bars, &shown);
        // The names' widths in this frame's text, before any strip is laid out with them.
        if names_shown {
            let titles: Vec<String> = [Side::Top, Side::Bottom]
                .iter()
                .flat_map(|side| arranged.lines[side.index()].iter())
                .flat_map(|(_, bars)| bars.iter().map(|bar| bar.title.clone()))
                .collect();
            layout::measure_names(&mut self.ui.bars, ui, titles.iter().map(String::as_str));
        }
        let mut zones = DropZones::default();
        for side in Side::ALL {
            let rects = match side {
                Side::Top => &geometry.rows,
                Side::Left => &geometry.left_columns,
                Side::Right => &geometry.right_columns,
                Side::Bottom => &geometry.bottom_rows,
            };
            for ((line, bars), rect) in arranged.lines[side.index()].iter().zip(rects) {
                zones.lines.push((side, *line, *rect));
                self.bar_line_ui(ui, side, *line, *rect, bars, &mut zones);
            }
        }
        self.new_line_zones(geometry, &mut zones);
        self.ui.docking.zones = zones;
        ui.painter()
            .rect_filled(geometry.bottom_bar, 0.0, palette::window().to_egui());
        let mut x = geometry.bottom_bar.min.x;
        for bar in &arranged.drafting {
            // The Drafting bar's toggles have no icons - Ortho, Polar, the snap modes are words -
            // and an icon-only button with no icon is a blank square: each is drawn as a word
            // that is lit while it is on, as QToolBar showed a text-only action's text.
            let (bar, texts) = worded(bar, ui.ctx());
            let strip = self.strip_of(&bar, false, icon_size);
            let room = (natural_width(&strip.items, icon_size) as f32)
                .min(geometry.bottom_bar.max.x - x)
                .max(0.0);
            let at = Rect::from_min_size(
                Pos2::new(x, geometry.bottom_bar.min.y),
                Vec2::new(room, geometry.bottom_bar.height()),
            );
            self.strip_ui(ui, &strip, at, room, &texts);
            x += room;
        }
    }

    /// One line of docked toolbars at `rect`: a row shares its length as Qt's toolbar layout did
    /// (`row_rooms`), a column as `column_rooms` does; each bar is drawn with its grip and its
    /// handle, and recorded where a drop lands.
    fn bar_line_ui(
        &mut self,
        ui: &mut Ui,
        side: Side,
        line: usize,
        rect: Rect,
        bars: &[&ToolBar],
        zones: &mut DropZones,
    ) {
        let icon_size = self.ui.bars.icon_size;
        let names_shown = self.ui.bars.names_shown;
        let vertical = side.vertical();
        let strips: Vec<(Strip, bool)> = bars
            .iter()
            .map(|bar| {
                let docked = layout::docked_bar(bar, names_shown, vertical);
                let named = matches!(
                    docked.items.first(),
                    Some(ToolBarItem::Widget(name)) if name.starts_with("toolBarName:")
                );
                (self.strip_of(&docked, vertical, icon_size), named)
            })
            .collect();
        let naturals: Vec<i32> = strips
            .iter()
            .map(|(strip, _)| natural_width(&strip.items, icon_size))
            .collect();
        let rooms = if vertical {
            layout::column_rooms(&naturals, rect.height(), layout::collapsed_bar(icon_size))
        } else {
            // The row is one band: the frame's colour to its end and the rule beside it the whole
            // width, so a short row has no stretch where the toolbars' own ground stops.
            ui.painter().rect_filled(rect, 0.0, palette::window().to_egui());
            row_rooms(&naturals, rect.width())
        };
        let mut along = if vertical { rect.min.y } else { rect.min.x };
        for (((strip, named), room), bar) in strips.iter().zip(rooms).zip(bars) {
            if room <= 0.0 {
                continue;
            }
            let at = if vertical {
                // Each bar at its own breadth against the column's outer edge, the window's side:
                // a column is as broad as its broadest bar, and a bar drawn the column's breadth
                // claimed the void beside its buttons as its own (the sixth look audit's
                // Properties bar docked left, Draw's 39 points in a 220 point column).
                let own = self.column_breadth(bar).min(rect.width());
                let x = match side {
                    Side::Right => rect.max.x - own,
                    _ => rect.min.x,
                };
                Rect::from_min_size(Pos2::new(x, along), Vec2::new(own, room))
            } else {
                Rect::from_min_size(Pos2::new(along, rect.min.y), Vec2::new(room, rect.height()))
            };
            self.strip_ui(ui, strip, at, room, &[]);
            zones.bars.push((bar.name.clone(), side, line, at));
            self.ui.docking.drawn.push((bar.name.clone(), at));
            // The handle: the bar's padding, where its grip is drawn, and its name when it shows
            // one. A bar short of its room shows its name still, unless it is one button (all
            // its tools behind it, `column_rooms`), whose handle is its padding alone.
            let padding = TOOL_BAR_PADDING_ALONG as f32;
            let lead = match strip.items.first() {
                Some(name) if *named && room >= padding * 2.0 + name.size(icon_size).0 as f32 => {
                    padding + name.size(icon_size).0 as f32
                }
                _ => padding,
            };
            let handle = if vertical {
                Rect::from_min_size(at.min, Vec2::new(at.width(), lead.min(room)))
            } else {
                Rect::from_min_size(at.min, Vec2::new(lead.min(room), at.height()))
            };
            self.grip_ui(ui, bar, BarPlace::on(side, line), at, handle);
            along += room;
        }
        if !vertical {
            // The rule between the row and the views, drawn after the bars, whose ground covers
            // the row. Between two bars of a row there is no rule: each bar's grip says where it
            // begins (it was a rule a little shorter than the buttons until the bars could be
            // moved).
            let y = match side {
                Side::Bottom => rect.min.y + 0.5,
                _ => rect.max.y - 0.5,
            };
            ui.painter().hline(
                rect.x_range(),
                ui.painter().round_to_pixel_center(y),
                egui::Stroke::new(1.0, palette::border().to_egui()),
            );
        }
    }

    /// A toolbar's strip as it is drawn this frame, down a side when `vertical`.
    pub(crate) fn strip_of(&self, bar: &ToolBar, vertical: bool, icon_size: i32) -> Strip {
        let bars = &self.ui.bars;
        let items = self.ui.tools.strip_items(bar, &|name| {
            if let Some(width) = name
                .strip_prefix(WORD_PREFIX)
                .and_then(|rest| rest.split_once(':'))
                .and_then(|(width, _)| width.parse::<i32>().ok())
            {
                return Some((width, crate::theme::style::CONTROL_HEIGHT as i32));
            }
            layout::top_widget_size(bars, name, icon_size)
        });
        Strip {
            name: bar.name.clone(),
            title: bar.title.clone(),
            vertical,
            icon_size,
            items,
        }
    }

    /// One toolbar drawn at `at`, `room` along it, its clicks kept to be triggered after the
    /// frame; `texts` are the actions a worded bar (`worded`) draws as words.
    fn strip_ui(&mut self, ui: &mut Ui, strip: &Strip, at: Rect, room: f32, texts: &[Action]) {
        self.strip_drawn(ui, strip, texts, StripShape::Along { at, room });
    }

    /// A toolbar drawn in `shape` - along a line (`tool_bar_strip`) or wrapped in a floating bar's
    /// width (`tool_bar_wrapped`) - the controls its owner draws itself drawn here, and its clicks
    /// kept to be triggered after the frame.
    pub(crate) fn strip_drawn(
        &mut self,
        ui: &mut Ui,
        strip: &Strip,
        texts: &[Action],
        shape: StripShape,
    ) {
        let exec = &mut self.exec;
        let ui_state = &mut self.ui;
        let mut status: Option<String> = None;
        let mut widget = |ui: &mut Ui, rect: Rect, name: &str| -> bool {
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            if let Some(title) = name.strip_prefix("toolBarName:") {
                // The name in the secondary text, centred on the buttons' row and set clear of
                // the grip before it (layout::NAME_LEAD).
                child.add_space(layout::NAME_LEAD as f32);
                let _ = controls::label_as(
                    &mut child,
                    "toolBarName",
                    title,
                    egui::Label::new(
                        RichText::new(title)
                            .small()
                            .color(palette::text_muted().to_egui()),
                    )
                    .selectable(false),
                );
                return false;
            }
            if let Some(action) =
                word_of(name).and_then(|named| texts.iter().find(|action| action.name == named))
            {
                let text = action.text.replace('&', "");
                let response = child
                    .add_enabled_ui(action.enabled, |ui| {
                        word_toggle(ui, &action.name, &text, action.checked == Some(true))
                    })
                    .inner;
                if response.hovered() {
                    status = Some(action.tip.clone());
                }
                let clicked = response.clicked();
                let _ = response.on_hover_text(action.tooltip());
                return clicked;
            }
            // A choice on a toolbar fills the room the bar gave it (egui sizes one by the
            // style's combo_width, which left the Surface Edit bar's list short of its slot
            // and the annotation scale's field running past its own).
            child.spacing_mut().combo_width = rect.width();
            child.spacing_mut().item_spacing.x = 2.0;
            match name {
                "editUndoButton" | "editRedoButton" => {
                    // Undo and Redo, each with its history dropped down from the strip beside it
                    // (historyButton): the button is one step, the k-th item of the list UNDO k
                    // or REDO k, run through the one executor by its action (undoStep<k>).
                    let history = exec.document.history();
                    let [undo_menu, redo_menu] = super::actions::history_menus(&exec.document);
                    let (action, menu) = if name == "editUndoButton" {
                        (
                            crate::drive::action::Action::new("editUndo", "&Undo")
                                .icon("Undo")
                                .tip("Undo the last command")
                                .enabled(history.can_undo()),
                            undo_menu,
                        )
                    } else {
                        (
                            crate::drive::action::Action::new("editRedo", "&Redo")
                                .icon("Redo")
                                .tip("Redo the command that was undone")
                                .enabled(history.can_redo()),
                            redo_menu,
                        )
                    };
                    let icon_size = ui_state.bars.icon_size;
                    let shown = child
                        .add_enabled_ui(action.enabled, |ui| {
                            crate::tools::flyout_button::split_button_at(
                                ui, rect, name, &action, &menu, icon_size, false,
                            )
                        })
                        .inner;
                    if let Some(step) = shown.chosen {
                        ui_state.triggered.push(step);
                    }
                    shown.triggered
                }
                "CurrentStyleCombo" => {
                    current_style_ui(&mut child, exec, &mut ui_state.panels);
                    false
                }
                "annotationScaleCombo" => {
                    // An editable choice, one framed control with its list's arrow inside: as wide
                    // as its slot on the strip, less the strip's 2 points between widgets.
                    child.spacing_mut().text_edit_width = (rect.width() - 2.0).max(24.0);
                    let _ = ui_state.annotation.scale_box_ui(&mut child, exec);
                    false
                }
                "surfaceEditActive" => {
                    // Its empty state ("No surface") is the choice's own hint
                    // (controls::choice_with_hint, in surface_edit_ui): painted here as well, it
                    // stood over the choice's "None".
                    ui_state.surface_edit.active_ui(&mut child, exec);
                    false
                }
                _ => false,
            }
        };
        let response = match shape {
            StripShape::Along { at, room } => {
                ui.scope_builder(UiBuilder::new().max_rect(at), |ui| {
                    tool_bar_strip(ui, strip, room, &mut widget)
                })
                .inner
            }
            StripShape::Wrapped { width } => {
                crate::tools::flyout_button::tool_bar_wrapped(ui, strip, width, &mut widget)
            }
        };
        for name in response.triggered {
            match name.as_str() {
                "editUndoButton" => self.ui.triggered.push(String::from("editUndo")),
                "editRedoButton" => self.ui.triggered.push(String::from("editRedo")),
                _ => match word_of(&name) {
                    Some(action) => self.ui.triggered.push(String::from(action)),
                    None => self.ui.triggered.push(name),
                },
            }
        }
        if let Some(tip) = response.status_tip.or(status) {
            self.ui.panels.status = tip;
        }
    }

    /// The four panels and the docks beside them, each in its dock of the window's chrome.
    fn panels_ui(&mut self, ui: &mut Ui, geometry: &Geometry) {
        if let Some(rect) = geometry.layers {
            self.dock_at(ui, rect, "LayersDock", layers_ui);
        }
        if let Some(rect) = geometry.lower_left {
            let tabs = self.ui.docks.lower_left_tabs();
            let front = self.ui.docks.lower_left().unwrap_or("ReferenceDataDock");
            // Two docks in the one place: their tabs are the title of its bar, the one in front
            // over the accent's bar and the others muted (drive::controls::title_tabs), as an IDE
            // draws a tabbed panel. A strip of tabs over the bar, whose title then said the tab in
            // front again, was two headings for one panel (the second look audit's N7).
            let mut chosen: Option<bool> = None;
            let tab_titles = |ui: &mut Ui| {
                let titles: Vec<&str> = tabs
                    .iter()
                    .map(|tab| {
                        if *tab == "SurfacesDock" {
                            "Surfaces"
                        } else {
                            "Reference Data"
                        }
                    })
                    .collect();
                let mut current = tabs.iter().position(|tab| *tab == front).unwrap_or(0);
                // The tabs stand on the bar's border, its last pixel row.
                ui.spacing_mut().interact_size.y = crate::dock_chrome::BAR_HEIGHT - 1.0;
                let changed =
                    controls::title_tabs(ui, "lowerLeftDockTabs", &titles, &mut current).changed();
                if changed && let Some(tab) = tabs.get(current) {
                    chosen = Some(*tab == "SurfacesDock");
                }
            };
            let grouped = tabs.len() > 1;
            if front == "SurfacesDock" {
                self.dock_titled_at(ui, rect, "SurfacesDock", grouped.then_some(tab_titles), |window, ui| {
                    window.ui.surfaces.ui(ui, &mut window.exec);
                });
            } else {
                self.dock_titled_at(ui, rect, "ReferenceDataDock", grouped.then_some(tab_titles), |window, ui| {
                    reference_ui(window, ui);
                });
            }
            if let Some(surfaces) = chosen {
                self.ui.docks.surfaces_in_front = surfaces;
            }
        }
        if let Some(rect) = geometry.properties {
            let mut docks: Vec<&'static str> = Vec::new();
            if self.ui.docks.properties {
                docks.push("PropertiesDock");
            }
            if self.ui.drawing.vertices_shown() {
                docks.push("VerticesDock");
            }
            if self.ui.survey.point_manager_shown() {
                docks.push(crate::survey::survey_workbench::POINTS_DOCK);
            }
            let count = docks.len().max(1) as f32;
            let each = (rect.height() - layout::SEPARATOR * (count - 1.0)) / count;
            for (at, dock) in docks.into_iter().enumerate() {
                let top = rect.min.y + at as f32 * (each + layout::SEPARATOR);
                let place = Rect::from_min_size(Pos2::new(rect.min.x, top), Vec2::new(rect.width(), each));
                match dock {
                    "PropertiesDock" => {
                        self.dock_at(ui, place, dock, properties_ui);
                    }
                    "VerticesDock" => self.dock_at(ui, place, dock, |window, ui| {
                        window.ui.drawing.dock_ui(ui, &mut window.exec.document);
                    }),
                    _ => self.dock_at(ui, place, dock, |window, ui| {
                        let name = crate::survey::survey_workbench::POINTS_DOCK;
                        window.ui.survey.dock_ui(ui, &mut window.exec, name);
                    }),
                }
            }
        }
        if let Some(rect) = geometry.command_line {
            self.dock_at(ui, rect, "CommandLineDock", command_line_ui);
        }
    }

    /// Dock `name` drawn at `rect` with its chrome's title bar, `content` under it.
    pub(crate) fn dock_at(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        name: &str,
        content: impl FnOnce(&mut MainWindow, &mut Ui),
    ) {
        self.dock_titled_at(ui, rect, name, None::<fn(&mut Ui)>, content);
    }

    /// `dock_at`, the bar's title drawn by `title` when one is given (a group's tabs).
    fn dock_titled_at(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        name: &str,
        title: Option<impl FnMut(&mut Ui)>,
        content: impl FnOnce(&mut MainWindow, &mut Ui),
    ) {
        let mut chrome = std::mem::replace(
            &mut self.ui.panels.chrome,
            DockChrome::new(ChromeIcons::none()),
        );
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min))
                .id_salt(("katanaPanel", name)),
            |ui| {
                ui.set_clip_rect(rect);
                match title {
                    Some(title) => {
                        chrome.dock_ui_titled(ui, name, title, |ui| content(self, ui));
                    }
                    None => {
                        chrome.dock_ui(ui, name, |ui| content(self, ui));
                    }
                }
            },
        );
        // A request the content's own chrome made meanwhile is kept with the real chrome's.
        let made = std::mem::replace(&mut self.ui.panels.chrome, chrome);
        drop(made);
    }

    /// The views, lent the scenery: the workspace draws its docks and runs what its bars asked.
    fn views_ui(&mut self, ui: &mut Ui, geometry: &Geometry) {
        let scene = Rc::clone(&self.exec.scene);
        let surfaces = scene.surfaces();
        let meshes = scene.meshes(self.exec.document.model());
        let workspace = self.exec.workspace.clone();
        let exec = &mut self.exec;
        ui.scope_builder(UiBuilder::new().max_rect(geometry.views), |ui| {
            workspace.ui(
                ui,
                exec,
                Scenery {
                    surfaces: &surfaces,
                    meshes: &meshes,
                },
            );
        });
    }

    /// The status bar (buildStatusBar): the message at the left, the jobs' widget, then the
    /// permanent ones - the update button, the CRS button, the frame stats, the selection count,
    /// the current layer, the snap and the coordinates.
    fn status_bar_ui(&mut self, ui: &mut Ui, geometry: &Geometry) {
        let now = std::time::Instant::now();
        // A sentence starts with a capital where a person reads it: an error's sentence is written
        // to follow a code ("InvalidState: nothing is selected; use SELECT first"), and the bar,
        // which shows it without the code, read "nothing is selected; ..." (the third look
        // audit's J). The log and what a line hands back keep it as it was.
        let message = sentence_start(
            &self
                .exec
                .log
                .status_message(now)
                .map(String::from)
                .unwrap_or_else(|| self.ui.panels.status.clone()),
        );
        let document = &self.exec.document;
        let crs = format!("CRS: {}", crate::project_crs_dialog::project_crs_label(document));
        let selection = format!(
            "{} selected / {} entities",
            document.selection().size(),
            document.model().entities.size()
        );
        let layer = format!("Layer: {}", document.current_layer());
        let mut crs_clicked = false;
        let bar = geometry.status_bar;
        // A band of the frame's colour under a rule, its words the secondary text a size down, and
        // a short rule between the readouts on the right: the status bar is read at a glance, and
        // at the body's size and colour it competed with the command line above it.
        ui.painter()
            .rect_filled(bar, 0.0, palette::window().to_egui());
        ui.painter().hline(
            bar.x_range(),
            ui.painter().round_to_pixel_center(bar.min.y + 0.5),
            egui::Stroke::new(1.0, palette::border().to_egui()),
        );
        let quiet = |text: &str| {
            egui::Label::new(
                RichText::new(text)
                    .small()
                    .color(palette::text_muted().to_egui()),
            )
            .selectable(false)
        };
        let inside = bar.shrink2(Vec2::new(crate::theme::style::GROUP_GAP, 0.0));
        // The readouts first, from the right edge in, and the message after in what they leave:
        // laid out after the message, a long prompt at a large text size took the whole bar and
        // the readouts were drawn over it ("CRS:" through the prompt's words).
        let readouts = ui
            .scope_builder(
                UiBuilder::new()
                    .max_rect(inside)
                    .layout(Layout::right_to_left(Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = crate::theme::style::GROUP_GAP;
                    ui.spacing_mut().interact_size.y = bar.height() - 4.0;
                    let divider = |ui: &mut Ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 12.0), Sense::hover());
                        ui.painter().vline(
                            ui.painter().round_to_pixel_center(rect.center().x),
                            rect.y_range(),
                            egui::Stroke::new(1.0, palette::border().to_egui()),
                        );
                    };
                    let coordinates = &self.ui.panels.coordinates;
                    let _ = controls::label_as(
                        ui,
                        "statusCoordinates",
                        coordinates,
                        egui::Label::new(RichText::new(coordinates).small().monospace())
                            .selectable(false),
                    );
                    divider(ui);
                    let snap = &self.ui.panels.snap;
                    let _ = controls::label_as(ui, "statusSnap", snap, quiet(snap));
                    let _ = controls::label_as(ui, "statusLayer", &layer, quiet(&layer));
                    divider(ui);
                    let stats = &self.ui.panels.frame_stats;
                    let mut count_tip = String::from("Entities selected, of all the drawing holds");
                    if is_timing(stats) {
                        count_tip.push_str(&format!("\n\nLast frame: {stats}"));
                    }
                    let _ = controls::label_as(ui, "statusSelectionCount", &selection, quiet(&selection))
                        .on_hover_text(count_tip);
                    // A view's readout a person reads - a section's station and elevation, its
                    // crossings - is shown; a frame's timings are a developer's, kept in the
                    // label's report (--report FrameStatsLabel, the tests' "Plan N drawn") and the
                    // count's tooltip, not printed in a person's status bar.
                    let shown = if is_timing(stats) { "" } else { stats.as_str() };
                    let _ = controls::label_as(ui, "FrameStatsLabel", stats, quiet(shown));
                    divider(ui);
                    crs_clicked = controls::button_as(
                        ui,
                        "statusProjectCrs",
                        &crs,
                        egui::Button::new(
                            RichText::new(crs.as_str())
                                .small()
                                .color(palette::text_muted().to_egui()),
                        )
                        .frame_when_inactive(false),
                    )
                    .on_hover_text("The project's coordinate system: click to change it")
                    .clicked();
                    self.ui.update.status_button_ui(ui, &mut self.exec);
                    self.exec.jobs.status_ui(ui);
                },
            )
            .response
            .rect;
        let mut left = inside;
        left.max.x = (readouts.min.x - crate::theme::style::GROUP_GAP).max(left.min.x);
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(left)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.set_clip_rect(left.intersect(ui.clip_rect()));
                ui.spacing_mut().interact_size.y = bar.height() - 4.0;
                ui.add(egui::Label::new(RichText::new(message.as_str()).small()).truncate());
            },
        );
        if crs_clicked {
            self.show_project_crs(None);
        }
    }

    /// The menu a step opened under its title (--panel of a menu), at the window's corner as
    /// main.cpp pops it up, a Menu target named as the menu.
    fn step_menu_ui(&mut self, ui: &mut Ui, composed: &Composed) {
        let Some(name) = self.ui.open_menu.clone() else {
            return;
        };
        let Some(menu) = crate::drive::steps::find_menu(&composed.menus, &name) else {
            self.ui.open_menu = None;
            return;
        };
        let menu = menu.clone();
        let mut chosen: Option<String> = None;
        let mut torn: Option<(String, Pos2)> = None;
        let area = egui::Area::new(egui::Id::new(("katanaStepMenu", name.as_str())))
            .order(egui::Order::Foreground)
            .fixed_pos(ui.max_rect().min)
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    // The look egui gives a menu it opens itself: items without a frame until
                    // the pointer is on them, so this grab shows what a person's click shows.
                    egui::containers::menu::menu_style(ui.style_mut());
                    begin_target(ui, TargetSpec::new(&menu.name, TargetKind::Menu, &menu.title.replace('&', "")));
                    let shown = ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                        crate::shell::dialog::menu_column(ui, |ui| {
                            crate::drive::action::menu_items_tearable_ui(
                                ui, &menu, &mut chosen, &mut torn,
                            );
                        });
                    });
                    end_target(ui, shown.response.rect);
                });
            });
        let _ = area;
        if let Some(chosen) = chosen {
            self.ui.open_menu = None;
            self.ui.triggered.push(chosen);
        }
        if let Some((menu, at)) = torn {
            self.ui.open_menu = None;
            if let Err(error) = self.tear_off_menu(&menu, Some(at)) {
                self.exec.log_message(&error.describe(), true);
            }
        }
    }

    /// Every dialog of the window's and of its workbenches', each drawn in its frame with the
    /// executor as its host.
    fn dialogs_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let exec = &mut self.exec;
        let state = &mut self.ui;
        let dialogs = &mut state.dialogs;
        if dialogs.layers_shown
            && let Some(dialog) = dialogs.layers.as_mut()
        {
            dialogs.layers_shown = dialog.ui(&ctx, exec);
        }
        if let Some(dialog) = dialogs.select_by_id.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.zoom_to.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.project_crs.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.start.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.script.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.reference.as_mut()
            && let Some(verb) = dialog.ui(ui, exec)
        {
            // A double-click puts the verb on the command line, to be finished there.
            state.panels.command_text = verb;
        }
        if let Some(dialog) = dialogs.shortcuts.as_mut() {
            dialog.ui(ui);
        }
        if let Some(dialog) = dialogs.about.as_mut() {
            dialog.ui(&ctx);
        }
        if dialogs.summary_shown
            && let Some(dialog) = dialogs.summary.as_mut()
        {
            dialogs.summary_shown = dialog.ui(&ctx, exec);
        }
        if let Some(dialog) = dialogs.plot.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.image.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.surface_from.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.surface_export.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.alignments.as_mut() {
            dialog.ui(ui, exec);
        }
        // The sheet editor is the C++'s second top-level window: drawn while it is shown, over the
        // whole screen in an area of its own - not in the room the main window's layout leaves,
        // which by now is below the screen (its toolbar was grabbed at y 886 of 860).
        if let Some(editor) = dialogs.sheets.as_mut()
            && editor.is_visible()
        {
            let screen = ctx.content_rect();
            egui::Area::new(egui::Id::new("katanaSheetEditorWindow"))
                .order(egui::Order::Middle)
                .fixed_pos(screen.min)
                .movable(false)
                .show(&ctx, |ui| {
                    let mut window = ui.new_child(egui::UiBuilder::new().max_rect(screen));
                    window.set_clip_rect(screen);
                    window
                        .painter()
                        .rect_filled(screen, 0.0, crate::theme::palette::window().to_egui());
                    editor.ui(&mut window, exec);
                });
        }
        if let Some(dialog) = dialogs.ifc_import.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.ifc_export.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.volumes.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.surface_styles.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.dataset_info.as_mut() {
            dialog.ui(ui, exec);
        }
        if let Some(dialog) = dialogs.export_options.as_mut() {
            crate::geo::gis_tool_dialog::GisTool::ui(dialog, ui, exec);
        }
        if let Some(dialog) = dialogs.attributes.as_mut() {
            dialog.ui(ui, exec);
        }
        if dialogs.style_manager_shown
            && let Some(dialog) = dialogs.style_manager.as_mut()
        {
            dialogs.style_manager_shown = dialog.ui(&ctx, exec);
        }
        // The workbenches' dialogs, in the order the workbenches were built.
        state.format.ui(ui, exec);
        state.annotation.ui(ui, exec);
        state.survey.ui(ui, exec);
        state.utilities.ui(ui, exec);
        state.road_objects.ui(ui, exec);
        state.surface_edit.ui(ui, exec);
        state.geo_dialogs.ui(ui, exec);
        state.update.ui(ui, exec);
        let _ = super::executor::lend!(exec, road, |road, host| Workbench::ui(road, ui, host));
        exec.take_road_events();
        let _ = super::executor::lend!(exec, online, |online, host| Workbench::ui(online, ui, host));
        // The import options dialog: accepted, its line run as the menu's would be.
        let finished = match dialogs.import_options.as_mut() {
            Some((_, ImportOptions::Vector(dialog))) => dialog.ui(ui, exec),
            Some((_, ImportOptions::Raster(dialog))) => dialog.ui(ui, exec),
            Some((_, ImportOptions::Cloud(dialog))) => dialog.ui(ui, exec),
            Some((_, ImportOptions::Placement(dialog))) => dialog.ui(ui, exec),
            Some((_, ImportOptions::Ifc(dialog))) => {
                dialog.ui(ui, exec);
                None
            }
            None => None,
        };
        // File > Plot Sheets' Plot dialog.
        let plotted = match dialogs.plot_sheets.as_mut() {
            Some((dialog, _)) => dialog.ui(&ctx, exec.headless),
            None => PlotAnswer::Open,
        };
        let input = match dialogs.input.as_mut() {
            Some(dialog) => dialog.ui(&ctx).map(|ok| (ok, dialog.value(), dialog.purpose.clone())),
            None => None,
        };
        // The start screen's answer, kept for the next start.
        self.remember_start_screen();
        match finished {
            Some(DialogResult::Accepted) => {
                if let Some((_, options)) = self.ui.dialogs.import_options.take() {
                    self.finish_import(&options);
                }
            }
            Some(DialogResult::Rejected) => {
                self.ui.dialogs.import_options = None;
                self.exec.log_message("Import cancelled.", false);
            }
            None => {}
        }
        match plotted {
            PlotAnswer::Accepted => {
                if let Some((dialog, set)) = self.ui.dialogs.plot_sheets.take() {
                    let request = dialog.request();
                    let _ = self.exec.plot_sheets(&set, &request);
                }
            }
            PlotAnswer::Cancelled => self.ui.dialogs.plot_sheets = None,
            PlotAnswer::Open => {}
        }
        if let Some((ok, value, purpose)) = input {
            self.ui.dialogs.input = None;
            if ok {
                self.input_answered(&purpose, &value);
            }
        }
    }

    /// A small question answered: what asked it carried out.
    fn input_answered(&mut self, purpose: &InputPurpose, value: &str) {
        match purpose {
            InputPurpose::Exaggeration => {
                if let Some(factor) = katana_core::text::parse_finite_double(value) {
                    let _ = self.exec.run_verb_line(&format!(
                        "EXAGGERATION {}",
                        katana_core::text::format_exact_real(factor)
                    ));
                }
            }
            InputPurpose::SectionAlignment => {
                if !value.is_empty() {
                    self.cut_section_down(value);
                }
            }
            InputPurpose::NewLayer => {
                if !value.is_empty() {
                    self.create_layer(value);
                }
            }
            InputPurpose::NewChildLayer(parent) => {
                if !value.is_empty() {
                    let path = katana_entity::layer_path::join_layer_path(parent, value);
                    self.create_layer(&path);
                }
            }
            InputPurpose::RenameLayer(from) => {
                if !value.is_empty() {
                    let command = katana_commands::rename_layer(from, value);
                    match self.exec.document.execute(command) {
                        Ok(()) => self
                            .exec
                            .log_message(&format!("Renamed '{from}' to '{value}'."), false),
                        Err(error) => self.exec.log_message(&error.describe(), true),
                    }
                }
            }
        }
    }

    fn create_layer(&mut self, name: &str) {
        let layer = Layer {
            name: String::from(name),
            ..Layer::default()
        };
        let command = katana_commands::create_layer(layer);
        if let Err(error) = self.exec.document.execute(command) {
            self.exec.log_message(&error.describe(), true);
        }
    }

    /// What the views raised since the last look (the C++ workspace's hooks). Also called after
    /// each call and by the headless pointer steps, so what a click or a tool said is logged
    /// before the step's record, as the C++'s signals log it during the click.
    pub(super) fn workspace_events(&mut self) {
        use crate::view_workspace::WorkspaceEvent;
        for event in self.exec.workspace.take_events() {
            match event {
                WorkspaceEvent::Status(text) => self.ui.panels.status = text,
                WorkspaceEvent::FrameStats(text) => self.ui.panels.frame_stats = text,
                WorkspaceEvent::Prompt(prompt) => {
                    // A running tool's prompt shows where its answer is typed, the command line's
                    // field, and in the view's band over the drawing; the status bar said it a
                    // third time at once (the third look audit's H), so it says it only when no
                    // tool runs.
                    if !prompt.is_empty() && !self.exec.workspace.active_tool_id().is_empty() {
                        self.ui.panels.status.clear();
                        self.ui.panels.placeholder = prompt;
                    } else {
                        self.ui.panels.status = prompt;
                    }
                }
                WorkspaceEvent::Error(error) => self.exec.log_message(&error, true),
                WorkspaceEvent::CursorMoved { world, snap } => {
                    self.ui.panels.coordinates = super::executor::plan_point(world);
                    self.ui.panels.snap = snap.map(|snap| String::from(snap.mode.name())).unwrap_or_default();
                }
                WorkspaceEvent::ActiveToolChanged(id) => {
                    if id == "draw.vertex.edit" {
                        self.ui.drawing.show_vertices();
                    }
                    self.ui.tools.set_active(&id);
                }
                WorkspaceEvent::ToolMessage(message) => self.exec.log_message(&message, false),
                // Type anywhere: a letter typed over the drawing with no tool running is the
                // start of a command.
                WorkspaceEvent::TextTyped(text) => self.ui.panels.command_text.push_str(&text),
                WorkspaceEvent::EntityDoubleClicked(id) => self.edit_double_clicked(id),
                WorkspaceEvent::ContextMenu { .. }
                | WorkspaceEvent::ActiveChanged
                | WorkspaceEvent::ViewsChanged
                | WorkspaceEvent::LinksChanged
                | WorkspaceEvent::ViewSettingsChanged => {}
            }
        }
    }

    /// A double click on entity `id` with no tool running: it alone selected, and its editor
    /// opened - a text's or a label's or a leader's where the window has one, the Properties panel
    /// otherwise.
    fn edit_double_clicked(&mut self, id: katana_entity::EntityId) {
        let document = &mut self.exec.document;
        let Some(entity) = document.model().entities.find(id) else {
            return;
        };
        let editor = match &entity.geometry {
            katana_entity::Geometry::Text(_) => Some("annotateEditText"),
            katana_entity::Geometry::Label(_) => Some("annotateEditLabel"),
            katana_entity::Geometry::Leader(_) => Some("annotateLeaders"),
            _ => None,
        };
        if document.selection().size() != 1 || !document.selection().contains(id) {
            document.selection_mut().set(vec![id]);
            document.notify_selection_changed();
        }
        if let Some(editor) = editor
            && self.trigger(editor).is_ok()
        {
            return;
        }
        self.ui.docks.properties = true;
    }

    /// A panel's title bar asked: Minimise and Close hide it (View > Panels brings it back).
    fn panel_request(&mut self, request: ChromeRequest) {
        if self.menu_panel_request(&request) {
            return;
        }
        let name = match &request {
            ChromeRequest::Minimise(name) | ChromeRequest::Close(name) => name.clone(),
            _ => return,
        };
        let docks = &mut self.ui.docks;
        match name.as_str() {
            "LayersDock" => docks.layers = false,
            "PropertiesDock" => docks.properties = false,
            "CommandLineDock" => docks.command_line = false,
            "ReferenceDataDock" => docks.reference = false,
            "SurfacesDock" => docks.surfaces = false,
            "VerticesDock" => {
                let _ = self.ui.drawing.trigger(&mut self.exec.document, crate::drawing::drawing_ui::VERTICES_PANEL_ACTION);
            }
            _ => self.ui.survey.show_point_manager(false),
        }
    }
}

use crate::plotting::plot_dialog::PlotAnswer;

/// The window's title: the project's folder name, or Untitled, and "*" while it is modified.
pub fn window_title(document: &katana_cad::Document) -> String {
    let name = document
        .project_directory()
        .and_then(|directory| directory.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| String::from("Untitled"));
    format!(
        "{name}{} - Katana",
        if document.is_modified() { "*" } else { "" }
    )
}

/// Whether a view's readout is a frame's timing - the plan's "Plan  20 drawn  5.4 ms (kept, 0.2
/// ms)", a 3D view's "3D  1036 tri  4.2 ms (scene 2.0 ms)" - a developer's measure, which every
/// one of them ends with; a section's readouts (its crossings, the station and elevation under
/// the pointer) never do.
fn is_timing(readout: &str) -> bool {
    readout.ends_with(" ms") || readout.ends_with(" ms)")
}

/// A menu drawn as a menu button with its items, by the one drawer every menu of the window uses
/// (drive::action::menu_items_ui: a tick and icon gutter, sections, inset separators, submenus) in
/// a column that scrolls when the menu is taller than the room under the bar (the Annotate menu);
/// a click on an action is the action chosen.
fn menu_ui(
    ui: &mut Ui,
    menu: &Menu,
    chosen: &mut Vec<String>,
    torn: &mut Option<(String, Pos2)>,
) {
    ui.menu_button(menu.title.replace('&', ""), |ui| {
        let mut one = None;
        crate::shell::dialog::menu_column(ui, |ui| {
            crate::drive::action::menu_items_tearable_ui(ui, menu, &mut one, torn);
        });
        if let Some(name) = one {
            chosen.push(name);
            ui.close();
        }
        if torn.is_some() {
            ui.close();
        }
    });
}

// ---- a panel's own tools -------------------------------------------------------------------------

/// A row of a list or tree in a panel (the Layers tree, a view's layers popup): denser than a
/// form's control.
pub(crate) const LIST_ROW: f32 = 22.0;

/// The side of a panel tool's icon: the design's 16 point grid, in a control-high square.
const PANEL_ICON: f32 = 16.0;

/// The slot before a layer's name that holds the current layer's mark, and the mark's radius: a
/// dot the size of a snap marker's, small beside the 14 point eye and padlock before it.
const CURRENT_MARK_SLOT: f32 = 8.0;
const CURRENT_MARK_RADIUS: f32 = 3.0;

/// A panel's tool: an icon in a square a control high, framed only under the pointer, its name
/// and what it does in the tooltip. It reports and is pressed as `name`, its accessible text
/// `label`, as the worded button it replaced was; with no icon of that name it shows `label`.
pub(crate) fn panel_tool(ui: &mut Ui, name: &str, icon: &str, label: &str, tip: &str) -> egui::Response {
    ui.scope(|ui| {
        // The icon's square padded to the control's height and no more: with the style's button
        // padding each tool was 32 points, and the Reference Data panel's seven ran off its edge.
        let side = crate::theme::style::CONTROL_HEIGHT;
        ui.spacing_mut().button_padding = Vec2::splat((side - PANEL_ICON) / 2.0);
        panel_tool_button(ui, name, icon, label, tip)
    })
    .inner
}

fn panel_tool_button(ui: &mut Ui, name: &str, icon: &str, label: &str, tip: &str) -> egui::Response {
    let side = crate::theme::style::CONTROL_HEIGHT;
    // The set has no closed eye yet: Hide is the open eye faded, beside Show's.
    let (icon, mode) = if icon == "LayerHidden" && !crate::icons::is_named_icon(icon) {
        ("LayerVisible", crate::icons::IconMode::Disabled)
    } else {
        (icon, crate::icons::IconMode::Normal)
    };
    let button = match crate::icons::texture(ui.ctx(), icon, PANEL_ICON, mode) {
        Some(texture) => egui::Button::image(egui::Image::new((
            texture.id(),
            Vec2::splat(PANEL_ICON),
        ))),
        None => egui::Button::new(label),
    }
    .frame_when_inactive(false)
    .min_size(Vec2::splat(side));
    let (label, tip) = (String::from(label), String::from(tip));
    controls::button_as(ui, name, &label, button).on_hover_ui(move |ui| {
        ui.strong(label);
        ui.label(tip);
    })
}

/// A short rule between two groups of a panel's tools.
pub(crate) fn group_rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(crate::theme::style::GROUP_GAP, crate::theme::style::CONTROL_HEIGHT),
        Sense::hover(),
    );
    ui.painter().vline(
        ui.painter().round_to_pixel_center(rect.center().x),
        rect.shrink2(Vec2::new(0.0, 5.0)).y_range(),
        egui::Stroke::new(1.0, palette::border().to_egui()),
    );
}

/// A two-state switch of a list row (a layer's eye or padlock, the eye of a view's layers popup):
/// the icon full while `on`, faint while off, framed only under the pointer, a click a press of
/// it. With no icon painter (a test's context) a word stands in the icon's place.
pub(crate) fn row_switch(ui: &mut Ui, icon: &str, on: bool, tip: &str) -> egui::Response {
    let side = 14.0;
    let response = match crate::icons::texture(ui.ctx(), icon, side, crate::icons::IconMode::Normal) {
        Some(texture) => ui.add(
            egui::Button::image(
                egui::Image::new((texture.id(), Vec2::splat(side))).tint(if on {
                    egui::Color32::WHITE
                } else {
                    egui::Color32::from_white_alpha(70)
                }),
            )
            .frame_when_inactive(false)
            .min_size(Vec2::splat(LIST_ROW)),
        ),
        None => ui.add(egui::Button::selectable(on, if on { "x" } else { "-" }).min_size(Vec2::splat(LIST_ROW))),
    };
    response.on_hover_text(tip)
}

// ---- a toolbar of words -------------------------------------------------------------------------

/// The name a worded action's slot on its strip goes by: `word:<width>:<action>`, the width the
/// word needs in points, so the strip lays it out without a font of its own to measure with.
const WORD_PREFIX: &str = "word:";

/// The action a worded slot stands for; None for any other item.
fn word_of(name: &str) -> Option<&str> {
    name.strip_prefix(WORD_PREFIX)
        .and_then(|rest| rest.split_once(':'))
        .map(|(_, action)| action)
}

/// A worded toggle of the Drafting bar, named `name`: its word in the text colour over a 2 point
/// accent bar while it is on, in the secondary text with no bar while it is off, raised under the
/// pointer. A filled pill for "on" read as a pressed button, and egui's selectable button drew
/// nothing at all for "off" - so at rest nothing said which modes were on. It reports and takes a
/// press as `controls::toggle` does: a Toggle with its checked state, a fill refused.
fn word_toggle(ui: &mut Ui, name: &str, text: &str, checked: bool) -> egui::Response {
    use crate::drive::mailbox::{Act, no_field};
    use crate::drive::registry::{ControlKind, ControlRecord, record, settle, take};
    let taken = take(ui, name);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(String::from(text), font, egui::Color32::PLACEHOLDER);
    let padding = 6.0;
    let size = Vec2::new(
        galley.size().x + 2.0 * padding,
        crate::theme::style::CONTROL_HEIGHT,
    );
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), checked, text)
    });
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() && ui.is_enabled() {
            painter.rect_filled(rect, egui::CornerRadius::same(4), palette::raised().to_egui());
        }
        let ink = if !ui.is_enabled() {
            palette::text_disabled()
        } else if checked {
            palette::text()
        } else {
            palette::text_muted()
        };
        let at = Pos2::new(rect.left() + padding, rect.center().y - galley.size().y / 2.0);
        let under = at.y + galley.size().y + 1.0;
        painter.galley_with_override_text_color(at, galley.clone(), ink.to_egui());
        if checked {
            // Under the word, not the whole slot: the bar marks the word that is on.
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(at.x, under),
                    Pos2::new(at.x + galley.size().x, (under + 2.0).min(rect.bottom())),
                ),
                egui::CornerRadius::ZERO,
                palette::accent().to_egui(),
            );
        }
    }
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Press => {
                if controls::accept_press(ui, taken) {
                    response
                        .flags
                        .insert(egui::response::Flags::FAKE_PRIMARY_CLICKED);
                    settle(ui, taken, Ok(()));
                }
            }
            Act::Fill(fill) => {
                if controls::accept_fill(ui, taken) {
                    settle(ui, taken, Err(no_field(name, fill)));
                }
            }
            Act::ScrollTo => {
                response.scroll_to_me(None);
                settle(ui, taken, Ok(()));
            }
        }
    }
    let mut control = ControlRecord::new(name, ControlKind::Toggle);
    control.text = String::from(text);
    control.checked = Some(checked);
    control.enabled = ui.is_enabled();
    control.visible = ui.is_visible();
    record(ui, &response, control);
    response
}

/// `bar` with every action that has no icon put as a word (a Widget slot `word_of` reads), and
/// those actions: a word's slot is its text in the button font and 6 points either side.
fn worded(bar: &ToolBar, ctx: &Context) -> (ToolBar, Vec<Action>) {
    let mut out = bar.clone();
    let mut texts = Vec::new();
    let font = egui::TextStyle::Button.resolve(&ctx.global_style());
    for item in &mut out.items {
        if let ToolBarItem::Action(action) = item
            && action.icon.is_empty()
        {
            let text = action.text.replace('&', "");
            let width = ctx.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(text, font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            });
            let slot = (width + 2.0 * 6.0).ceil() as i32;
            texts.push(action.clone());
            *item = ToolBarItem::Widget(format!("{WORD_PREFIX}{slot}:{}", action.name));
        }
    }
    (out, texts)
}

// ---- the Properties toolbar and panel ----------------------------------------------------------

/// The Properties toolbar's current style (CurrentStyleCombo): ByLayer, then the drawing's styles;
/// a current style the list does not hold is shown, and marked. A choice is the current style for
/// new work: session state, not a command.
fn current_style_ui(ui: &mut Ui, exec: &mut super::executor::Executor, panels: &mut Panels) {
    let document = &exec.document;
    let names = document.model().styles.names();
    let mut items = vec![String::from("ByLayer")];
    let mut data = vec![String::new()];
    for name in &names {
        items.push(name.clone());
        data.push(name.clone());
    }
    let current = document.current_style().to_owned();
    let index = if current.is_empty() {
        Some(0)
    } else {
        data.iter().position(|each| *each == current)
    };
    let index = index.unwrap_or_else(|| {
        items.push(format!("{current}  (not in the drawing)"));
        data.push(current.clone());
        items.len() - 1
    });
    panels.current_style.choose(index, &items);
    ui.horizontal(|ui| {
        // Clear of the grip before it, as a bar's name is (layout::NAME_LEAD): the Properties bar
        // shows no name, its "Style" stands first, and the dots touched it ("#Style").
        ui.add_space(layout::NAME_LEAD as f32);
        ui.label(RichText::new("Style").color(palette::text_muted().to_egui()));
        ui.spacing_mut().combo_width = (ui.available_width() - 4.0).max(60.0);
        let response = controls::choice(ui, "CurrentStyleCombo", &items, &mut panels.current_style)
            .on_hover_text(
                "Current Style: the style everything drawn from now on is given. ByLayer draws \
                 with the layer's linetype and colour.",
            );
        if response.changed()
            && let Some(chosen) = panels.current_style.index
            && let Some(name) = data.get(chosen)
        {
            match exec.document.set_current_style(name) {
                Ok(()) => exec.log_message(
                    &format!(
                        "New work is drawn in {}.",
                        if name.is_empty() { "ByLayer" } else { name }
                    ),
                    false,
                ),
                Err(error) => exec.log_message(&error.describe(), true),
            }
        }
    });
}

/// The Properties panel: the selection's Style row above the property tree.
fn properties_ui(window: &mut MainWindow, ui: &mut Ui) {
    let exec = &mut window.exec;
    let panels = &mut window.ui.panels;
    let document = &exec.document;
    let names = document.model().styles.names();
    let mut items = vec![String::from("ByLayer")];
    items.extend(names);
    let mut styles: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for id in document.selection().ids() {
        if let Some(entity) = document.model().entities.find(id) {
            styles.insert(entity.style.clone());
        }
    }
    let any = !styles.is_empty();
    let mut apply = false;
    ui.horizontal(|ui| {
        // Muted, as every label beside a field is, the property tree's under it included.
        crate::shell::dialog::form_label(ui, "Style");
        ui.add_enabled_ui(any, |ui| {
            let focused = ui.memory(|memory| memory.has_focus(ui.id()));
            if !focused && styles.len() == 1 {
                let style = styles.iter().next().cloned().unwrap_or_default();
                if panels.property_style.text.is_empty() {
                    panels.property_style.text = if style.is_empty() {
                        String::from("ByLayer")
                    } else {
                        style
                    };
                }
            } else if !focused
                && styles.len() != 1
                && !styles.iter().eq(panels.property_styles_seen.iter())
            {
                // Nothing selected, or a selection of several styles: no one style is the
                // selection's, so the choice is empty and its hint says why.
                panels.property_style.text.clear();
                panels.property_style.index = None;
            }
            // What an empty choice says (controls::choice_with_hint): "No selection" with nothing
            // selected and "Varies" when the selection's styles differ - not "ByLayer", which
            // claims a style of a selection that is not there, nor the bare "None".
            let hint = if any { "Varies" } else { "No selection" };
            panels.property_styles_seen = styles.iter().cloned().collect();
            // The choice - one framed control, its list's arrow inside its right end
            // (drive::controls::editable_choice_frame) - takes the row's width less Apply, so
            // Apply is on the panel at its 240 points and the two end at its edge.
            let apply_width = 56.0;
            ui.spacing_mut().text_edit_width =
                (ui.available_width() - apply_width - ui.spacing().item_spacing.x).max(40.0);
            let response = controls::choice_with_hint(
                ui,
                "PropertyStyle",
                &items,
                &mut panels.property_style,
                hint,
            );
            if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                apply = true;
            }
            if controls::button(ui, "PropertyStyleApply", "Apply")
                .on_hover_text("Apply Style: give the selected entities this style, in one undoable step.")
                .clicked()
            {
                apply = true;
            }
        });
    });
    if apply {
        let text = panels.property_style.current_text(&items);
        apply_property_style(exec, text.trim());
    }
    let MainWindow { exec, ui: state } = window;
    state
        .properties
        .ui(ui, &exec.document, exec.clipboard.as_mut());
}

/// MainWindow::applyPropertyStyle: the selection's entities that are not in the chosen style yet
/// get it, in one undoable command.
fn apply_property_style(exec: &mut super::executor::Executor, text: &str) {
    let ids = exec.document.selection().ids();
    if ids.is_empty() {
        exec.log_message("Style: nothing is selected.", true);
        return;
    }
    if text.is_empty() {
        exec.log_message(
            "Style: the selection's styles vary; choose one, or ByLayer, to give them all.",
            true,
        );
        return;
    }
    // Names are case-sensitive, so only "ByLayer" itself means none.
    let style = if text == "ByLayer" { "" } else { text };
    let changing: Vec<katana_entity::EntityId> = ids
        .into_iter()
        .filter(|id| {
            exec.document
                .model()
                .entities
                .find(*id)
                .is_some_and(|entity| entity.style != style)
        })
        .collect();
    if changing.is_empty() {
        exec.log_message(&format!("Style: the selection is already in {text}."), false);
        return;
    }
    let count = changing.len();
    let command = katana_commands::set_entity_style(changing, style.to_owned());
    if let Err(error) = exec.document.execute(command) {
        exec.log_message(&format!("Style: {}", error.describe()), true);
        return;
    }
    exec.log_message(
        &format!(
            "Style {text} given to {} {}.",
            crate::format::grouped(count as u64),
            if count == 1 { "entity" } else { "entities" }
        ),
        false,
    );
}

// ---- the Layers panel -----------------------------------------------------------------------

/// The Layers panel: its four tools, and the layers as a tree of rows - shown, locked, colour,
/// count - the current layer marked "  <".
fn layers_ui(window: &mut MainWindow, ui: &mut Ui) {
    let mut asked: Option<&'static str> = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::style::GAP / 2.0;
        for (name, icon, label, tip) in [
            ("LayerNewButton", "LayerNew", "New Layer", "Add a layer at the top of the tree."),
            ("LayerNewChildButton", "LayerNewChild", "New Child Layer", "Add a layer beneath the selected one."),
            ("LayerRenameButton", "Rename", "Rename Layer", "Rename the selected layer and everything beneath it."),
            ("LayerDeleteButton", "Erase", "Delete Layer", "Delete the selected layer."),
        ] {
            if panel_tool(ui, name, icon, label, tip).clicked() {
                asked = Some(name);
            }
        }
    });
    let document = &window.exec.document;
    let layers = &document.model().layers;
    let mut toggled: Option<(String, bool, bool)> = None;
    let mut made_current: Option<String> = None;
    egui::ScrollArea::vertical()
        .id_salt("katanaLayerTree")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // A tree's rows are denser than a form's: a 22 point row, nothing between rows.
            ui.spacing_mut().interact_size.y = LIST_ROW;
            ui.spacing_mut().item_spacing = Vec2::new(crate::theme::style::GAP / 2.0, 0.0);
            for path in layers.names() {
                let Some(layer) = layers.find(&path) else {
                    continue;
                };
                let depth = path.matches('/').count();
                let leaf = path.rsplit('/').next().unwrap_or(&path).to_owned();
                ui.horizontal(|ui| {
                    ui.add_space(14.0 * depth as f32);
                    // Shown and locked as the eye and the padlock, lit while the layer is shown
                    // or locked and faint while it is not; a click flips each as its box did.
                    if row_switch(ui, "LayerVisible", layer.visible, if layer.visible { "Shown: click to hide" } else { "Hidden: click to show" })
                        .clicked()
                    {
                        toggled = Some((path.clone(), true, !layer.visible));
                    }
                    if row_switch(ui, "LayerLocked", layer.locked, if layer.locked { "Locked: click to unlock" } else { "Unlocked: click to lock" })
                        .clicked()
                    {
                        toggled = Some((path.clone(), false, !layer.locked));
                    }
                    // The current layer: an accent dot in a slot every row keeps before its name,
                    // and its name strong. It was a typed "  <" after the name, which read as part
                    // of it.
                    let current = path == document.current_layer();
                    let (slot, _) =
                        ui.allocate_exact_size(Vec2::new(CURRENT_MARK_SLOT, LIST_ROW), Sense::hover());
                    if current {
                        ui.painter().circle_filled(
                            slot.center(),
                            CURRENT_MARK_RADIUS,
                            palette::accent().to_egui(),
                        );
                    }
                    let text = if !layers.effectively_visible(&path) {
                        RichText::new(leaf).color(palette::text_disabled().to_egui())
                    } else if current {
                        RichText::new(leaf).strong()
                    } else {
                        RichText::new(leaf)
                    };
                    let picked = window.ui.panels.layer_selected.as_deref() == Some(path.as_str());
                    let count = format!("{}", document.model().entities.count_on_layer(&path));
                    // One button the rest of the row: the name at its left, the count at its
                    // right in the secondary text, so the whole row is the target and the
                    // selection's band runs under both.
                    let response = ui
                        .add(
                            egui::Button::selectable(picked, text)
                                .right_text(
                                    RichText::new(count)
                                        .small()
                                        .color(palette::text_muted().to_egui()),
                                )
                                .frame_when_inactive(false)
                                .truncate()
                                .min_size(Vec2::new(ui.available_width(), LIST_ROW)),
                        )
                        .on_hover_text(if current {
                            format!("{path} (the current layer: new work is drawn on it)")
                        } else {
                            path.clone()
                        });
                    if response.clicked() {
                        window.ui.panels.layer_selected = Some(path.clone());
                    }
                    if response.double_clicked() {
                        made_current = Some(path.clone());
                    }
                });
            }
        });
    if let Some((path, visible_column, checked)) = toggled {
        layer_box_changed(&mut window.exec, &path, visible_column, checked);
    }
    if let Some(path) = made_current
        && let Err(error) = window.exec.document.set_current_layer(&path)
    {
        window.exec.log_message(&error.describe(), true);
    }
    match asked {
        Some("LayerNewButton") => {
            if !window.exec.headless {
                window.ui.dialogs.input = Some(InputDialog::text(
                    "New Layer",
                    "Layer name (use / to nest, e.g. design/surface/tin1):",
                    "",
                    InputPurpose::NewLayer,
                ));
            }
        }
        Some("LayerNewChildButton") => match window.ui.panels.layer_selected.clone() {
            None => window
                .exec
                .log_message("Select the layer to nest under first.", true),
            Some(parent) => {
                window.ui.dialogs.input = Some(InputDialog::text(
                    "New Nested Layer",
                    &format!("New layer under '{parent}':"),
                    "",
                    InputPurpose::NewChildLayer(parent),
                ));
            }
        },
        Some("LayerRenameButton") => match window.ui.panels.layer_selected.clone() {
            None => window.exec.log_message("Select a layer to rename.", true),
            Some(from) => {
                window.ui.dialogs.input = Some(InputDialog::text(
                    "Rename Layer",
                    "New full path (use / to nest):",
                    &from,
                    InputPurpose::RenameLayer(from.clone()),
                ));
            }
        },
        Some(_) => {
            if let Some(name) = window.ui.panels.layer_selected.clone() {
                delete_layer(&mut window.exec, &name);
            }
        }
        None => {}
    }
}

/// A layer's shown or locked box changed (onLayerItemChanged): the layer updated as one command;
/// the current layer cannot be locked.
fn layer_box_changed(exec: &mut super::executor::Executor, path: &str, visible_column: bool, checked: bool) {
    let Some(current) = exec.document.model().layers.find(path).cloned() else {
        return;
    };
    let mut changed = current;
    if visible_column {
        changed.visible = checked;
    } else {
        changed.locked = checked;
        if checked && path == exec.document.current_layer() {
            exec.log_message("The current layer cannot be locked.", true);
            return;
        }
    }
    let command = katana_commands::update_layer(changed);
    if let Err(error) = exec.document.execute(command) {
        exec.log_message(&error.describe(), true);
    }
}

/// Delete Layer: a branch is asked about first (never headless, where it is refused).
fn delete_layer(exec: &mut super::executor::Executor, name: &str) {
    let layers = &exec.document.model().layers;
    let branch = layers.has_children(name);
    if branch {
        let under = layers.subtree(name).len().saturating_sub(1);
        if exec.headless
            || !super::executor::ask_yes_no(
                "Delete Layer Tree",
                &format!("'{name}' contains {under} nested layers. Delete all of them?"),
            )
        {
            return;
        }
    }
    let command = if branch {
        katana_commands::delete_layer_tree(name)
    } else {
        katana_commands::delete_layer(name)
    };
    if let Err(error) = exec.document.execute(command) {
        exec.log_message(&error.describe(), true);
    }
}

/// MainWindow::toggleLayerThroughPanel: the layer's visibility box flipped as a click does, and
/// whether the window came through it properly - the document records the change, and the panel
/// shows it.
pub fn toggle_layer_through_panel(window: &mut MainWindow, layer: &str) -> Status {
    let Some(before) = window.exec.document.model().layers.find(layer) else {
        return Err(make_error(ErrorCode::NotFound, "no such layer", layer));
    };
    let was_visible = before.visible;
    layer_box_changed(&mut window.exec, layer, true, !was_visible);
    let after = window.exec.document.model().layers.find(layer);
    match after {
        Some(after) if after.visible != was_visible => Ok(()),
        _ => Err(make_error(
            ErrorCode::Internal,
            "the document did not record the change",
            layer,
        )),
    }
}

// ---- the Reference Data panel -------------------------------------------------------------------

/// The Reference Data panel (buildReferenceDock): its tools, the layers' table and how the
/// selected one is drawn. Every change is a REFS line through the one executor.
fn reference_ui(window: &mut MainWindow, ui: &mut Ui) {
    let mut asked: Option<&'static str> = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::style::GAP / 2.0;
        // Grouped as the work goes: bring one in and find it; show or hide it; read it and build
        // its overviews; take it out.
        for (at, (name, icon, label, tip)) in [
            ("ReferenceImportButton", "Import", "Import", "Import a drawing, an image or a point cloud, as File > Import does."),
            ("ReferenceZoomButton", "ZoomTo", "Zoom To", "Frame the selected reference layer in every plan view."),
            ("referenceShow", "LayerVisible", "Show", "Show the selected reference layer (REFS SHOW)."),
            ("referenceHide", "LayerHidden", "Hide", "Hide the selected reference layer (REFS HIDE)."),
            ("referenceInfo", "DatasetInfo", "Information", "The selected layer's source and display (REFS INFO), and what its file holds."),
            ("referenceOverviews", "BuildOverviews", "Build Overviews", "Build overviews for the selected raster, written beside its file as .ovr (REFS OVERVIEWS ... CONFIRM)."),
            ("referenceRemove", "Erase", "Remove", "Remove the selected reference layer (REFS REMOVE). The file is not touched."),
        ]
        .into_iter()
        .enumerate()
        {
            if matches!(at, 2 | 4 | 6) {
                group_rule(ui);
            }
            if panel_tool(ui, name, icon, label, tip).clicked() {
                asked = Some(name);
            }
        }
    });
    let reference = &window.exec.reference;
    // Three columns, which fit the panel's 240: a fourth, Kind ("Raster", "Point cloud"), ran the
    // last column's header off the panel's edge ("Show", the third look audit's J), and the size
    // already says the kind - pixels for a raster, points for a cloud.
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut ids: Vec<u64> = Vec::new();
    for raster in reference.rasters() {
        let mut detail = format!("{} x {} px", raster.width, raster.height);
        if !raster.has_geotransform {
            detail.push_str("  (not georeferenced)");
        }
        rows.push(vec![
            raster.name.clone(),
            detail,
            format!("{}%", (raster.opacity * 100.0).round() as i64),
        ]);
        ids.push(raster.id);
    }
    for cloud in reference.point_clouds() {
        let mut detail = format!("{} pts", crate::format::grouped(cloud.points.len() as u64));
        if cloud.is_decimated() {
            detail.push_str(&format!(
                " of {}",
                crate::format::grouped(cloud.source_point_count)
            ));
        }
        rows.push(vec![
            cloud.name.clone(),
            detail,
            String::from(cloud.color_mode.name()),
        ]);
        ids.push(cloud.id);
    }
    let mut current = window
        .ui
        .panels
        .reference_selected
        .and_then(|id| ids.iter().position(|each| *each == id));
    // The window's one table look (a raised header, stripes, the chosen row lit), scrolled across
    // inside the panel rather than cut at its edge; with nothing imported, the panel says so, as
    // the Surfaces panel does, rather than showing nothing at all under its tools. The list was
    // a bare row of words, "terrain Raster 120 x 90 px 100%" (the second look audit's T8).
    if rows.is_empty() {
        // Still the list a driver names: a fill of it is refused as naming no row.
        let _ = controls::rows(ui, "referenceList", &rows, &mut current);
        let _ = crate::dialog_layout::empty_table(
            ui,
            "No reference data. Import brings in an image, a raster or a point cloud to show \
             under the drawing.",
        );
    } else {
        let shown = ui
            .scope(|ui| {
                scroll_bar_seen(ui);
                egui::ScrollArea::horizontal()
                    .id_salt("referenceListScroll")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        crate::shell::dialog::table_or_empty(
                            ui,
                            "referenceList",
                            &["Name", "Size", "Shown"],
                            &rows,
                            current,
                            "",
                        )
                    })
                    .inner
            })
            .inner;
        if let Some(at) = shown.chosen
            && current != Some(at)
        {
            window.ui.panels.reference_selected = ids.get(at).copied();
        }
        if shown.double_clicked.is_some() {
            asked = Some("ReferenceZoomButton");
        }
    }
    reference_display_ui(window, ui);
    match asked {
        Some("ReferenceImportButton") => window.import_file(),
        Some("ReferenceZoomButton") => zoom_to_selected_reference(window),
        Some("referenceShow") => {
            let _ = run_reference_line(window, "SHOW", "");
        }
        Some("referenceHide") => {
            let _ = run_reference_line(window, "HIDE", "");
        }
        Some("referenceRemove") => {
            let _ = run_reference_line(window, "REMOVE", "");
        }
        Some("referenceInfo") => {
            let _ = run_reference_line(window, "INFO", "");
        }
        Some("referenceOverviews") => {
            let raster = window
                .ui
                .panels
                .reference_selected
                .and_then(|id| window.exec.reference.find_raster(id).map(|raster| raster.name.clone()));
            match raster {
                None => window
                    .exec
                    .log_message("Select a raster first: overviews are a raster's.", true),
                Some(name) => {
                    // Writing beside a person's file is asked for: CONFIRM is the answer. A
                    // headless run has nobody to ask, so its line goes without it.
                    let confirmed = !window.exec.headless
                        && super::executor::ask_yes_no(
                            "Build Overviews",
                            &format!("Write the overviews of '{name}' beside its file?"),
                        );
                    if window.exec.headless || confirmed {
                        let _ = run_reference_line(
                            window,
                            "OVERVIEWS",
                            if confirmed { "CONFIRM" } else { "" },
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

/// The opacities referenceOpacity offers and the REFS OPACITY value each runs (buildReferenceDock).
const REFERENCE_OPACITIES: [(&str, &str); 4] =
    [("100%", "1"), ("75%", "0.75"), ("50%", "0.5"), ("25%", "0.25")];

/// The colourings referenceColour offers, in the C++'s order: each shows its name and runs its
/// word (toString, toWord).
const REFERENCE_COLOURS: [katana_interop::PointColorMode; 5] = [
    katana_interop::PointColorMode::Elevation,
    katana_interop::PointColorMode::Intensity,
    katana_interop::PointColorMode::Classification,
    katana_interop::PointColorMode::SourceColor,
    katana_interop::PointColorMode::Flat,
];

/// How the selected layer is drawn (buildReferenceDock's display row and
/// showSelectedReferenceDisplay): referenceOpacity for a raster, referenceColour for a point
/// cloud, each enabled only for its kind and showing the selected layer's value. A change by a
/// person or a driver runs REFS OPACITY or REFS COLOR; the value read from the layer runs nothing,
/// as the C++'s refreshingReferences_ guard has it.
fn reference_display_ui(window: &mut MainWindow, ui: &mut Ui) {
    let selected = window.ui.panels.reference_selected;
    let raster_opacity = selected
        .and_then(|id| window.exec.reference.find_raster(id))
        .map(|raster| raster.opacity);
    let cloud_colour = selected
        .and_then(|id| window.exec.reference.find_point_cloud(id))
        .map(|cloud| cloud.color_mode);
    // The C++ keeps a choice's last index while no layer of its kind is selected.
    if let Some(opacity) = raster_opacity {
        let at = if opacity > 0.875 {
            0
        } else if opacity > 0.625 {
            1
        } else if opacity > 0.375 {
            2
        } else {
            3
        };
        window.ui.panels.reference_opacity = Choice::at(at);
    }
    if let Some(mode) = cloud_colour
        && let Some(at) = REFERENCE_COLOURS.iter().position(|each| *each == mode)
    {
        window.ui.panels.reference_colour = Choice::at(at);
    }
    let opacities: Vec<String> = REFERENCE_OPACITIES
        .iter()
        .map(|(shown, _)| String::from(*shown))
        .collect();
    let colours: Vec<String> = REFERENCE_COLOURS
        .iter()
        .map(|mode| String::from(mode.name()))
        .collect();
    let mut opacity_changed = false;
    let mut colour_changed = false;
    // A two-column form: the labels in the secondary text in a column as wide as the wider, the
    // choices the rest of the panel's width. Each choice is sized to its row, a point inside the
    // panel's padding for its frame's stroke: an egui Grid gave its second column the grid's
    // whole width, and the choices ran past the panel's right edge.
    let muted = |text: &str| RichText::new(text).color(palette::text_muted().to_egui());
    let font = egui::TextStyle::Body.resolve(ui.style());
    let label_width = ["Opacity", "Colour"]
        .iter()
        .map(|text| {
            ui.painter()
                .layout_no_wrap(String::from(*text), font.clone(), egui::Color32::PLACEHOLDER)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max)
        .ceil();
    let row = |ui: &mut Ui, label: &str, add: &mut dyn FnMut(&mut Ui)| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = crate::theme::style::GROUP_GAP;
            let drawn = ui.add(egui::Label::new(muted(label)).selectable(false));
            // The label column's width, whatever this row's label is.
            let short = label_width - drawn.rect.width();
            if short > 0.0 {
                ui.add_space(short);
            }
            ui.spacing_mut().combo_width = (ui.available_width() - 1.0).max(60.0);
            add(ui);
        });
    };
    row(ui, "Opacity", &mut |ui| {
        ui.add_enabled_ui(raster_opacity.is_some(), |ui| {
            opacity_changed = controls::choice(
                ui,
                "referenceOpacity",
                &opacities,
                &mut window.ui.panels.reference_opacity,
            )
            .on_hover_text("How much of the selected raster shows (REFS OPACITY)")
            .changed();
        });
    });
    row(ui, "Colour", &mut |ui| {
        ui.add_enabled_ui(cloud_colour.is_some(), |ui| {
            colour_changed = controls::choice(
                ui,
                "referenceColour",
                &colours,
                &mut window.ui.panels.reference_colour,
            )
            .on_hover_text("How the selected point cloud is coloured (REFS COLOR)")
            .changed();
        });
    });
    if opacity_changed
        && let Some(&(_, value)) = window
            .ui
            .panels
            .reference_opacity
            .index
            .and_then(|at| REFERENCE_OPACITIES.get(at))
    {
        let _ = run_reference_line(window, "OPACITY", value);
    }
    if colour_changed
        && let Some(&mode) = window
            .ui
            .panels
            .reference_colour
            .index
            .and_then(|at| REFERENCE_COLOURS.get(at))
    {
        let word = katana_interop::reference_data::to_word(mode);
        let _ = run_reference_line(window, "COLOR", word);
    }
}

/// MainWindow::runReferenceLine: REFS <action> <selected id> [words], through the one executor;
/// refused, and said, when nothing is selected.
fn run_reference_line(window: &mut MainWindow, action: &str, words: &str) -> crate::host::VerbOutcome {
    let Some(id) = window.ui.panels.reference_selected else {
        window.exec.log_message("Select a reference layer first.", true);
        return crate::host::VerbOutcome::new(false, "", "no reference layer is selected");
    };
    let line = if words.is_empty() {
        format!("REFS {action} {id}")
    } else {
        format!("REFS {action} {id} {words}")
    };
    window.exec.run_verb_line(&line)
}

/// The Reference Data panel's Zoom To: the selected layer framed in every plan view.
fn zoom_to_selected_reference(window: &mut MainWindow) {
    let Some(id) = window.ui.panels.reference_selected else {
        return;
    };
    let reference = &window.exec.reference;
    let bounds = reference
        .find_raster(id)
        .map(|raster| raster.world_bounds())
        .or_else(|| reference.find_point_cloud(id).map(|cloud| cloud.world_bounds()))
        .unwrap_or_default();
    if bounds.empty() {
        window
            .exec
            .log_message("That layer has no extent to zoom to.", true);
        return;
    }
    window.exec.workspace.zoom_to(&bounds);
}

// ---- the command line ---------------------------------------------------------------------------

/// The space above a text box's first line: egui's TextEdit margin, `Margin::symmetric(4, 2)`.
const LOG_MARGIN: f32 = 2.0;

/// The command line: its log, and the field a line is typed in - Enter runs it, Up and Down browse
/// the history, Esc cancels the running tool, and several lines pasted run at once.
fn command_line_ui(window: &mut MainWindow, ui: &mut Ui) {
    let mut log = window.exec.log.text();
    // The log over the field a line is typed in: the field a control high, the log the rest.
    let field = crate::theme::style::CONTROL_HEIGHT + ui.spacing().item_spacing.y;
    let room = (ui.available_height() - field).max(20.0);
    // The log a whole number of lines high, and what is left over above it: kept at its bottom,
    // a log of any other height showed part of a line, cut through, at its top. The log's text
    // starts LOG_MARGIN below its top (egui's text box margin), so a whole line shows at the top
    // when the view is that margin more than whole lines.
    let line = ui.text_style_height(&egui::TextStyle::Monospace);
    let lines = ((room - LOG_MARGIN) / line).floor().max(1.0);
    let height = (lines * line + LOG_MARGIN).min(room);
    ui.add_space(room - height);
    // A new line, or the log laid out again at another text size or height, shows its last line,
    // whole. stick_to_bottom alone kept the offset of the old layout when View > Text Size changed
    // it, and the log stood part way down its text with a half line cut at its top and its foot
    // (Extra Large, the second look audit's T8). Only then: every frame, it would stop a person
    // scrolling back through it.
    let laid_out = (window.exec.log.blocks().len(), line, height);
    let to_the_end = window.ui.panels.log_laid_out != laid_out;
    window.ui.panels.log_laid_out = laid_out;
    ui.allocate_ui(Vec2::new(ui.available_width(), height), |ui| {
        // The log is read, not typed in: drawn on the panel itself, with no well or frame of a
        // field, so the one field of the panel is the line below it.
        let visuals = ui.visuals_mut();
        visuals.text_edit_bg_color = Some(palette::panel().to_egui());
        visuals.extreme_bg_color = palette::panel().to_egui();
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.noninteractive,
        ] {
            widget.bg_stroke = egui::Stroke::NONE;
        }
        let shown = egui::ScrollArea::vertical()
            .id_salt("katanaCommandLog")
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .min_scrolled_height(height)
            .max_height(height)
            .show(ui, |ui| {
                // The log is as wide as its panel, as the C++'s QPlainTextEdit filled its dock:
                // egui's text box takes its width from text_edit_width, 280 points by default, and
                // the log wrapped there with the rest of the panel empty.
                ui.spacing_mut().text_edit_width = ui.available_width();
                command_log_box(ui, &mut log)
            });
        // At its end, the log starts at a whole logical line: a line wrapped over the log's top
        // showed its last words alone ("in."), and at Extra Large a pixel or two of the row above
        // the first whole one - its descenders - showed over it (the third look audit's J). What
        // is above the first row that starts a line is covered with the log's own ground. Only at
        // the end: scrolled back, a person reads a long line part by part.
        let (galley, galley_pos) = &shown.inner;
        let at_end = shown.state.offset.y + 1.0
            >= shown.content_size.y - shown.inner_rect.height();
        if at_end
            && let Some(top) = log_cover_bottom(galley, *galley_pos, shown.inner_rect.top())
        {
            let cover = Rect::from_min_max(
                shown.inner_rect.min,
                Pos2::new(shown.inner_rect.max.x, top),
            );
            ui.painter_at(shown.inner_rect)
                .rect_filled(cover, 0.0, palette::panel().to_egui());
        }
        if to_the_end {
            // The offset of its end as this pass laid it out, kept for the pass that is shown:
            // this one is laid out again (an offset given before the content is measured - "past
            // the end" - drew the content off the view in the pass that was shown).
            let mut state = shown.state;
            state.offset.y = (shown.content_size.y - shown.inner_rect.height()).max(0.0);
            state.store(ui.ctx(), shown.id);
            ui.ctx()
                .request_discard("the command log scrolled to its last line");
        }
    });
    let placeholder = window.ui.panels.placeholder.clone();
    let response = controls::line_with(
        ui,
        "commandInput",
        &mut window.ui.panels.command_text,
        LineOptions {
            placeholder: &placeholder,
            width: Some(ui.available_width()),
            ..LineOptions::default()
        },
    );
    let (enter, up, down, escape) = ui.input(|input| {
        (
            input.key_pressed(Key::Enter),
            input.key_pressed(Key::ArrowUp),
            input.key_pressed(Key::ArrowDown),
            input.key_pressed(Key::Escape),
        )
    });
    let focused = response.has_focus() || response.lost_focus();
    if response.lost_focus() && enter {
        let text = std::mem::take(&mut window.ui.panels.command_text);
        window.exec.run_command_line(&text);
        response.request_focus();
        return;
    }
    if !focused {
        return;
    }
    let history = window.exec.interpreter.history().to_vec();
    let size = history.len();
    if up && size > 0 {
        let cursor = window.exec.history_cursor.min(size).saturating_sub(1);
        window.exec.history_cursor = cursor;
        window.ui.panels.command_text.clone_from(&history[cursor]);
    } else if down && size > 0 {
        let cursor = (window.exec.history_cursor + 1).min(size);
        window.exec.history_cursor = cursor;
        window.ui.panels.command_text = history.get(cursor).cloned().unwrap_or_default();
    } else if escape {
        window.ui.panels.command_text.clear();
        window
            .exec
            .with_parts(|workspace, parts| workspace.cancel(parts));
    } else if window.ui.panels.command_text.contains('\n') {
        // Several lines pasted run at once, a line at a time.
        let text = std::mem::take(&mut window.ui.panels.command_text);
        window.exec.run_command_line(&text);
    }
}

/// A panel's table that scrolls across shows that it does: its scroll bar's handle in the
/// secondary text colour (egui's `foreground_color`) rather than the raised grey on the input's
/// well, which in a 240-wide panel read as no bar at all - the Vertices table's Height cut at the
/// panel's edge and its Bulge out of view with nothing to say so (the third look audit's J). For
/// the scroll areas inside `ui` only: the window's thin bars elsewhere are the theme's.
pub fn scroll_bar_seen(ui: &mut Ui) {
    ui.spacing_mut().scroll.foreground_color = true;
}

/// `text` with its first letter a capital, the rest as it is (drive::controls::sentence_start,
/// the one implementation, which a choice's shown values take too).
pub fn sentence_start(text: &str) -> String {
    controls::sentence_start(text)
}

/// The command log's box: what `controls::text_with` draws for a read-only, fixed-pitch text box
/// named commandLog - the same record, the same answers to a driver's acts - with each line that
/// reports an error ("! ...", command_log) in the error token. Every line was the text colour, and
/// a refusal read as one more reply (the third look audit's H). Answers the galley as laid out
/// and where it was drawn, for the log's top line (`first_whole_line_top`).
fn command_log_box(ui: &mut Ui, log: &mut String) -> (std::sync::Arc<egui::Galley>, Pos2) {
    use crate::drive::mailbox::{Act, no_button};
    use crate::drive::registry::{ControlKind, ControlRecord, record, settle, take};
    const NAME: &str = "commandLog";
    let taken = take(ui, NAME);
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if controls::accept_fill(ui, taken) {
                    let written = text.replace("\\n", "\n");
                    changed = *log != written;
                    *log = written;
                    settle(ui, taken, Ok(()));
                }
            }
            Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
            Act::ScrollTo => {}
        }
    }
    // What a person reads: each error's sentence, its code (InvalidState:, NotFound:) left in
    // the log's record - what --report, the drivers and the tests read - and in what a line hands
    // back, as the status bar shows it (command_log::without_error_code). The log printed the
    // enum's names, "ParseFailure: ..." (the fourth look audit).
    let mut shown = shown_log(log);
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = egui::text::LayoutJob::default();
        for line in text.as_str().split_inclusive('\n') {
            let colour = if line.starts_with("! ") {
                palette::error()
            } else {
                palette::text()
            };
            job.append(
                line,
                0.0,
                egui::TextFormat::simple(font.clone(), colour.to_egui()),
            );
        }
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let output = egui::TextEdit::multiline(&mut shown)
        .id_salt(NAME)
        .desired_rows(3)
        .desired_width(f32::INFINITY)
        .interactive(false)
        .code_editor()
        .layouter(&mut layouter)
        .show(ui);
    let mut response = output.response.response;
    if changed {
        response.mark_changed();
    }
    if let Some(taken) = &taken
        && matches!(taken.posted.act, Act::ScrollTo)
    {
        response.scroll_to_me(None);
        settle(ui, taken, Ok(()));
    }
    let mut kept = ControlRecord::new(NAME, ControlKind::Text);
    kept.enabled = ui.is_enabled();
    kept.visible = ui.is_visible();
    kept.value = log.clone();
    record(ui, &response, kept);
    (output.galley, output.galley_pos)
}

/// How far down from the log's top (`view_top`) its own ground is painted over what is drawn
/// there, the log's galley drawn at `at`: to the top of the INK of its first whole line
/// (`first_whole_line_top`), on a whole pixel above it - None when there is nothing to cover.
///
/// To the ink, not the row's top: a glyph's descender may reach below its row, and at Extra Large
/// the row above the first whole one - wholly above the view but for its descenders - showed a
/// pixel of them over the first line, whose row started within half a point of the top and so was
/// not covered at all (the fourth look audit, S\crops\main_text_xl_284_722.png). The space
/// between a row's top and its ink is the font's own, so covering it hides nothing of the line.
pub fn log_cover_bottom(galley: &egui::Galley, at: Pos2, view_top: f32) -> Option<f32> {
    let row = galley
        .rows
        .iter()
        .enumerate()
        .filter(|(index, _)| *index == 0 || galley.rows[index - 1].ends_with_newline)
        .map(|(_, row)| row)
        .find(|row| at.y + row.pos.y >= view_top - 0.5)?;
    let top = at.y + row.pos.y;
    let ink = row.visuals.mesh_bounds;
    // A row of no glyphs (a blank line) has no ink: its top is the line's.
    let ink_top = if ink.is_positive() {
        (top + ink.min.y).max(top)
    } else {
        top
    };
    let bottom = ink_top.floor();
    (bottom > view_top + 0.5).then_some(bottom)
}

/// The command log's text as the log panel shows it: each line in the words a person reads
/// (command_log::shown_log - an error as a sentence without its code, a warning record as the
/// sentence it carries), the log's own record kept as it was logged.
pub fn shown_log(log: &str) -> String {
    crate::command_log::shown_log(log)
}

/// The top on the screen of the first row of `galley`, drawn at `at`, that starts a logical line
/// and lies wholly below `view_top`; None when no row does (one line longer than the view).
pub fn first_whole_line_top(galley: &egui::Galley, at: Pos2, view_top: f32) -> Option<f32> {
    galley
        .rows
        .iter()
        .enumerate()
        .filter(|(index, _)| *index == 0 || galley.rows[index - 1].ends_with_newline)
        .map(|(_, row)| at.y + row.pos.y)
        .find(|top| *top >= view_top - 0.5)
}

// ---- a view's report -------------------------------------------------------------------------

/// main.cpp's reportWidget for a view of the window (PlanView<id>, RenderView<id>), painted afresh
/// first: what it drew, by the keys its record uses. None when `name` is no view.
pub fn report_view(window: &mut MainWindow, name: &str) -> Option<String> {
    // A toolbar's place, a torn-off menu's or every toolbar's (main_window::docking_ui).
    if let Some(report) = window.docking_report(name) {
        return Some(report);
    }
    let real = katana_core::text::format_exact_real;
    if let Some(id) = name.strip_prefix("PlanView").and_then(|id| id.parse::<u32>().ok()) {
        let id = id as katana_cad::view_set::ViewId;
        return window.exec.with_parts(|workspace, parts| {
            workspace.with_plan_view(id, |plan, state| {
                let _ = plan.paint(&*parts, state);
                let mut text = format!(
                    "drawn={} ghosts={} grips={}",
                    plan.last_drawn_entity_count(),
                    plan.last_ghost_count(),
                    plan.grip_count()
                );
                let stats = plan.last_paint_stats();
                // meshplan: the meshes drawn as their faces (docs/plan_view.md, "Meshes in plan").
                if stats.meshes_drawn > 0 {
                    text.push_str(&format!(
                        " meshes={} mesh_triangles={} mesh_covered={}",
                        stats.meshes_drawn,
                        stats.mesh_triangles_in_view,
                        stats.mesh_pixels_covered
                    ));
                }
                if stats.surfaces_drawn > 0 {
                    text.push_str(&format!(
                        " surfaces={} triangles={} edges={} tinted={} contours={} labels={} \
                         covered={} legends={}",
                        stats.surfaces_drawn,
                        stats.surface_triangles_in_view,
                        stats.surface_edges_stroked,
                        stats.surface_triangles_tinted,
                        stats.surface_contour_runs,
                        stats.surface_labels,
                        stats.surface_pixels_covered,
                        stats.surface_legends
                    ));
                }
                for problem in &stats.surface_problems {
                    text.push_str(&format!(" problem=\"{}\"", problem.text()));
                }
                text
            })
        });
    }
    if let Some(id) = name.strip_prefix("RenderView").and_then(|id| id.parse::<u32>().ok()) {
        let id = id as katana_cad::view_set::ViewId;
        let scene = Rc::clone(&window.exec.scene);
        let surfaces = scene.surfaces();
        let meshes = scene.meshes(window.exec.document.model());
        return window.exec.workspace.with_render_view(
            id,
            &window.exec.document,
            Scenery {
                surfaces: &surfaces,
                meshes: &meshes,
            },
            |view, scene, state| {
                let painted = view.painted_pixel_count(scene, state);
                let camera = &state.camera;
                let at = camera.target();
                let degrees = katana_math::numerics::RAD_TO_DEG;
                let mut text = format!(
                    "painted={painted} target={},{},{} distance={} azimuth={} elevation={} \
                     projection={}",
                    real(at.x),
                    real(at.y),
                    real(at.z),
                    real(camera.distance()),
                    real(camera.azimuth() * degrees),
                    real(camera.elevation() * degrees),
                    if camera.projection() == katana_render::camera::Projection::Perspective {
                        "perspective"
                    } else {
                        "orthographic"
                    }
                );
                let legends = view.drawn_legends();
                if !legends.is_empty() {
                    text.push_str(&format!(" legends={}", legends.len()));
                    for legend in legends {
                        text.push_str(&format!(" legend=\"{}\"", legend.caption));
                    }
                }
                text
            },
        );
    }
    None
}

/// A host for what only reads: the executor as a dialog sees it.
pub fn host_of(window: &mut MainWindow) -> &mut dyn Host {
    &mut window.exec
}

/// Draws nothing; keeps a rect's area allocated so the layout below it holds.
pub fn reserve(ui: &mut Ui, rect: Rect) {
    let _ = ui.allocate_rect(rect, Sense::hover());
}

#[cfg(test)]
mod tests {
    use egui::Pos2;

    use super::{is_timing, sentence_start};

    #[test]
    fn the_log_starts_at_the_first_row_below_its_top_that_begins_a_line() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        // A middle line long enough to wrap at 60 points, between two short ones.
        let galley = ctx.fonts_mut(|fonts| {
            fonts.layout(
                String::from("one
wrapped words wrapped words wrapped words
three"),
                egui::FontId::monospace(10.0),
                egui::Color32::WHITE,
                60.0,
            )
        });
        let rows = &galley.rows;
        assert!(rows.len() >= 4, "the premise: the middle line wraps ({})", rows.len());
        let at = Pos2::new(0.0, 100.0);
        let top = |index: usize| at.y + rows[index].pos.y;
        let last = rows.len() - 1;
        // The view's top on the first row: that row.
        assert_eq!(super::first_whole_line_top(&galley, at, top(0)), Some(top(0)));
        // Below the first row's top: not the wrapped line's second row, its first.
        assert_eq!(super::first_whole_line_top(&galley, at, top(0) + 1.0), Some(top(1)));
        // Inside the wrapped line: past all of it, to the last line.
        assert_eq!(super::first_whole_line_top(&galley, at, top(1) + 1.0), Some(top(last)));
        // Below every line's start: none.
        assert_eq!(super::first_whole_line_top(&galley, at, top(last) + 1.0), None);
    }

    #[test]
    fn the_status_bars_sentence_starts_with_a_capital_and_keeps_the_rest() {
        assert_eq!(
            sentence_start("nothing is selected; use SELECT first"),
            "Nothing is selected; use SELECT first"
        );
        assert_eq!(sentence_start("Line: Specify next point"), "Line: Specify next point");
        assert_eq!(sentence_start("élan"), "Élan");
        assert_eq!(sentence_start("3 selected"), "3 selected");
        assert_eq!(sentence_start(""), "");
    }

    // The readouts as the views write them (viewport_widget's paint, render_view_widget's frame,
    // section_view_widget's cut and pointer readouts).
    #[test]
    fn a_frames_timing_is_told_from_a_readout_a_person_reads() {
        assert!(is_timing("Plan  20 drawn  5.4 ms"));
        assert!(is_timing("Plan  20 drawn  5.4 ms (kept, 0.2 ms)"));
        assert!(is_timing("3D  1036 tri  4.2 ms"));
        assert!(is_timing("3D  1036 tri  4.2 ms (scene 12.0 ms)"));
        assert!(!is_timing("Section  length 120.00  3 crossings"));
        assert!(!is_timing("Section  length 120.00  3 crossings, surfaces changed since the cut"));
        assert!(!is_timing("Station 12.000   Elevation 3.250   (V x2)"));
        assert!(!is_timing(""));
    }
}
