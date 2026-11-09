//! tests/cad/annotation/test_annotation_verbs.cpp: the annotation verbs of the
//! command line (annotation/annotation_verbs.rs, docs/annotation.md "The
//! command line"), driven as an agent drives them: key=value options,
//! key=value replies, one undo step per edit.

use katana_cad::CommandInterpreter;
use katana_entity::{
    ArrowHead, CalloutShape, DimensionGeometry, DimensionKind, EntityId, Geometry, TextGeometry,
    TextJustify,
};
use katana_math::DEG_TO_RAD;

use crate::{Session, double_eq, leading_number, near};

#[track_caller]
fn text_of(s: &Session, id: EntityId) -> TextGeometry {
    let entity = s.entity(id);
    match entity.geometry {
        Geometry::Text(text) => text,
        other => panic!("not a text: {other:?}"),
    }
}

#[track_caller]
fn dimension_of(s: &Session, id: EntityId) -> DimensionGeometry {
    let entity = s.entity(id);
    match entity.geometry {
        Geometry::Dimension(dimension) => dimension,
        other => panic!("not a dimension: {other:?}"),
    }
}

// The id a "created dimension id=N ..." reply names: the text from its first
// "id=" to the blank after it.
#[track_caller]
fn id_field(reply: &str) -> String {
    let Some(at) = reply.find("id=") else {
        panic!("no id= in {reply:?}");
    };
    let rest = &reply[at + 3..];
    String::from(rest.split(' ').next().unwrap_or(rest))
}

mod annotation_verbs {
    use super::*;

    #[test]
    fn annoscale_reads_sets_and_undoes() {
        let mut s = Session::default();
        assert_eq!(s.run("ANNOSCALE"), "annoscale=1000");
        assert_eq!(s.run("ANNOSCALE 1:500"), "annoscale=500");
        double_eq(s.document.annotation_scale(), 500.0);
        assert_eq!(s.steps(), 1);
        assert_eq!(s.run("ANNOSCALE 500"), "annoscale=500");
        assert_eq!(s.steps(), 1, "the same scale again is no step");
        s.run("UNDO");
        double_eq(s.document.annotation_scale(), 1000.0);
        assert!(
            s.document.metadata().unknown_keys.is_empty(),
            "the default is no key at all"
        );
        assert!(s.refused("ANNOSCALE -5"));
        assert!(s.refused("ANNOSCALE 2:500"));
    }

    #[test]
    fn text_styles_are_made_set_listed_and_deleted() {
        let mut s = Session::default();
        let made = s.run(concat!(
            "TEXTSTYLE NEW Road font=\"DejaVu Sans\" paper=3.5 width=0.8 oblique=15 bold=on ",
            "mask=on margin=0.75 colour=#FF0000"
        ));
        assert!(made.contains("name=Road"), "{made}");
        let Some(road) = s.document.model().text_styles.find("Road").cloned() else {
            panic!("no text style Road");
        };
        assert_eq!(road.font_family, "DejaVu Sans");
        double_eq(road.paper_height, 3.5);
        near(road.oblique, 15.0 * DEG_TO_RAD, 1e-15);
        assert!(road.bold);
        assert!(road.color.is_some());
        s.run("TEXTSTYLE SET Road colour=bylayer readable=off");
        assert!(
            s.document
                .model()
                .text_styles
                .find("Road")
                .is_some_and(|style| style.color.is_none())
        );
        let list = s.run("TEXTSTYLE LIST");
        assert!(
            list.contains("name=Road font=\"DejaVu Sans\" paper=3.5"),
            "{list}"
        );
        assert!(list.contains("name=Standard"));
        assert!(
            s.refused("TEXTSTYLE NEW Bad widht=2"),
            "an unknown option is an error"
        );
        assert!(s.refused("TEXTSTYLE DELETE Standard"));
        s.run("TEXTSTYLE DELETE Road");
        assert!(s.document.model().text_styles.find("Road").is_none());
    }

