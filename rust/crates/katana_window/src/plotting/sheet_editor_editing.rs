//! The sheet editor's editing of several viewports at once, and the furniture that goes with it
//! (docs/plotting.md, "Editing on the canvas"): the Edit and View menus - cut, copy, paste,
//! duplicate, delete, select all, Tab through the views, zoom to the selection, the paper grid,
//! the rulers, the next and previous sheet - the sheet list's thumbnails and dragging, and the
//! cursor's position in the status bar. The port of plotting/sheet_editor_editing.
//!
//! Every menu entry is an action with an object name and a shortcut, and each one calls a public
//! SheetEditor function that returns a Status, so an agent drives the same edits without the
//! menus. Each edit is ONE undoable step through cad's viewport_edits.
//!
//! The clipboard is the host's (`Host::clipboard`): the copied views travel as the sheet set's
//! JSON in its text (plotting::viewport_clipboard), and Paste is enabled while it holds them. The
//! C++ heard the system clipboard change; here the editor asks it at each frame.

use std::time::{Duration, Instant};

use egui::{Key, KeyboardShortcut, Modifiers};
use katana_cad::plotting::generators::{SheetTemplate, blank_sheet};
use katana_cad::plotting::sheet_commands::move_sheet;
use katana_cad::plotting::sheet_set::{Sheet, SheetSet, next_sheet_id};
use katana_cad::plotting::viewport_edits::{
    copy_viewports, duplicate_viewports, move_viewports, paste_viewports, paste_viewports_into,
    remove_viewports,
};
use katana_cad::plot::paper_dimensions;
use katana_core::{ErrorCode, Status};
use katana_geometry::Point2;

use crate::canvas::{Brush, Canvas, Color, Image, Pen, RasterCanvas, RectF};
use crate::drive::action::{Action, Menu};
use crate::host::Host;
use crate::plotting::viewport_clipboard::{
    clipboard_holds_viewports, copy_viewports_to_clipboard, viewports_on_clipboard,
};
use crate::sheet_editor::{SheetEditor, THUMBNAIL_DELAY_MS};

fn views(count: usize) -> String {
    format!("{count} view{}", if count == 1 { "" } else { "s" })
}

/// A sheet's picture before it is painted: blank paper of its shape.
fn blank_paper(sheet: &Sheet, box_size: (i32, i32)) -> Image {
    let paper = paper_dimensions(sheet.paper, sheet.landscape);
    let (width, height) = (f64::from(box_size.0), f64::from(box_size.1));
    let scale = ((width - 2.0) / paper.width_mm).min((height - 2.0) / paper.height_mm);
    let (w, h) = (paper.width_mm * scale, paper.height_mm * scale);
    let mut image = Image::new(box_size.0.max(1) as u32, box_size.1.max(1) as u32);
    RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
        let rect = RectF::new((width - w) / 2.0, (height - h) / 2.0, w, h);
        canvas.fill_rect(rect, &Brush::solid(Color::WHITE));
        canvas.set_pen(Pen::new(Color::rgb(120, 122, 128)));
        canvas.set_brush(Brush::none());
        canvas.draw_rect(rect.adjusted(0.5, 0.5, -0.5, -0.5));
    });
    image
}

fn keys(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, key)
}

impl SheetEditor {
    // ---- the menus ---------------------------------------------------------------------------

    /// The Edit menu (sheetEditMenu). Cut, Copy, Duplicate and Delete only with a selection, Paste
    /// only with views on the clipboard: a disabled action leaves its key to the text box that
    /// has the focus.
    pub(crate) fn edit_menu(&self) -> Menu {
        let selection = !self.canvas.selected_ids().is_empty();
        let make = |name: &str, text: &str, tip: &str| Action::new(name, text).tip(tip);
        Menu::new("sheetEditMenu", "&Edit")
            .action(
                make(
                    "sheetCut",
                    "Cu&t",
                    "Copy the selected views to the clipboard and remove them",
                )
                .shortcut(keys(Modifiers::COMMAND, Key::X))
                .enabled(selection),
            )
            .action(
                make(
                    "sheetCopy",
                    "&Copy",
                    "Copy the selected views to the clipboard, for this sheet, another sheet or \
                     another project",
                )
                .shortcut(keys(Modifiers::COMMAND, Key::C))
                .enabled(selection),
            )
            .action(
                make(
                    "sheetPaste",
                    "&Paste",
                    "Paste the copied views onto this sheet, with new ids",
                )
                .shortcut(keys(Modifiers::COMMAND, Key::V))
                .enabled(self.paste_ready),
            )
            .action(
                make(
                    "sheetDuplicateViews",
                    "&Duplicate",
                    "Copies of the selected views, 5 mm right and down",
                )
                .shortcut(keys(Modifiers::COMMAND, Key::D))
                .enabled(selection),
            )
            .action(
                make("sheetDeleteViews", "De&lete", "Remove the selected views")
                    .shortcut(keys(Modifiers::NONE, Key::Delete))
                    .enabled(selection),
            )
            .separator()
            .action(
                make("sheetSelectAll", "Select &All", "Select every view on this sheet")
                    .shortcut(keys(Modifiers::COMMAND, Key::A)),
            )
            // Tab itself reaches the canvas, which steps; these name it in the menu.
            .action(make(
                "sheetSelectNextView",
                "Select &Next View\tTab",
                "Select the next view on the sheet",
            ))
            .action(make(
                "sheetSelectPreviousView",
                "Select Pre&vious View\tShift+Tab",
                "Select the previous view on the sheet",
            ))
    }

