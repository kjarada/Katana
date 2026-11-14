//! File > Export View as Image, Edit > Copy View as Image and the SNAPSHOT verb: a picture of a
//! view, as a file or on the clipboard (docs/desktop.md, "Plot to PDF and view images"). The port
//! of plotting/view_image_export.
//!
//! ```text
//! SNAPSHOT <file.png|.jpg|.jpeg|.tif|.tiff> [width=N] [height=N] [scale=F]
//!          [bg=theme|white|none] [view=plan|3d]
//! SNAPSHOT CLIPBOARD [the same options]
//! ```
//!
//! The size: width and height both given are the image's; one given takes the other from the
//! view's proportions; neither is the view's size on screen times scale= (1 unless given, 0.25 to
//! 8). A side is 1 to 10000 pixels - an A0 sheet's width at 300 dpi is 9933. The plan view is
//! painted afresh at that size by the plan painter (the plan view's render to image), without the
//! grid, on the view's own ground (theme), on white - drawn as a plot draws it, white pens black
//! and line weights in millimetres, since the screen's white pens would vanish - or on nothing
//! (none: a transparent PNG, or the clipboard; a JPEG has no transparency and the TIFF written
//! here is RGB, so bg=none with either is refused). A 3D view is grabbed as it is drawn on screen
//! and scaled to the size; bg= is the plan view's, and bg=white or bg=none with view=3d is refused
//! rather than dropped.
//!
//! The verb is the window's - there is a view to picture only there - and a headless run's
//! --screenshot grabs the whole window. What it did is logged as a record: file="path" (or
//! clipboard=yes) view= width= height=.
//!
//! The dialog is non-modal and runs nothing itself: it writes the SNAPSHOT line its fields
//! describe and Export hands it to the window's one executor. Object names:
//!
//! ```text
//! viewImageDialog      the dialog (File > Export View as Image, fileExportViewImage)
//! viewImagePath        the file; its extension is the format
//! viewImageBrowse      choose it with a file dialog (not headless)
//! viewImageFormat      PNG / JPEG / TIFF: sets the path's extension
//! viewImageView        Plan view / 3D view
//! viewImageSize        1x .. 4x the view's size on screen, or Custom size
//! viewImageWidth       pixels, for a custom size
//! viewImageHeight      pixels, for a custom size
//! viewImageBackground  Theme / White / Transparent
//! viewImageCommand     the exact line Export will run (read-only)
//! viewImageExport      run it
//! viewImageStatus      what happened last
//! viewImageClose       close
//! ```
//!
//! WHAT DIFFERS FROM THE C++. The size boxes show no " px" (the foundation's whole-number box
//! takes no suffix), and the command line is in the window's own face (its line field takes no
//! font).

use std::io::Cursor;
use std::path::Path;

use egui::Ui;
use katana_cad::command_interpreter::CommandInterpreter;
use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::text::{format_general, parse_finite_double, parse_integer};
use katana_core::{Error, ErrorCode, Result, Status};

use crate::canvas::{Image, encode_png};
use crate::drive::controls::{self, Choice, LineOptions};
use crate::host::Host;
use crate::ifc_dialogs::{from_native_separators, get_save_file_name, to_native_separators};
use crate::plotting::tiff_writer::write_tiff;
use crate::shell::DialogFrame;

/// What a plan view is pictured on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Background {
    #[default]
    Theme,
    White,
    None,
}

/// Which view is pictured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum View {
    #[default]
    Plan,
    Model3D,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotRequest {
    /// Empty for the clipboard.
    pub path: String,
    pub clipboard: bool,
    /// 0: from the view.
    pub width: i32,
    /// 0: from the view.
    pub height: i32,
    pub scale: f64,
    pub background: Background,
    pub view: View,
}

impl Default for SnapshotRequest {
    fn default() -> Self {
        Self {
            path: String::new(),
            clipboard: false,
            width: 0,
            height: 0,
            scale: 1.0,
            background: Background::Theme,
            view: View::Plan,
        }
    }
}

/// The largest side of a snapshot, in pixels: an A0 sheet's 841 mm at 300 dpi is 9933, and an
/// image beyond that costs gigabytes for nothing a screen or a report shows.
pub const MAXIMUM_SNAPSHOT_SIDE: i32 = 10000;

const USAGE: &str = "usage: SNAPSHOT <file.png|.jpg|.tif> | CLIPBOARD [width=N] [height=N] \
                     [scale=F] [bg=theme|white|none] [view=plan|3d]";

