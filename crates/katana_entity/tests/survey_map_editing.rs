//! The twin of tests/entity/customisation/test_survey_map_editing.cpp: editing
//! a survey code map by index, and what a match says about itself.
//!
//! A rule's only identity is its place in the map, and its place is also its
//! precedence, so these tests check both what each edit leaves in the map and
//! what the codes it answered for resolve to afterwards.

use katana_core::error::ErrorCode;
use katana_entity::{
    SurveyAttribute, SurveyMap, SurveyMatchKind, SurveyRule, SurveySection, validate,
};

fn map_rule(key: &str, model: &str) -> SurveyRule {
    SurveyRule {
        key: String::from(key),
        section: SurveySection::Map,
        model: String::from(model),
        ..SurveyRule::default()
    }
}

/// Four rules whose order can be read back from their models.
fn abcd() -> SurveyMap {
    let mut map = SurveyMap::default();
    assert!(map.add(map_rule("A*", "A")).is_ok());
    assert!(map.add(map_rule("B*", "B")).is_ok());
    assert!(map.add(map_rule("C*", "C")).is_ok());
    assert!(map.add(map_rule("D*", "D")).is_ok());
    map
}

fn order(map: &SurveyMap) -> String {
    map.rules().iter().map(|rule| rule.model.as_str()).collect()
}

fn attribute(kind: &str, name: &str, value: &str) -> SurveyAttribute {
    SurveyAttribute {
        r#type: String::from(kind),
        name: String::from(name),
        value: String::from(value),
    }
}

mod survey_map_editing {
    use super::*;

    #[test]
    fn at_gives_a_copy_of_the_rule_there_and_refuses_an_index_past_the_end() {
        let map = abcd();
        let third = map.at(2).expect("a third rule");
        assert_eq!(third.key, "C*");
        assert_eq!(third.model, "C");

        let past = map.at(4).expect_err("four rules: indices 0 to 3");
        assert_eq!(past.code, ErrorCode::NotFound);
    }

    #[test]
    fn replace_changes_one_rule_and_the_codes_it_now_answers_for() {
        let mut map = abcd();
        assert!(map.replace(1, map_rule("WM*", "SURVEY SERVICES")).is_ok());
        assert_eq!(order(&map), "ASURVEY SERVICESCD");
        // The key changed from B* to WM*, so B01 has lost its rule and WM01 has
        // gained one - the lookup follows the edit rather than the old key.
        assert!(map.lookup("B01").empty());
        assert_eq!(map.lookup("WM01").resolved.model, "SURVEY SERVICES");
    }

    #[test]
    fn remove_takes_out_one_rule_and_its_codes_fall_to_the_next_rule_that_matches() {
        let mut map = SurveyMap::default();
        assert!(map.add(map_rule("WM*", "SURVEY SERVICES")).is_ok());
        assert!(map.add(map_rule("W*", "SURVEY WATER")).is_ok());
        assert_eq!(
            map.lookup("WM01").resolved.model,
            "SURVEY SERVICES",
            "the longer prefix wins"
        );

        assert!(map.remove(0).is_ok());
        assert_eq!(map.size(), 1);
        assert_eq!(
            map.lookup("WM01").resolved.model,
            "SURVEY WATER",
            "with WM* gone, W* is the most specific rule left"
        );
    }

    #[test]
    fn insert_goes_before_the_rule_at_that_index_and_at_the_size_appends() {
        let mut map = abcd();
        assert!(map.insert(1, map_rule("X*", "X")).is_ok());
        assert_eq!(order(&map), "AXBCD");
        assert!(map.insert(map.size(), map_rule("Y*", "Y")).is_ok());
        assert_eq!(order(&map), "AXBCDY");
        assert!(map.insert(0, map_rule("Z*", "Z")).is_ok());
        assert_eq!(order(&map), "ZAXBCDY");
        assert_eq!(map.lookup("X1").resolved.model, "X");
    }

