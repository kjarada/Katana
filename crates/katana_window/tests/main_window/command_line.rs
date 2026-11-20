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
        // Each error a sentence, its first word a capital as the status bar shows it
        // (command_log::shown_log_line; "! unknown command" read as a fragment, the sixth look
        // audit's H).
        assert_eq!(
            panels::shown_log(log),
            "> LAYER SET nothing\n! No layer nothing [LAYER]\n! A number was expected\n  at \
             word 3\nlayer=0 current"
        );
        // A box's failure, its title then the error ("Open Failed: NotFound: ..."), shows the
        // title as words and the sentence without its code, as the status bar does (the sixth
        // look audit's I: the code leaked; command_log::shown_log_line). A line that is not an
        // error and only mentions a code is as it was.
        assert_eq!(
            panels::shown_log("! Style: NotFound: x\n! Open Failed: NotFound: no file\nNotFound: a reply"),
            "! Style: x\n! Open failed: no file\nNotFound: a reply"
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
    fn a_warning_line_is_drawn_in_the_warning_colour_as_an_error_line_is_in_the_error_colour() {
        // The seventh look audit: a far import's advice, a warning record, was the text colour
        // and read as one more reply; errors had their colour since the third audit's H.
        use katana_window::command_log::{LogTone, log_line_colour, log_line_tone};
        use katana_window::theme::palette;
        let log = "> IMPORT far.geojson\nwarning text=\"the imported data sits far away\"\n\
                   ! NotFound: no file\nimported features=3";
        let shown = panels::shown_log(log);
        let lines: Vec<&str> = shown.split('\n').collect();
        assert_eq!(lines[1], "Warning: The imported data sits far away");
        let tones: Vec<LogTone> = lines.iter().map(|line| log_line_tone(line)).collect();
        assert_eq!(
            tones,
            [LogTone::Plain, LogTone::Warning, LogTone::Error, LogTone::Plain]
        );
        // A line end left on, as the panel's layouter cuts the text, changes nothing.
        assert_eq!(log_line_tone("Warning: x\n"), LogTone::Warning);
        assert!(log_line_colour(lines[1]) == palette::warning());
        assert!(log_line_colour(lines[2]) == palette::error());
        assert!(log_line_colour(lines[3]) == palette::text());
        // The three are told apart.
        assert!(palette::warning() != palette::error() && palette::warning() != palette::text());
    }

    #[test]
    fn a_wrapped_lines_last_row_at_the_logs_top_is_shown_not_covered() {
        // The seventh look audit (main_125pct_selection, mesh_only): scrolled to its end, the
        // log's top row was blank. It was the last row of a line wrapped over the log's top
        // ("rules, built in." of the customisation line), covered because it does not start a
        // line. A whole row shows; only a row cut through by the log's top is covered.
        let mut harness = katana_window::drive::Harness::new();
        katana_window::theme::apply(harness.context());
        let mut laid_out = None;
        harness
            .frame(|ui| {
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let job = egui::text::LayoutJob::simple(
                    String::from(
                        "Customisation: Katana Standard, 221 definitions and 1103 survey code \
                         rules, built in.\nOpened /root/project (20 entities).\n20 selected",
                    ),
                    font,
                    egui::Color32::WHITE,
                    300.0,
                );
                laid_out = Some(ui.fonts_mut(|fonts| fonts.layout_job(job)));
            })
            .drop_without_applying_deltas();
        let galley = laid_out.expect("laid out");
        assert!(
            !galley.rows[0].ends_with_newline,
            "the premise: the first line wraps"
        );
        let tail = galley
            .rows
            .iter()
            .position(|row| row.ends_with_newline)
            .expect("the first line ends");
        assert!(tail >= 1, "the premise: its last row is a row of its own");
        let at = egui::pos2(0.0, 0.0);
        // The log's top on the wrapped line's last row: nothing of its ink is covered (at most
        // the font's own space over it).
        let row = &galley.rows[tail];
        let view_top = row.pos.y;
        if let Some(cover) = panels::log_cover_bottom(&galley, at, view_top) {
            let ink_top = row.pos.y + row.visuals.mesh_bounds.min.y;
            assert!(cover <= ink_top, "the cover {cover} stops above the row's ink at {ink_top}");
        }
        // Its top a point into that row, the row cut through is covered down to the next row's
        // ink, wherever that row starts.
        let cover = panels::log_cover_bottom(&galley, at, view_top + 1.0).expect("a cover");
        let next = &galley.rows[tail + 1];
        assert!(cover <= next.pos.y + next.visuals.mesh_bounds.min.y);
        assert!(cover > view_top + 1.5);
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

    // The ninth look audit: the log, three rows high, showed only the end of a long record - a
    // far import's warning started mid-sentence at its top. Following its newest output, the log
    // shows a record taller than itself from the record's first row.
    //
    // Worked by hand for a log of 14-point rows three rows high: the view is 3 x 14 and the text
    // box's top margin, 44; the content is its rows and the margin above and below, 14 R + 4; a
    // row r's top in the content is 2 + 14 r.
    #[test]
    fn the_log_follows_a_record_taller_than_itself_from_the_records_first_row() {
        let view = 3.0 * 14.0 + panels::LOG_MARGIN;
        let content = |rows: f32| rows * 14.0 + 2.0 * panels::LOG_MARGIN;
        let row_top = |row: f32| panels::LOG_MARGIN + 14.0 * row;
        // Ten rows, the newest record the last two: at its end, 144 - 44, as it always was - the
        // last row's foot at the view's foot.
        assert_eq!(
            panels::log_follow_offset(0.0, true, Some(row_top(8.0)), content(10.0), view),
            100.0
        );
        // The newest record the last three, just the view's height: at its end too, where all
        // three show.
        assert_eq!(
            panels::log_follow_offset(0.0, true, Some(row_top(7.0)), content(10.0), view),
            100.0
        );
        // The newest record the last five, rows 5 to 9: row 5 at the view's top, the margin
        // above it - 14 x 5 - so rows 5, 6 and 7 show whole (row 7's foot at 2 + 14 x 8 - 70,
        // the view's 44) and the record is read from its start.
        let offset = panels::log_follow_offset(0.0, true, Some(row_top(5.0)), content(10.0), view);
        assert_eq!(offset, 70.0);
        assert_eq!(row_top(5.0) - offset, panels::LOG_MARGIN);
        assert_eq!(row_top(8.0) - offset, view);
        // The whole log one record of ten rows: from its top.
        assert_eq!(
            panels::log_follow_offset(55.0, true, Some(row_top(0.0)), content(10.0), view),
            0.0
        );
        // A row's place rounded to a pixel is not a taller record: a quarter point above the
        // end is at the end.
        assert_eq!(
            panels::log_follow_offset(0.0, true, Some(99.75), content(10.0), view),
            100.0
        );
    }

    #[test]
    fn an_empty_log_and_one_shorter_than_its_view_are_not_scrolled() {
        let view = 3.0 * 14.0 + panels::LOG_MARGIN;
        // Nothing logged: the text box's three empty rows and its margins, 46, two points more
        // than the view - at its end, which is where there is no record to start from.
        assert_eq!(panels::log_follow_offset(0.0, true, None, 46.0, view), 2.0);
        // Content shorter than the view has no end to scroll to.
        assert_eq!(panels::log_follow_offset(7.0, true, None, 30.0, view), 0.0);
        assert_eq!(
            panels::log_follow_offset(7.0, true, Some(panels::LOG_MARGIN), 30.0, view),
            0.0
        );
    }

    #[test]
    fn a_log_a_person_has_scrolled_stays_where_they_left_it() {
        let view = 3.0 * 14.0 + panels::LOG_MARGIN;
        // Not following: the offset as it is, whatever the newest record - one taller than the
        // view, one shorter, none.
        for record_top in [Some(72.0), Some(114.0), None] {
            assert_eq!(
                panels::log_follow_offset(30.0, false, record_top, 144.0, view),
                30.0
            );
        }
    }

    #[test]
    fn a_records_first_row_is_found_by_where_the_record_starts_in_the_text() {
        let mut harness = katana_window::drive::Harness::new();
        katana_window::theme::apply(harness.context());
        let mut laid_out = None;
        // The first line wraps at 300 points; then "bb" and "ccc" a row each.
        let long = "Customisation: Katana Standard, 221 definitions and 1103 survey code rules, \
                    built in.";
        let text = format!("{long}\nbb\nccc");
        harness
            .frame(|ui| {
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let job = egui::text::LayoutJob::simple(
                    text.clone(),
                    font,
                    egui::Color32::WHITE,
                    300.0,
                );
                laid_out = Some(ui.fonts_mut(|fonts| fonts.layout_job(job)));
            })
            .drop_without_applying_deltas();
        let galley = laid_out.expect("laid out");
        let tail = galley
            .rows
            .iter()
            .position(|row| row.ends_with_newline)
            .expect("the first line ends");
        assert!(tail >= 1, "the premise: the first line wraps");
        assert_eq!(galley.rows.len(), tail + 3, "the premise: then a row a line");
        let top = |row: usize| Some(galley.rows[row].pos.y);
        let chars = long.chars().count();
        // The text's start is its first row, and so is any character of that row.
        assert_eq!(panels::log_record_top(&galley, 0), top(0));
        assert_eq!(panels::log_record_top(&galley, 1), top(0));
        // The last character of the wrapped line is on the line's last row, not its first.
        assert_eq!(panels::log_record_top(&galley, chars - 1), top(tail));
        // A record starting after the wrapped line's end - its characters and the line end -
        // starts on the row after the line's last.
        assert_eq!(panels::log_record_top(&galley, chars + 1), top(tail + 1));
        // "bb" and its line end further on: the last row.
        assert_eq!(panels::log_record_top(&galley, chars + 4), top(tail + 2));
        // At the text's end, and past it: the last row.
        assert_eq!(panels::log_record_top(&galley, chars + 7), top(tail + 2));
        assert_eq!(panels::log_record_top(&galley, chars + 700), top(tail + 2));
    }

    /// The window's log as drawn once idle: the text box's rectangle and the view it scrolls in.
    fn log_drawn(window: &mut MainWindow) -> (egui::Rect, egui::Rect) {
        let mut headless = Headless::new();
        headless
            .settle(window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        let record = headless
            .snapshot()
            .control("CommandLineDock", "commandLog")
            .expect("the log is drawn")
            .clone();
        (record.rect, record.clip)
    }

    #[test]
    fn the_window_shows_a_long_newest_record_from_its_first_row_and_a_short_one_at_the_logs_end() {
        // Twelve short records, a row each, so that the log scrolls; then one record of
        // twelve lines, none long enough to wrap.
        const LINES: usize = 12;
        let short = |window: &mut MainWindow| {
            for at in 0..LINES {
                window.exec.log.append(&format!("short {at}"));
            }
        };
        let mut before = MainWindow::new();
        short(&mut before);
        let (short_rect, short_view) = log_drawn(&mut before);
        // A short newest record: the log at its end, the text box's foot at the view's.
        assert!(
            (short_rect.bottom() - short_view.bottom()).abs() <= 1.0,
            "the log's foot at {} in a view that ends at {}",
            short_rect.bottom(),
            short_view.bottom()
        );

        let mut window = MainWindow::new();
        short(&mut window);
        let record: Vec<String> = (0..LINES).map(|at| format!("line {at}")).collect();
        window.exec.log.append(&record.join("\n"));
        let (rect, view) = log_drawn(&mut window);
        // A row's height, from the two logs: the second is the first and twelve rows more.
        let line = (rect.height() - short_rect.height()) / LINES as f32;
        assert!(line > 4.0, "the premise: the record added {LINES} rows of {line}");
        assert!(
            view.height() < line * LINES as f32,
            "the premise: the record ({} rows of {line}) is taller than the log ({})",
            LINES,
            view.height()
        );
        // The record is the text's last twelve rows, over the box's bottom margin.
        let record_top = rect.bottom() - panels::LOG_MARGIN - line * LINES as f32;
        let expected = view.top() + panels::LOG_MARGIN;
        assert!(
            (record_top - expected).abs() <= 1.5,
            "the record's first row at {record_top}, the view's first row at {expected}: at its \
             end the log would have it {} above",
            line * LINES as f32 + panels::LOG_MARGIN - view.height()
        );
    }
}