// A picture of a view is a small multiple of it or a large one at most: under a quarter it is a
// thumbnail nobody can read, over eight it passes the side limit on any screen.
const MINIMUM_SCALE: f64 = 0.25;
const MAXIMUM_SCALE: f64 = 8.0;

/// A TIFF carries the resolution it is to print at; a view picture has none, so it is marked at
/// the screen's nominal 96 dpi, as Windows' logical inch.
const SCREEN_DPI: f64 = 96.0;

/// QFileInfo::suffix: what follows the last '.' of the path's file name; empty for none.
fn suffix_of(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rfind('.').map_or("", |at| &name[at + 1..])
}

/// The format a path's extension names, lower case; empty for none we write.
fn format_of(path: &str) -> &'static str {
    match suffix_of(path).to_lowercase().as_str() {
        "png" => "png",
        "jpg" | "jpeg" => "jpeg",
        "tif" | "tiff" => "tiff",
        _ => "",
    }
}

fn refused(why: &str, word: &str) -> Error {
    make_error(ErrorCode::InvalidArgument, format!("{why}; {USAGE}"), word)
}

fn side(text: &str) -> Option<i32> {
    let value = parse_integer(text)?;
    if !(1..=i64::from(MAXIMUM_SNAPSHOT_SIDE)).contains(&value) {
        return None;
    }
    i32::try_from(value).ok()
}

/// QString::number(value, 'g', 10).
fn number_text(value: f64) -> String {
    format_general(value, 10)
}

/// A SNAPSHOT line read by the interpreter's word rules. InvalidArgument naming the word, with
/// the usage, for anything the grammar above does not take.
pub fn parse_snapshot(line: &str) -> Result<SnapshotRequest> {
    let tokens = CommandInterpreter::tokenize(line)?;
    if tokens.len() < 2 || tokens[1].is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, USAGE));
    }
    let mut request = SnapshotRequest::default();
    let target = &tokens[1];
    if target.eq_ignore_ascii_case("CLIPBOARD") {
        request.clipboard = true;
    } else if format_of(target).is_empty() {
        return Err(refused(
            "the file's extension names its format: .png, .jpg, .jpeg, .tif or .tiff",
            target,
        ));
    } else {
        request.path.clone_from(target);
    }
    for raw in &tokens[2..] {
        let Some(equals) = raw.find('=').filter(|at| *at > 0) else {
            return Err(refused("not a SNAPSHOT option", raw));
        };
        let key = raw[..equals].to_lowercase();
        let value = raw[equals + 1..].to_lowercase();
        match key.as_str() {
            "width" | "height" => {
                let Some(pixels) = side(&value) else {
                    return Err(refused(
                        &format!("{key}= is a whole number of pixels from 1 to 10000"),
                        raw,
                    ));
                };
                if key == "width" {
                    request.width = pixels;
                } else {
                    request.height = pixels;
                }
            }
            "scale" => {
                let Some(scale) = parse_finite_double(&value)
                    .filter(|scale| (MINIMUM_SCALE..=MAXIMUM_SCALE).contains(scale))
                else {
                    return Err(refused("scale= is the view's size times 0.25 to 8", raw));
                };
                request.scale = scale;
            }
            "bg" => {
                request.background = match value.as_str() {
                    "theme" => Background::Theme,
                    "white" => Background::White,
                    "none" | "transparent" => Background::None,
                    _ => return Err(refused("bg= is theme, white or none", raw)),
                };
            }
            "view" => {
                request.view = match value.as_str() {
                    "plan" => View::Plan,
                    "3d" => View::Model3D,
                    _ => return Err(refused("view= is plan or 3d", raw)),
                };
            }
            _ => return Err(refused("not a SNAPSHOT option", raw)),
        }
    }
    // The 3D view is grabbed as it is drawn, on its own ground, so a ground asked of it would be
    // dropped - it once was, and an opaque image written for bg=none with nothing said.
    if request.view == View::Model3D && request.background != Background::Theme {
        return Err(refused(
            "bg= is the plan view's; a 3D view is pictured on its own ground",
            if request.background == Background::White {
                "bg=white"
            } else {
                "bg=none"
            },
        ));
    }
    // Only a PNG, or the clipboard, keeps transparency: a JPEG has none, and the TIFF written here
    // (tiff_writer) is RGB.
    if request.background == Background::None
        && !request.clipboard
        && format_of(&request.path) != "png"
    {
        return Err(refused(
            "bg=none needs a .png: a JPEG or TIFF has no transparency",
            &request.path,
        ));
    }
    Ok(request)
}

