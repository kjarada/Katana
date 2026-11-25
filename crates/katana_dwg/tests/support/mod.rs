//! What the dwg tests share: a Katana drawing built by hand, written to DWG and read back; a
//! drawing as another program writes one, built with acadrust; the comparisons; the standard's
//! mt19937 (as katana_dxf's test support has it); and a scratch folder per test.
//!
//! No DWG file is committed: every DWG a test reads is made in the test, from values the test
//! states, so that what is expected is worked out from them and not from a file of unknown
//! origin (docs/dwg.md, "Tests").

use std::path::PathBuf;

use katana_dwg::{DwgRelease, read_dwg, write_dwg};
use katana_dxf::{DxfImport, ExportOptions, ImportOptions};
use katana_entity::entity::set_heights;
use katana_entity::{Color, Entity, EntityId, Geometry, Layer, Model};
use katana_geometry::Point2;

/// EXPECT_NEAR(actual, expected, epsilon).
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

pub fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b, 255)
}

/// Adds `geometry` on `layer` with `heights` (none when empty); its id.
#[track_caller]
pub fn add(
    model: &mut Model,
    geometry: impl Into<Geometry>,
    layer: &str,
    heights: &[Option<f64>],
) -> EntityId {
    let mut entity = Entity {
        geometry: geometry.into(),
        layer: layer.to_owned(),
        ..Entity::default()
    };
    if !heights.is_empty() {
        set_heights(&mut entity.properties, heights);
    }
    match model.entities.add(entity) {
        Ok(id) => id,
        Err(error) => panic!("{}", error.describe()),
    }
}

#[track_caller]
pub fn add_layer(model: &mut Model, layer: Layer) {
    if let Err(error) = model.layers.add(layer) {
        panic!("{}", error.describe());
    }
}

/// `model` as a DWG of `release`, the whole of it.
#[track_caller]
pub fn dwg_of(model: &Model, release: DwgRelease) -> Vec<u8> {
    match write_dwg(model, &ExportOptions::default(), release) {
        Ok(written) => written.bytes,
        Err(error) => panic!("{}: {}", release.name(), error.describe()),
    }
}

/// `bytes` read as a DWG with the default options.
#[track_caller]
pub fn read(bytes: &[u8]) -> DxfImport {
    match read_dwg(bytes, &ImportOptions::default()) {
        Ok(imported) => imported,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// `model` written as `release` and read back.
#[track_caller]
pub fn round_trip(model: &Model, release: DwgRelease) -> DxfImport {
    read(&dwg_of(model, release))
}

pub fn layer_named<'a>(imported: &'a DxfImport, name: &str) -> Option<&'a Layer> {
    imported.layers.iter().find(|layer| layer.name == name)
}

/// Whether some warning holds `part`.
pub fn warned(imported: &DxfImport, part: &str) -> bool {
    imported
        .warnings
        .iter()
        .any(|warning| warning.contains(part))
}

/// std::mt19937, whose sequence the standard fixes (katana_dxf's test support has the same).
pub struct Mt19937 {
    state: [u32; 624],
    index: usize,
}

impl Mt19937 {
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

    /// `random() % bound`, 0 for a bound of 0.
    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (u64::from(self.next_u32()) % bound as u64) as usize
    }
}

/// A folder of the test's own, unique to the test and the process, removed on drop.
pub struct Scratch {
    pub root: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("katana_dwg_{name}_{}", std::process::id()));
        if root.exists() {
            let _ = std::fs::remove_dir_all(&root);
        }
        std::fs::create_dir_all(&root).expect("the scratch folder is made");
        Self { root }
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Drawings as another program writes them, built with acadrust - the library Katana reads DWG
/// with, standing in for that program - so that a test reads a DWG Katana did not write: blocks
/// and inserts, a true colour, paper space, the R12 format. Every value is stated here, and
/// each test works out from these what Katana must make of them.
pub mod foreign {
    use acadrust::entities::{Circle, Insert, Line, Text};
    use acadrust::tables::{BlockRecord, Layer};
    use acadrust::{CadDocument, Color, DwgWriter, DxfVersion, EntityType, Vector3};

    #[track_caller]
    fn added(document: &mut CadDocument, entity: EntityType) {
        if let Err(error) = document.add_entity(entity) {
            panic!("{error}");
        }
    }