    #[test]
    fn styled_text_is_paper_sized_at_the_annotation_scale() {
        let mut s = Session::default();
        s.run("ANNOSCALE 200");
        s.run("TEXTSTYLE NEW Notes paper=5");
        let reply = s.run("MTEXT 10,20 \"LOT 7\\nDP 1234\" style=Notes justify=MC");
        assert!(reply.contains("created text id="), "{reply}");
        let ids = s.document.last_created_entities();
        assert_eq!(ids.len(), 1);
        let text = text_of(&s, ids[0]);
        assert_eq!(text.text, "LOT 7\nDP 1234");
        assert_eq!(text.justify, TextJustify::MiddleCentre);
        // 5 mm at 1:200 is 1 m: its box at this scale.
        double_eq(text.height, 1.0);

        // The plain TEXT verb is unchanged: a model-unit text, no style.
        s.run("TEXT 0,0 2.5 \"plain\"");
        let plain = text_of(&s, s.last());
        assert!(plain.style.is_empty());
        double_eq(plain.paper_height, 0.0);

        // With an option it is styled; TEXTEDIT changes it as one step.
        s.run("TEXT 0,0 \"styled\" paper=2 rotation=90");
        let styled = s.last();
        let before = s.steps();
        s.run(&format!("TEXTEDIT {styled} justify=TR text=\"re worded\""));
        assert_eq!(s.steps(), before + 1);
        let edited = text_of(&s, styled);
        assert_eq!(edited.text, "re worded");
        assert_eq!(edited.justify, TextJustify::TopRight);
        assert!(s.refused("TEXT 0,0 \"x\" style=Nowhere"));
    }

    #[test]
    fn label_styles_labels_and_their_layout() {
        let mut s = Session::default();
        let defaults = s.run("LABELSTYLE DEFAULTS");
        assert!(defaults.contains("added=7"), "{defaults}");
        assert_eq!(s.steps(), 1, "the whole standard set is one step");
        assert_eq!(s.run("LABELSTYLE DEFAULTS"), "added=0");
        assert!(s.run("LABELSTYLE VALUES segment").contains("bearing"));
        assert_eq!(
            s.run("LABELSTYLE CHECK area \"{area:ha:.4f} ha\""),
            "valid=yes"
        );
        assert!(s.refused("LABELSTYLE CHECK area \"{bearing}\""));
        s.run(
            "LABELSTYLE NEW Short kind=segment text=\"{distance:.2f}\" placement=below priority=4",
        );
        assert!(
            s.refused("LABELSTYLE SET Short kind=point"),
            "the kind is fixed"
        );

        s.run("PLINE 0,0 30,0 30,40 CLOSE");
        let lot = s.last();
        let created = s.run(&format!("LABEL {lot} style=\"Bearing Distance\""));
        assert!(created.contains("created labels=1"), "{created}");
        let list = s.run("LABEL LIST");
        // (0,0)->(30,0) east 30; (30,0)->(30,40) north 40; back: 50 at 216 52 12.
        assert!(list.contains("90°00'00\\\" 30.000"), "{list}");
        assert!(list.contains("216°52'12\\\" 50.000"), "{list}");

        let layout = s.run("LABEL LAYOUT scale=500");
        assert!(layout.contains("placed=3"), "{layout}");
        assert!(layout.contains("suppressed=0"), "{layout}");

        // The label follows its polyline: a vertex moves, the text changes.
        s.run(&format!("SELECT {lot}"));
        s.run("MOVE 100,0");
        assert!(s.run("LABEL LIST").contains("30.000"));
        assert!(
            s.refused(&format!("LABEL {lot} style=\"Spot Level\"")),
            "a point style cannot label a polyline"
        );
    }

    #[test]
    fn many_labels_are_one_step() {
        let mut s = Session::default();
        s.run("LABELSTYLE DEFAULTS");
        s.run("POINT 0,0");
        let a = s.last();
        s.run("POINT 10,0");
        let b = s.last();
        s.run(&format!("SELECT {a} {b}"));
        s.run("PROP SET point P");
        let before = s.steps();
        let reply = s.run("LABEL SELECTION style=\"Point Number\"");
        assert!(reply.contains("labels=2"), "{reply}");
        assert_eq!(s.steps(), before + 1);
        s.run("UNDO");
        assert_eq!(s.run("LABEL LIST"), "");
    }