/// The line parse_snapshot reads back: the path quoted with '/' separators (or CLIPBOARD), then
/// only the options that differ from the defaults.
pub fn snapshot_command_line(request: &SnapshotRequest) -> String {
    let mut line = if request.clipboard {
        String::from("SNAPSHOT CLIPBOARD")
    } else {
        format!("SNAPSHOT \"{}\"", from_native_separators(&request.path))
    };
    if request.width > 0 {
        line.push_str(&format!(" width={}", request.width));
    }
    if request.height > 0 {
        line.push_str(&format!(" height={}", request.height));
    }
    if request.scale != 1.0 {
        line.push_str(&format!(" scale={}", number_text(request.scale)));
    }
    match request.background {
        Background::White => line.push_str(" bg=white"),
        Background::None => line.push_str(" bg=none"),
        Background::Theme => {}
    }
    if request.view == View::Model3D {
        line.push_str(" view=3d");
    }
    line
}

/// The image's size in pixels for a view of `view_size` (logical pixels), as the grammar says:
/// never below 1 or above MAXIMUM_SNAPSHOT_SIDE on a side.
pub fn snapshot_size(request: &SnapshotRequest, view_size: (i32, i32)) -> (i32, i32) {
    let w = f64::from(view_size.0.max(1));
    let h = f64::from(view_size.1.max(1));
    let (width, height) = if request.width > 0 && request.height > 0 {
        (f64::from(request.width), f64::from(request.height))
    } else if request.width > 0 {
        (
            f64::from(request.width),
            f64::from(request.width) * h / w,
        )
    } else if request.height > 0 {
        (
            f64::from(request.height) * w / h,
            f64::from(request.height),
        )
    } else {
        (w * request.scale, h * request.scale)
    };
    let clamp = |pixels: f64| (pixels.round() as i32).clamp(1, MAXIMUM_SNAPSHOT_SIDE);
    (clamp(width), clamp(height))
}

/// The image as the bytes of a JPEG file. A JPEG has no alpha: laid on the image's own ground,
/// which the snapshot filled opaque, it loses nothing.
fn encode_jpeg(image: &Image) -> std::result::Result<Vec<u8>, String> {
    let rgb: Vec<u8> = image
        .to_straight_rgba8()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect();
    let picture = ::image::RgbImage::from_raw(image.width(), image.height(), rgb)
        .ok_or_else(|| String::from("the image's pixels are not its size"))?;
    let mut bytes = Cursor::new(Vec::new());
    picture
        .write_to(&mut bytes, ::image::ImageFormat::Jpeg)
        .map_err(|error| format!("{error}"))?;
    Ok(bytes.into_inner())
}

/// Writes `image` to `path` in the format its extension names. FileExportFailure when it cannot
/// be written.
pub fn write_snapshot(image: &Image, path: &Path) -> Status {
    let shown = String::from_utf8_lossy(&path_to_utf8(path)).into_owned();
    let format = format_of(&shown);
    if format == "tiff" {
        return write_tiff(image, path, SCREEN_DPI);
    }
    let encoded = if format == "jpeg" {
        encode_jpeg(image)
    } else {
        encode_png(image).map_err(|error| error.describe())
    };
    encoded
        .and_then(|bytes| std::fs::write(path, bytes).map_err(|error| format!("{error}")))
        .map_err(|why| {
            make_error(
                ErrorCode::FileExportFailure,
                "the image could not be written",
                format!("{shown}: {why}"),
            )
        })
}

// ---- the dialog ---------------------------------------------------------------------------------

/// Asked before Export writes over a file that is there: true to write it.
pub type ConfirmReplace = Box<dyn FnMut(&str) -> bool>;

const FORMATS: [&str; 3] = ["PNG", "JPEG", "TIFF"];
const EXTENSIONS: [&str; 3] = ["png", "jpg", "tif"];
const VIEWS: [&str; 2] = ["Plan view", "3D view"];
const SIZES: [&str; 5] = [
    "1x the view",
    "2x the view",
    "3x the view",
    "4x the view",
    "Custom size",
];
const BACKGROUNDS: [&str; 3] = ["Theme", "White", "Transparent"];

fn items(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| String::from(*name)).collect()
}

/// `path` with the extension of the format at `index` in viewImageFormat.
fn in_format(path: &str, index: usize) -> String {
    let suffix = suffix_of(path);
    let stem = if suffix.is_empty() {
        path
    } else {
        &path[..path.len() - suffix.len() - 1]
    };
    format!("{stem}.{}", EXTENSIONS[index.min(EXTENSIONS.len() - 1)])
}

