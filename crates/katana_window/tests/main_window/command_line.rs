//! The command line panel as the window lays it out (main_window::panels).

use katana_window::MainWindow;
use katana_window::drive::headless::Headless;
use katana_window::main_window::panels;

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

    #[test]
    fn an_error_line_shows_its_sentence_and_the_logs_record_keeps_its_code() {
        // The fourth look audit: the log printed the enum's names ("ParseFailure:", "NotFound:")
        // before an error's sentence. The panel shows the sentence; the record a driver and
        // --report read, and what the line handed back, keep the code, as the status bar's
        // sentence does (command_log::without_error_code).
        let log = "> LAYER SET nothing\n! NotFound: no layer nothing [LAYER]\n\
                   ! ParseFailure: a number was expected\n  at word 3\nlayer=0 current";
        assert_eq!(
            panels::shown_log(log),
            "> LAYER SET nothing\n! no layer nothing [LAYER]\n! a number was expected\n  at \
             word 3\nlayer=0 current"
        );
        // A line that only mentions a code, or names one mid-sentence, is as it was.
        assert_eq!(
            panels::shown_log("! Style: NotFound: x\nNotFound: a reply"),
            "! Style: NotFound: x\nNotFound: a reply"
        );

        let mut window = MainWindow::new();
        window.exec.log.log("NotFound: no layer nothing", true);
        let mut headless = Headless::new();
        headless
            .settle(&mut window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        let record = headless
            .snapshot()
            .control("CommandLineDock", "commandLog")
            .expect("the log is drawn")
            .clone();
        assert!(
            record.value.contains("! NotFound: no layer nothing"),
            "{}",
            record.value
        );
    }

    #[test]
    fn the_log_is_covered_down_to_its_first_whole_lines_ink() {
        // The fourth look audit (S\crops\main_text_xl_284_722.png): at Extra Large a pixel of the
        // descenders of the row above the log's first whole line showed over it. That row ended
        // within half a point of the log's top, so the cover - to the first whole row's TOP - was
        // not drawn; it is drawn to the first whole line's INK.
        let mut harness = katana_window::drive::Harness::new();
        katana_window::theme::apply(harness.context());
        let mut laid_out = None;
        let _ = harness.frame(|ui| {
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            let job = egui::text::LayoutJob::simple(
                String::from("gjpqy, gjpqy\nOpened /root/project (20 entities)."),
                font,
                egui::Color32::WHITE,
                f32::INFINITY,
            );
            laid_out = Some(ui.fonts_mut(|fonts| fonts.layout_job(job)));
        });
        let galley = laid_out.expect("laid out");
        assert_eq!(galley.rows.len(), 2, "the premise: two rows");
        let row = &galley.rows[1];
        let at = egui::pos2(0.0, 0.0);
        // The log's top 0.2 above the second row's top: the first row is above it but for its
        // last fifth of a point, the second is the first whole line.
        let view_top = row.pos.y - 0.2;
        let row_top = panels::first_whole_line_top(&galley, at, view_top).expect("a whole line");
        assert!(
            row_top - view_top <= 0.5,
            "the premise: covered to the row's top, nothing would be covered"
        );
        let ink_top = row.pos.y + row.visuals.mesh_bounds.min.y;
        assert!(
            ink_top - row.pos.y >= 1.0,
            "the premise: the line's ink starts below its row's top ({} below)",
            ink_top - row.pos.y
        );
        let cover = panels::log_cover_bottom(&galley, at, view_top).expect("a cover");
        assert!(cover > view_top + 0.5, "the cover reaches into the view");
        assert!(
            cover <= ink_top,
            "the cover {cover} stops above the line's ink at {ink_top}"
        );
        assert!(cover.fract().abs() < 1e-4, "on a whole pixel: {cover}");
        // No whole line in the view: nothing to cover to.
        assert_eq!(
            panels::log_cover_bottom(&galley, at, galley.rect.bottom() + 10.0),
            None
        );
    }
}
