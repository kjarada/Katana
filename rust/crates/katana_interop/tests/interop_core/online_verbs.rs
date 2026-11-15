//! tests/interop/test_online_verbs.cpp: the ONLINE verbs' grammar and replies
//! (docs/gis_online.md, "Verbs"): what an agent types and what it reads back.
//! The replies are for machines, so they are tested as records - a word, then
//! key=value fields that split on spaces outside quotes - and never as a
//! snapshot of prose.

use std::collections::BTreeMap;

use katana_core::{Error, ErrorCode};
use katana_interop::online_catalogue::{
    OnlineCatalogue, OnlineLayer, OnlineLayerKind, built_in_catalogue,
};
use katana_interop::online_fetch::{OnlineImport, OnlineStats};
use katana_interop::online_verbs::{
    OnlineAreaKind, OnlineVerb, format_error, format_import, format_info, format_layers,
    format_providers, parse_online_command, reply_value,
};

use crate::{double_eq, must, refused};

/// One reply line read back as a machine would: the record's word and its
/// fields, quotes and escapes undone.
struct Record {
    word: String,
    fields: BTreeMap<String, String>,
}

fn read_record(line: &str) -> Record {
    let bytes = line.as_bytes();
    let first = line.find(' ');
    let mut record = Record {
        word: String::from(&line[..first.unwrap_or(line.len())]),
        fields: BTreeMap::new(),
    };
    let Some(mut at) = first else {
        return record;
    };
    while at < bytes.len() {
        at += 1;
        let Some(equals) = line[at..].find('=').map(|found| found + at) else {
            break;
        };
        let key = String::from(&line[at..equals]);
        let mut value: Vec<u8> = Vec::new();
        at = equals + 1;
        if at < bytes.len() && bytes[at] == b'"' {
            at += 1;
            while at < bytes.len() && bytes[at] != b'"' {
                if bytes[at] == b'\\' && at + 1 < bytes.len() {
                    at += 1;
                }
                value.push(bytes[at]);
                at += 1;
            }
            // The closing quote.
            at += 1;
        } else {
            let end = line[at..].find(' ').map(|found| found + at);
            value.extend_from_slice(&bytes[at..end.unwrap_or(bytes.len())]);
            at = end.unwrap_or(bytes.len());
        }
        record
            .fields
            .insert(key, String::from_utf8_lossy(&value).into_owned());
    }
    record
}

fn read_records(text: &str) -> Vec<Record> {
    let mut records = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let end = text[start..]
            .find('\n')
            .map_or(text.len(), |found| found + start);
        records.push(read_record(&text[start..end]));
        start = end + 1;
    }
    records
}

fn built_in() -> OnlineCatalogue {
    must(built_in_catalogue())
}

mod online_verbs {
    use super::*;

    #[test]
    fn each_verb_parses() {
        let providers = must(parse_online_command("online providers nsw   cadastre"));
        assert_eq!(providers.verb, OnlineVerb::Providers);
        assert_eq!(providers.filter, "nsw cadastre");
        let layers = must(parse_online_command("ONLINE LAYERS data-gov-au flood maps"));
        assert_eq!(layers.provider, "data-gov-au");
        assert_eq!(layers.filter, "flood maps");
        let info = must(parse_online_command("ONLINE INFO nsw-spatial lots"));
        assert_eq!(info.verb, OnlineVerb::Info);
        assert_eq!(info.layer, "lots");
        let custom = must(parse_online_command(
            "ONLINE CUSTOM https://example.org/wms?service=WMS",
        ));
        assert_eq!(custom.url, "https://example.org/wms?service=WMS");
        let key = must(parse_online_command("ONLINE KEY maptiler \"a b\""));
        assert_eq!(key.key_name, "maptiler");
        assert_eq!(key.key_value, "a b");
        let removal = must(parse_online_command("ONLINE KEY maptiler"));
        assert!(removal.key_value.is_empty());
    }