    #[test]
    fn auto_label_rules_run_preview_and_clear() {
        let mut s = Session::default();
        s.run("LABELSTYLE DEFAULTS");
        s.run("LAYER NEW cadastre");
        s.run("LAYER SET cadastre");
        s.run("PLINE 0,0 20,0 20,20 0,20 CLOSE");
        s.run("PLINE 40,0 60,0 60,20 CLOSE");
        s.run("LAYER SET 0");
        s.run("LINE 0,50 50,50");
        s.run("AUTOLABEL RULE ADD lots style=\"Lot Area\" layer=cadastre");
        s.run(concat!(
            "AUTOLABEL RULE ADD bearings style=\"Bearing Distance\" layer=cadastre ",
            "labellayer=labels"
        ));
        assert!(
            s.run("AUTOLABEL RULE LIST")
                .contains("name=bearings style=\"Bearing Distance\"")
        );
        assert!(s.refused("AUTOLABEL RULE ADD bad style=Missing"));

        let preview = s.run("AUTOLABEL PREVIEW");
        assert!(preview.contains("preview created=4"), "{preview}");
        assert_eq!(s.run("LABEL LIST"), "", "a preview changes nothing");

        let before = s.steps();
        let run = s.run("AUTOLABEL RUN");
        assert!(run.contains("autolabel created=4"), "{run}");
        assert_eq!(s.steps(), before + 1);
        assert!(s.run("AUTOLABEL RUN").contains("created=0 kept=4"));
        assert_eq!(
            s.steps(),
            before + 1,
            "a re-run with nothing to do is no step"
        );
        assert!(s.run("AUTOLABEL CLEAR bearings").contains("removed=2"));
        assert!(s.run("AUTOLABEL CLEAR").contains("removed=2"));
    }

    #[test]
    fn dimensions_of_every_kind_from_typed_and_named_points() {
        let mut s = Session::default();
        s.run("LINE 0,0 10,0");
        let line = s.last();
        let linear = s.run(&format!("DIM LINEAR #{line}.start #{line}.end at=5,4"));
        assert!(linear.contains("kind=linear measures=10"), "{linear}");
        assert!(linear.contains("associative=yes"));
        let dim = id_field(&linear);

        let baseline = s.run(&format!("DIM BASELINE {dim} 20,0 30,0"));
        assert!(baseline.contains("created dimensions=2"), "{baseline}");
        let chain = s.run(&format!("DIM CONTINUE {dim} 25,0"));
        assert!(chain.contains("created dimensions=1"), "{chain}");

        s.run("LINE 0,0 0,10");
        let other = s.last();
        let angular = s.run(&format!("DIM ANGULAR {line} {other} at=3,3"));
        assert!(angular.contains("kind=angular"), "{angular}");
        assert!(angular.contains("text=\"90°00'00\\\"\""), "{angular}");

        s.run("CIRCLE 50,50 5");
        let circle = s.last();
        assert!(
            s.run(&format!("DIM RADIUS {circle}"))
                .contains("text=R5.000")
        );
        assert!(
            s.run(&format!("DIM DIAMETER {circle} at=60,50"))
                .contains("measures=10")
        );
        assert!(
            s.run("DIM ORDINATE 30,40 at=30,60 datum=10,10")
                .contains("kind=ordinate-x measures=20")
        );
        assert!(s.run("DIM ALIGNED 0,0 3,4 at=0,5").contains("measures=5"));
        assert!(
            s.run("DIM HORIZONTAL 0,0 3,4 at=0,9")
                .contains("measures=3")
        );

        // The legacy form still makes an aligned dimension.
        s.run("DIM 0,0 6,8 2");
        let legacy = dimension_of(&s, s.last());
        assert_eq!(legacy.kind, DimensionKind::Aligned);

        // The measured line moves; the linear dimension follows it.
        s.run(&format!("SELECT {line}"));
        s.run("SCALE 0,0 2");
        let followed = dimension_of(&s, leading_number(&dim));
        double_eq(followed.measurement(), 20.0);
    }

