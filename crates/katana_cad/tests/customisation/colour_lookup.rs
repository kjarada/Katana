//! What a colour NAME means in a drawing (colour_lookup): the Document's own table, then the
//! standard names - and the same answer as the function survey coding takes
//! (tests/cad/customisation/test_colour_lookup.cpp).

use std::sync::Arc;

use katana_cad::Document;
use katana_cad::colour_lookup::{colour_lookup, resolve_colour};
use katana_cad::survey_coding::{ColourOf, SurveyCodingOptions, apply_survey_codes};
use katana_commands::{EntityAttributes, create_point, set_entity_property};
use katana_entity::{Color, ColourTable, EntityId, PropertyValue, SurveyMap, SurveyRule};
use katana_geometry::Point2;

// A table with one name of its own, in a colour no standard name has.
fn site_colours() -> ColourTable {
    let mut colours = ColourTable::default();
    assert!(colours.add("sui gas", Color::new(12, 34, 56, 255)).is_ok());
    colours
}

fn add_coded_point(document: &mut Document, at: Point2, code: &str) -> EntityId {
    assert!(
        document
            .execute(create_point(at, EntityAttributes::default()))
            .is_ok()
    );
    let ids = document.model().entities.ids();
    let id = ids[ids.len() - 1];
    assert!(
        document
            .execute(set_entity_property(
                vec![id],
                "code",
                PropertyValue::from(code)
            ))
            .is_ok()
    );
    id
}

// One rule: gas mains go on GAS, in the customisation's own colour.
fn gas_mains() -> SurveyMap {
    let mut map = SurveyMap::new();
    let rule = SurveyRule {
        key: String::from("GM*"),
        model: String::from("GAS"),
        colour: String::from("sui gas"),
        linestyle: String::from("GAS Main"),
        ..SurveyRule::default()
    };
    assert!(map.add(rule).is_ok());
    map
}

// The Document's resolution as a survey coding keeps it. A SurveyCodingOptions holds a shared,
// owned function, which cannot borrow the Document as the C++'s std::function did: the table as it
// is now, then the standard names - what colour_lookup answers for as long as the Document is not
// changed, which it cannot be while a coding is planned from it.
fn colour_of(document: &Document) -> ColourOf {
    let colours = document.customisation_state().colours.clone();
    Arc::new(move |name: &str| katana_entity::colour_names::resolve_colour(&colours, name))
}

mod colour_lookup {
    use super::*;

    #[test]
    fn a_standard_name_resolves_in_a_document_with_no_customisation() {
        let document = Document::default();
        // Red is full red and nothing else, however the name is written.
        for name in ["red", "RED", " Red "] {
            let red = resolve_colour(&document, name).expect(name);
            assert_eq!(red.r, 255, "{name}");
            assert_eq!(red.g, 0, "{name}");
            assert_eq!(red.b, 0, "{name}");
        }
        // A name nothing knows leaves the colour alone: no guess is made.
        assert!(resolve_colour(&document, "sui gas").is_none());
        assert!(resolve_colour(&document, "pen 025").is_none());
        assert!(resolve_colour(&document, "").is_none());
    }

    #[test]
    fn the_customisations_own_names_resolve_beside_the_standard_ones() {
        let mut document = Document::default();
        document.set_colour_table(site_colours());
        // Its own name, compared as colour names are.
        assert_eq!(
            resolve_colour(&document, "sui gas"),
            Some(Color::new(12, 34, 56, 255))
        );
        assert_eq!(
            resolve_colour(&document, "SUI_Gas"),
            Some(Color::new(12, 34, 56, 255))
        );
        // The standard names are still there.
        let red = resolve_colour(&document, "red").expect("red is a standard name");
        assert_eq!(red.r, 255);
        assert_eq!(red.g, 0);
        // And a name neither has is still unknown.
        assert!(resolve_colour(&document, "sui water").is_none());
    }

    #[test]
    fn the_lookup_function_asks_the_document_at_each_call() {
        // The C++ makes both lookups BEFORE the table is there and changes the
        // Document under them. A lookup that borrows the Document cannot outlive
        // a change to it here, so the property is held at each state: made
        // after each change, the lookup answers from the table that is there
        // then - the one installed last - and never from an earlier one.
        let mut document = Document::default();
        {
            let lookup = colour_lookup(&document);
            assert!(lookup("sui gas").is_none());
        }
        document.set_colour_table(site_colours());
        {
            let lookup = colour_lookup(&document);
            assert_eq!(lookup("sui gas"), Some(Color::new(12, 34, 56, 255)));
            let options = SurveyCodingOptions {
                colour_of: Some(colour_of(&document)),
                ..SurveyCodingOptions::default()
            };
            let coding = options.colour_of.as_ref().expect("a lookup was given");
            assert_eq!(coding("sui gas"), Some(Color::new(12, 34, 56, 255)));
        }
        // A table installed later is the one it answers from.
        document.set_colour_table(ColourTable::default());
        let lookup = colour_lookup(&document);
        assert!(lookup("sui gas").is_none());
        assert!(lookup("red").is_some());
    }

    #[test]
    fn survey_coding_given_the_lookup_colours_by_the_customisations_names() {
        // A rule naming a colour only the customisation defines. With the
        // Document's lookup the style it makes has that colour.
        let mut document = Document::default();
        document.set_survey_map(gas_mains());
        document.set_colour_table(site_colours());
        add_coded_point(&mut document, Point2::new(0.0, 0.0), "GM01");

        let options = SurveyCodingOptions {
            colour_of: Some(colour_of(&document)),
            ..SurveyCodingOptions::default()
        };
        let coded = apply_survey_codes(&document, &options, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()))
            .expect("something changes");
        assert!(document.execute(coded).is_ok());

        let style = document
            .model()
            .styles
            .find("GAS Main")
            .expect("the style was made");
        assert_eq!(style.color, Some(Color::new(12, 34, 56, 255)));
    }

    #[test]
    fn survey_coding_given_no_lookup_still_leaves_colours_alone_whatever_the_document_knows() {
        // Offered, not applied behind a caller's back: with no lookup passed, no
        // colour is set, although the Document could have resolved the name.
        let mut document = Document::default();
        document.set_survey_map(gas_mains());
        document.set_colour_table(site_colours());
        add_coded_point(&mut document, Point2::new(0.0, 0.0), "GM01");

        let coded = apply_survey_codes(&document, &SurveyCodingOptions::default(), None)
            .unwrap_or_else(|error| panic!("{}", error.describe()))
            .expect("something changes");
        assert!(document.execute(coded).is_ok());

        let style = document
            .model()
            .styles
            .find("GAS Main")
            .expect("the style was made");
        assert!(style.color.is_none());
    }
}
