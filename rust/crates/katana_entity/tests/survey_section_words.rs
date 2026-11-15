//! The twin of tests/entity/customisation/test_survey_section_words.cpp: the
//! words a rule's section is known by. One table gives them
//! (SURVEY_SECTION_WORDS): the EnumName of a SurveySection reads it for what a
//! reply and a label show, and the Katana customisation format reads it for a
//! rule's "sets" - so what a person reads in the window is what they write in
//! a file.
//!
//! The nine words below are written out by hand from the table "The words a
//! rule is shown by" in docs/customisation.md, not taken from the table under
//! test.

use katana_entity::customisation::{customisation_from_json, customisation_to_json};
use katana_entity::entity::to_string;
use katana_entity::survey_map::SURVEY_SECTION_WORDS;
use katana_entity::{
    Customisation, CustomisationWriteOptions, SurveyRule, SurveySection, validate,
};

/// In the enumeration's order.
const WORDS: [(SurveySection, &str); 9] = [
    (SurveySection::Map, "feature"),
    (SurveySection::VertexSymbol, "symbol"),
    (SurveySection::VertexTextStyle, "text"),
    (SurveySection::Pipe, "pipe"),
    (SurveySection::VertexPipe, "vertexPipe"),
    (SurveySection::SegmentPipe, "segmentPipe"),
    (SurveySection::StringAttribute, "attributes"),
    (SurveySection::VertexAttribute, "vertexAttributes"),
    (SurveySection::Tinable, "surface"),
];

mod survey_section_words {
    use super::*;

    #[test]
    fn each_section_is_shown_by_its_katana_word_and_the_table_holds_all_nine_in_order() {
        for (section, word) in WORDS {
            assert_eq!(to_string(section), word);
        }
        assert_eq!(SURVEY_SECTION_WORDS.len(), WORDS.len());
        for (i, (section, word)) in WORDS.iter().enumerate() {
            assert_eq!(SURVEY_SECTION_WORDS[i].section, *section, "{i}");
            assert_eq!(SURVEY_SECTION_WORDS[i].word, *word, "{i}");
        }
    }

    #[test]
    fn a_file_holds_a_rules_section_by_the_word_it_is_shown_by() {
        for (section, word) in WORDS {
            let mut customisation = Customisation {
                name: String::from("Words"),
                ..Customisation::default()
            };
            let rule = SurveyRule {
                key: String::from("A*"),
                section,
                ..SurveyRule::default()
            };
            assert!(customisation.map.add(rule).is_ok(), "{word}");

            let written =
                customisation_to_json(&customisation, &CustomisationWriteOptions::default())
                    .expect("written");
            assert!(
                written.contains(&format!("\"sets\": \"{word}\"")),
                "{written}"
            );

            let read = customisation_from_json(written.as_bytes()).expect("read back");
            assert_eq!(read.map.size(), 1, "{word}");
            assert_eq!(read.map.rules()[0].section, section, "{word}");
        }
    }

    #[test]
    fn a_rule_whose_layer_is_not_a_layer_path_is_refused_by_the_word_layer() {
        // "A//B" has an empty level, which no layer path may. The field is
        // SurveyRule::model, and a person knows it as the rule's layer.
        let rule = SurveyRule {
            key: String::from("WM*"),
            model: String::from("A//B"),
            ..SurveyRule::default()
        };
        let error = validate(&rule).expect_err("an empty level");
        assert!(
            error
                .message
                .starts_with("the layer of rule \"WM*\" is not a valid layer path: "),
            "{}",
            error.message
        );
    }
}
