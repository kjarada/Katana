//! The twin of tests/entity/customisation/test_survey_map_index.cpp: the survey
//! map's key index gives the answer testing every rule gives.
//!
//! The index is only a speed-up: match must return the very rules, in the very
//! order, that the original definition does - every rule whose key matches,
//! stable-sorted so that an exact key comes first, then longer prefixes before
//! shorter, then the bare `*`, ties in read order. That definition is written
//! out below independently of the implementation, and the two are compared
//! over codes chosen to hit every boundary.

use katana_entity::{SurveyMap, SurveyRule, SurveySection};

/// The definition, done the slow way: test every rule, then sort.
fn every_rule_tested(map: &SurveyMap, code: &str) -> Vec<usize> {
    let rules = map.rules();
    let specificity = |rule: &SurveyRule| {
        if !rule.key.ends_with('*') {
            return usize::MAX;
        }
        rule.key.len() - 1
    };
    let mut found = Vec::new();
    for (i, rule) in rules.iter().enumerate() {
        let key = rule.key.as_bytes();
        let prefix = &key[..key.len() - 1];
        let matches = if key.last() != Some(&b'*') {
            key == code.as_bytes()
        } else {
            code.as_bytes().starts_with(prefix)
        };
        if matches {
            found.push(i);
        }
    }
    // A stable sort, most specific first.
    found.sort_by_key(|&index| std::cmp::Reverse(specificity(&rules[index])));
    found
}

fn rule(key: &str, model: &str) -> SurveyRule {
    SurveyRule {
        key: String::from(key),
        section: SurveySection::Map,
        model: String::from(model),
        ..SurveyRule::default()
    }
}

/// Overlapping keys of every shape, several of them repeated so that ties in
/// read order are exercised, and in no particular order.
fn overlapping() -> SurveyMap {
    let mut map = SurveyMap::default();
    for (key, model) in [
        ("WM*", "a"),
        ("*", "b"),
        ("W*", "c"),
        ("WM01", "d"),
        ("WM0*", "e"),
        ("WM*", "f"),
        ("PABB", "g"),
        ("*", "h"),
        ("PAB*", "i"),
        ("WM01", "j"),
        ("P*", "k"),
        ("WM01*", "l"),
        ("PABB*", "m"),
        ("1*", "n"),
        ("10*", "o"),
        ("W*", "p"),
    ] {
        assert!(map.add(rule(key, model)).is_ok());
    }
    map
}

const CODES: [&str; 21] = [
    "", "W", "WM", "WM0", "WM01", "WM012", "WM02", "WN01", "PABB", "PAB", "PABBX", "PA", "P", "1",
    "10", "100", "2", "wm01", "ZZ99", " WM01", "WM01 ",
];

fn expect_index_agrees(map: &SurveyMap) {
    for code in CODES {
        assert_eq!(
            map.match_indices(code),
            every_rule_tested(map, code),
            "code \"{code}\""
        );
        // match is the same list as references.
        let references = map.r#match(code);
        let indices = map.match_indices(code);
        assert_eq!(references.len(), indices.len());
        for (reference, &index) in references.iter().zip(&indices) {
            assert!(std::ptr::eq(*reference, &map.rules()[index]));
        }
    }
}

mod survey_map_index {
    use super::*;

    #[test]
    fn the_index_finds_the_same_rules_in_the_same_order_as_testing_every_rule() {
        let map = overlapping();
        expect_index_agrees(&map);

        // Worked by hand for one code, so the reference itself is checked too.
        // WM012 matches WM01* (l, 11), WM0* (e, 4), WM* (a, 0; f, 5), W* (c, 2;
        // p, 15) and * (b, 1; h, 7): longest prefix first, ties in read order.
        assert_eq!(map.match_indices("WM012"), [11, 4, 0, 5, 2, 15, 1, 7]);
        // WM01 matches the exact key twice (d, 3; j, 9), then WM01* (its prefix
        // is the whole code), WM0*, WM*, W* and *.
        assert_eq!(map.match_indices("WM01"), [3, 9, 11, 4, 0, 5, 2, 15, 1, 7]);
    }

    #[test]
    fn every_edit_keeps_the_index_in_step_with_the_rules() {
        let mut map = overlapping();
        assert!(map.remove(0).is_ok());
        expect_index_agrees(&map);
        assert!(map.insert(3, rule("WM0*", "x")).is_ok());
        expect_index_agrees(&map);
        assert!(map.r#move(0, 10).is_ok());
        expect_index_agrees(&map);
        assert!(map.r#move(12, 2).is_ok());
        expect_index_agrees(&map);
        assert!(map.replace(5, rule("ZZ*", "y")).is_ok());
        expect_index_agrees(&map);
        assert_eq!(map.lookup("ZZ99").resolved.model, "y");

        // A copy carries its own index, and it answers for the copy.
        let mut copy = map.clone();
        assert!(copy.remove(0).is_ok());
        expect_index_agrees(&copy);
        expect_index_agrees(&map);
    }
}
