//! The `qt` layer: the desktop window, in egui and wgpu.
//!
//! The port of src/katana_qt (docs/rust.md, "Step 7"). One module tree, declared here: a folder of
//! the C++ is an inline block whose files are in `src/<folder>/`, with no `mod.rs`, and a module
//! follows the C++ file (`geo/buffer_dialog.cpp` is `katana_window::geo::buffer_dialog`,
//! `view_workspace.cpp` is `katana_window::view_workspace`). The modules with no C++ file are the
//! foundation's: `drive` (the named controls, their registry and mailbox, the action rows, the
//! harness, the program's view of the window), `canvas` (the one painter's surface), `host` (what
//! a dialog sees of the window) and `shell` (the dialog frame and the workbenches).
//!
//! Each step-7 agent adds or uncomments lines only in its own commented block below; the
//! integrator union-merges this file. Root re-exports are explicit names, in the agent's block.
//!
//! Nothing here opens a window: only the program `katana` (src/bin/katana/main.rs) names eframe,
//! and the foundation's tests fail when another file does.

// ---- the foundation (F0) ------------------------------------------------------------------------

/// The one painter's surface (the subset of QPainter the painters call), the image it paints into,
/// and PNG. C adds the raster and PDF backends here.
pub mod canvas {
    // F0
    pub mod image;
    pub mod painter;
    pub mod png_file;

    // `self::`: the image crate is a dependency of this one too, and a bare `image` would name
    // both.
    pub use self::image::{Image, ImageFormat, premultiply, unpremultiply};
    pub use painter::{
        Brush, BrushStyle, Canvas, Color, Elide, FillRule, Font, FontMetrics, FontSize, FontWeight,
        GradientStop, HAlign, IntRect, LinearGradient, Path, PathElement, Pen, PenCap, PenJoin,
        PenStyle, PointF, RectF, RenderHint, SizeF, TextFlags, Transform, VAlign,
    };
    pub use png_file::{decode_png, encode_png, read_png, write_png};

    // C: the raster canvas (tiny-skia, cosmic-text), the PDF canvas (krilla), image reading, and
    // the text both canvases draw (fonts resolved, lines shaped and laid out, the metrics).
    pub(crate) mod coverage;
    pub mod image_files;
    pub mod pdf;
    pub mod raster;
    pub mod text;

    pub use image_files::{decode_image, read_image};
    pub use pdf::{PdfCanvas, PdfDocument};
    pub use raster::RasterCanvas;
    pub use text::{SCREEN_DPI, TextMetrics, font_pixels, resolved_family, text_path};
}

/// What a dialog, a panel or a workbench sees of the window: the one executor (command_runner),
/// the drawing, the jobs, the views, the clipboard and the person's settings.
pub mod host {
    // F0
    pub mod clipboard;
    pub mod command_runner;
    pub mod jobs;
    pub mod recording;
    pub mod user_settings;
    pub mod view_access;

    pub use clipboard::{Clipboard, DesktopClipboard, MemoryClipboard};
    pub use command_runner::{GeoPhase, Host, JobAwait, VerbOutcome, geo_phase, with_geo};
    pub use jobs::{
        CompletedJob, JobApply, JobControl, JobFinished, JobId, JobOutcome, JobProgress, JobReport,
        JobShared, JobWork, Jobs, NO_JOB, complete, job_apply, job_finished, job_work, run_work,
    };
    pub use recording::{Answer, HostParts, InlineJobs, RecordingHost};
    pub use user_settings::{SettingValue, UserSettings};
    pub use view_access::{PickId, ViewAccess};

    // H: the job runner itself is the top-level `jobs` (jobs.cpp), over host::jobs.
}

/// The named controls (every control carries its C++ object name), their per-frame registry, the
/// mailbox a driver posts acts to, the action rows menus and toolbars are drawn from, the test
/// harness, and the program's view of the window (main.cpp's calls).
pub mod drive {
    // F0
    pub mod action;
    pub mod controls;
    pub mod harness;
    pub mod mailbox;
    pub mod program;
    pub mod registry;

