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
use super::layout::{self, Geometry, bar_breadth, natural_width, row_rooms, widget_size};
use super::window::{ImportOptions, MainWindow};
use crate::dock_chrome::{ChromeIcons, ChromeRequest, DockChrome, DockRole, DockTab};
use crate::drive::action::{Menu, MenuItem, ToolBar, ToolBarItem, action_item};
use crate::drive::controls::{self, Choice, LineOptions, TextOptions};
use crate::drive::registry::{TargetKind, TargetSpec, WINDOW, begin_target, end_target};
use crate::host::Host;
use crate::import_placement::DialogResult;
use crate::shell::{DialogFrame, Workbench};
use crate::tools::flyout_button::{Strip, tool_bar_strip};
use crate::view_workspace::Scenery;

// ---- the panels' state ----------------------------------------------------------------------

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
    /// The status bar's message: a running tool's prompt, a view's status, an error.
    pub status: String,
    pub coordinates: String,
    pub snap: String,
    pub frame_stats: String,
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
            ("SurveyPointsDock", "SurveyPoints", "Survey Points"),
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
            status: String::new(),
            coordinates: String::from("0.0000, 0.0000"),
            snap: String::new(),
            frame_stats: String::new(),
        }
    }
}

/// The icons the chrome draws: the window's set, painted by the canvas.
pub fn chrome_icons() -> ChromeIcons {
    ChromeIcons::new(|name: &str, pixels: u32| {
        crate::icons::icon_image(
            &crate::icons::CanvasRasteriser,
            name,
            f64::from(pixels),
            1.0,
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
        let geometry = layout::geometry(screen, &self.ui.docks, right_extra);
        self.menu_bar_ui(ui, &geometry, &composed);
        self.tool_bars_ui(ui, &geometry, &composed);
        self.panels_ui(ui, &geometry);
        self.views_ui(ui, &geometry);
        self.status_bar_ui(ui, &geometry);
        self.step_menu_ui(ui, &composed);
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

    /// The menu bar: each menu a button that opens its items, as a person clicks it.
    fn menu_bar_ui(&mut self, ui: &mut Ui, geometry: &Geometry, composed: &Composed) {
        let triggered = &mut self.ui.triggered;
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(geometry.menu_bar)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    for menu in &composed.menus {
                        menu_ui(ui, menu, triggered);
                    }
                });
            },
        );
    }

    /// The toolbars along the top in their rows, Draw and Annotate down the left, Modify down the
    /// right, the Drafting bar along the bottom.
    fn tool_bars_ui(&mut self, ui: &mut Ui, geometry: &Geometry, composed: &Composed) {
        let icon_size = self.ui.bars.icon_size;
        let names_shown = self.ui.bars.names_shown;
        let mut rows: [Vec<&ToolBar>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let mut left: Vec<&ToolBar> = Vec::new();
        let mut right: Vec<&ToolBar> = Vec::new();
        let mut bottom: Vec<&ToolBar> = Vec::new();
        for (bar, place) in &composed.bars {
            if !self.ui.bars.shown(&bar.name) {
                continue;
            }
            match place {
                BarPlace::Top(row) => rows[(*row).min(2)].push(bar),
                BarPlace::Left => left.push(bar),
                BarPlace::Right => right.push(bar),
                BarPlace::Bottom => bottom.push(bar),
                BarPlace::Nowhere => {}
            }
        }
        for (at, row) in rows.iter().enumerate() {
            let rect = geometry.rows[at];
            let strips: Vec<Strip> = row
                .iter()
                .map(|bar| {
                    let mut bar = (*bar).clone();
                    if names_shown && bar.name != "PropertiesToolBar" {
                        bar.items
                            .insert(0, ToolBarItem::Widget(format!("toolBarName:{}", bar.title)));
                    }
                    self.strip_of(&bar, false, icon_size)
                })
                .collect();
            let naturals: Vec<i32> = strips
                .iter()
                .map(|strip| natural_width(&strip.items, icon_size))
                .collect();
            let rooms = row_rooms(&naturals, rect.width());
            let mut x = rect.min.x;
            for (strip, room) in strips.iter().zip(rooms) {
                if room <= 0.0 {
                    continue;
                }
                let at = Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(room, rect.height()));
                self.strip_ui(ui, strip, at, room);
                x += room;
            }
        }
        for (column, rect) in [(left, geometry.left_bars), (right, geometry.right_bars)] {
            let mut y = rect.min.y;
            for bar in column {
                let strip = self.strip_of(bar, true, icon_size);
                let natural = natural_width(&strip.items, icon_size) as f32;
                let room = natural.min(rect.max.y - y).max(0.0);
                if room <= 0.0 {
                    continue;
                }
                let at = Rect::from_min_size(
                    Pos2::new(rect.min.x, y),
                    Vec2::new(bar_breadth(icon_size), room),
                );
                self.strip_ui(ui, &strip, at, room);
                y += room;
            }
        }
        let mut x = geometry.bottom_bar.min.x;
        for bar in bottom {
            let strip = self.strip_of(bar, false, icon_size);
            let room = (natural_width(&strip.items, icon_size) as f32)
                .min(geometry.bottom_bar.max.x - x)
                .max(0.0);
            let at = Rect::from_min_size(
                Pos2::new(x, geometry.bottom_bar.min.y),
                Vec2::new(room, geometry.bottom_bar.height()),
            );
            self.strip_ui(ui, &strip, at, room);
            x += room;
        }
    }

    /// A toolbar's strip as it is drawn this frame.
    fn strip_of(&self, bar: &ToolBar, vertical: bool, icon_size: i32) -> Strip {
        let items = self.ui.tools.strip_items(bar, &|name| {
            if let Some(title) = name.strip_prefix("toolBarName:") {
                let (_, height) = crate::tools::flyout_button::ToolButtonLook::TOOL
                    .button_size(icon_size);
                return Some((layout::name_width(title), height));
            }
            widget_size(name, icon_size)
        });
        Strip {
            name: bar.name.clone(),
            title: bar.title.clone(),
            vertical,
            icon_size,
            items,
        }
    }

    /// One toolbar drawn at `at`, its clicks kept to be triggered after the frame.
    fn strip_ui(&mut self, ui: &mut Ui, strip: &Strip, at: Rect, room: f32) {
        let exec = &mut self.exec;
        let ui_state = &mut self.ui;
        let mut widget = |ui: &mut Ui, rect: Rect, name: &str| -> bool {
            let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
            if let Some(title) = name.strip_prefix("toolBarName:") {
                let _ = controls::label_as(
                    &mut child,
                    "toolBarName",
                    title,
                    egui::Label::new(
                        RichText::new(title).color(crate::theme::palette::text_muted().to_egui()),
                    ),
                );
                return false;
            }
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
                    let _ = ui_state.annotation.scale_box_ui(&mut child, exec);
                    false
                }
                "surfaceEditActive" => {
                    ui_state.surface_edit.active_ui(&mut child, exec);
                    false
                }
                _ => false,
            }
        };
        let response = ui
            .scope_builder(UiBuilder::new().max_rect(at), |ui| {
                tool_bar_strip(ui, strip, room, &mut widget)
            })
            .inner;
        for name in response.triggered {
            match name.as_str() {
                "editUndoButton" => self.ui.triggered.push(String::from("editUndo")),
                "editRedoButton" => self.ui.triggered.push(String::from("editRedo")),
                _ => self.ui.triggered.push(name),
            }
        }
        if let Some(tip) = response.status_tip {
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
            let mut body = rect;
            if tabs.len() > 1 {
                // A tab strip over the group, its tabs the docks' titles.
                let strip = Rect::from_min_size(rect.min, Vec2::new(rect.width(), crate::view_workspace::TAB_STRIP));
                body.min.y = strip.max.y;
                let mut chosen = None;
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(strip)
                        .layout(Layout::left_to_right(Align::Center)),
                    |ui| {
                        for tab in &tabs {
                            let title = if *tab == "SurfacesDock" {
                                "Surfaces"
                            } else {
                                "Reference Data"
                            };
                            if ui.selectable_label(*tab == front, title).clicked() {
                                chosen = Some(*tab);
                            }
                        }
                    },
                );
                if let Some(tab) = chosen {
                    self.ui.docks.surfaces_in_front = tab == "SurfacesDock";
                }
            }
            if front == "SurfacesDock" {
                self.dock_at(ui, body, "SurfacesDock", |window, ui| {
                    window.ui.surfaces.ui(ui, &mut window.exec);
                });
            } else {
                self.dock_at(ui, body, "ReferenceDataDock", |window, ui| {
                    reference_ui(window, ui);
                });
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
    fn dock_at(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        name: &str,
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
                chrome.dock_ui(ui, name, |ui| content(self, ui));
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
        let message = self
            .exec
            .log
            .status_message(now)
            .map(String::from)
            .unwrap_or_else(|| self.ui.panels.status.clone());
        let document = &self.exec.document;
        let crs = format!("CRS: {}", crate::project_crs_dialog::project_crs_label(document));
        let selection = format!(
            "{} selected / {} entities",
            document.selection().size(),
            document.model().entities.size()
        );
        let layer = format!("Layer: {}", document.current_layer());
        let mut crs_clicked = false;
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(geometry.status_bar)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.label(message.as_str());
                self.exec.jobs.status_ui(ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let _ = controls::label(ui, "statusCoordinates", &self.ui.panels.coordinates);
                    let _ = controls::label(ui, "statusSnap", &self.ui.panels.snap);
                    let _ = controls::label(ui, "statusLayer", &layer);
                    let _ = controls::label(ui, "statusSelectionCount", &selection)
                        .on_hover_text("Entities selected, of all the drawing holds");
                    let _ = controls::label(ui, "FrameStatsLabel", &self.ui.panels.frame_stats);
                    crs_clicked = controls::button_as(
                        ui,
                        "statusProjectCrs",
                        &crs,
                        egui::Button::new(crs.as_str()).frame(false),
                    )
                    .on_hover_text("The project's coordinate system: click to change it")
                    .clicked();
                    self.ui.update.status_button_ui(ui, &mut self.exec);
                });
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
        let area = egui::Area::new(egui::Id::new(("katanaStepMenu", name.as_str())))
            .order(egui::Order::Foreground)
            .fixed_pos(ui.max_rect().min)
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    begin_target(ui, TargetSpec::new(&menu.name, TargetKind::Menu, &menu.title.replace('&', "")));
                    let shown = ui.vertical(|ui| menu_items_ui(ui, &menu, &mut chosen));
                    end_target(ui, shown.response.rect);
                });
            });
        let _ = area;
        if let Some(chosen) = chosen {
            self.ui.open_menu = None;
            self.ui.triggered.push(chosen);
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
        if let Some(editor) = dialogs.sheets.as_mut() {
            editor.ui(ui, exec);
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
                    self.ui.panels.status.clone_from(&prompt);
                    // A running tool's prompt shows where its answer is typed.
                    if !prompt.is_empty() && !self.exec.workspace.active_tool_id().is_empty() {
                        self.ui.panels.placeholder = prompt;
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

/// A menu drawn as a menu button with its items; a click on an action is the action chosen.
fn menu_ui(ui: &mut Ui, menu: &Menu, chosen: &mut Vec<String>) {
    ui.menu_button(menu.title.replace('&', ""), |ui| {
        let mut one = None;
        menu_items_ui(ui, menu, &mut one);
        if let Some(name) = one {
            chosen.push(name);
            ui.close();
        }
    });
}

/// A menu's items: actions, titled sections, separators and submenus.
fn menu_items_ui(ui: &mut Ui, menu: &Menu, chosen: &mut Option<String>) {
    for item in &menu.items {
        match item {
            MenuItem::Action(action) => {
                if action_item(ui, action).clicked() && !action.name.is_empty() {
                    *chosen = Some(action.name.clone());
                }
            }
            MenuItem::Section(title) => {
                let _ = crate::theme::menu_section(ui, title);
            }
            MenuItem::Separator => {
                ui.separator();
            }
            MenuItem::Menu(submenu) => {
                ui.menu_button(submenu.title.replace('&', ""), |ui| {
                    menu_items_ui(ui, submenu, chosen);
                });
            }
        }
    }
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
        ui.label(" Style ");
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
        ui.label("Style");
        ui.add_enabled_ui(any, |ui| {
            if !ui.memory(|memory| memory.has_focus(ui.id())) && styles.len() == 1 {
                let style = styles.iter().next().cloned().unwrap_or_default();
                if panels.property_style.text.is_empty() {
                    panels.property_style.text = if style.is_empty() {
                        String::from("ByLayer")
                    } else {
                        style
                    };
                }
            }
            let response = controls::choice(ui, "PropertyStyle", &items, &mut panels.property_style);
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
        for (name, label, tip) in [
            ("LayerNewButton", "New Layer", "Add a layer at the top of the tree."),
            ("LayerNewChildButton", "New Child Layer", "Add a layer beneath the selected one."),
            ("LayerRenameButton", "Rename Layer", "Rename the selected layer and everything beneath it."),
            ("LayerDeleteButton", "Delete Layer", "Delete the selected layer."),
        ] {
            if controls::button(ui, name, label).on_hover_text(tip).clicked() {
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
        .show(ui, |ui| {
            for path in layers.names() {
                let Some(layer) = layers.find(&path) else {
                    continue;
                };
                let depth = path.matches('/').count();
                let leaf = path.rsplit('/').next().unwrap_or(&path).to_owned();
                ui.horizontal(|ui| {
                    ui.add_space(12.0 * depth as f32);
                    let mut visible = layer.visible;
                    if ui.checkbox(&mut visible, "").changed() {
                        toggled = Some((path.clone(), true, visible));
                    }
                    let mut locked = layer.locked;
                    if ui.checkbox(&mut locked, "").changed() {
                        toggled = Some((path.clone(), false, locked));
                    }
                    let current = path == document.current_layer();
                    let shown = if current { format!("{leaf}  <") } else { leaf };
                    let text = if layers.effectively_visible(&path) {
                        RichText::new(shown)
                    } else {
                        RichText::new(shown).color(egui::Color32::from_rgb(130, 130, 130))
                    };
                    let picked = window.ui.panels.layer_selected.as_deref() == Some(path.as_str());
                    let response = ui.selectable_label(picked, text).on_hover_text(path.as_str());
                    if response.clicked() {
                        window.ui.panels.layer_selected = Some(path.clone());
                    }
                    if response.double_clicked() {
                        made_current = Some(path.clone());
                    }
                    ui.label(format!("{}", document.model().entities.count_on_layer(&path)));
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
        for (name, label, tip) in [
            ("ReferenceImportButton", "Import", "Import a drawing, an image or a point cloud, as File > Import does."),
            ("ReferenceZoomButton", "Zoom To", "Frame the selected reference layer in every plan view."),
            ("referenceShow", "Show", "Show the selected reference layer (REFS SHOW)."),
            ("referenceHide", "Hide", "Hide the selected reference layer (REFS HIDE)."),
            ("referenceInfo", "Information", "The selected layer's source and display (REFS INFO), and what its file holds."),
            ("referenceOverviews", "Build Overviews", "Build overviews for the selected raster, written beside its file as .ovr (REFS OVERVIEWS ... CONFIRM)."),
            ("referenceRemove", "Remove", "Remove the selected reference layer (REFS REMOVE). The file is not touched."),
        ] {
            if controls::button(ui, name, label).on_hover_text(tip).clicked() {
                asked = Some(name);
            }
        }
    });
    let reference = &window.exec.reference;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut ids: Vec<u64> = Vec::new();
    for raster in reference.rasters() {
        let mut detail = format!("{} x {} px", raster.width, raster.height);
        if !raster.has_geotransform {
            detail.push_str("  (not georeferenced)");
        }
        rows.push(vec![
            raster.name.clone(),
            String::from("Raster"),
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
            String::from("Point cloud"),
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
    let response = controls::rows(ui, "referenceList", &rows, &mut current);
    if response.changed() {
        window.ui.panels.reference_selected = current.and_then(|at| ids.get(at).copied());
    }
    if response.double_clicked() {
        asked = Some("ReferenceZoomButton");
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
    ui.horizontal(|ui| {
        ui.label("Opacity");
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
        ui.label("Colour");
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

/// The command line: its log, and the field a line is typed in - Enter runs it, Up and Down browse
/// the history, Esc cancels the running tool, and several lines pasted run at once.
fn command_line_ui(window: &mut MainWindow, ui: &mut Ui) {
    let mut log = window.exec.log.text();
    let height = (ui.available_height() - 30.0).max(20.0);
    ui.allocate_ui(Vec2::new(ui.available_width(), height), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("katanaCommandLog")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let _ = controls::text_with(
                    ui,
                    "commandLog",
                    &mut log,
                    TextOptions {
                        read_only: true,
                        rows: 6,
                        monospace: true,
                    },
                );
            });
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

// ---- a view's report -------------------------------------------------------------------------

/// main.cpp's reportWidget for a view of the window (PlanView<id>, RenderView<id>), painted afresh
/// first: what it drew, by the keys its record uses. None when `name` is no view.
pub fn report_view(window: &mut MainWindow, name: &str) -> Option<String> {
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
