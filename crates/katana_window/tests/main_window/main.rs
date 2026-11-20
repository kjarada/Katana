//! K's cases (step 7 wave 4): the twins of the C++ cases that build the main window, or the parts of
//! it only the main window composes - one module a C++ file, the suites as modules inside:
//!
//! * tests/qt_widgets/geo/test_geo_menus.cpp: the geoprocessing lanes' menu items
//!   (geo::menu_table);
//! * tests/qt_widgets/test_screen_fit_dialogs.cpp: the terrain and road dialogs keep their buttons
//!   in sight on a screen too small for them;
//! * tests/qt_widgets/terrain/test_surfaces_panel.cpp's two cases that install the Surfaces panel in
//!   the window: its dock reached from View > Panels and the Terrain menu, and brought to the front
//!   of its tab group.
//!
//! Not twinned here, with why (for the integrator's parity.toml or a later round):
//!
//! * DefinitionEditor.TheWindowIsNotClosedOverAFormThatIsNotSaved builds the customisation
//!   workbench over CU2's definition editor bench (its fixture library, its `fill` of
//!   definitionLength): it belongs beside CU2's twins in tests/codes, where that bench is; the
//!   workbench's confirm_close it tests is WS's, unchanged here.
//! * GeneralTexts.NoTextTheStyleManagerShowsNamesAnotherProgram,
//!   NoTextTheSymbolLibraryOrItsFileDialogsShowNamesAnotherProgram and
//!   NoTextTheSurveyCodeManagerOrItsFileDialogsShowNamesAnotherProgram peek into the system file
//!   dialogs the managers open (FileDialogPeek): the Rust window's file dialogs are the system's
//!   own (rfd), which no test may open - proposed exempt for that part, the dialogs' own texts
//!   being held by the gate's -DFORBID=12[dD] test.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

mod command_line;
mod geo_menus;
mod imported_meshes;
mod menu_texts;
mod meshes_in_plan;
mod panel_floating;
mod screen_fit_dialogs;
mod status_bar_rules;
mod surfaces_panel;
mod tool_bar_columns;
mod tool_bar_docking;