    #[test]
    fn move_leaves_the_rule_at_the_target_with_the_others_in_order() {
        let mut forward = abcd();
        assert!(forward.r#move(0, 2).is_ok());
        assert_eq!(order(&forward), "BCAD"); // A taken out of ABCD, put back at index 2

        let mut backward = abcd();
        assert!(backward.r#move(3, 1).is_ok());
        assert_eq!(order(&backward), "ADBC"); // D taken out of ABCD, put back at index 1

        let mut still = abcd();
        assert!(still.r#move(2, 2).is_ok());
        assert_eq!(order(&still), "ABCD");
    }

    #[test]
    fn moving_one_of_two_equal_keys_ahead_changes_which_one_wins() {
        // Equal keys tie on specificity, and the tie goes to the earlier rule,
        // so order is how an editor says which of two files' opinions stands.
        let mut map = SurveyMap::default();
        assert!(map.add(map_rule("WM*", "FIRST")).is_ok());
        assert!(map.add(map_rule("WM*", "SECOND")).is_ok());
        assert_eq!(map.lookup("WM01").resolved.model, "FIRST");
        assert!(map.r#move(1, 0).is_ok());
        assert_eq!(map.lookup("WM01").resolved.model, "SECOND");
    }

    #[test]
    fn an_edit_that_fails_leaves_the_map_exactly_as_it_was() {
        let before = abcd();
        let mut map = before.clone();

        assert!(
            map.replace(0, map_rule(" A*", "A")).is_err(),
            "a key with a leading blank"
        );
        assert!(
            map.replace(0, map_rule("A*", "SURVEY//DETAIL")).is_err(),
            "an empty path segment"
        );
        assert!(map.replace(4, map_rule("E*", "E")).is_err(), "past the end");
        assert!(
            map.insert(0, map_rule("E *x", "E")).is_err(),
            "a `*` before the end"
        );
        assert!(
            map.insert(5, map_rule("E*", "E")).is_err(),
            "size() is 4, so 5 is past the end"
        );
        assert!(map.remove(4).is_err());
        assert!(map.r#move(4, 0).is_err());
        assert!(map.r#move(0, 4).is_err());

        assert_eq!(map, before);
        assert_eq!(
            map.lookup("A1").resolved.model,
            "A",
            "and the index still answers"
        );
    }

    #[test]
    fn validate_refuses_a_key_with_surrounding_whitespace_and_a_model_that_is_not_a_layer_path() {
        assert!(validate(&map_rule("WM*", "SURVEY SERVICES")).is_ok());
        assert!(
            validate(&map_rule("PABB", "")).is_ok(),
            "no model is a rule about something else"
        );
        assert!(
            validate(&map_rule("PABB", "SURVEY/DETAIL")).is_ok(),
            "a nested layer is a path"
        );

        assert!(
            validate(&map_rule("WM* ", "SURVEY SERVICES")).is_err(),
            "trailing blank"
        );
        assert!(
            validate(&map_rule("\tWM*", "SURVEY SERVICES")).is_err(),
            "leading tab"
        );
        assert!(
            validate(&map_rule("WM*", "/SURVEY")).is_err(),
            "a leading separator"
        );
        assert!(
            validate(&map_rule("WM*", "SURVEY/..")).is_err(),
            "a parent-directory segment"
        );

        // The same refusal reaches add(), which is what a survey code file's
        // reader calls.
        let mut map = SurveyMap::default();
        assert!(map.add(map_rule("WM* ", "SURVEY SERVICES")).is_err());
        assert!(map.empty());
    }

    #[test]
    fn two_maps_are_equal_only_with_the_same_rules_in_the_same_order() {
        let one = abcd();
        let mut two = abcd();
        assert_eq!(one, two);

        assert!(two.r#move(0, 1).is_ok());
        assert_ne!(
            one, two,
            "the same rules in another order resolve codes differently"
        );
        assert!(two.r#move(1, 0).is_ok());
        assert_eq!(one, two);

        let changed = SurveyRule {
            comment: String::from("abutment"),
            ..map_rule("A*", "A")
        };
        assert!(two.replace(0, changed).is_ok());
        assert_ne!(one, two, "a comment is part of a rule");
    }
}

mod survey_map_match_kind {
    use super::*;

    #[test]
    fn the_most_specific_key_that_matched_says_how_the_code_was_met() {
        let mut map = SurveyMap::default();
        assert!(map.add(map_rule("PABB", "SURVEY BRIDGE")).is_ok());
        assert!(map.add(map_rule("WM*", "SURVEY SERVICES")).is_ok());
        let everything = SurveyRule {
            key: String::from("*"),
            section: SurveySection::StringAttribute,
            attributes: vec![attribute("text", "DepthLocation", "Top of Pipe")],
            ..SurveyRule::default()
        };
        assert!(map.add(everything).is_ok());

        assert_eq!(map.lookup("PABB").kind, SurveyMatchKind::Exact);
        assert_eq!(map.lookup("WM01").kind, SurveyMatchKind::Prefix);
        assert_eq!(map.lookup("ZZ99").kind, SurveyMatchKind::FallbackOnly);

        let mut none = SurveyMap::default();
        assert!(none.add(map_rule("WM*", "SURVEY SERVICES")).is_ok());
        assert_eq!(none.lookup("ZZ99").kind, SurveyMatchKind::None);
        assert!(!none.lookup("ZZ99").matched());
    }

    #[test]
    fn only_the_star_answering_with_attributes_is_not_a_match_but_with_a_model_it_is() {
        // Decision D5: "matched" means the map says something about THIS code.
        // A `*` that only attaches attributes answers every code alike; a `*`
        // that names a model is an instruction for codes nobody listed.
        let mut attributes_only = SurveyMap::default();
        let everything = SurveyRule {
            key: String::from("*"),
            section: SurveySection::StringAttribute,
            attributes: vec![attribute("text", "DepthLocation", "Top of Pipe")],
            ..SurveyRule::default()
        };
        assert!(attributes_only.add(everything.clone()).is_ok());
        assert!(!attributes_only.lookup("ZZ99").matched());
        assert!(
            !attributes_only.lookup("ZZ99").empty(),
            "a rule did match; it is just `*`"
        );

        let mut with_model = attributes_only.clone();
        assert!(
            with_model
                .add(map_rule("*", "SURVEY MISCELLANEOUS"))
                .is_ok()
        );
        let found = with_model.lookup("ZZ99");
        assert_eq!(found.kind, SurveyMatchKind::FallbackOnly);
        assert!(found.matched());

        let mut exact = SurveyMap::default();
        assert!(exact.add(everything).is_ok());
        let comment = SurveyRule {
            key: String::from("PABB"),
            section: SurveySection::StringAttribute,
            attributes: vec![attribute("text", "Note", "abutment")],
            ..SurveyRule::default()
        };
        assert!(exact.add(comment).is_ok());
        assert!(
            exact.lookup("PABB").matched(),
            "an exact key is always a match"
        );
    }
}
