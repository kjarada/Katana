// A stand-in for the twin of tests/surveyio/topcon_test_support.hpp, which is
// being ported beside this file as tests/topcon_support: the helpers the opcode
// field file's tests take from it, with its names (snake case) and its
// behaviour, so that the test file switches to topcon_support by its `mod`
// line alone. What the tests of the readers on the raw builder share:
// fixtures on disk, finding things in a read project, and reading every
// truncation and random damage of a file.

#![allow(
    dead_code,
    reason = "A stand-in for the shared support of four readers' tests; this binary uses part."
)]

use std::path::{Path, PathBuf};

use katana_core::{ErrorCode, Result};
use katana_survey::{
    AzimuthObservation, DistanceObservation, GnssBaselineObservation,
    GnssGeocentricBaselineObservation, HorizontalAngleObservation, HorizontalDirectionObservation,
    LevelDifferenceObservation, Observation, SurveyFeature, SurveyPoint, SurveyProject,
    SurveyStation, UnpositionedPoint, VerticalAngleObservation, ZenithAngleObservation,
};
use katana_surveyio::{ReadOptions, ReadResult, format_registry, read_survey};

use crate::support::Mt19937;

pub fn data_directory() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/surveyio/data"
    ))
}

/// The fixture's bytes, as the C++ reads them with an istreambuf_iterator. A
/// missing fixture fails the test, as the C++'s EXPECT does.
pub fn fixture(relative: impl AsRef<Path>) -> Vec<u8> {
    let path = data_directory().join(relative.as_ref());
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("missing fixture {}: {error}", path.display()))
}

pub fn read(format_id: &str, bytes: &[u8], name: &str) -> Result<ReadResult> {
    read_survey(
        format_registry(),
        format_id,
        bytes,
        name,
        &ReadOptions::default(),
    )
}

pub fn point<'p>(project: &'p SurveyProject, id: &str) -> Option<&'p SurveyPoint> {
    project.points.iter().find(|p| p.id == id)
}

pub fn unpositioned<'p>(project: &'p SurveyProject, id: &str) -> Option<&'p UnpositionedPoint> {
    project.unpositioned_points.iter().find(|p| p.id == id)
}

pub fn feature<'p>(project: &'p SurveyProject, code: &str) -> Option<&'p SurveyFeature> {
    project.features.iter().find(|f| f.code == code)
}

/// An observation kind with a `to`: what the C++ template's requires-clause
/// picks out of the variant.
pub trait ObservationTo: Clone {
    fn of(observation: &Observation) -> Option<&Self>;
    fn to(&self) -> &str;
}

macro_rules! observation_to {
    ($($kind:ident => $variant:ident),* $(,)?) => {
        $(
            impl ObservationTo for $kind {
                fn of(observation: &Observation) -> Option<&Self> {
                    if let Observation::$variant(typed) = observation {
                        Some(typed)
                    } else {
                        None
                    }
                }

                fn to(&self) -> &str {
                    &self.to
                }
            }
        )*
    };
}

observation_to! {
    DistanceObservation => Distance,
    HorizontalAngleObservation => HorizontalAngle,
    VerticalAngleObservation => VerticalAngle,
    ZenithAngleObservation => ZenithAngle,
    AzimuthObservation => Azimuth,
    GnssBaselineObservation => GnssBaseline,
    LevelDifferenceObservation => LevelDifference,
    HorizontalDirectionObservation => HorizontalDirection,
    GnssGeocentricBaselineObservation => GnssGeocentricBaseline,
}

/// The observations of kind T a station holds towards `to`, in file order.
pub fn observations_to<T: ObservationTo>(station: &SurveyStation, to: &str) -> Vec<T> {
    station
        .observations
        .iter()
        .filter_map(T::of)
        .filter(|typed| typed.to() == to)
        .cloned()
        .collect()
}

pub fn any_warning_contains(result: &ReadResult, record: usize, text: &str) -> bool {
    result
        .warnings
        .iter()
        .any(|warning| warning.record == record && warning.message.contains(text))
}

pub fn any_not_carried_contains(result: &ReadResult, text: &str) -> bool {
    result
        .not_carried
        .iter()
        .any(|missing| missing.contains(text))
}

/// Short samples of the OTHER field formats Katana reads, written here by hand
/// in each format's own layout (GSI words, JobXML elements, Survey Controller
/// records, RINEX header labels in columns 61-80), so that the probes can be
/// shown not to claim them without these tests depending on another reader's
/// fixtures.
pub struct ForeignSample {
    pub name: &'static str,
    pub bytes: &'static str,
}

