//! tests/geo/test_formats.cpp: its FormatsVerb suite, FORMATS through the
//! executor. The file's Formats suite is step 5's (katana_gis and
//! katana_interop) and step 6's wave 2 (B: a GeoPackage holding only a raster
//! routed to raster import).

mod formats_verb {
    use katana_app::geo::replies::parse_records;
    use katana_app::geo::{handles, help_text};
    use katana_core::ErrorCode;
    use katana_core::json::Json;

    use crate::geo_fixture::{GeoFixture, holds, lacks, must, refused};

    /// The C++ fixture's: its scratch folder named after the test.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana-formats-", name)
    }

    fn parse_json(text: &str) -> Json {
        Json::parse(text).unwrap_or_else(|error| panic!("{error}: {text}"))
    }

    fn words(items: &[&str]) -> Json {
        Json::array_of(items.iter().map(|item| Json::from(*item)))
    }

    #[test]
    fn formats_lists_writable_vector_drivers_as_records() {
        let mut fixture = fixture("FormatsListsWritableVectorDriversAsRecords");
        assert!(handles("FORMATS"));
        holds(&help_text(), "FORMATS [RASTER|VECTOR] [READ|WRITE]");
        let reply = must(fixture.run("FORMATS VECTOR WRITE"));
        holds(
            &reply,
            "format driver=FlatGeobuf kind=vector read=vector write=vector extensions=fgb vsi=yes \
             description=FlatGeobuf",
        );
        holds(
            &reply,
            "format driver=GPKG kind=raster,vector read=raster,vector write=raster,vector \
             extensions=gpkg,gpkg.zip vsi=yes",
        );
        // A raster format writes no layers.
        lacks(&reply, "driver=GTiff");
        holds(&reply, "\nlisted formats=");
        holds(&reply, " kind=vector capability=write filter= gdal=3.");
        // Nothing changed, nothing ran: FORMATS answers at prepare.
        let prepared = must(fixture.prepare("FORMATS"));
        assert!(prepared.reply.is_some());
        assert_eq!(fixture.document.model().entities.size(), 0);
    }

    #[test]
    fn formats_finds_words_in_names_descriptions_and_extensions() {
        let mut fixture = fixture("FormatsFindsWordsInNamesDescriptionsAndExtensions");
        let reply = must(fixture.run("FORMATS READ fgb"));
        holds(&reply, "format driver=FlatGeobuf ");
        // Every format listed holds the word: FlatGeobuf, and whatever else
        // claims it (GDAL's raster tile index reads a .gti.fgb).
        let records = parse_records(&reply);
        assert!(records.len() >= 2);
        for record in &records[..records.len() - 1] {
            let text = format!(
                "{} {} {}",
                record.get("driver").expect("driver="),
                record.get("description").expect("description="),
                record.get("extensions").expect("extensions=")
            );
            holds(&text, "fgb");
        }
        holds(
            &reply,
            &format!(
                "listed formats={} kind=any capability=read filter=fgb",
                records.len() - 1
            ),
        );
        let none = must(fixture.run("FORMATS RASTER nothing-has-this-word"));
        assert!(none.starts_with("listed formats=0 kind=raster"), "{none}");
    }

    #[test]
    fn formats_options_gives_a_drivers_options() {
        let mut fixture = fixture("FormatsOptionsGivesADriversOptions");
        let reply = must(fixture.run("FORMATS OPTIONS GPKG"));
        assert!(reply.starts_with("format driver=GPKG "), "{reply}");
        holds(
            &reply,
            "option driver=GPKG list=open name=LIST_ALL_TABLES type=string-select default=AUTO \
             scope=vector choices=AUTO,YES,NO min= max= description=\"Whether all tables",
        );
        holds(
            &reply,
            "option driver=GPKG list=open name=ZLEVEL type=int default=6 scope=raster choices= \
             min=1 max=9",
        );
        holds(&reply, "\nlisted driver=GPKG open=");
        // A driver's name of two words, as it is, unquoted or quoted.
        for line in [
            "FORMATS OPTIONS ESRI Shapefile",
            "FORMATS OPTIONS \"ESRI Shapefile\"",
        ] {
            let shapefile = fixture
                .run(line)
                .unwrap_or_else(|error| panic!("{line}: {}", error.describe()));
            holds(&shapefile, "list=layer_creation name=ENCODING");
        }
        let unknown = refused(
            fixture.run("FORMATS OPTIONS NoSuchDriver"),
            "a driver of no format",
        );
        assert_eq!(unknown.code, ErrorCode::NotFound);
        let nameless = refused(fixture.run("FORMATS OPTIONS"), "OPTIONS with no driver");
        assert_eq!(nameless.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn formats_json_is_the_records_as_data() {
        let mut fixture = fixture("FormatsJsonIsTheRecordsAsData");
        let reply = must(fixture.run("FORMATS VECTOR WRITE fgb JSON"));
        let formats = parse_json(&reply);
        assert_eq!(formats.size(), 1, "{reply}");
        assert_eq!(formats[0]["driver"], Json::from("FlatGeobuf"));
        assert_eq!(formats[0]["kinds"], words(&["vector"]));
        assert_eq!(formats[0]["write"], words(&["vector"]));
        assert_eq!(formats[0]["extensions"], words(&["fgb"]));
        assert_eq!(formats[0]["vsi"], Json::from(true));
        assert!(
            formats[0]["help_url"]
                .as_str()
                .expect("help_url is text")
                .starts_with("https://gdal.org/")
        );

        let options = must(fixture.run("FORMATS OPTIONS GPKG JSON"));
        let described = parse_json(&options);
        assert_eq!(described["format"]["driver"], Json::from("GPKG"));
        assert!(!described["open_options"].empty());
        assert!(!described["creation_options"].empty());
    }

    #[test]
    fn formats_refuses_two_kinds_or_two_capabilities() {
        let mut fixture = fixture("FormatsRefusesTwoKindsOrTwoCapabilities");
        for line in ["FORMATS RASTER VECTOR", "FORMATS READ WRITE"] {
            let error = refused(fixture.run(line), line);
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }
        // A keyword quoted is a word to look for.
        let quoted = must(fixture.run("FORMATS \"raster\""));
        holds(&quoted, "kind=any capability=any filter=raster");
    }
}
