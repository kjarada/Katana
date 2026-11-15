//! The command line panel as the window lays it out (main_window::panels).

use katana_window::MainWindow;
use katana_window::drive::headless::Headless;

mod command_line {
    use super::*;

    // Seen on the owner's screen (2026-11-15): the log wrapped at about 280 points with the rest
    // of its panel empty - egui's text box takes text_edit_width, 280 by default, where the C++'s
    // QPlainTextEdit filled its dock. The headless window's command line dock is 660 points wide.
    #[test]
    fn the_log_is_as_wide_as_its_panel() {
        let mut window = MainWindow::new();
        let mut headless = Headless::new();
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        let frame = headless.snapshot();
        let dock = frame
            .target("CommandLineDock")
            .expect("the command line is a dock of the window")
            .rect;
        let log = frame
            .control("CommandLineDock", "commandLog")
            .expect("the log is drawn in it")
            .rect;
        assert!(
            log.width() > 0.9 * dock.width(),
            "the log {} points wide in a dock of {}",
            log.width(),
            dock.width()
        );
    }
}
