//! tests/geo/test_pipeline_verb.cpp: the PipelineContract suite, the steps
//! whose pipeline the bridge leaves to print. The file's PipelineVerb suite
//! drives GDAL PIPELINE through the app layer (step 6).

mod pipeline_contract {
    use std::collections::BTreeSet;

    use katana_core::json::Json;
    use katana_core::text::is_ascii_space;
    use katana_gis::processing::describe;

    use crate::{must, strings};

    /// `std::regex_replace(text, std::regex(R"(([:\[,])\s*-?(Infinity|NaN))"),
    /// "$1null")`, by hand (no regex crate): a delimiter, white space, an
    /// optional minus and a bare Infinity or NaN become the delimiter and
    /// null. Matches are found left to right and do not overlap; `\s` is the
    /// C locale's white space.
    fn bare_numbers_as_null(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        let mut i = 0;
        while i < bytes.len() {
            if matches!(bytes[i], b':' | b'[' | b',') {
                let mut j = i + 1;
                while j < bytes.len() && is_ascii_space(bytes[j]) {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'-' {
                    j += 1;
                }
                let rest = &bytes[j..];
                let word = if rest.starts_with(b"Infinity") {
                    Some("Infinity".len())
                } else if rest.starts_with(b"NaN") {
                    Some("NaN".len())
                } else {
                    None
                };
                if let Some(length) = word {
                    // Every cut is at an ASCII byte: whole characters.
                    out.push_str(&text[copied..=i]);
                    out.push_str("null");
                    i = j + length;
                    copied = i;
                    continue;
                }
            }
            i += 1;
        }
        out.push_str(&text[copied..]);
        out
    }

    #[test]
    fn the_steps_that_print_are_the_ones_gdal_declares_so() {
        // The bridge leaves a pipeline's output unbound - to print - only
        // when its last step is one of these; a GDAL that adds a printing
        // step fails here, naming it, rather than a pipeline failing in front
        // of a person.
        let spec = must(describe(&strings(&["pipeline"])));
        // GDAL writes a default of Infinity (vector grid's radius) bare,
        // which is no JSON; as a value it is null here.
        let usage = Json::parse(bare_numbers_as_null(&spec.usage_json))
            .unwrap_or_else(|error| panic!("{error}"));
        let mut printing = BTreeSet::new();
        let steps = usage
            .at("pipeline_algorithms")
            .unwrap_or_else(|error| panic!("{error}"));
        for step in steps.iter() {
            let outputs = step
                .at("output_arguments")
                .unwrap_or_else(|error| panic!("{error}"));
            for output in outputs.iter() {
                let name = output.at("name").unwrap_or_else(|error| panic!("{error}"));
                if name.as_str() == Some("output-string") {
                    let step_name = step
                        .at("name")
                        .ok()
                        .and_then(Json::as_str)
                        .expect("a pipeline step has a name");
                    printing.insert(String::from(step_name));
                }
            }
        }
        assert_eq!(
            printing,
            BTreeSet::from([
                String::from("compare"),
                String::from("export-schema"),
                String::from("info"),
            ])
        );
    }
}