    /// The View menu (sheetViewMenu). The canvas holds whether the grid and the rulers are on (an
    /// agent sets them there); the actions show its state, never a copy of it.
    pub(crate) fn view_menu(&self) -> Menu {
        let selection = !self.canvas.selected_ids().is_empty();
        Menu::new("sheetViewMenu", "&View")
            .action(
                Action::new("sheetFitPage", "Fit Page")
                    .icon("ZoomExtents")
                    .shortcut(keys(Modifiers::NONE, Key::Home)),
            )
            .action(
                Action::new("sheetZoomSelection", "&Zoom to Selection")
                    .tip("Fill the window with the selected views")
                    .shortcut(keys(Modifiers::SHIFT, Key::Home))
                    .enabled(selection),
            )
            .separator()
            .action(
                Action::new("sheetSnapGrid", "Snap to &Grid")
                    .tip(
                        "Snap dragged views to a 5 mm paper grid, drawn faintly; the edges of other \
                         views still come first",
                    )
                    .shortcut(keys(Modifiers::NONE, Key::F9))
                    .checkable(self.canvas.snaps_to_grid()),
            )
            .action(
                Action::new("sheetShowRulers", "&Rulers")
                    .tip("Rulers in paper millimetres along the top and left")
                    .shortcut(keys(Modifiers::COMMAND, Key::R))
                    .checkable(self.canvas.rulers_shown()),
            )
            .separator()
            // PgUp and PgDn step through the sheets from the canvas and the list, not the window:
            // a spin box or a text box of the properties keeps them for its own paging
            // (shortcut_scope).
            .action(
                Action::new("sheetPreviousSheet", "Pre&vious Sheet")
                    .tip("Show the sheet before this one")
                    .shortcut(keys(Modifiers::NONE, Key::PageUp)),
            )
            .action(
                Action::new("sheetNextSheet", "&Next Sheet")
                    .tip("Show the sheet after this one")
                    .shortcut(keys(Modifiers::NONE, Key::PageDown)),
            )
    }

    /// The Edit and View menus' actions, and the list's buttons, triggered; false for a name that
    /// is none of them.
    pub(crate) fn trigger_editing(&mut self, host: &mut dyn Host, name: &str) -> bool {
        let status = match name {
            "sheetCut" => self.cut_selection(host),
            "sheetCopy" => self.copy_selection(host),
            "sheetPaste" => self.paste(host),
            "sheetDuplicateViews" => self.duplicate_selection(host),
            "sheetDeleteViews" => self.remove_selected_viewport(host),
            "sheetSelectAll" => {
                self.canvas.select_all();
                Ok(())
            }
            "sheetSelectNextView" => {
                self.canvas.cycle_selection(true);
                Ok(())
            }
            "sheetSelectPreviousView" => {
                self.canvas.cycle_selection(false);
                Ok(())
            }
            "sheetZoomSelection" => {
                self.canvas.zoom_to_selection();
                Ok(())
            }
            "sheetSnapGrid" => {
                let on = !self.canvas.snaps_to_grid();
                self.canvas.set_snap_to_grid(on);
                Ok(())
            }
            "sheetShowRulers" => {
                let on = !self.canvas.rulers_shown();
                self.canvas.set_rulers_shown(on);
                Ok(())
            }
            "sheetPreviousSheet" => {
                if self.current_sheet() > 0 {
                    self.set_current_sheet(&*host, self.current_sheet() - 1);
                }
                Ok(())
            }
            "sheetNextSheet" => {
                self.set_current_sheet(&*host, self.current_sheet() + 1);
                Ok(())
            }
            _ => return self.list_button(host, name),
        };
        if let Err(error) = status {
            self.report(host, &error.describe(), true);
        }
        true
    }

