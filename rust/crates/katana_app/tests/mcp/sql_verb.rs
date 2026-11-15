//! tests/geo/test_sql_verb.cpp: its GisQueryMcp cases, katana_gis_query
//! (geo::sql_mcp) driven as a client drives it. The file's GIS SQL cases are
//! the verb's (step 6's V).

mod gis_query_mcp {
    use katana_core::json::Json;

    use crate::mcp_client::{Client, failed, object, string_of, strings};

    #[test]
    fn gis_query_returns_rows_as_json() {
        // Two 50 x 40 lots, one owned by Smith: 2000 m2 each, typed as numbers.
        let mut client = Client::new();
        let _ = client.run_lines(&[
            "RECT 0,0 50,40",
            "RECT 50,0 100,40",
            "SELECT 1",
            "MODIFY SET PROP=owner:Smith",
        ]);
        let result = client.call(
            "katana_gis_query",
            object([(
                "sql",
                Json::from(
                    "SELECT \"katana_id\", owner, ST_Area(geometry) AS area FROM polygons ORDER \
                     BY katana_id",
                ),
            )]),
        );
        assert!(!failed(&result), "{}", result.dump());
        let query = &result["structuredContent"];
        assert_eq!(
            string_of(&query["line"]),
            "GIS SQL \"SELECT [katana_id], owner, ST_Area(geometry) AS area FROM polygons ORDER \
             BY katana_id\" DRAWING dialect=sqlite"
        );
        assert_eq!(query["columns"], strings(&["katana_id", "owner", "area"]));
        assert_eq!(
            query["column_types"],
            strings(&["integer", "string", "real"])
        );
        assert_eq!(query["rows"].size(), 2);
        assert_eq!(query["rows"][0]["katana_id"], Json::from(1));
        assert_eq!(query["rows"][0]["owner"], Json::from("Smith"));
        assert_eq!(query["rows"][0]["area"], Json::from(2000.0));
        // no owner: null, not ""
        assert!(query["rows"][1]["owner"].is_null());
        assert_eq!(query["matched"], Json::from(2));
        assert_eq!(query["used"], Json::from(2));
    }

    #[test]
    fn a_statement_that_is_not_a_select_is_refused() {
        let mut client = Client::new();
        let _ = client.run_lines(&["RECT 0,0 50,40"]);
        let result = client.call(
            "katana_gis_query",
            object([("sql", Json::from("DELETE FROM polygons"))]),
        );
        assert!(
            result
                .find("isError")
                .and_then(Json::as_bool)
                .unwrap_or(false),
            "{}",
            result.dump()
        );
        assert_eq!(client.session.document().model().entities.size(), 1);
    }
}
