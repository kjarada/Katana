// Twin of tests/surveyio/topcon_test_support.hpp: what the tests of the readers
// on the raw builder share (RW5, GTS, the opcode field file, Sokkia SDR):
// fixtures on disk, the angles the expected values are worked in, and finding
// things in a read project.

#![allow(
    dead_code,
    reason = "Each test binary includes this module and uses its own part of it."
)]

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use katana_core::{ErrorCode, Result};
use katana_survey::{
    AzimuthObservation, DistanceObservation, GnssBaselineObservation,
    GnssGeocentricBaselineObservation, HorizontalAngleObservation, HorizontalDirectionObservation,
    LevelDifferenceObservation, Observation, SurveyFeature, SurveyPoint, SurveyProject,
    SurveyStation, UnpositionedPoint, VerticalAngleObservation, ZenithAngleObservation,
};
use katana_surveyio::{ReadOptions, ReadResult, format_registry, read_survey};

/// tests/surveyio/data, where the C++ reads its fixtures.
pub fn data_directory() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/surveyio/data"
    ))
}

/// The bytes of a fixture under data_directory() (or at an absolute path).
pub fn fixture(relative: impl AsRef<Path>) -> Vec<u8> {
    let path = data_directory().join(relative);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("missing fixture {}: {error}", path.display()))
}

/// Degrees, minutes and seconds to radians, the way a surveyor writes the
/// expected value down: through whole arc-seconds.
pub fn dms(degrees: i32, minutes: i32, seconds: f64) -> f64 {
    ((f64::from(degrees) * 60.0 + f64::from(minutes)) * 60.0 + seconds) * (PI / 648000.0)
}

/// `bytes` read as format `format_id` through the program's registry.
pub fn read(format_id: &str, bytes: impl AsRef<[u8]>, name: &str) -> Result<ReadResult> {
    read_survey(
        format_registry(),
        format_id,
        bytes.as_ref(),
        name,
        &ReadOptions::default(),
    )
}

/// The result of a read that must succeed, or the test fails with the error.
pub fn ok<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

pub fn point<'a>(project: &'a SurveyProject, id: &str) -> Option<&'a SurveyPoint> {
    project.points.iter().find(|p| p.id == id)
}

pub fn unpositioned<'a>(project: &'a SurveyProject, id: &str) -> Option<&'a UnpositionedPoint> {
    project.unpositioned_points.iter().find(|p| p.id == id)
}

pub fn feature<'a>(project: &'a SurveyProject, code: &str) -> Option<&'a SurveyFeature> {
    project.features.iter().find(|f| f.code == code)
}

/// An observation kind that is TO a point, for observations_to.
pub trait Targeted: Clone {
    fn of(observation: &Observation) -> Option<&Self>;
    fn target(&self) -> &str;
}

macro_rules! targeted {
    ($($kind:ident => $variant:ident),* $(,)?) => {
        $(
            impl Targeted for $kind {
                fn of(observation: &Observation) -> Option<&Self> {
                    if let Observation::$variant(typed) = observation {
                        Some(typed)
                    } else {
                        None
                    }
                }
                fn target(&self) -> &str {
                    &self.to
                }
            }
        )*
    };
}

targeted! {
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
pub fn observations_to<T: Targeted>(station: &SurveyStation, to: &str) -> Vec<T> {
    station
        .observations
        .iter()
        .filter_map(T::of)
        .filter(|typed| typed.target() == to)
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

/// EXPECT_NEAR: |actual - expected| <= epsilon.
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_DOUBLE_EQ: within 4 units in the last place.
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}

/// Short samples of the OTHER field formats Katana reads, written here by hand
/// in each format's own layout (GSI words, JobXML elements, Survey Controller
/// records, RINEX header labels in columns 61-80), so that the Topcon probes
/// can be shown not to claim them without these tests depending on another
/// reader's fixtures.
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
            "31..00+0000000000141421 87..10+0000000000001600 \r\n"
        ),
    },
    ForeignSample {
        name: "station8.gsi",
        bytes: concat!(
            "110001+00000001 81..10+00100000 82..10+00200000 83..10+00005000 \r\n",
            "110002+00000002 21.022+04500000 22.022+09000000 31..00+00141421 \r\n",
            "410003+000000KB 42....+00000001 \r\n"
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
            "</PointRecord>\n</FieldBook>\n</JOBFile>\n"
        ),
    },
    ForeignSample {
        name: "site.dc",
        bytes: concat!(
            "00NMSC V10-70       000001-Jan-26 08:00 113111\n",
            "10NMSITE            121111\n",
            "E0NM             S1        1.500000000000000\n"
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
            "G05  23629347.915   124171637.94305     -353.232          45.000\n"
        ),
    },
    ForeignSample {
        name: "base2560.24n",
        bytes: concat!(
            "     2.11           N: GPS NAV DATA                         RINEX VERSION / TYPE\n",
            "hand written        katana tests        20240912 120000 UTC PGM / RUN BY / DATE\n",
            "                                                            END OF HEADER\n",
            " 5 24  9 12 12  0  0.0 1.234567890123D-04 1.023181539495D-11 0.000000000000D+00\n"
        ),
    },
    ForeignSample {
        name: "points.xml",
        bytes: concat!(
            "<?xml version=\"1.0\"?>\n",
            "<LandXML xmlns=\"http://www.landxml.org/schema/LandXML-1.2\" version=\"1.2\">\n",
            "<CgPoints><CgPoint name=\"SS\">1000 2000 50</CgPoint></CgPoints>\n",
            "</LandXML>\n"
        ),
    },
];