    /// Whether the clipboard holds views to paste, asked again: Paste follows the clipboard,
    /// which another program may fill.
    pub(crate) fn update_paste_ready(&mut self, host: &mut dyn Host) {
        self.paste_ready = clipboard_holds_viewports(host.clipboard());
    }

    // ---- the selection's edits ---------------------------------------------------------------

    /// Every selected viewport moved by `delta_mm` (the arrows: 1 mm, Shift 10).
    pub fn nudge_selection(&mut self, host: &mut dyn Host, delta_mm: Point2) -> Status {
        self.observe(&*host);
        let ids = self.canvas.selected_ids();
        if ids.is_empty() {
            return Err(katana_core::Error::new(
                ErrorCode::InvalidState,
                "no views are selected",
            ));
        }
        let step = if ids.len() == 1 {
            "MOVE_VIEWPORT"
        } else {
            "MOVE_VIEWPORTS"
        };
        let status = move_viewports(host.document_mut(), self.current_sheet(), &ids, delta_mm, step);
        self.observe(&*host);
        status
    }

    /// Copies of the selected viewports on the clipboard (no step).
    pub fn copy_selection(&mut self, host: &mut dyn Host) -> Status {
        self.observe(&*host);
        let ids = self.canvas.selected_ids();
        if ids.is_empty() {
            return Err(katana_core::Error::new(
                ErrorCode::InvalidState,
                "select the views to copy first",
            ));
        }
        let copies = copy_viewports(&host.document().sheet_set(), self.current_sheet(), &ids)?;
        copy_viewports_to_clipboard(host.clipboard(), &copies)?;
        // Paste is ready now, whenever the clipboard gets round to saying so.
        self.paste_ready = true;
        self.report(host, &format!("Copied {}.", views(copies.len())), false);
        Ok(())
    }

    pub fn cut_selection(&mut self, host: &mut dyn Host) -> Status {
        self.copy_selection(host)?;
        let ids = self.canvas.selected_ids();
        let status = remove_viewports(host.document_mut(), self.current_sheet(), &ids, "CUT_VIEWPORTS");
        self.observe(&*host);
        status
    }

    /// The clipboard's viewports onto the current sheet, with new ids, one step from where they
    /// were copied while that place is taken (cad's paste_offset); the pasted ones are selected.
    pub fn paste(&mut self, host: &mut dyn Host) -> Status {
        self.observe(&*host);
        let viewports = viewports_on_clipboard(host.clipboard())?;
        if host.document().sheet_set().sheets.is_empty() {
            // A sheet to paste onto, added in the paste's own step: one Undo takes both away,
            // rather than leaving a blank sheet behind.
            let mut set = SheetSet::clone(&host.document().sheet_set());
            let mut sheet = blank_sheet(&SheetTemplate::default(), String::from("SHEET 1"));
            sheet.id = next_sheet_id(&set);
            set.sheets.push(sheet);
            let pasted = paste_viewports_into(&mut set, 0, viewports, None)?;
            host.document_mut().set_sheet_set(&set, "PASTE_VIEWPORTS")?;
            self.observe(&*host);
            self.set_current_sheet(&*host, 0);
            self.canvas.set_selection(&pasted, "");
            self.report(
                host,
                &format!("Pasted {} onto a new sheet.", views(pasted.len())),
                false,
            );
            return Ok(());
        }
        let ids = paste_viewports(
            host.document_mut(),
            self.current_sheet(),
            viewports,
            None,
            "PASTE_VIEWPORTS",
        )?;
        self.observe(&*host);
        self.canvas.set_selection(&ids, "");
        self.report(host, &format!("Pasted {}.", views(ids.len())), false);
        Ok(())
    }

    pub fn duplicate_selection(&mut self, host: &mut dyn Host) -> Status {
        self.observe(&*host);
        let selected = self.canvas.selected_ids();
        if selected.is_empty() {
            return Err(katana_core::Error::new(
                ErrorCode::InvalidState,
                "select the views to duplicate first",
            ));
        }
        let ids = duplicate_viewports(
            host.document_mut(),
            self.current_sheet(),
            &selected,
            "DUPLICATE_VIEWPORTS",
        )?;
        self.observe(&*host);
        self.canvas.set_selection(&ids, "");
        Ok(())
    }