    #[track_caller]
    pub fn bytes_of(document: &CadDocument) -> Vec<u8> {
        match DwgWriter::write_to_vec(document) {
            Ok(bytes) => bytes,
            Err(error) => panic!("{error}"),
        }
    }

    /// An R2018 site, by hand:
    ///
    /// ```text
    /// layer Kerb    true colour (250, 5, 5)       layer Trees   index 3 (green)
    /// on Kerb:      a line (0,0,0)-(30,40,0); a circle at (100,100) radius 5;
    ///               a text "Straße 7" at (10,20), height 2.5
    /// block TREE:   a circle at its origin, radius 1.5, on layer 0
    /// on Trees:     an insert of TREE at (200,300), scaled 2 in X and Y
    /// paper space:  a line (0,0)-(420,297) - a sheet's border, not the model
    /// ```
    pub fn site(version: DxfVersion) -> CadDocument {
        let mut document = CadDocument::with_version(version);
        for layer in [
            Layer::with_color("Kerb", Color::from_rgb(250, 5, 5)),
            Layer::with_color("Trees", Color::Index(3)),
        ] {
            if let Err(error) = document.layers.add(layer) {
                panic!("{error}");
            }
        }
        let mut line = Line::from_coords(0.0, 0.0, 0.0, 30.0, 40.0, 0.0);
        line.common.layer = String::from("Kerb");
        added(&mut document, EntityType::Line(line));
        let mut circle = Circle::from_coords(100.0, 100.0, 0.0, 5.0);
        circle.common.layer = String::from("Kerb");
        added(&mut document, EntityType::Circle(circle));
        let mut text = Text::with_value("Straße 7", Vector3::new(10.0, 20.0, 0.0)).with_height(2.5);
        text.common.layer = String::from("Kerb");
        added(&mut document, EntityType::Text(text));

        // A block's entities are its record's: an entity owned by the record goes into it
        // (acadrust's document.rs, add_entity, which routes by the owner handle).
        let mut record = BlockRecord::new("TREE");
        record.handle = document.allocate_handle();
        record.block_entity_handle = document.allocate_handle();
        record.block_end_handle = document.allocate_handle();
        let owner = record.handle;
        if let Err(error) = document.block_records.add(record) {
            panic!("{error}");
        }
        let mut canopy = Circle::from_coords(0.0, 0.0, 0.0, 1.5);
        canopy.common.owner_handle = owner;
        added(&mut document, EntityType::Circle(canopy));
        let mut tree = Insert::new("TREE", Vector3::new(200.0, 300.0, 0.0)).with_scale(2.0, 2.0, 1.0);
        tree.common.layer = String::from("Trees");
        added(&mut document, EntityType::Insert(tree));

        let border = Line::from_coords(0.0, 0.0, 0.0, 420.0, 297.0, 0.0);
        if let Err(error) = document.add_paper_space_entity(EntityType::Line(border)) {
            panic!("{error}");
        }
        document
    }

    /// Two lines on layer 0 of an R2018 drawing, the first damaged: its start is not a number.
    /// The second runs (5,5)-(15,5).
    pub fn one_line_damaged() -> CadDocument {
        let mut document = CadDocument::with_version(DxfVersion::AC1032);
        added(
            &mut document,
            EntityType::Line(Line::from_coords(f64::NAN, 0.0, 0.0, 10.0, 0.0, 0.0)),
        );
        added(
            &mut document,
            EntityType::Line(Line::from_coords(5.0, 5.0, 0.0, 15.0, 5.0, 0.0)),
        );
        document
    }

    /// An R12 drawing (AC1009, R11's format too): a line (1,2)-(4,6) and a circle at (5,3)
    /// radius 2, on layer 0.
    pub fn release_12() -> CadDocument {
        let mut document = CadDocument::with_version(DxfVersion::AC1009);
        added(
            &mut document,
            EntityType::Line(Line::from_coords(1.0, 2.0, 0.0, 4.0, 6.0, 0.0)),
        );
        added(
            &mut document,
            EntityType::Circle(Circle::from_coords(5.0, 3.0, 0.0, 2.0)),
        );
        document
    }
}