pub const FOREIGN_SAMPLES: [ForeignSample; 7] = [
    ForeignSample {
        name: "station.gsi",
        bytes: concat!(
            "*110001+0000000000000001 81..10+0000000000100000 82..10+0000000000200000 ",
            "83..10+0000000000005000 \r\n",
            "*110002+0000000000000002 21.022+0000000004500000 22.022+0000000009000000 ",
            "31..00+0000000000141421 87..10+0000000000001600 \r\n",
        ),
    },
    ForeignSample {
        name: "station8.gsi",
        bytes: concat!(
            "110001+00000001 81..10+00100000 82..10+00200000 83..10+00005000 \r\n",
            "110002+00000002 21.022+04500000 22.022+09000000 31..00+00141421 \r\n",
            "410003+000000KB 42....+00000001 \r\n",
        ),
    },
    ForeignSample {
        name: "job.jxl",
        bytes: concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<JOBFile jobName=\"SITE\" version=\"5.6\" product=\"Field\">\n<FieldBook>\n",
            "<StationRecord ID=\"1\"><StationName>SS1</StationName>",
            "<TheodoliteHeight>1.5</TheodoliteHeight></StationRecord>\n",
            "<PointRecord ID=\"2\"><Name>BS</Name><Circle><HorizontalCircle>0</HorizontalCircle>",
            "<VerticalCircle>90</VerticalCircle><EDMDistance>10</EDMDistance></Circle>",
            "</PointRecord>\n</FieldBook>\n</JOBFile>\n",
        ),
    },
    ForeignSample {
        name: "site.dc",
        bytes: concat!(
            "00NMSC V10-70       000001-Jan-26 08:00 113111\n",
            "10NMSITE            121111\n",
            "E0NM             S1        1.500000000000000\n",
        ),
    },
    ForeignSample {
        name: "base2560.24o",
        bytes: concat!(
            "     3.04           OBSERVATION DATA    M                   RINEX VERSION / TYPE\n",
            "hand written        katana tests        20240912 120000 UTC PGM / RUN BY / DATE\n",
            "BASE                                                        MARKER NAME\n",
            "  -4052052.7340  4212836.1970 -2545105.1730                  APPROX POSITION XYZ\n",
            "        1.5000        0.0000        0.0000                  ANTENNA: DELTA H/E/N\n",
            "G    4 C1C L1C D1C S1C                                      SYS / # / OBS TYPES\n",
            "                                                            END OF HEADER\n",
            "> 2024 09 12 12 00  0.0000000  0  1\n",
            "G05  23629347.915   124171637.94305     -353.232          45.000\n",
        ),
    },
    ForeignSample {
        name: "base2560.24n",
        bytes: concat!(
            "     2.11           N: GPS NAV DATA                         RINEX VERSION / TYPE\n",
            "hand written        katana tests        20240912 120000 UTC PGM / RUN BY / DATE\n",
            "                                                            END OF HEADER\n",
            " 5 24  9 12 12  0  0.0 1.234567890123D-04 1.023181539495D-11 0.000000000000D+00\n",
        ),
    },
    ForeignSample {
        name: "points.xml",
        bytes: concat!(
            "<?xml version=\"1.0\"?>\n",
            "<LandXML xmlns=\"http://www.landxml.org/schema/LandXML-1.2\" version=\"1.2\">\n",
            "<CgPoints><CgPoint name=\"SS\">1000 2000 50</CgPoint></CgPoints>\n",
            "</LandXML>\n",
        ),
    },
];

/// Every prefix of `bytes`, and bytes of every value, must come back as an
/// error or a result - never a crash, never a hang, and never an Internal
/// error: read_survey turns a panic in a reader into one, so a reader that
/// panicked on every input would otherwise pass as having answered. Returns how
/// many reads were made so a test can say it made them.
pub fn read_every_truncation_and_noise(format_id: &str, bytes: &[u8], name: &str) -> usize {
    let mut reads = 0usize;
    let mut answered = |result: &Result<ReadResult>, what: &str| {
        if let Err(error) = result
            && error.code == ErrorCode::Internal
        {
            panic!("{} ({what})", error.describe());
        }
        reads += 1;
    };
    for length in 0..=bytes.len() {
        answered(&read(format_id, &bytes[..length], name), "a truncation");
    }
    let mut random = Mt19937::new(20260924);
    let mutated = bytes.to_vec();
    for _ in 0..300 {
        let mut copy = mutated.clone();
        // uniform_int_distribution<std::size_t>(0, size - 1). The C++'s
        // `copy[where(random)] = byte(random)` draws the byte first: the right
        // of a built-in assignment is sequenced before its left (C++17).
        let last = copy.len().saturating_sub(1) as u64;
        let mut flips = 0;
        while flips < 8 && !copy.is_empty() {
            let value = random.uniform(0, 255) as u8;
            let at = random.uniform(0, last) as usize;
            copy[at] = value;
            flips += 1;
        }
        answered(&read(format_id, &copy, name), "bytes changed at random");
        let length = random.uniform(0, 255) as usize * 4;
        let mut noise = vec![0u8; length];
        for c in &mut noise {
            *c = random.uniform(0, 255) as u8;
        }
        answered(&read(format_id, &noise, name), "random bytes");
    }
    reads
}