    #[test]
    fn import_takes_every_option() {
        let command = must(parse_online_command(
            "ONLINE IMPORT sentinel2 truecolour area=lonlat:151.215,-33.865,151.2,-33.875 res=10 \
             layer=\"imagery/sentinel 2\" from=2026-01-01 to=2026-03-31 cloud=15 time=2026-02-01 \
             crs=EPSG:7856 timeout=120 tag=building",
        ));
        assert_eq!(command.verb, OnlineVerb::Import);
        assert_eq!(command.provider, "sentinel2");
        assert_eq!(command.layer, "truecolour");
        assert_eq!(command.area, OnlineAreaKind::Box);
        assert!(command.box_is_lon_lat);
        double_eq(command.r#box.min_x, 151.2);
        double_eq(command.r#box.max_y, -33.865);
        assert_eq!(command.resolution, Some(10.0));
        assert_eq!(command.target_layer, "imagery/sentinel 2");
        assert_eq!(command.from_date, "2026-01-01");
        assert_eq!(command.to_date, "2026-03-31");
        assert_eq!(command.max_cloud, Some(15.0));
        assert_eq!(command.time, "2026-02-01");
        assert_eq!(command.crs, "EPSG:7856");
        assert_eq!(command.timeout_seconds, Some(120));
        assert_eq!(command.tag, "building");
        for (text, kind) in [
            ("view", OnlineAreaKind::View),
            ("drawing", OnlineAreaKind::Drawing),
            ("selection", OnlineAreaKind::Selection),
            ("334000,6250000,335000,6251000", OnlineAreaKind::Box),
        ] {
            let parsed = must(parse_online_command(&format!(
                "ONLINE IMPORT osm buildings area={text}"
            )));
            assert_eq!(parsed.area, kind, "{text}");
            assert!(!parsed.box_is_lon_lat);
        }
    }

    #[test]
    fn malformed_lines_are_refused_with_the_usage() {
        for bad in [
            "ONLINE",
            "ONLINE FETCH x y",
            "ONLINE LAYERS",
            "ONLINE INFO nsw-spatial",
            "ONLINE IMPORT nsw-spatial lots",
            "ONLINE IMPORT nsw-spatial area=view",
            "ONLINE IMPORT a b area=1,2,3",
            "ONLINE IMPORT a b area=1,1,1,5",
            "ONLINE IMPORT a b area=lonlat:190,0,191,1",
            "ONLINE IMPORT a b area=view res=0",
            "ONLINE IMPORT a b area=view res=-2",
            "ONLINE IMPORT a b area=view from=2026-13-01",
            "ONLINE IMPORT a b area=view from=26-01-01",
            "ONLINE IMPORT a b area=view cloud=101",
            "ONLINE IMPORT a b area=view colour=red",
            "ONLINE IMPORT a b area=view layer=",
            "ONLINE IMPORT a b area=view from=2026-03-01 to=2026-01-01",
            "ONLINE IMPORT a b area=view timeout=0",
            "ONLINE KEY",
            "ONLINE KEY a b c",
            "ONLINE CUSTOM",
            "ONLINE IMPORT a b area=view layer=\"unclosed",
        ] {
            let error = refused(parse_online_command(bad), bad);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{bad}");
        }
        let missing_area = refused(
            parse_online_command("ONLINE IMPORT nsw-spatial lots"),
            "an import without an area",
        );
        assert!(
            missing_area.context.contains("usage: ONLINE IMPORT"),
            "{}",
            missing_area.context
        );
    }

    #[test]
    fn replies_are_records_a_machine_can_split() {
        let catalogue = built_in();
        let providers = read_records(&format_providers(&catalogue.filter("nsw")));
        assert_eq!(providers.len(), 2);
        assert_eq!(providers[0].word, "providers");
        assert_eq!(providers[0].fields["count"], "1");
        assert_eq!(providers[1].word, "provider");
        assert_eq!(providers[1].fields["id"], "nsw-spatial");
        assert_eq!(providers[1].fields["group"], "Australia/NSW");
        assert_eq!(providers[1].fields["title"], "NSW Spatial Services");

        let layers = read_records(&format_layers(
            catalogue.find_provider("osm").expect("the osm provider"),
        ));
        assert!(layers.len() >= 3);
        assert_eq!(layers[1].word, "layer");
        assert_eq!(layers[1].fields["provider"], "osm");
        assert_eq!(layers[1].fields["id"], "buildings");
        assert_eq!(layers[1].fields["kind"], "vector");
        assert_eq!(layers[1].fields["service"], "overpass");
        assert_eq!(layers[1].fields["licence"], "ODbL 1.0");

        let dem = catalogue
            .find_provider("copernicus")
            .and_then(|provider| provider.find_layer("dem"))
            .expect("the copernicus dem");
        let mut info: BTreeMap<String, String> = BTreeMap::new();
        for record in read_records(&format_info(dem)) {
            if record.word == "field" {
                info.insert(
                    record.fields["name"].clone(),
                    record.fields["value"].clone(),
                );
            }
        }
        assert_eq!(info["kind"], "elevation");
        assert_eq!(info["service"], "cog");
        assert_eq!(info["coverage"], "-180,-90,180,90");
        assert_eq!(info["verified"], "2026-09-25");
        assert!(
            info["attribution"].contains("Airbus"),
            "{}",
            info["attribution"]
        );
    }

    #[test]
    fn a_key_in_an_endpoint_is_never_in_a_reply() {
        let layer = OnlineLayer {
            provider_id: String::from("p"),
            id: String::from("l"),
            endpoint: String::from("https://tiles.example.org/{z}/{x}/{y}.png?key=SECRET"),
            ..OnlineLayer::default()
        };
        assert!(!format_info(&layer).contains("SECRET"));
    }

    #[test]
    fn an_import_reply_says_what_landed_and_on_what_terms() {
        let layer = OnlineLayer {
            provider_id: String::from("nsw-spatial"),
            id: String::from("lots"),
            ..OnlineLayer::default()
        };
        let result = OnlineImport {
            kind: OnlineLayerKind::Vector,
            stats: OnlineStats {
                requests: 3,
                duplicates: 1,
                ..OnlineStats::default()
            },
            licence: String::from("CC BY 4.0"),
            attribution: String::from("\u{a9} State of New South Wales"),
            source_url: String::from("https://x/y"),
            retrieved: String::from("2026-09-25"),
            warnings: vec![String::from("the service repeated a page")],
            ..OnlineImport::default()
        };
        let records = read_records(&format_import(
            &layer,
            &result,
            10,
            "online/nsw-spatial/lots",
        ));
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].word, "imported");
        assert_eq!(records[0].fields["entities"], "10");
        assert_eq!(records[0].fields["target"], "online/nsw-spatial/lots");
        assert_eq!(records[0].fields["requests"], "3");
        assert_eq!(records[0].fields["duplicates"], "1");
        assert_eq!(
            records[0].fields["attribution"],
            "\u{a9} State of New South Wales"
        );
        assert_eq!(records[1].word, "warning");
        assert_eq!(records[1].fields["text"], "the service repeated a page");

        let error = read_records(&format_error(
            OnlineVerb::Import,
            &Error::new(ErrorCode::InvalidArgument, "say \"why\"").with_context("ctx"),
        ));
        assert_eq!(error[0].word, "error");
        assert_eq!(error[0].fields["verb"], "IMPORT");
        assert_eq!(error[0].fields["code"], "InvalidArgument");
        assert_eq!(error[0].fields["message"], "say \"why\"");
        assert_eq!(reply_value("plain"), "plain");
        assert_eq!(reply_value(""), "\"\"");
        assert_eq!(reply_value("a=b"), "\"a=b\"");
    }
}