    /// Sheet `from` moved to `to` and shown: what a drop in the list does.
    pub fn move_sheet_to(&mut self, host: &mut dyn Host, from: usize, to: usize) -> Status {
        self.observe(&*host);
        if from == to && from < host.document().sheet_set().sheets.len() {
            return Ok(());
        }
        move_sheet(host.document_mut(), from, to)?;
        self.observe(&*host);
        self.set_current_sheet(&*host, to);
        Ok(())
    }

    // ---- thumbnails --------------------------------------------------------------------------

    /// The pictures the list shows: the last painted of each sheet stands in until it is painted
    /// again, and a round of painting is started a moment later.
    pub(crate) fn refresh_thumbnails(&mut self, host: &dyn Host) {
        let set = host.document().sheet_set();
        self.thumbnails.prune(&set);
        let box_size = self.thumbnails.box_size();
        let mut pictures = Vec::with_capacity(set.sheets.len());
        for sheet in &set.sheets {
            let mut picture = self.thumbnails.cached(&sheet.id);
            if picture.is_null() {
                picture = blank_paper(sheet, box_size);
            }
            self.picture_serial += 1;
            pictures.push((sheet.id.clone(), picture, self.picture_serial));
        }
        self.pictures = pictures;
        if self.shown && !set.sheets.is_empty() {
            self.thumbnails_due = Some(Instant::now() + Duration::from_millis(THUMBNAIL_DELAY_MS));
        }
    }

    /// One stale picture painted, the next a turn later, so the editor stays responsive. A hidden
    /// editor paints nothing; showing it starts again.
    pub(crate) fn paint_next_thumbnail(&mut self, host: &dyn Host) {
        self.thumbnails_due = None;
        if !self.shown {
            return;
        }
        let set = host.document().sheet_set();
        let from = self.source(host.document());
        for index in 0..set.sheets.len() {
            if !self.thumbnails.is_stale(&set, index, &from) {
                continue;
            }
            let picture = self.thumbnails.thumbnail(&set, index, &from, self.canvas.ratio);
            self.set_picture(index, &set.sheets[index].id, picture);
            self.thumbnails_due = Some(Instant::now());
            return;
        }
    }

    fn set_picture(&mut self, index: usize, id: &str, picture: Image) {
        self.picture_serial += 1;
        let entry = (String::from(id), picture, self.picture_serial);
        if index < self.pictures.len() {
            self.pictures[index] = entry;
        } else {
            self.pictures.push(entry);
        }
    }

    /// How many of the list's pictures are still to be painted.
    pub fn pending_thumbnails(&mut self, host: &dyn Host) -> usize {
        self.observe(host);
        let set = host.document().sheet_set();
        let from = self.source(host.document());
        self.thumbnails.stale_count(&set, &from)
    }

    /// Every stale picture painted now rather than while idle.
    pub fn render_thumbnails_now(&mut self, host: &dyn Host) {
        self.observe(host);
        let set = host.document().sheet_set();
        let from = self.source(host.document());
        for index in 0..set.sheets.len() {
            if self.thumbnails.is_stale(&set, index, &from) {
                let picture = self.thumbnails.thumbnail(&set, index, &from, self.canvas.ratio);
                self.set_picture(index, &set.sheets[index].id, picture);
            }
        }
    }

    pub fn thumbnails(&self) -> &crate::plotting::sheet_thumbnails::SheetThumbnails {
        &self.thumbnails
    }

    /// The picture the list shows beside row `row` (its icon); None for no row.
    pub fn list_picture(&self, row: usize) -> Option<&Image> {
        self.pictures.get(row).map(|(_, picture, _)| picture)
    }

    /// Row `row`'s picture as a texture, made again when it changed.
    pub(crate) fn list_texture(
        &mut self,
        ctx: &egui::Context,
        row: usize,
    ) -> Option<egui::load::SizedTexture> {
        let (id, picture, serial) = self.pictures.get(row)?.clone();
        if picture.is_null() {
            return None;
        }
        let ratio = picture.device_pixel_ratio().max(1.0);
        let size = egui::vec2(
            (f64::from(picture.width()) / ratio) as f32,
            (f64::from(picture.height()) / ratio) as f32,
        );
        let fresh = self
            .textures
            .get(&id)
            .is_some_and(|(made, _)| *made == serial);
        if !fresh {
            let texture = ctx.load_texture(
                format!("sheetThumbnail_{id}"),
                picture.to_color_image(),
                egui::TextureOptions::LINEAR,
            );
            self.textures.insert(id.clone(), (serial, texture));
        }
        self.textures
            .get(&id)
            .map(|(_, texture)| egui::load::SizedTexture::new(texture.id(), size))
    }
}
