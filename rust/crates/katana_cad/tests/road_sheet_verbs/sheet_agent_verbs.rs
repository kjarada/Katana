// The sheet verbs an agent drives every sheet feature with
// (tests/cad/plotting/test_sheet_agent_verbs.cpp; plotting/sheet_verbs.rs,
// docs/plotting.md "Sheets on the command line"): the drawing register and
// revision table as views, a legend's scope, a plan's grid, key plans,
// GENERATE register, the page setup, SHEETS CHECK, ARRANGE, VIEW FIT and
// BESTROTATE, SHEET SUGGESTPAPER and a section's automatic scale. Each edit is
// one undo step and a refused one is none; each reply is in the words the
// options take.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use katana_cad::plotting::preflight::findings_from_json;
use katana_cad::plotting::sheet_verbs::{is_sheet_verb, sheet_verb_help};
use katana_cad::plotting::tables::{REGISTER_SHEET_NAME, drawing_register};
use katana_cad::plotting::{
    Finding, GridStyle, LegendScope, Severity, SheetVerbContext, Viewport, ViewportKind,
};
use katana_cad::terrain::surface_style::built_in_surface_styles;
use katana_cad::{Document, PaperSize, PlotColourMode, SurfaceContours, SurfaceDisplayStyle};
use katana_core::ErrorCode;
use katana_geometry::{Box2, Point3};
use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

use crate::{Session, contains, double_eq, must, near};

// The view at `index` on the sheet at `sheet`, as the set holds it now.
#[track_caller]
fn view(session: &Session, sheet: usize, index: usize) -> Viewport {
    let set = session.set();
    match set
        .sheets
        .get(sheet)
        .and_then(|each| each.viewports.get(index))
    {
        Some(viewport) => viewport.clone(),
        None => panic!("no view {index} on sheet {sheet}"),
    }
}

// The lines of `text`, split at every "\n" (an empty last one kept).
fn lines_of(text: &str) -> Vec<String> {
    text.split('\n').map(String::from).collect()
}

// A drawing 400 x 100 m: a line along its long side and one across, so a
// plan of it has something to fit and to turn.
#[track_caller]
fn draw_road(s: &mut Session) {
    s.ok("LINE 0,0 400,0");
    s.ok("LINE 0,0 0,100");
    s.ok("LINE 400,0 400,100");
}

// `ground` in the drawing in style Fine, shown or hidden.
fn store_of(ground: &Arc<TinSurface>, visible: bool) -> SurfaceStore {
    let mut store = SurfaceStore::new();
    must(store.add(NamedSurface {
        name: String::from("ground"),
        surface: Some(Arc::clone(ground)),
        source: String::from("test"),
        style: String::from("Fine"),
        visible,
    }));
    store
}

mod sheet_agent_verbs {
    use super::*;

    // ---- the drawing register and the revision table -------------------------------------

    #[test]
    fn the_register_and_revision_table_are_views_like_any_other() {
        let mut s = Session::default();
        s.ok("SHEET NEW COVER");
        let before = s.steps();
        let added = s.ok("VIEW ADD 1 register");
        assert_eq!(s.steps(), before + 1);
        assert!(contains(&added, "kind=sheet_index"), "{added}");
        assert_eq!(view(&s, 0, 0).kind, ViewportKind::SheetIndex);
        assert!(!view(&s, 0, 0).rect.empty(), "placed on the sheet");
        let table = s.ok("VIEW ADD 1 revision_table");
        assert!(
            contains(&table, "kind=revisions") && contains(&table, "revisions=all"),
            "{table}"
        );
        assert!(contains(
            &s.ok("VIEW ADD 1 sheet_index"),
            "kind=sheet_index"
        ));

        // The table shows the newest so many: revisions=n, all for every one.
        let id = view(&s, 0, 1).id;
        assert!(contains(
            &s.ok(&format!("VIEW SET {id} revisions=3")),
            "revisions=3"
        ));
        assert_eq!(view(&s, 0, 1).revision_limit, 3);
        assert!(contains(
            &s.ok(&format!("VIEW SET {id} revisions=all")),
            "revisions=all"
        ));
        assert_eq!(view(&s, 0, 1).revision_limit, 0);
        let steps = s.steps();
        let register = view(&s, 0, 0).id;
        assert!(contains(
            &s.refused(&format!("VIEW SET {register} revisions=2"))
                .message,
            "revisions"
        ));
        assert_eq!(s.steps(), steps, "a refusal is no step");
    }