/// File > Export View as Image (see the module comment).
pub struct ViewImageDialog {
    frame: DialogFrame,
    visible: bool,
    suggested: String,
    path: String,
    format: Choice,
    view: Choice,
    size: Choice,
    width: i64,
    height: i64,
    background: Choice,
    status: String,
    /// As the plot dialog's: asked before Export writes over a file.
    confirm_replace: Option<ConfirmReplace>,
}

/// The dialog and the window, lent together to the dialog frame's body and footer.
struct Lent<'a, 'h> {
    dialog: &'a mut ViewImageDialog,
    host: &'a mut (dyn Host + 'h),
}

impl ViewImageDialog {
    /// The dialog on `suggested_path`: the file Browse and a new dialog start from.
    pub fn new(suggested_path: &str) -> Self {
        Self {
            frame: DialogFrame::new("viewImageDialog", "Export View as Image"),
            visible: false,
            suggested: String::from(suggested_path),
            path: String::from(suggested_path),
            format: Choice::at(0),
            view: Choice::at(0),
            size: Choice::at(0),
            width: 1920,
            height: 1080,
            background: Choice::at(0),
            status: String::new(),
            confirm_replace: None,
        }
    }

    /// Sets who is asked before a file that is there is written over.
    pub fn set_confirm_replace(&mut self, confirm: ConfirmReplace) {
        self.confirm_replace = Some(confirm);
    }

    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    /// The Close button's: hidden, its fields kept.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// What viewImagePath holds.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Types `path` into viewImagePath.
    pub fn set_path(&mut self, path: &str) {
        self.path = String::from(path);
    }

    /// What viewImageStatus says.
    pub fn status(&self) -> &str {
        &self.status
    }

    fn custom_size(&self) -> bool {
        self.size.index == Some(SIZES.len() - 1)
    }

