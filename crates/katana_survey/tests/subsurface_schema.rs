//! The twin of tests/survey/test_subsurface_schema.cpp: reading a delivery
//! schema and checking a schedule against it.

use katana_survey::subsurface::*;

// A schema of the tests' own, in the shape tools/utility_schema_domains.py
// writes: the real ones are clients' documents and stay out of the
// repository. Features belong under a type code; sizes are open.
const SCHEMA: &str = "kind,attribute,value,detail,label
schema,Example Utility Schema,0.1,,
parent,TypeCode,,,
identifier,AssetId,,,
prefix,AssetId,TypeCode,,
field,AssetId,Alphanumerical,Yes,Asset Id
field,TypeCode,Domain List: Type Code,Yes,Type Code
field,Feature,Domain List: Feature,Yes,Feature
field,Status,Domain List: Status,Yes,Status
field,Size,Domain List: Size,No,Size
field,Surveyed,Date (YYYY/MM/DD),Yes,Date Surveyed
field,Depth,Real Number (metres),Yes,Depth
field,Report,Free Text,Conditional,Report
domain,TypeCode,W,,
domain,TypeCode,E,,
domain,Feature,Valve,W,
domain,Feature,Pit,W,
domain,Feature,Pit,E,
domain,Feature,Other,,
domain,Status,Live,,
domain,Status,Dead,,
open,Size,,,
domain,Size,Unknown,,
";

fn schema() -> DeliverySchema {
    parse_delivery_schema(SCHEMA).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn check(text: &str, schema: &DeliverySchema) -> SchemaCheck {
    check_delivery(text, schema).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn problems(check: &SchemaCheck) -> Vec<String> {
    check
        .findings
        .iter()
        .map(|finding| {
            format!(
                "{} {} {} {}",
                if finding.severity == FindingSeverity::Error {
                    "E"
                } else {
                    "W"
                },
                finding.line_number,
                finding.attribute,
                finding.problem
            )
        })
        .collect()
}

mod subsurface_delivery_schema {
    use super::*;

    #[test]
    fn reads_a_schema_file() {
        let parsed = schema();
        assert_eq!(parsed.title, "Example Utility Schema");
        assert_eq!(parsed.version, "0.1");
        assert_eq!(parsed.fields.len(), 8);
        assert_eq!(parsed.fields[5].label, "Date Surveyed");
        assert_eq!(parsed.fields[7].requirement, Requirement::Conditional);
        assert_eq!(parsed.parent_attribute, "TypeCode");
        assert_eq!(parsed.identifier_attribute, "AssetId");
        assert_eq!(parsed.prefixes["AssetId"], "TypeCode");
        assert_eq!(parsed.domains["Feature"].values.len(), 4);
        assert!(parsed.domains["Size"].open);
    }

    #[test]
    fn refuses_a_schema_file_it_cannot_use() {
        let error = |text: &str| match parse_delivery_schema(text) {
            Ok(_) => String::from("ok"),
            Err(error) => error.describe(),
        };
        assert!(error("kind,attribute,value,detail\nrule,X,,\n").contains("kind \"rule\""));
        assert!(
            error("kind,attribute,value,detail\nfield,X,Free Text,Yes\ndomain,Y,1,\n")
                .contains("a domain for \"Y\", which is not a field")
        );
        assert!(error("kind,attribute,value,detail\nschema,S,1,\n").contains("no fields"));
        assert!(
            error("kind,attribute,value,detail\nfield,X,Free Text,Maybe\n")
                .contains("not Yes, No or Conditional")
        );
        assert!(error("kind,attribute\n").contains("\"value\" column"));
    }

    #[test]
    fn checks_every_row_against_the_schema() {
        // Lines: 1 header, 2 clean, 3..8 one problem each (7 has two).
        let checked = check(
            concat!(
                "AssetId,Type Code,Feature,Status,Size,date surveyed,Depth,",
                "Report,easting\n",
                "W-1,W,Valve,Live,600,2026/09/20,1.2,,1\n",
                "E-1,E,Valve,Live,600,2026/09/20,1.2,,1\n",
                "E-2,E,Pit,live,2 x 1,2026/09/20,1.2,,1\n",
                "E-3,E,Pit,Live,big,2026/09/20,1.2,,1\n",
                "E-4,E,Pit,Live,Unknown,2026/02/30,1.2,,1\n",
                "X-5,E,Other,,,2024/02/29,deep,,1\n",
                "E-6,E,Pit,Dead,,2026/09/20,0.5,see file,1\n",
            ),
            &schema(),
        );
        assert_eq!(checked.rows, 7);
        assert_eq!(checked.assets, 7);
        assert_eq!(checked.not_in_schema, ["easting"]);
        assert_eq!(checked.conditional_not_checked, 1);
        let expected = [
            "E 3 Feature \"Valve\" is not listed under TypeCode \"E\"",
            "W 4 Status \"live\" is listed as \"Live\"; values must be spelt exactly",
            "E 5 Size \"big\" is not in the domain",
            "E 6 Surveyed \"2026/02/30\" is not a date as YYYY/MM/DD",
            "E 7 Status is empty",
            "E 7 Depth \"deep\" is not a number",
            "W 7 AssetId does not start with its TypeCode \"E\"",
        ];
        assert_eq!(problems(&checked), expected);
        assert_eq!(checked.count(FindingSeverity::Error), 5);
        assert_eq!(checked.count(FindingSeverity::Warning), 2);
    }

    #[test]
    fn a_mandatory_attribute_with_no_column_is_one_finding() {
        let checked = check(
            "AssetId,TypeCode,Feature,Status,Surveyed\n\
             W-1,W,Pit,Live,2026/09/20\nW-2,W,Pit,Live,2026/09/20\n",
            &schema(),
        );
        assert_eq!(checked.findings.len(), 1);
        assert_eq!(checked.findings[0].attribute, "Depth");
        assert_eq!(
            checked.findings[0].problem,
            "is mandatory and has no column"
        );
        assert_eq!(checked.findings[0].line_number, 0);
    }

    #[test]
    fn the_report_groups_findings_and_counts_their_rows() {
        let parsed = schema();
        let checked = check(
            "AssetId,TypeCode,Feature,Status,Surveyed,Depth\n\
             W-1,W,Pit,,2026/09/20,1\nW-2,W,Pit,,2026/09/20,1\n\
             W-3,W,Pit,Live,2026/09/20,1\n",
            &parsed,
        );
        let text = render_schema_check(&checked, &parsed);
        assert!(
            text.contains("against Example Utility Schema v0.1"),
            "{text}"
        );
        assert!(
            text.contains("3 rows, 3 assets (AssetId): 2 errors, 0 warnings"),
            "{text}"
        );
        assert!(
            text.contains("error    Status is empty (2 rows: lines 2, 3)"),
            "{text}"
        );

        let clean = check(
            "AssetId,TypeCode,Feature,Status,Surveyed,Depth\nW-1,W,Pit,Live,2026/09/20,1\n",
            &parsed,
        );
        assert!(render_schema_check(&clean, &parsed).contains("The schedule meets the schema."));
    }
}