    #[test]
    fn leaders_and_numbered_balloons() {
        let mut s = Session::default();
        s.run("POINT 5,5");
        let pit = s.last();
        let leader = s.run(&format!(
            "LEADER #{pit} 15,15 25,15 text=\"PIT 12\\nIL 10.50\" arrow=dot callout=box"
        ));
        assert!(leader.contains("associative=yes"), "{leader}");
        let made = s.leader(s.last());
        assert_eq!(made.vertices.len(), 3);
        assert_eq!(made.text, "PIT 12\nIL 10.50");
        assert_eq!(made.arrow, ArrowHead::Dot);
        assert_eq!(made.callout, CalloutShape::Box);

        assert!(s.run("BALLOON 0,0 5,5").contains("text=1"));
        assert!(s.run("BALLOON 10,0 15,5").contains("text=2"));
        assert!(s.run("BALLOON 20,0 25,5 n=7").contains("text=7"));
        assert!(s.run("BALLOON 30,0 35,5").contains("text=8"));
        assert!(s.refused("LEADER 0,0"));
        assert!(s.refused("LEADER 0,0 1,1 arrow=zigzag"));
    }

    #[test]
    fn help_names_every_verb() {
        let help = CommandInterpreter::help_text();
        for verb in [
            "ANNOSCALE",
            "TEXTSTYLE",
            "MTEXT",
            "TEXTEDIT",
            "LABELSTYLE",
            "LABEL ",
            "AUTOLABEL",
            "DIM LINEAR",
            "DIM BASELINE",
            "LEADER",
            "BALLOON",
        ] {
            assert!(help.contains(verb), "{verb}");
        }
    }

    #[test]
    fn dimstyle_paper_sizes_a_style() {
        let mut s = Session::default();
        s.run("DIMSTYLE SET Standard PAPER on");
        assert!(
            s.document
                .model()
                .dimension_styles
                .find("Standard")
                .is_some_and(|style| style.paper_sized)
        );
        assert!(s.run("DIMSTYLE LIST").contains("paper-sized"));
    }

    #[test]
    fn label_layout_names_each_piece_that_found_no_room() {
        // Five numbered points at one place in a style that may not move its
        // labels: the first label (6) is placed and the other four (7 to 10)
        // find no room - the placer's WithNoRoomALabelIsSuppressedAndCounted.
        let mut s = Session::default();
        s.run("LABELSTYLE NEW pt kind=point text={point} displace=off");
        for i in 1..=5 {
            s.run("POINT 0,0");
            let last = s.last();
            s.run(&format!("SELECT {last}"));
            s.run(&format!("PROP SET point P{i}"));
        }
        s.run("LABEL 1 2 3 4 5 style=pt");
        let layout = s.run("LABEL LAYOUT");
        assert!(
            layout.contains("considered=5 placed=1 displaced=0 suppressed=4 orphaned=0"),
            "{layout}"
        );
        assert!(layout.contains("\nlabel=6 piece=0 x="), "{layout}");
        assert!(
            layout.contains(concat!(
                "\nlabel=7 piece=0 suppressed=yes\nlabel=8 piece=0 suppressed=yes",
                "\nlabel=9 piece=0 suppressed=yes\nlabel=10 piece=0 suppressed=yes"
            )),
            "{layout}"
        );
        // Every label at its own place: none is named.
        assert!(
            !s.run("LABEL LAYOUT collisions=off")
                .contains("suppressed=yes")
        );
    }

    #[test]
    fn label_set_moves_and_restyles_a_label_as_one_step_and_none_takes_back() {
        let mut s = Session::default();
        s.run("LABELSTYLE DEFAULTS");
        s.run("LAYER NEW labels");
        s.run("PLINE 0,0 30,0 30,40 0,40 CLOSE");
        s.run("LABEL 1 style=\"Lot Area\"");
        let before = s.steps();
        assert_eq!(
            s.run("LABEL SET 2 at=12,34 text=\"LOT 7\" layer=labels"),
            "updated label id=2"
        );
        assert_eq!(s.steps(), before + 1);
        assert!(
            s.run("LABEL LIST 2")
                .contains("layer=labels rule=\"\" at=12,34 text=\"LOT 7\"")
        );
        s.run("LABEL SET 2 at=none text=none");
        // 30 x 40 = 1200 m2, 0.12 ha, a line each: the style's own words again.
        let listed = s.run("LABEL LIST 2");
        assert!(
            listed.contains("rule=\"\" text=\"1200.0 m²\\n0.1200 ha\""),
            "{listed}"
        );
        assert!(s.refused("LABEL SET 1 at=0,0"), "a polyline is not a label");
    }
}