    pub use action::{Action, Menu, MenuItem, ToolBar, ToolBarItem, action_item, shortcut_text};
    pub use controls::{
        Choice, LineOptions, SpinRange, TextOptions, button, button_as, check, choice, custom,
        label, label_as, line, line_with, radio, rows, spin, tabs, text, text_with, toggle, whole,
    };
    pub use harness::{HEADLESS_HEIGHT, HEADLESS_WIDTH, Harness, WorkbenchHost};
    pub use mailbox::{Act, ActId, ActOutcome, Posted, Taken};
    pub use program::{DrivenWindow, MenuSigns, SheetsPlot};
    pub use registry::{
        ControlKind, ControlRecord, Drive, Part, ScrollRecord, Snapshot, TargetKind, TargetRecord,
        TargetSpec, WINDOW, begin_part, begin_target, end_part, end_target, install, live, outcome,
        post, record, set_target_frame, settle, simplified, snapshot, take, waiting,
    };

    // D: main.cpp's port over DrivenWindow, the step engine, the headless and desktop runners and
    // the soft renderer.
    pub mod desktop;
    pub mod headless;
    pub mod run;
    pub mod soft_renderer;
    pub mod steps;
}

/// The window's frame for what it shows: the dialog frame (screen_fit) and the workbenches.
pub mod shell {
    // F0
    pub mod dialog;
    pub mod workbench;

    pub use dialog::{
        DialogFrame, DialogShown, FOOTER_SHARE, LEAST_SCROLLING, MARGIN, MenuShown,
        SCREEN_FIT_FOOTER, SCREEN_FIT_SCROLL, frame_of, menu_column, popup_menu, room_on_screen,
    };
    pub use workbench::{DockSpec, Workbench};
}

/// The theme's tokens as data (theme.hpp); C applies them to egui's style (theme.cpp).
pub mod theme {
    // F0
    pub mod palette;

    pub use palette::{
        MONOSPACE_FAMILIES, TEXT_SIZES, TOOL_RUNS_BEHIND_OVERFLOW, TextSize, accent, accent_text,
        border, error, hover, monospace_points, overlay_pixels, panel, points_of_pixels, raised,
        text, text_disabled, text_muted, text_size_from, text_size_steps, to_string, ui_points,
        viewport, window,
    };

    // C: theme.cpp, applied to egui's style; the text sizes read back from their names; the
    // command line's fixed pitch.
    pub mod style;

    pub use style::{
        apply, menu_icon_cell, menu_section, menu_separator, monospace_font, monospace_font_id,
        overflow_arrow, overlay_font, paint_overflow_arrow, set_text_size, text_size, ui_font,
        ui_font_id, visuals,
    };
}

// F0: the small top-level units every dialog uses.
pub mod command_word;
pub mod format;
pub mod project_folder;
pub mod view_focus;

// ---- the folders of the C++ ---------------------------------------------------------------------

pub mod annotation {
    // F0
    pub mod form_widgets;

    // P
    pub mod annotation_painter;
    // AN
    pub mod annotation_managers;
    pub(crate) mod dialog_support;
    pub mod label_edit_dialog;
    pub mod label_layout_report;
    pub mod leader_manager;
    pub mod text_edit_dialog;

    pub use annotation_managers::{LabelStyleManagerDialog, TextStyleManagerDialog};
    pub use label_edit_dialog::{LabelEditDialog, LabelEditForm};
    pub use label_layout_report::{LabelLayoutReportDialog, LabelLayoutSummary};
    pub use leader_manager::{LeaderManagerContext, LeaderManagerDialog};
    pub use text_edit_dialog::TextEditDialog;
    // V3: pub mod annotation_workbench;
    //     pub mod dimension_style_manager;
}

pub mod bim {
    // I
    pub mod road_object_icons;

    // BM
    pub mod corridor_dialog;
    pub mod corridor_quantities_dialog;
    pub mod road_dialog_support;
    pub mod road_object_dialog;
    pub mod road_object_manager;
    pub mod road_object_workbench;
    pub mod road_template_manager;
    // V3: pub mod road_workbench;
}

