//! katana_tool_icon_sheet <output.png>: every catalogue tool's icon at the sizes the window shows
//! it, with its menu, name, id and aliases, and how many tools have only the placeholder glyph. A
//! developer tool, as its C++ is (src/katana_qt/tool_icon_sheet.cpp); katana_window::
//! tool_icon_sheet holds the sheet.

use katana_app::console::Terminal;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = katana_window::tool_icon_sheet::run(&args, &mut Terminal);
    std::process::exit(code);
}
