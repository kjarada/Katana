//! katana_make_icons <output-directory>: the application icon (katana.ico, katana.png) and the
//! contact sheet of every icon (icon_sheet.png), written by the painting code the window uses. A
//! developer tool, as its C++ is (src/katana_qt/make_icons.cpp); katana_window::make_icons says
//! what it writes and why the .ico is committed rather than built.

use katana_app::console::Terminal;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = katana_window::make_icons::run(&args, &mut Terminal);
    std::process::exit(code);
}