pub mod customisation {
    // F0
    pub mod document_watcher;
    pub mod filter_bar;
    pub mod row_table_model;
    pub mod scope_filter_widget;

    pub use document_watcher::{DocumentChanges, DocumentWatcher};
    pub use filter_bar::{FilterBar, FilterBarChanges};
    pub use row_table_model::{Column, ItemFlags, Role, RowTableModel, Value};
    pub use scope_filter_widget::{
        FilterField, ScopeChoice, ScopeFilterView, ScopeFilterWidget, scope_filter_views,
    };

    // H
    pub mod code_manager_support;
    // P:   pub mod customisation_context;
    // P
    pub mod definition_thumbnails;
    pub mod style_painter;
    // P (the previews, left for CU1): pub mod customisation_context;
    //      pub mod name_picker;
    //      pub mod style_preview;
    // CU1: pub mod drawing_summary_dialog;
    //      pub mod global_modify_dialog;
    //      pub mod hatch_patterns_tab;
    // CU2: pub mod code_manager;
    //      pub mod code_manager_form;
    //      pub mod code_manager_tabs;
    //      pub mod definition_editor;
    //      pub mod symbol_library;
    //      pub mod symbol_library_model;
    // UP:  pub mod linework_labels;
    //      pub mod settings_dialog;
    // WS:  pub mod customisation_workbench;
}

pub mod drawing {
    // P
    pub mod feedback_painter;
    // VP: pub mod drawing_ui;
    //     pub mod grip_controller;
    //     pub mod vertex_panel;
}

pub mod geo {
    // H
    pub mod geo_dialog_support;
    pub mod geo_workbench;
    pub mod point_pick;
    pub mod terrain_dialog_support;

    pub use geo_workbench::{GeoEffect, GeoEffects, GeoWorkbench, started_job, started_record};
    // GA: pub mod buffer_dialog;
    //     pub mod clip_dialog;
    //     pub mod coverage_dialog;
    //     pub mod dissolve_dialog;
    //     pub mod formats_dialog;
    //     pub mod geometry_check_dialog;
    //     pub mod gis_tool_dialog;
    //     pub mod hull_dialog;
    //     pub mod overlay_dialog;
    //     pub mod sql_dialog;
    // GB
    pub mod argument_form;
    pub mod binding_picker;
    pub mod contours_dialog;
    pub mod dem_tools_dialog;
    pub mod drape_dialog;
    pub mod gdal_toolbox_dialog;
    pub mod grid_dem_dialog;
    pub mod pipeline_builder;
    pub mod slope_analysis_dialog;
    pub(crate) mod terrain_dialog_parts;
    pub mod terrain_shading_dialog;
    pub mod viewshed_dialog;
    pub mod zonal_stats_dialog;
    // K:  pub mod menu_table;
}

pub mod plotting {
    // C:
    pub mod tiff_writer;
    pub mod viewport_clipboard;
    // P
    pub mod plot_style;
    pub mod section_painter;
    // SH1: pub mod grid_properties;
    //      pub mod legend_painter;
    //      pub mod legend_properties;
    //      pub mod plot_drawing_dialog;
    //      pub mod plot_output;
    //      pub mod sheet_list_widget;
    //      pub mod sheet_readout;
    //      pub mod sheet_rulers;
    //      pub mod sheet_tables;
    //      pub mod sheet_thumbnails;
    //      pub mod view_image_export;
    // SH2: pub mod plot_dialog;
    //      pub mod sheet_arrange;
    //      pub mod sheet_canvas_editing;
    //      pub mod sheet_checks;
    //      pub mod sheet_editor_editing;
    //      pub mod sheet_set_menu;
}

pub mod survey {
    // UT
    pub mod utility_dialog;
    pub mod utility_workbench;
    // SV
    pub mod reduction_options_widget;
    pub mod reduction_report_view;
    pub mod survey_dialogs;
    pub mod survey_import_wizard;
    pub mod survey_job_support;
    pub mod survey_jobs_dialog;
    pub mod survey_parcel_dialog;
    pub mod survey_points_ui;
    pub mod survey_task;
    pub mod survey_templates;
    pub mod survey_workbench;

