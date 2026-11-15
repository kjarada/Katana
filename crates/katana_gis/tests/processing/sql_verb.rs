//! tests/geo/test_sql_verb.cpp: the GisSqlContract suite, the GDAL arguments
//! GIS SQL binds. The file's other suites drive the verb through the app
//! layer and the MCP server (step 6).

mod gis_sql_contract {
    use katana_gis::processing::ArgType;

    use crate::contract_support::expect_argument;

    #[test]
    fn the_arguments_the_verb_binds_are_gdals() {
        expect_argument(&["vector", "sql"], "sql", ArgType::StringList, true);
        expect_argument(&["vector", "sql"], "dialect", ArgType::String, false);
    }
}