    /// The request the fields describe, or why there is none.
    pub fn request(&self) -> Result<SnapshotRequest> {
        let path = self.path.trim();
        if path.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "no image file is named",
            ));
        }
        let mut request = SnapshotRequest {
            path: String::from(path),
            view: if self.view.index == Some(1) {
                View::Model3D
            } else {
                View::Plan
            },
            ..SnapshotRequest::default()
        };
        if self.custom_size() {
            request.width = i32::try_from(self.width).unwrap_or(MAXIMUM_SNAPSHOT_SIDE);
            request.height = i32::try_from(self.height).unwrap_or(MAXIMUM_SNAPSHOT_SIDE);
        } else {
            request.scale = (self.size.index.unwrap_or(0) + 1) as f64;
        }
        // The ground is the plan view's to choose (parse_snapshot).
        request.background = if request.view == View::Plan {
            match self.background.index {
                Some(1) => Background::White,
                Some(2) => Background::None,
                _ => Background::Theme,
            }
        } else {
            Background::Theme
        };
        // The line's own reader is the judge, so the dialog refuses what the verb would: an
        // extension it does not write, a transparent JPEG.
        parse_snapshot(&snapshot_command_line(&request))?;
        Ok(request)
    }

    /// The line viewImageCommand shows: the one Export runs, or nothing while there is none.
    pub fn command(&self) -> String {
        self.request()
            .map(|request| snapshot_command_line(&request))
            .unwrap_or_default()
    }

    /// The format chosen takes the path with it.
    fn follow_format(&mut self) {
        let path = String::from(self.path.trim());
        if !path.is_empty() {
            // The suggestion in another format is still the suggestion.
            let suggested = self.path == self.suggested;
            self.path = in_format(&path, self.format.index.unwrap_or(0));
            if suggested {
                self.suggested.clone_from(&self.path);
            }
        }
    }

    /// Chooses the format by its place in viewImageFormat (PNG, JPEG, TIFF), as picking it does.
    pub fn set_format(&mut self, index: usize) {
        self.format = Choice::at(index.min(FORMATS.len() - 1));
        self.follow_format();
    }

    /// As the plot dialog's suggest_path, in the format chosen.
    pub fn suggest_path(&mut self, path: &str) {
        let in_chosen = in_format(path, self.format.index.unwrap_or(0));
        if self.path == self.suggested {
            self.path.clone_from(&in_chosen);
        }
        self.suggested = in_chosen;
    }

    /// What Export does: the SNAPSHOT line handed to the window's executor and the outcome said in
    /// viewImageStatus.
    pub fn export_image(&mut self, host: &mut dyn Host) {
        let made = match self.request() {
            Ok(made) => made,
            Err(error) => {
                self.status = error.message;
                return;
            }
        };
        // As Plot to PDF asks (the plot dialog's plot).
        if let Some(confirm) = self.confirm_replace.as_mut()
            && Path::new(&made.path).exists()
            && !confirm(&made.path)
        {
            self.status = format!(
                "Not exported: {} is there and was kept.",
                to_native_separators(&made.path)
            );
            return;
        }
        let outcome = host.run(&snapshot_command_line(&made));
        let last_line = |text: &str| String::from(text.rsplit('\n').next().unwrap_or(""));
        self.status = if outcome.ok {
            format!("Exported: {}", last_line(&outcome.reply))
        } else {
            format!("Not exported: {}", last_line(&outcome.error))
        };
    }

    fn browse(&mut self, host: &dyn Host) {
        if host.headless() {
            self.status = String::from(
                "A headless session opens no file dialog: fill viewImagePath with the path \
                 instead.",
            );
            return;
        }
        let path = get_save_file_name(
            "Export View as Image",
            self.path.trim(),
            "PNG (*.png);;JPEG (*.jpg *.jpeg);;TIFF (*.tif *.tiff)",
        );
        if !path.is_empty() {
            self.path = to_native_separators(&path);
        }
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent = Lent {
            dialog: &mut *self,
            host: &mut *host,
        };
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.dialog.body(ui, &*lent.host),
            |ui, lent| lent.dialog.footer(ui, &mut *lent.host),
        );
        self.frame = frame;
        if shown.closed {
            self.close();
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &dyn Host) {
        // Wide enough for a path and the line it runs to be read whole.
        ui.set_min_width(560.0);
        egui::Grid::new("viewImageForm")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Image:");
                ui.horizontal(|ui| {
                    controls::line_with(
                        ui,
                        "viewImagePath",
                        &mut self.path,
                        LineOptions {
                            placeholder: "the image to write: .png, .jpg or .tif",
                            ..LineOptions::default()
                        },
                    );
                    if controls::button(ui, "viewImageBrowse", "Browse...").clicked() {
                        self.browse(host);
                    }
                });
                ui.end_row();

                ui.label("Format:");
                if controls::choice(ui, "viewImageFormat", &items(&FORMATS), &mut self.format)
                    .changed()
                {
                    self.follow_format();
                }
                ui.end_row();

                ui.label("View:");
                controls::choice(ui, "viewImageView", &items(&VIEWS), &mut self.view);
                ui.end_row();

                ui.label("Size:");
                controls::choice(ui, "viewImageSize", &items(&SIZES), &mut self.size);
                ui.end_row();

                ui.label("Custom size:");
                let custom = self.custom_size();
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(custom, |ui| {
                        let sides = 1..=i64::from(MAXIMUM_SNAPSHOT_SIDE);
                        controls::whole(ui, "viewImageWidth", &mut self.width, sides.clone());
                        ui.label("by");
                        controls::whole(ui, "viewImageHeight", &mut self.height, sides);
                    });
                });
                ui.end_row();

                ui.label("Background:");
                let plan = self.view.index == Some(0);
                ui.add_enabled_ui(plan, |ui| {
                    controls::choice(
                        ui,
                        "viewImageBackground",
                        &items(&BACKGROUNDS),
                        &mut self.background,
                    )
                    .on_hover_text(
                        "The plan view's ground: as on screen, white, or none (a PNG only)",
                    );
                });
                ui.end_row();

                ui.label("Command:");
                let made = self.request();
                let mut command = made
                    .as_ref()
                    .map(snapshot_command_line)
                    .unwrap_or_default();
                let waiting = match &made {
                    Ok(_) => String::new(),
                    Err(error) => format!("nothing to run yet: {}", error.message),
                };
                controls::line_with(
                    ui,
                    "viewImageCommand",
                    &mut command,
                    LineOptions {
                        placeholder: &waiting,
                        read_only: true,
                        ..LineOptions::default()
                    },
                )
                .on_hover_text(
                    "The line Export hands to the command line - type it there and it does the \
                     same",
                );
                ui.end_row();
            });
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            let status = self.status.clone();
            controls::label_as(
                ui,
                "viewImageStatus",
                &status,
                egui::Label::new(status.as_str()).wrap().selectable(true),
            );
            let ready = self.request().is_ok();
            let pressed = ui
                .add_enabled_ui(ready, |ui| {
                    controls::button(ui, "viewImageExport", "Export").clicked()
                })
                .inner;
            if pressed {
                self.export_image(&mut *host);
                ui.ctx().request_repaint();
            }
            if controls::button(ui, "viewImageClose", "Close").clicked() {
                self.close();
            }
        });
    }
}