    pub use survey_workbench::{SurveyServices, SurveyWorkbench};
}

pub mod terrain {
    // P
    pub mod surface_plan_painter;
    // TR
    pub mod surface_edit_dialogs;
    pub mod surface_edit_ui;
    pub mod surfaces_panel;
    pub mod surface_style_manager;
    // TR: pub mod volumes_dialog;
    // K:  pub mod main_window_surface_display;
}

pub mod tools {
    // I
    pub mod flyout_button;
    pub mod icons_annotate;
    pub mod icons_draw_curves;
    pub mod icons_draw_lines;
    pub mod icons_drawing;
    pub mod icons_everyday;
    pub mod icons_inquiry;
    pub mod icons_modify_edit;
    pub mod icons_modify_transform;
    pub mod icons_terrain;
    pub mod tool_icons;
    pub mod tool_menus;

    pub use flyout_button::{
        ButtonIcon, ButtonPaint, ButtonState, DrawnParts, Flyout, MeasuredButton, MenuSign,
        SignHolder, Strip, StripItem, ToolButtonLook, drawn_parts, flyout_button_at,
        menu_sign_clashes, overflow_paint, paint_overflow_arrow, paint_tool_button,
        split_button_at, split_paint, strip_sign_holder, tool_bar_strip, tool_button_at,
    };
    pub use tool_icons::{ToolInk, has_tool_icon, paint_tool_icon};
    pub use tool_menus::{
        BarMenu, ToolActions, ToolFamily, ToolMenuTargets, fill_tool_menus, tool_id_for_command,
        with_mnemonic,
    };

    // VP: pub mod tool_host;
}

// ---- the top-level units of the C++, by agent ---------------------------------------------------

// D
pub mod dock_chrome;
// H
pub mod attribute_manager;
pub mod command_log;
pub mod command_reference_dialog;
pub mod import_placement;
pub mod jobs;
pub mod keyboard_shortcuts_dialog;
pub mod property_panel;
pub mod script_runner;
pub mod select_by_id_dialog;
// P:   pub mod kept_name_combo;
// P
pub mod plan_painter;
// P (the previews, left for CU1): pub mod kept_name_combo;
// I
pub mod icons;
pub mod make_icons;
pub mod tool_icon_sheet;
// BM
pub mod alignment_manager;
// UT
pub mod ifc_dialogs;
pub mod project_crs_dialog;
pub mod view_scale_dialog;
pub mod zoom_to_dialog;
// VP:  pub mod plan_context_menu;
//      pub mod viewport_widget;
// V3:  pub mod gpu_scene_view;
//      pub mod render_view_widget;
//      pub mod section_view_widget;
// SH1: pub mod sheet_painter;
// CU1: pub mod layer_manager;
//      pub mod style_manager;
// GA:  pub mod dataset_info_dialog;
//      pub mod gis_dialog_support;
//      pub mod gis_export_dialog;
//      pub mod gis_import_dialogs;
//      pub mod gis_online;
//      pub mod gis_online_dialog;
// GB
pub mod surface_raster_dialog;
// UP:  pub mod update_dialog;
//      pub mod update_workbench;
// WS:  pub mod start_dialog;
//      pub mod view_layers_popup;
//      pub mod view_workspace;
// SH2: pub mod sheet_editor;
// K:   pub mod main_window;
//      pub mod main_window_ifc;

// ---- root re-exports, by agent ------------------------------------------------------------------

// F0
pub use canvas::{Canvas, Color, Image};
// P
pub use drive::{Action, DrivenWindow, Harness};
pub use host::{Host, RecordingHost, VerbOutcome, ViewAccess};
pub use plan_painter::{PlanFrame, PlanPaintCache, PlanPaintOptions, PlanSource, paint_plan};
pub use shell::{DialogFrame, Workbench};
// H
pub use command_log::CommandLog;
pub use jobs::JobRunner;

// I
pub use icons::{Icon, IconMode, Rasteriser};
