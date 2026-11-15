//! katana_tool_icon_sheet <output.png>
//!
//! A DEVELOPER tool, as katana_make_icons is: every tool in the catalogue
//! (katana_cad::interactive_tool) drawn at the two sizes the application uses - 20 px on a
//! toolbar, 16 px in a menu - and large, with its menu, name, id and aliases, on the theme's own
//! ground. It is how a tool's author reviews an icon without launching the application, and it
//! prints how many tools still have only the placeholder glyph. The program is
//! src/bin/katana_tool_icon_sheet, which calls `run`.

use std::path::Path;

use katana_app::console::Console;
use katana_cad::interactive_tool::{tool_catalog, tool_catalog_problems};
use katana_core::Status;

use crate::canvas::{Brush, Canvas, Pen, PointF, RectF, RenderHint, write_png};
use crate::icons::{IconMode, Rasteriser, default_rasteriser, icon_image};
use crate::theme::palette;
use crate::tools::tool_icons::has_tool_icon;

/// The sheet's rows and width, in pixels.
const ROW: i32 = 64;
const WIDTH: i32 = 760;

/// The sheet of every catalogue tool into `path`, and how many tools there are and how many have
/// no icon of their own.
pub fn write_tool_icon_sheet(path: &Path, raster: &dyn Rasteriser) -> (Status, usize, usize) {
    let tools = tool_catalog().all();
    let height = 1.max(tools.len() as i32) * ROW + 16;
    let mut placeholders = 0;
    // Each tool's icon at the three sizes the sheet shows, as QIcon::pixmap painted them.
    let mut pixmaps = Vec::new();
    for tool in &tools {
        let at = |size: f64| icon_image(raster, &tool.id, size, 1.0, IconMode::Normal);
        pixmaps.push((at(16.0), at(20.0), at(48.0)));
        if !has_tool_icon(&tool.id) {
            placeholders += 1;
        }
    }
    let sheet = raster.paint(
        WIDTH as u32,
        height as u32,
        1.0,
        &mut |canvas: &mut dyn Canvas| {
            canvas.fill_rect(
                RectF::new(0.0, 0.0, f64::from(WIDTH), f64::from(height)),
                &Brush::solid(palette::window()),
            );
            canvas.set_render_hint(RenderHint::Antialiasing, true);
            for ((row, tool), (small, toolbar, large)) in (0_i32..).zip(&tools).zip(&pixmaps) {
                let y = 8 + row * ROW;
                let mut draw = |x: i32, top: i32, image: Option<&crate::canvas::Image>| {
                    if let Some(image) = image {
                        canvas.draw_image_at(PointF::new(f64::from(x), f64::from(top)), image);
                    }
                };
                draw(12, y + 22, small.as_ref());
                draw(40, y + 20, toolbar.as_ref());
                draw(72, y + 4, large.as_ref());
                canvas.set_pen(Pen::new(palette::text()));
                let aliases = tool.aliases.join(" ");
                canvas.draw_text(
                    PointF::new(136.0, f64::from(y + 24)),
                    &format!("{} > {} > {}", tool.category, tool.group, tool.name),
                );
                canvas.set_pen(Pen::new(palette::text_muted()));
                canvas.draw_text(
                    PointF::new(136.0, f64::from(y + 44)),
                    &format!("{}   {aliases}", tool.id),
                );
            }
        },
    );
    (write_png(&sheet, path), tools.len(), placeholders)
}

/// The program: `args` as the command line gives them (the program's own name first), its words
/// to `console`, and the exit code - 2 for no output named, 1 for a sheet that could not be
/// written, 0 when it is.
pub fn run(args: &[String], console: &mut dyn Console) -> i32 {
    if args.len() != 2 {
        console.err("usage: katana_tool_icon_sheet <output.png>\n");
        return 2;
    }
    for problem in tool_catalog_problems() {
        console.err(&format!("catalogue refused a tool: {problem}\n"));
    }
    let Some(raster) = default_rasteriser() else {
        console.err("katana_tool_icon_sheet: this build has no raster canvas to paint with\n");
        return 1;
    };
    let (written, tools, placeholders) =
        write_tool_icon_sheet(Path::new(&args[1]), raster.as_ref());
    if written.is_err() {
        console.err(&format!("could not write {}\n", args[1]));
        return 1;
    }
    console.out(&format!(
        "{tools} tools, {placeholders} without an icon of their own\n"
    ));
    0
}