/// std::mt19937, GCC's sequence (katana_geometry's test support has the same
/// engine).
pub struct Random32 {
    state: [u32; 624],
    index: usize,
}

impl Random32 {
    pub fn new(seed: u32) -> Self {
        let mut state = [0; 624];
        state[0] = seed;
        for index in 1..624 {
            state[index] = 1812433253_u32
                .wrapping_mul(state[index - 1] ^ (state[index - 1] >> 30))
                .wrapping_add(index as u32);
        }
        Self { state, index: 624 }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.index == 624 {
            for index in 0..624 {
                let value =
                    (self.state[index] & 0x80000000) | (self.state[(index + 1) % 624] & 0x7fffffff);
                self.state[index] = self.state[(index + 397) % 624]
                    ^ (value >> 1)
                    ^ if value & 1 != 0 { 0x9908b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= value >> 11;
        value ^= (value << 7) & 0x9d2c5680;
        value ^= (value << 15) & 0xefc60000;
        value ^ (value >> 18)
    }

    /// std::uniform_int_distribution over [low, high] on this engine, as
    /// libstdc++ draws it from an engine of exactly 32 bits: Lemire's
    /// nearly-divisionless method in 64-bit products.
    pub fn uniform(&mut self, low: u32, high: u32) -> u32 {
        let range = high - low + 1;
        let mut product = u64::from(self.next_u32()) * u64::from(range);
        let mut low_bits = product as u32;
        if low_bits < range {
            let threshold = range.wrapping_neg() % range;
            while low_bits < threshold {
                product = u64::from(self.next_u32()) * u64::from(range);
                low_bits = product as u32;
            }
        }
        low + (product >> 32) as u32
    }
}

/// Every prefix of `bytes`, and bytes of every value, must come back as an
/// error or a result - never a crash, never a hang, and never an Internal
/// error: read_survey turns a panic a reader raises into one, so a reader that
/// panicked on every input would otherwise pass as having answered. Returns how
/// many reads were made so a test can say it made them.
pub fn read_every_truncation_and_noise(format_id: &str, bytes: &[u8], name: &str) -> usize {
    let mut reads = 0;
    let mut failures = Vec::new();
    let mut answered = |result: &Result<ReadResult>, what: &str| {
        if let Err(error) = result
            && error.code == ErrorCode::Internal
        {
            failures.push(format!("{} ({what})", error.describe()));
        }
        reads += 1;
    };
    for length in 0..=bytes.len() {
        answered(&read(format_id, &bytes[..length], name), "a truncation");
    }
    let mut random = Random32::new(20260924);
    for _ in 0..300 {
        let mut copy = bytes.to_vec();
        let last = if copy.is_empty() {
            0
        } else {
            copy.len() as u32 - 1
        };
        let mut flips = 0;
        while flips < 8 && !copy.is_empty() {
            // C++17 sequences the right of an assignment first: the byte is
            // drawn before the place it goes.
            let value = random.uniform(0, 255) as u8;
            let at = random.uniform(0, last) as usize;
            copy[at] = value;
            flips += 1;
        }
        answered(&read(format_id, copy, name), "bytes changed at random");
        let length = random.uniform(0, 255) as usize * 4;
        let noise: Vec<u8> = (0..length).map(|_| random.uniform(0, 255) as u8).collect();
        answered(&read(format_id, noise, name), "random bytes");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    reads
}