    #[test]
    fn generate_register_puts_a_cover_first_in_one_step() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN ONE");
        s.ok("SHEET NEW PLAN TWO");
        let before = s.steps();
        let reply = s.ok("GENERATE register paper=A3");
        assert_eq!(s.steps(), before + 1);
        assert!(
            contains(&reply, "generated 1 sheet: 1 (the drawing register)"),
            "{reply}"
        );
        let set = s.set();
        assert_eq!(set.sheets.len(), 3);
        assert_eq!(set.sheets[0].name, REGISTER_SHEET_NAME);
        assert_eq!(set.sheets[1].name, "PLAN ONE");
        assert_eq!(drawing_register(&set).len(), 3);
        // One register is enough; and it replaces nothing.
        assert_eq!(
            s.refused("GENERATE register").code,
            ErrorCode::AlreadyExists
        );
        assert!(contains(
            &s.refused("GENERATE register replace=on").message,
            "replaces no sheets"
        ));
        assert!(s.document.undo().is_ok());
        assert_eq!(s.set().sheets.len(), 2);
    }

    // ---- the legend, the grid, the key plan ------------------------------------------------

    #[test]
    fn a_legends_scope_and_a_plans_grid_are_view_options() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        s.ok("VIEW ADD 1 plan scale=500 centre=0,0");
        s.ok("VIEW ADD 1 legend");
        let steps = s.steps();
        assert!(contains(
            &s.ok("VIEW SET vp2 legend=whole_set"),
            "legend=whole_set"
        ));
        assert_eq!(view(&s, 0, 1).legend_scope, LegendScope::WholeSet);
        assert_eq!(s.steps(), steps + 1);
        assert!(contains(
            &s.ok("VIEW SET vp2 scope=drawing"),
            "legend=whole_drawing"
        ));
        assert!(contains(
            &s.ok("VIEW SET vp2 legend=this_sheet"),
            "legend=this_sheet"
        ));
        assert!(contains(
            &s.refused("VIEW SET vp2 legend=everywhere").message,
            "this_sheet"
        ));
        assert!(contains(
            &s.refused("VIEW SET vp1 legend=whole_set").message,
            "legend"
        ));

        let steps = s.steps();
        let gridded = s.ok("VIEW SET vp1 grid=crosses gridinterval=50");
        assert!(
            contains(&gridded, "grid=crosses gridinterval=50"),
            "{gridded}"
        );
        assert_eq!(s.steps(), steps + 1, "both options, one step");
        assert_eq!(view(&s, 0, 0).grid_style, GridStyle::Crosses);
        double_eq(view(&s, 0, 0).grid_interval, 50.0);
        assert!(contains(
            &s.ok("VIEW SET vp1 grid=lines gridinterval=auto"),
            "grid=lines gridinterval=auto"
        ));
        assert!(contains(&s.ok("VIEW SET vp1 grid=off"), "grid=none"));
        let steps = s.steps();
        assert!(contains(
            &s.refused("VIEW SET vp1 grid=dots").message,
            "ticks"
        ));
        assert!(contains(
            &s.refused("VIEW SET vp1 gridinterval=-5").message,
            "gridinterval"
        ));
        assert!(contains(
            &s.refused("VIEW SET vp2 grid=ticks").message,
            "plan"
        ));
        assert_eq!(s.steps(), steps);
    }

    #[test]
    fn a_key_plan_takes_a_grid_and_stores_no_outlines() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        s.ok("VIEW ADD 1 plan scale=500 centre=0,0");
        s.ok("SHEET NEW KEY");
        let added = s.ok("VIEW ADD 2 keyplan grid=ticks");
        assert!(
            contains(&added, "kind=key_plan") && contains(&added, "scale=auto centre=auto"),
            "{added}"
        );
        assert!(contains(&added, "grid=ticks"));
        assert!(view(&s, 1, 0).marks.is_empty(), "outlined live, when drawn");
    }

    // ---- the page setup --------------------------------------------------------------------

    #[test]
    fn the_page_setup_is_printed_and_set_in_one_step() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        assert_eq!(
            s.ok("SHEETS PAGESETUP"),
            "pagesetup style=colour lineweight=1 dpi=300 pattern=\"{set}{n:02} {name}\" \
             filepersheet=off"
        );
        let steps = s.steps();
        assert_eq!(
            s.ok(
                "SHEETS PAGESETUP style=mono lineweight=0.7 dpi=150 pattern=\"{n} {name}\" \
                 filepersheet=on"
            ),
            "pagesetup style=monochrome lineweight=0.7 dpi=150 pattern=\"{n} {name}\" \
             filepersheet=on"
        );
        assert_eq!(s.steps(), steps + 1);
        assert_eq!(s.set().page_setup.colour_mode, PlotColourMode::Monochrome);
        assert!(s.set().page_setup.file_per_sheet);
        assert!(contains(
            &s.ok("SHEETS PAGESETUP style=grey"),
            "style=greyscale"
        ));
        // Refused whole, naming what: nothing changes.
        assert!(s.run("SHEETS PAGESETUP dpi=5").is_err());
        assert!(s.run("SHEETS PAGESETUP style=sepia").is_err());
        assert!(s.run("SHEETS PAGESETUP pattern={nope}").is_err());
        assert!(s.run("SHEETS PAGESETUP paper=A1").is_err());
        assert_eq!(s.steps(), steps + 2);
        double_eq(s.set().page_setup.dpi, 150.0);
    }

    // ---- SHEETS CHECK ----------------------------------------------------------------------

    #[test]
    fn sheets_check_prints_a_finding_a_line_an_agent_can_split() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        s.ok("VIEW ADD 1 plan scale=437 centre=5000,5000");
        s.ok("SHEET NEW EMPTY");
        let reply = s.ok("SHEETS CHECK");
        let lines = lines_of(&reply);
        assert!(lines.len() >= 2, "{reply}");
        assert!(lines[0].starts_with("checked 2 sheets: "), "{}", lines[0]);
        let mut empty = false;
        let mut non_standard = false;
        let mut set_level = false;
        for line in &lines[1..] {
            // severity code sheet view message="..."
            assert!(
                line.starts_with("error ")
                    || line.starts_with("warning ")
                    || line.starts_with("info "),
                "{line}"
            );
            assert!(contains(line, " message=\""), "{line}");
            empty = empty || line.starts_with("warning sheet.empty 2 - message=");
            non_standard =
                non_standard || line.starts_with("info scale.non-standard 1 vp1 message=");
            set_level = set_level || contains(line, " field.empty - - message=");
        }
        assert!(empty, "{reply}");
        assert!(non_standard, "{reply}");
        assert!(set_level, "{reply}");

        // One sheet, and as JSON; checking changes nothing.
        let steps = s.steps();
        let one = s.ok("SHEETS CHECK sheets=2");
        assert!(one.starts_with("checked 1 sheet: "), "{one}");
        assert!(!contains(&one, "scale.non-standard"));
        let findings = must(findings_from_json(&s.ok("SHEETS CHECK json")));
        assert_eq!(findings.len(), lines.len() - 1);
        assert_eq!(s.steps(), steps);
        assert!(s.run("SHEETS CHECK sheets=9").is_err());

        // The window's own checker, when it gives one.
        let asked = Rc::new(Cell::new(false));
        let answered = Rc::clone(&asked);
        s.interpreter.set_sheet_context(Some(Box::new(
            move |_document: &Document| -> SheetVerbContext<'static> {
                let answered = Rc::clone(&answered);
                SheetVerbContext {
                    check: Some(Box::new(
                        move |_document: &Document, sheets: &[usize]| -> Vec<Finding> {
                            answered.set(sheets.len() == 1 && sheets[0] == 0);
                            vec![Finding {
                                severity: Severity::Error,
                                code: String::from("viewport.outside"),
                                sheet_index: Some(0),
                                sheet_id: String::from("s1"),
                                viewport_id: String::from("vp1"),
                                subject: String::new(),
                                message: String::from("off the paper"),
                                fix: String::from("move it"),
                            }]
                        },
                    )),
                    ..SheetVerbContext::default()
                }
            },
        )));
        assert_eq!(
            s.ok("SHEETS CHECK sheets=1"),
            "checked 1 sheet: 1 error\nerror viewport.outside 1 vp1 message=\"off the paper\" \
             fix=\"move it\""
        );
        assert!(asked.get());
    }

    // A plot is warned, before the paper, of a surface it will print without its
    // contours: the plan painter refuses an interval that crosses the triangles
    // more than four million times, and says so only in the sheet's problems. An
    // agent reads the same warning from SHEETS CHECK (docs/plotting.md,
    // "Preflight checks"), headless, with no painter.
    #[test]
    fn sheets_check_warns_of_a_surface_whose_contours_a_too_fine_interval_would_leave_out_of_the_plot()
     {
        let mut s = Session::default();
        // The 10 x 10 square z = x: each of its two triangles runs from 0 to 10,
        // so an interval of a millionth crosses it ten million times a triangle,
        // past the four million the plan allows.
        let ground = Arc::new(must(TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 10.0),
                Point3::new(10.0, 10.0, 10.0),
                Point3::new(0.0, 10.0, 0.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Vec::new(),
        )));
        let mut styles = built_in_surface_styles();
        styles.styles.push(SurfaceDisplayStyle {
            name: String::from("Fine"),
            contours: SurfaceContours {
                show: true,
                interval: 1.0e-6,
                ..SurfaceContours::default()
            },
            ..SurfaceDisplayStyle::default()
        });
        must(
            s.document
                .set_surface_styles(styles.clone(), Document::SET_SURFACE_STYLES),
        );
        must(
            s.document
                .set_surfaces(store_of(&ground, true), Document::SET_SURFACES),
        );
        s.ok("SHEET NEW PLAN");
        s.ok("VIEW ADD 1 plan scale=500 centre=5,5");

        let reply = s.ok("SHEETS CHECK");
        let mut found = String::new();
        for line in lines_of(&reply) {
            if line.starts_with("warning plan.surface-style 1 vp1 message=") {
                found = line;
            }
        }
        assert!(!found.is_empty(), "{reply}");
        assert!(
            contains(
                &found,
                "the surface ground (style Fine) without part of its style: contours: an interval \
                 of 0.000001 crosses this surface's triangles more than 4000000 times"
            ),
            "{found}"
        );
        assert!(found.ends_with(" subject=\"ground\""), "{found}");

        // As JSON, the same finding, for an agent that wants the fields.
        let findings = must(findings_from_json(&s.ok("SHEETS CHECK json")));
        let Some(at) = findings
            .iter()
            .find(|finding| finding.code == "plan.surface-style")
        else {
            panic!("no plan.surface-style finding");
        };
        assert_eq!(at.subject, "ground");
        assert_eq!(at.viewport_id, "vp1");

        // Mended, nothing is said; hidden, the surface is not drawn and nothing is said.
        if let Some(fine) = styles.styles.last_mut() {
            fine.contours.interval = 1.0;
        }
        must(
            s.document
                .set_surface_styles(styles.clone(), Document::SET_SURFACE_STYLES),
        );
        assert!(!contains(&s.ok("SHEETS CHECK"), "plan.surface-style"));
        if let Some(fine) = styles.styles.last_mut() {
            fine.contours.interval = 1.0e-6;
        }
        must(
            s.document
                .set_surface_styles(styles, Document::SET_SURFACE_STYLES),
        );
        assert!(contains(&s.ok("SHEETS CHECK"), "plan.surface-style"));
        must(
            s.document
                .set_surfaces(store_of(&ground, false), Document::SET_SURFACES),
        );
        assert!(!contains(&s.ok("SHEETS CHECK"), "plan.surface-style"));
    }

    // ---- ARRANGE -----------------------------------------------------------------------------

    #[test]
    fn arrange_align_distribute_and_match_scale_are_each_one_step() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLANS");
        s.ok("VIEW ADD 1 plan rect=30,40,130,120 scale=500 centre=0,0");
        s.ok("VIEW ADD 1 plan rect=100,60,200,140 scale=1000 centre=0,0");
        s.ok("VIEW ADD 1 plan rect=300,50,380,110 scale=250 centre=0,0");

        let steps = s.steps();
        let arranged = s.ok("ARRANGE 1");
        assert!(
            arranged.starts_with("arranged sheet 1 moved="),
            "{arranged}"
        );
        assert!(contains(&arranged, "overlapping= unplaced="), "{arranged}");
        assert_eq!(s.steps(), steps + 1);
        let a: Box2 = view(&s, 0, 0).rect;
        let b: Box2 = view(&s, 0, 1).rect;
        assert!(
            !(a.min.x < b.max.x - 0.5
                && b.min.x < a.max.x - 0.5
                && a.min.y < b.max.y - 0.5
                && b.min.y < a.max.y - 0.5),
            "vp1 and vp2 no longer overlap"
        );

        let steps = s.steps();
        let aligned = s.ok("ARRANGE ALIGN top vp2 vp3");
        assert!(
            aligned.starts_with("aligned vp2,vp3 on sheet 1 edge=top moved="),
            "{aligned}"
        );
        double_eq(view(&s, 0, 1).rect.max.y, view(&s, 0, 2).rect.max.y);
        assert_eq!(s.steps(), steps + 1);

        let steps = s.steps();
        let distributed = s.ok("ARRANGE DISTRIBUTE across vp1 vp2 vp3");
        assert!(
            distributed.starts_with("distributed vp1,vp2,vp3 on sheet 1 axis=horizontal"),
            "{distributed}"
        );
        assert!(s.steps() <= steps + 1);

        let steps = s.steps();
        let matched = s.ok("ARRANGE MATCHSCALE vp1 vp2 vp3");
        assert!(
            matched.starts_with("matched vp2,vp3 to vp1 scale=500 changed=vp2,vp3"),
            "{matched}"
        );
        double_eq(view(&s, 0, 1).scale, 500.0);
        double_eq(view(&s, 0, 2).scale, 500.0);
        assert_eq!(s.steps(), steps + 1);

        // Refused whole: an edge it does not know, a view that is not there,
        // views on two sheets.
        s.ok("SHEET NEW OTHER");
        s.ok("VIEW ADD 2 plan");
        let steps = s.steps();
        assert!(contains(
            &s.refused("ARRANGE ALIGN diagonal vp1 vp2").message,
            "left, right"
        ));
        assert_eq!(
            s.refused("ARRANGE ALIGN left vp1 vp99").code,
            ErrorCode::NotFound
        );
        assert!(contains(
            &s.refused("ARRANGE ALIGN left vp1 vp4").message,
            "a sheet at a time"
        ));
        assert_eq!(s.steps(), steps);
    }

    // ---- VIEW FIT, VIEW BESTROTATE, SHEET SUGGESTPAPER ---------------------------------------

    #[test]
    fn view_fit_and_best_rotate_fit_a_plan_to_what_it_shows() {
        let mut s = Session::default();
        draw_road(&mut s);
        s.ok("SHEET NEW PLAN");
        s.ok("VIEW ADD 1 plan rect=30,40,330,280 scale=1000 centre=200,50");
        let steps = s.steps();
        let fitted = s.ok("VIEW FIT vp1");
        assert!(fitted.starts_with("fitted vp1\nview id=vp1"), "{fitted}");
        assert_eq!(s.steps(), steps + 1);
        // 400 x 100 m at 1:1000 is 400 x 100 mm: the rectangle takes the drawing's shape.
        let rect = view(&s, 0, 0).rect;
        assert!(
            rect.width() / rect.height() > 2.5,
            "{} x {}",
            rect.width(),
            rect.height()
        );

        // A long thin drawing turned 30 degrees: the best rotation lies it along the paper.
        s.ok("VIEW SET vp1 rotation=30 rect=30,40,330,280");
        let steps = s.steps();
        let turned = s.ok("VIEW BESTROTATE vp1");
        assert!(turned.starts_with("rotated vp1 rotation="), "{turned}");
        assert_eq!(s.steps(), steps + 1);
        near(
            view(&s, 0, 0).rotation.abs() % (3.14159265358979 / 2.0),
            0.0,
            1e-6,
        );

        s.ok("VIEW ADD 1 notes");
        assert!(contains(
            &s.refused("VIEW FIT vp2").message,
            "plans and key plans"
        ));
        assert_eq!(s.refused("VIEW BESTROTATE vp9").code, ErrorCode::NotFound);
    }

    #[test]
    fn suggest_paper_says_the_paper_and_applies_it_only_when_asked() {
        let mut s = Session::default();
        draw_road(&mut s);
        s.ok("SHEET NEW PLAN paper=A4");
        s.ok("VIEW ADD 1 plan scale=500 centre=200,50");
        let steps = s.steps();
        let advice = s.ok("SHEET SUGGESTPAPER 1");
        // 400 m at 1:500 is 800 mm across: more than A1's 774 mm of room, so A0.
        assert!(
            advice.starts_with("sheet 1 view=vp1 scale=500 paper=A0 orientation=landscape"),
            "{advice}"
        );
        assert_eq!(s.steps(), steps, "advice alone changes nothing");
        assert_eq!(s.set().sheets[0].paper, PaperSize::A4);
        assert!(contains(
            &s.ok("SHEET SUGGESTPAPER 1 scale=1000"),
            "paper=A2"
        ));

        let applied = s.ok("SHEET SUGGESTPAPER 1 apply=on");
        assert!(contains(&applied, "paper=A0"), "{applied}");
        assert_eq!(s.steps(), steps + 1);
        assert_eq!(s.set().sheets[0].paper, PaperSize::A0);

        s.ok("SHEET NEW NOTES");
        s.ok("VIEW ADD 2 notes");
        assert!(contains(
            &s.refused("SHEET SUGGESTPAPER 2").message,
            "no plan"
        ));
    }

    // ---- sections ----------------------------------------------------------------------------

    #[test]
    fn a_section_takes_an_automatic_scale() {
        let mut s = Session::default();
        s.ok("ALIGN NEW ROAD 0,0 200,0");
        s.ok("SHEET NEW SECTIONS");
        s.ok("VIEW ADD 1 long_section");
        assert!(contains(&s.ok("VIEW SET vp1 scale=auto"), "scale=auto"));
        assert!(view(&s, 0, 0).auto_scale);
        assert!(contains(&s.ok("VIEW SET vp1 scale=500"), "scale=500"));
        assert!(!view(&s, 0, 0).auto_scale);
    }

    // ---- HELP --------------------------------------------------------------------------------

    #[test]
    fn help_names_every_verb_and_option() {
        let help = sheet_verb_help();
        for word in [
            "SHEETS CHECK",
            "SHEETS PAGESETUP",
            "SHEET SUGGESTPAPER",
            "VIEW FIT",
            "VIEW BESTROTATE",
            "ARRANGE n",
            "ARRANGE ALIGN",
            "ARRANGE DISTRIBUTE",
            "ARRANGE MATCHSCALE",
            "register",
            "sheet_index",
            "revisions=",
            "legend=this_sheet",
            "grid=none",
            "gridinterval=",
            "PLOTSHEETS",
            "format=",
            "style=",
            "folder=",
            "pattern=",
        ] {
            assert!(contains(&help, word), "{word}");
        }
        assert!(is_sheet_verb("arrange"));
        let mut s = Session::default();
        assert_eq!(s.ok("HELP ARRANGE"), help);
    }
}
