//! The twins of tests/qt_widgets/plotting/test_sheet_sections.cpp: section smarts in the sheet
//! painter (docs/plotting.md, "Section smarts") - the automatic scale and exaggeration and the
//! title block that reports them, cut and fill shaded and tabled, a chainage range shown exactly,
//! the centreline's levels and each service's level and depth, and a note dropped rather than
//! drawn over another. Painted into images at 4 px a millimetre and read back pixel by pixel; the
//! numbers themselves are tested without pixels in katana_cad's section_fit and
//! section_annotation twins.

use katana_cad::plot::PlotColourMode;
use katana_cad::plotting::frame::built_in_frame;
use katana_cad::plotting::sheet_set::{Sheet, SheetSet, Viewport, ViewportKind, scale_text};
use katana_cad::scene::SceneSurface;
use katana_entity::entity::set_heights;
use katana_entity::{Alignment, Entity, Geometry, Layer, Model};
use katana_geometry::{
    AlignmentPi, Box2, HorizontalAlignment, Point2, Point3, ProfilePvi, Segment2,
    VerticalAlignment,
};
use katana_terrain::tin_surface::TinSurface;
use katana_window::canvas::{Color, Image};
use katana_window::plan_painter::PlanSource;
use katana_window::sheet_painter::{
    SheetPaintCache, SheetPaintStats, SheetSource, resolve_section_viewport,
};

use crate::support::{
    PPMM, Painting, WHITE, bx, colour_in, ink_in, near, painted_with, pixels_of, text_in,
};

// The painter's shades and the design's ink.
const CUT: Color = Color::rgb(250, 200, 200);
const FILL: Color = Color::rgb(200, 236, 200);
const DESIGN_RED: Color = Color::rgb(210, 0, 0);

fn tin(corners: [Point3; 4]) -> TinSurface {
    TinSurface::create(corners.to_vec(), vec![[0, 1, 2], [0, 2, 3]], Vec::new())
        .expect("the four corners are a surface")
}

/// A road east along y = 50 from x 10 to x 190, over ground that is the plane z = 18 + 0.05 x +
/// 0.04 y. Chainage c (from `start`) is at x = 10 + c - start, where the ground is 20.5 + 0.05 (c -
/// start). Its design profile is flat at 25, so the two cross 90 m along: fill before, cut after.
/// Across the road the ground rises 0.04 a metre to the north - to the left, looking along it,
/// where the offsets are negative.
struct Road {
    model: Model,
    ground: TinSurface,
    design_pad: Option<TinSurface>,
}

impl Road {
    fn new(profile: bool, start: f64) -> Self {
        let mut model = Model::new();
        let pi = |x: f64| AlignmentPi {
            point: Point2::new(x, 50.0),
            ..AlignmentPi::default()
        };
        let road = Alignment {
            name: String::from("ROAD"),
            horizontal: HorizontalAlignment {
                pis: vec![pi(10.0), pi(190.0)],
                start_station: start,
            },
            vertical: profile.then(|| VerticalAlignment {
                pvis: vec![
                    ProfilePvi {
                        station: start,
                        elevation: 25.0,
                        curve_length: 0.0,
                    },
                    ProfilePvi {
                        station: start + 180.0,
                        elevation: 25.0,
                        curve_length: 0.0,
                    },
                ],
            }),
            ..Alignment::default()
        };
        model.alignments.add(road).expect("the road is added");
        Self {
            model,
            ground: tin([
                Point3::new(0.0, 0.0, 18.0),
                Point3::new(200.0, 0.0, 28.0),
                Point3::new(200.0, 100.0, 32.0),
                Point3::new(0.0, 100.0, 22.0),
            ]),
            design_pad: None,
        }
    }

    /// The road with its profile, numbered from 0.
    fn designed() -> Self {
        Self::new(true, 0.0)
    }

    /// A service along the road at `y`, at `level` (none: drawn flat), on a layer of its own.
    fn service(&mut self, layer: &str, y: f64, level: Option<f64>) {
        if !self.model.layers.contains(layer) {
            self.model
                .layers
                .add(Layer {
                    name: String::from(layer),
                    ..Layer::default()
                })
                .expect("the layer is added");
        }
        let mut entity = Entity {
            geometry: Geometry::Line(Segment2::new(Point2::new(0.0, y), Point2::new(200.0, y))),
            layer: String::from(layer),
            ..Entity::default()
        };
        if level.is_some() {
            set_heights(&mut entity.properties, &[level, level]);
        }
        self.model
            .entities
            .add(entity)
            .expect("the service is added");
    }

    fn source(&self) -> SheetSource<'_> {
        let mut source = SheetSource {
            plan: PlanSource {
                model: Some(&self.model),
                ..PlanSource::default()
            },
            ..SheetSource::default()
        };
        source.surfaces.push(SceneSurface {
            name: String::from("GROUND"),
            surface: Some(&self.ground),
            ..SceneSurface::default()
        });
        if let Some(pad) = &self.design_pad {
            source.surfaces.push(SceneSurface {
                name: String::from("DESIGN PAD"),
                surface: Some(pad),
                ..SceneSurface::default()
            });
        }
        source
    }
}

fn long_section(rect: Box2, scale: f64, exaggeration: f64, centre: Point2) -> Viewport {
    let mut viewport = Viewport {
        id: String::from("vp1"),
        kind: ViewportKind::LongSection,
        rect,
        scale,
        vertical_exaggeration: exaggeration,
        centre,
        ..Viewport::default()
    };
    viewport.source.alignment = String::from("ROAD");
    viewport
}

fn cross_section(rect: Box2, chainage: f64, scale: f64, centre: Point2) -> Viewport {
    let mut viewport = Viewport {
        id: String::from("vp2"),
        kind: ViewportKind::CrossSections,
        rect,
        scale,
        vertical_exaggeration: 1.0,
        centre,
        ..Viewport::default()
    };
    viewport.source.alignment = String::from("ROAD");
    viewport.source.stations = vec![chainage];
    viewport.source.section_half_width = 20.0;
    viewport
}

fn sheet_with(viewports: Vec<Viewport>) -> SheetSet {
    SheetSet {
        sheets: vec![Sheet {
            id: String::from("s1"),
            name: String::from("SECTIONS"),
            viewports,
            ..Sheet::default()
        }],
        ..SheetSet::default()
    }
}

fn painted_in(
    set: &SheetSet,
    source: &SheetSource<'_>,
    name: &str,
    mode: PlotColourMode,
) -> (Image, SheetPaintStats) {
    painted_with(
        set,
        source,
        &Painting {
            colour_mode: mode,
            ..Painting::named(name)
        },
    )
}

fn painted(set: &SheetSet, source: &SheetSource<'_>, name: &str) -> (Image, SheetPaintStats) {
    painted_in(set, source, name, PlotColourMode::Colour)
}

/// Pixels within 12 of `colour` on every channel: close enough to tell a shade from the grid's
/// grey and the ground's brown.
fn shade_in(image: &Image, paper: &Box2, colour: Color) -> usize {
    colour_in(image, paper, colour, 12)
}

// The long section the tests below draw: 23..410 x 150..287 on A3, its section above the 7 mm
// title strip, 157..287. The band's four rows (design, ground, cut/fill, chainage) are 32 mm, so
// the plot is laid out 45..408 x 190..285 (the band's names 22 mm to its left), middle (226.5,
// 237.5). At 1 : 1000 and 10 times, centred on (CH 90, RL 25): X(c) = 136.5 + c and Y(z) = 237.5 +
// 10 (z - 25). The plot is then cut to the chainages shown, and the band and the names go with it.
fn long_rect() -> Box2 {
    bx(23.0, 150.0, 410.0, 287.0)
}

fn long_x(chainage: f64) -> f64 {
    136.5 + chainage
}

fn long_y(level: f64) -> f64 {
    237.5 + 10.0 * (level - 25.0)
}

// The band's rows, top down: design 181..189, ground 173..181, cut/fill 165..173, chainage
// 157..165.
const CUT_FILL_ROW: f64 = 165.0;
const CHAINAGE_ROW: f64 = 157.0;

// The cross section the tests below draw: 23..210 x 35..145, its section 42..145. Under its plot
// 4 mm of offsets, a 4.5 mm caption and 1 mm; to its left the 6 mm the short levels need; so the
// plot is 29..208 x 51.5..143, middle (118.5, 97.25). At 1 : 200 true scale centred on (0,
// `level`): X(o) = 118.5 + 5 o and Y(z) = 97.25 + 5 (z - level).
fn cross_rect() -> Box2 {
    bx(23.0, 35.0, 210.0, 145.0)
}

fn cross_x(offset: f64) -> f64 {
    118.5 + 5.0 * offset
}

fn cross_y(level: f64, centre: f64) -> f64 {
    97.25 + 5.0 * (level - centre)
}

mod sheet_sections {
    use super::*;

    // ---- cut and fill ---------------------------------------------------------------------------

    #[test]
    fn cut_and_fill_are_shaded_between_the_design_and_the_ground() {
        let road = Road::designed();
        let viewport = long_section(long_rect(), 1000.0, 10.0, Point2::new(90.0, 25.0));
        let (paper, stats) = painted(
            &sheet_with(vec![viewport.clone()]),
            &road.source(),
            "cut_and_fill_are_shaded",
        );
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);

        // At CH 40 the ground is 22.5, under the design's 25: fill, green.
        let fill = bx(long_x(35.0), long_y(22.8), long_x(45.0), long_y(24.7));
        assert!(shade_in(&paper, &fill, FILL) > 40 * 76 * 8 / 10);
        assert_eq!(shade_in(&paper, &fill, CUT), 0);
        // At CH 140 the ground is 27.5, over the design: cut, red.
        let cut = bx(long_x(135.0), long_y(25.3), long_x(145.0), long_y(27.2));
        assert!(shade_in(&paper, &cut, CUT) > 40 * 76 * 8 / 10);
        assert_eq!(shade_in(&paper, &cut, FILL), 0);
        // Nothing is shaded above the higher of the two or below the lower.
        assert_eq!(
            shade_in(
                &paper,
                &bx(long_x(35.0), long_y(25.3), long_x(45.0), long_y(29.0)),
                FILL
            ),
            0
        );
        assert_eq!(
            shade_in(
                &paper,
                &bx(long_x(135.0), long_y(21.0), long_x(145.0), long_y(24.7)),
                CUT
            ),
            0
        );

        // Without a design there is nothing to shade.
        let ground = Road::new(false, 0.0);
        let (bare, _) = painted(
            &sheet_with(vec![viewport]),
            &ground.source(),
            "cut_and_fill_are_shaded_no_design",
        );
        assert_eq!(shade_in(&bare, &long_rect(), FILL), 0);
        assert_eq!(shade_in(&bare, &long_rect(), CUT), 0);
    }

    // The shading follows the plot style as every line does: in greyscale the cut and fill print
    // as the greys of their colours, not in pink and green.
    #[test]
    fn cut_and_fill_shading_follows_the_plot_style() {
        let road = Road::designed();
        let viewport = long_section(long_rect(), 1000.0, 10.0, Point2::new(90.0, 25.0));
        let (grey, _) = painted_in(
            &sheet_with(vec![viewport]),
            &road.source(),
            "cut_and_fill_shading_grey",
            PlotColourMode::Greyscale,
        );
        let fill = bx(long_x(35.0), long_y(22.8), long_x(45.0), long_y(24.7));
        let cut = bx(long_x(135.0), long_y(25.3), long_x(145.0), long_y(27.2));
        assert_eq!(colour_in(&grey, &fill, FILL, 4), 0);
        assert_eq!(colour_in(&grey, &cut, CUT, 4), 0);
        let luma = |c: Color| {
            let l = ((299 * u32::from(c.r) + 587 * u32::from(c.g) + 114 * u32::from(c.b) + 500)
                / 1000) as u8;
            Color::rgb(l, l, l)
        };
        assert!(colour_in(&grey, &fill, luma(FILL), 3) > 40 * 76 * 8 / 10);
        assert!(colour_in(&grey, &cut, luma(CUT), 3) > 40 * 76 * 8 / 10);
    }

    #[test]
    fn the_data_band_has_a_cut_and_fill_row_when_there_is_a_design() {
        let road = Road::designed();
        let viewport = long_section(long_rect(), 1000.0, 10.0, Point2::new(90.0, 25.0));
        let (paper, _) = painted(
            &sheet_with(vec![viewport.clone()]),
            &road.source(),
            "the_data_band",
        );
        // "CUT/FILL" in the names column, 21.5 mm left of the plot's CH 0...
        let names = long_x(0.0) - 21.0;
        assert!(
            text_in(
                &paper,
                &bx(
                    names,
                    CUT_FILL_ROW + 1.5,
                    names + 12.0,
                    CUT_FILL_ROW + 6.5
                )
            ) > 20
        );
        // ...and design less ground turned in each 10 m column: "+2.500" at CH 40, "0.000" at CH
        // 90, "-2.500" at CH 140.
        for chainage in [40.0, 90.0, 140.0] {
            assert!(
                text_in(
                    &paper,
                    &bx(
                        long_x(chainage) - 0.6,
                        CUT_FILL_ROW + 1.0,
                        long_x(chainage) + 0.6,
                        CUT_FILL_ROW + 7.0
                    )
                ) > 8,
                "{chainage}"
            );
        }

        // Without a design the band is the ground and the chainage, 157..173: no row names above
        // it.
        let ground = Road::new(false, 0.0);
        let (bare, _) = painted(
            &sheet_with(vec![viewport]),
            &ground.source(),
            "the_data_band_no_design",
        );
        assert!(
            text_in(
                &bare,
                &bx(
                    names,
                    CHAINAGE_ROW + 1.5,
                    names + 12.0,
                    CHAINAGE_ROW + 6.5
                )
            ) > 20
        );
        assert_eq!(text_in(&bare, &bx(names, 174.0, names + 12.0, 189.0)), 0);
    }

    #[test]
    fn a_design_profile_lies_over_the_ground_whatever_the_chainage_starts_at() {
        // The same road numbered from CH 1000, its profile too: the design is still over the
        // ground, filling to CH 1090 and cutting after.
        let road = Road::new(true, 1000.0);
        let viewport = long_section(long_rect(), 1000.0, 10.0, Point2::new(1090.0, 25.0));
        let (paper, stats) = painted(
            &sheet_with(vec![viewport]),
            &road.source(),
            "a_design_profile_from_1000",
        );
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert!(
            shade_in(
                &paper,
                &bx(long_x(35.0), long_y(22.8), long_x(45.0), long_y(24.7)),
                FILL
            ) > 40 * 76 * 8 / 10
        );
        assert!(
            shade_in(
                &paper,
                &bx(long_x(135.0), long_y(25.3), long_x(145.0), long_y(27.2)),
                CUT
            ) > 40 * 76 * 8 / 10
        );
    }

    // ---- a chainage range -----------------------------------------------------------------------

    #[test]
    fn a_long_section_with_a_range_shows_exactly_that_range_with_its_ends_labelled() {
        let road = Road::designed();
        let mut viewport = long_section(long_rect(), 1000.0, 10.0, Point2::new(90.0, 25.0));
        viewport.source.chainage_from = 30.0;
        viewport.source.chainage_to = 150.0;
        let (paper, stats) = painted(
            &sheet_with(vec![viewport.clone()]),
            &road.source(),
            "a_long_section_with_a_range",
        );
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);

        // The plot is x 166.5..286.5, the range's ends: nothing of the section - no line, no
        // shade, no band - is drawn past either. The band's names stand in the 22 mm left of it,
        // from x 145.
        assert_eq!(ink_in(&paper, &bx(long_x(152.0), 158.0, 408.0, 284.0)), 0);
        assert_eq!(
            ink_in(&paper, &bx(46.0, 158.0, long_x(30.0) - 23.0, 284.0)),
            0
        );
        // Right up to them it is drawn: the shades at both ends.
        assert!(
            shade_in(
                &paper,
                &bx(long_x(31.0), long_y(22.3), long_x(35.0), long_y(24.7)),
                FILL
            ) > 0
        );
        assert!(
            shade_in(
                &paper,
                &bx(long_x(145.0), long_y(25.3), long_x(149.0), long_y(27.5)),
                CUT
            ) > 0
        );
        // Each end is written in full just inside it, "30.000" and "150.000", down its chainage
        // row and with its levels above.
        for row in [CHAINAGE_ROW, CUT_FILL_ROW] {
            assert!(
                text_in(
                    &paper,
                    &bx(
                        long_x(30.0) + 0.7,
                        row + 1.0,
                        long_x(30.0) + 1.9,
                        row + 7.0
                    )
                ) > 8
            );
            assert!(
                text_in(
                    &paper,
                    &bx(
                        long_x(150.0) - 1.9,
                        row + 1.0,
                        long_x(150.0) - 0.7,
                        row + 7.0
                    )
                ) > 8
            );
        }

        // Without a range the range is the whole road: nothing past CH 180, and its last column
        // has its levels too.
        viewport.source.chainage_from = 0.0;
        viewport.source.chainage_to = 0.0;
        let (whole, _) = painted(
            &sheet_with(vec![viewport]),
            &road.source(),
            "a_long_section_with_a_range_whole",
        );
        assert_eq!(ink_in(&whole, &bx(long_x(182.0), 158.0, 408.0, 284.0)), 0);
        assert!(
            shade_in(
                &whole,
                &bx(long_x(170.0), long_y(25.3), long_x(178.0), long_y(28.0)),
                CUT
            ) > 0
        );
        assert!(
            text_in(
                &whole,
                &bx(
                    long_x(180.0) - 1.9,
                    CUT_FILL_ROW + 1.0,
                    long_x(180.0) - 0.7,
                    CUT_FILL_ROW + 7.0
                )
            ) > 8
        );
    }

    // ---- cross sections -------------------------------------------------------------------------

    #[test]
    fn a_cross_section_labels_the_design_and_ground_levels_at_its_centreline() {
        // At CH 60 the profile's design is 25.000 and the ground 23.500: filled 1.500. Three
        // lines, right of the centreline, just above the design.
        let road = Road::designed();
        let viewport = cross_section(cross_rect(), 60.0, 200.0, Point2::new(0.0, 24.0));
        let (paper, stats) = painted(
            &sheet_with(vec![viewport.clone()]),
            &road.source(),
            "a_cross_section_labels",
        );
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert_eq!(stats.section_notes_dropped, 0);
        let design = cross_y(25.0, 24.0);
        assert!(
            text_in(
                &paper,
                &bx(
                    cross_x(0.0) + 1.0,
                    design + 1.4,
                    cross_x(0.0) + 15.0,
                    design + 7.4
                )
            ) > 60
        );
        // Nothing left of it: the note took its first place.
        assert_eq!(
            text_in(
                &paper,
                &bx(
                    cross_x(0.0) - 15.0,
                    design + 1.4,
                    cross_x(0.0) - 1.0,
                    design + 7.4
                )
            ),
            0
        );
        // A tick across the centreline at the design's level, in the design's red.
        assert!(
            colour_in(
                &paper,
                &bx(
                    cross_x(0.0) - 0.9,
                    design - 0.3,
                    cross_x(0.0) + 0.9,
                    design + 0.3
                ),
                DESIGN_RED,
                40
            ) > 0
        );

        // Without a profile, the ground alone.
        let ground = Road::new(false, 0.0);
        let (bare, _) = painted(
            &sheet_with(vec![viewport]),
            &ground.source(),
            "a_cross_section_labels_ground",
        );
        let ground_y = cross_y(23.5, 24.0);
        assert!(
            text_in(
                &bare,
                &bx(
                    cross_x(0.0) + 1.0,
                    ground_y + 1.4,
                    cross_x(0.0) + 15.0,
                    ground_y + 2.8
                )
            ) > 20
        );
        assert_eq!(
            text_in(
                &bare,
                &bx(
                    cross_x(0.0) + 1.0,
                    ground_y + 3.4,
                    cross_x(0.0) + 15.0,
                    ground_y + 8.0
                )
            ),
            0
        );
        assert_eq!(colour_in(&bare, &cross_rect(), DESIGN_RED, 40), 0);
    }

    #[test]
    fn a_cross_section_shades_between_a_design_surface_and_the_ground() {
        // A pad at 23.5 across the whole section, level with the ground at the centreline. The
        // ground (21.5 + 0.04 y) rises to the north, left of the centreline: it is cut there, and
        // filled to the right.
        let mut road = Road::designed();
        road.design_pad = Some(tin([
            Point3::new(50.0, 30.0, 23.5),
            Point3::new(90.0, 30.0, 23.5),
            Point3::new(90.0, 70.0, 23.5),
            Point3::new(50.0, 70.0, 23.5),
        ]));
        let viewport = cross_section(cross_rect(), 60.0, 200.0, Point2::new(0.0, 23.5));
        let (paper, stats) = painted(
            &sheet_with(vec![viewport]),
            &road.source(),
            "a_cross_section_shades",
        );
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        // 8 to 12 m left the ground is 23.82 to 23.98, over the pad's 97.25: cut from y 98.85 up.
        // 8 to 12 m right it is 23.18 to 23.02: fill down to 95.65. Between the lines' pens, each
        // is all shade.
        let cut = bx(cross_x(-12.0), 97.8, cross_x(-8.0), 98.5);
        let fill = bx(cross_x(8.0), 96.0, cross_x(12.0), 96.8);
        assert!(shade_in(&paper, &cut, CUT) > 80 * 2 * 8 / 10);
        assert!(shade_in(&paper, &fill, FILL) > 80 * 3 * 8 / 10);
        assert_eq!(shade_in(&paper, &cut, FILL), 0);
        assert_eq!(shade_in(&paper, &fill, CUT), 0);
    }

    #[test]
    fn a_service_is_labelled_with_its_layer_level_and_depth_and_its_offset_under_it() {
        // Water 5 m left of the centreline (y 55) at 22.00, where the ground is 23.70: "WATER RL
        // 22.00 D 1.70", stood up left of its line from just above its marker, and "-5.00" under
        // the marker.
        let mut road = Road::designed();
        road.service("WATER", 55.0, Some(22.0));
        let viewport = cross_section(cross_rect(), 60.0, 200.0, Point2::new(0.0, 23.0));
        let (paper, stats) = painted(
            &sheet_with(vec![viewport.clone()]),
            &road.source(),
            "a_service_is_labelled",
        );
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert_eq!(stats.section_notes_dropped, 0);
        let x = cross_x(-5.0);
        let marker = cross_y(22.0, 23.0);
        // The marker: a ring at the level.
        assert!(text_in(&paper, &bx(x - 1.0, marker - 1.0, x + 1.0, marker + 1.0)) > 4);
        // The note: about 16 mm of turned text from 1.2 mm above the marker.
        assert!(text_in(&paper, &bx(x - 1.7, marker + 1.4, x - 0.5, marker + 12.0)) > 40);
        assert_eq!(
            text_in(&paper, &bx(x + 0.5, marker + 1.4, x + 1.7, marker + 12.0)),
            0
        );
        // The offset, under the marker.
        assert!(text_in(&paper, &bx(x - 1.5, marker - 2.4, x + 1.5, marker - 1.3)) > 5);

        // Hidden, it is neither drawn nor noted.
        let mut hidden = viewport;
        hidden.hidden_layers.hide("WATER");
        let (without, _) = painted(
            &sheet_with(vec![hidden]),
            &road.source(),
            "a_service_is_labelled_hidden",
        );
        assert_eq!(
            text_in(&without, &bx(x - 1.7, marker - 2.4, x + 1.7, marker + 12.0)),
            0
        );
    }

    #[test]
    fn notes_stand_clear_of_other_services_lines_and_one_with_no_room_is_dropped() {
        // Water 3 m left and gas 3.3 m left, both at 22.00: 1.5 mm apart on the paper. The water,
        // nearer the middle, is placed first; left of its line its note would lie on the gas's, so
        // it goes right, and the gas's note goes left of the gas: each beside its own line and on
        // no other.
        let mut road = Road::designed();
        road.service("WATER", 53.0, Some(22.0));
        road.service("GAS", 53.3, Some(22.0));
        let viewport = cross_section(cross_rect(), 60.0, 200.0, Point2::new(0.0, 23.0));
        let (paper, stats) = painted(
            &sheet_with(vec![viewport.clone()]),
            &road.source(),
            "notes_stand_clear",
        );
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert_eq!(stats.section_notes_dropped, 0);
        let marker = cross_y(22.0, 23.0);
        let water = cross_x(-3.0);
        let gas = cross_x(-3.3);
        assert!(
            text_in(
                &paper,
                &bx(water + 0.5, marker + 1.4, water + 1.7, marker + 12.0)
            ) > 40
        );
        assert!(
            text_in(
                &paper,
                &bx(gas - 1.7, marker + 1.4, gas - 0.5, marker + 12.0)
            ) > 40
        );
        // Between the two lines, nothing.
        assert_eq!(
            text_in(
                &paper,
                &bx(gas + 0.2, marker + 1.4, water - 0.2, marker + 12.0)
            ),
            0
        );

        // Power 3.6 m left as well: the gas, between two lines 1.5 mm either side, has no place
        // that is not on one of them. It is left out, and counted; the power's note goes left of
        // it.
        road.service("POWER", 53.6, Some(22.0));
        let (three, stats) = painted(
            &sheet_with(vec![viewport]),
            &road.source(),
            "notes_stand_clear_three",
        );
        assert_eq!(stats.section_notes_dropped, 1);
        let power = cross_x(-3.6);
        assert!(
            text_in(
                &three,
                &bx(water + 0.5, marker + 1.4, water + 1.7, marker + 12.0)
            ) > 40
        );
        assert!(
            text_in(
                &three,
                &bx(power - 1.7, marker + 1.4, power - 0.5, marker + 12.0)
            ) > 40
        );
        assert_eq!(
            text_in(
                &three,
                &bx(power + 0.2, marker + 1.4, water - 0.2, marker + 12.0)
            ),
            0
        );
    }

    // ---- the automatic scale --------------------------------------------------------------------

    #[test]
    fn an_automatic_section_scale_is_fitted_and_reported_in_the_title_block() {
        let road = Road::designed();
        // The whole drawing area: 23..410 x 35..287. The band's four rows leave the plot 45..408 x
        // 75..285, 363 x 210 mm. The 180 m road in 90% of 363 mm needs 1 : 551: 1 : 750. Its
        // levels, 20.5 to 29.5, are 9 m; at 1 : 750 90% of 210 mm allows 15.75 times: 10.
        let area = bx(23.0, 35.0, 410.0, 287.0);
        let mut viewport = long_section(area, 500.0, 1.0, Point2::default());
        viewport.auto_scale = true;
        viewport.auto_centre = true;
        let mut cache = SheetPaintCache::new();
        let fit = resolve_section_viewport(&viewport, &road.source(), &mut cache);
        assert_eq!(fit.scale, 750.0);
        assert_eq!(fit.exaggeration, 10.0);

        // Painted automatically, the sheet is the sheet painted at H 1:750 V 1:75: its section,
        // its title and its title block's scale.
        let (automatic, _) = painted(
            &sheet_with(vec![viewport.clone()]),
            &road.source(),
            "an_automatic_section_scale_auto",
        );
        let mut fixed = viewport;
        fixed.auto_scale = false;
        fixed.scale = 750.0;
        fixed.vertical_exaggeration = 10.0;
        assert_eq!(scale_text(&fixed), "H 1:750 V 1:75");
        let (as_fixed, _) = painted(
            &sheet_with(vec![fixed]),
            &road.source(),
            "an_automatic_section_scale_fixed",
        );
        assert!(automatic == as_fixed);
        // Not the sheet at the viewport's own H 1:500 V 1:500: the title block's scale cell says
        // otherwise.
        let (own, _) = painted(
            &sheet_with(vec![long_section(area, 500.0, 1.0, Point2::default())]),
            &road.source(),
            "an_automatic_section_scale_own",
        );
        let scale_cell = pixels_of(
            &built_in_frame()
                .as_ref()
                .expect("the built-in frame reads")
                .cell("scale")
                .expect("the frame has a scale cell")
                .rect,
        );
        assert!(automatic.copy(scale_cell) != own.copy(scale_cell));
    }

    #[test]
    fn automatic_cross_sections_share_one_scale_that_fits_their_width() {
        // Two cross sections, 20 m each side, in 23..210 x 35..287: each row is 122.5 mm of the
        // 245 above the title, its plot 179 x 111 mm (a 6 mm level column; the offsets and caption
        // under it). 40 m in 90% of 179 mm needs 1 : 248: 1 : 250. The deeper section spans 2.3 m
        // of level (at CH 60 the ground's 22.7 to the design's 25; at CH 120 the design's 25 to
        // the ground's 27.3): 9.2 mm at 1 : 250, so 10 times fits 90% of 111 mm and 20 does not.
        let road = Road::designed();
        let mut viewport = cross_section(
            bx(23.0, 35.0, 210.0, 287.0),
            60.0,
            100.0,
            Point2::default(),
        );
        viewport.source.stations = vec![60.0, 120.0];
        viewport.auto_scale = true;
        viewport.auto_centre = true;
        let mut cache = SheetPaintCache::new();
        let fit = resolve_section_viewport(&viewport, &road.source(), &mut cache);
        assert_eq!(fit.scale, 250.0);
        assert_eq!(fit.exaggeration, 10.0);
        let (_, stats) = painted(
            &sheet_with(vec![viewport.clone()]),
            &road.source(),
            "automatic_cross_sections",
        );
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert_eq!(stats.viewports_drawn, 1);

        // A viewport that is not automatic keeps its own.
        viewport.auto_scale = false;
        let own = resolve_section_viewport(&viewport, &road.source(), &mut cache);
        assert_eq!(own.scale, 100.0);
        assert_eq!(own.exaggeration, 1.0);
    }

    #[test]
    fn a_stored_scale_that_is_not_positive_is_reported_not_drawn() {
        // A scale read from a file unchecked: 0 would put every label at infinity.
        let road = Road::designed();
        let viewport = long_section(long_rect(), 0.0, 10.0, Point2::new(90.0, 25.0));
        let (paper, stats) = painted(
            &sheet_with(vec![viewport]),
            &road.source(),
            "a_stored_scale_that_is_not_positive",
        );
        assert_eq!(stats.problems.len(), 1);
        assert!(
            stats.problems[0].contains("not a positive number"),
            "{}",
            stats.problems[0]
        );
        assert_eq!(stats.viewports_drawn, 0);
        assert_eq!(shade_in(&paper, &long_rect(), CUT), 0);
    }

    // ---- nothing outside, nothing over another --------------------------------------------------

    #[test]
    fn nothing_of_a_section_is_drawn_outside_its_viewport() {
        // A long section with a range and cross sections full of services, on a sheet with no
        // frame: every pixel off the two rectangles (and their 0.25 mm outlines) is still the
        // paper's white.
        let mut road = Road::designed();
        road.service("WATER", 55.0, Some(22.0));
        road.service("GAS", 53.3, Some(22.5));
        road.service("POWER", 45.0, Some(30.0));
        road.service("KERB", 47.0, None);
        let mut along = long_section(
            bx(40.0, 170.0, 300.0, 260.0),
            500.0,
            10.0,
            Point2::new(90.0, 25.0),
        );
        along.source.chainage_from = 20.0;
        along.source.chainage_to = 160.0;
        along.auto_scale = true;
        along.auto_centre = true;
        let mut across = cross_section(
            bx(60.0, 40.0, 200.0, 150.0),
            60.0,
            100.0,
            Point2::new(0.0, 24.0),
        );
        across.id = String::from("vp2");
        across.source.stations = vec![40.0, 90.0, 140.0];
        across.auto_scale = true;
        across.auto_centre = true;
        let mut set = sheet_with(vec![along.clone(), across.clone()]);
        set.sheets[0].frame.clear();
        let (paper, stats) = painted(&set, &road.source(), "nothing_of_a_section_outside");
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert_eq!(stats.viewports_drawn, 2);

        let inside = |p: Point2, r: &Box2| {
            p.x > r.min.x - 0.6 && p.x < r.max.x + 0.6 && p.y > r.min.y - 0.6 && p.y < r.max.y + 0.6
        };
        let mut outside = 0;
        for y in 0..paper.height() {
            for x in 0..paper.width() {
                let at = Point2::new(
                    (f64::from(x) + 0.5) / PPMM,
                    297.0 - (f64::from(y) + 0.5) / PPMM,
                );
                if !inside(at, &along.rect)
                    && !inside(at, &across.rect)
                    && paper.pixel(x, y) != WHITE
                {
                    outside += 1;
                }
            }
        }
        assert_eq!(outside, 0);
        // And something is drawn in each.
        assert!(text_in(&paper, &along.rect) > 500);
        assert!(text_in(&paper, &across.rect) > 500);
    }

    #[test]
    fn axis_values_step_out_until_they_clear_each_other() {
        // A road from CH 100000 drawn at 1 : 5 in a viewport too low for its band (40 mm: the plot
        // is 26 mm with the band's 32 under it), so its chainages are written under the plot, at
        // y 159.7..161.2. At 200 mm a metre the 10 mm grid would be 0.05 m, and "100090.05" is
        // wider than 10 mm less the 2 mm between labels: the step goes to 0.1 m, 20 mm.
        let road = Road::new(true, 100_000.0);
        let mut viewport = long_section(bx(23.0, 150.0, 410.0, 190.0), 5.0, 1.0, Point2::default());
        viewport.auto_centre = true;
        let (paper, _) = painted(
            &sheet_with(vec![viewport]),
            &road.source(),
            "axis_values_step_out",
        );

        // The columns with ink along the labels' strip, and the runs of them: glyphs of one label
        // are under 1 mm apart, labels at least 2 mm. The widest gap in a label is its decimal
        // point's, which sits on the baseline under the strip: 0.75 mm in Windows' Arial.
        let strip = pixels_of(&bx(24.0, 159.8, 409.0, 161.1));
        let mut inked: Vec<i32> = Vec::new();
        for x in strip.x..=strip.right() {
            for y in strip.y..=strip.bottom() {
                let cell = bx(
                    f64::from(x) / PPMM,
                    297.0 - f64::from(y + 1) / PPMM,
                    f64::from(x + 1) / PPMM,
                    297.0 - f64::from(y) / PPMM,
                );
                if text_in(&paper, &cell) > 0 {
                    inked.push(x);
                    break;
                }
            }
        }
        assert!(!inked.is_empty());
        let mut labels: Vec<(i32, i32)> = vec![(inked[0], inked[0])];
        for &x in &inked {
            let last = labels.len() - 1;
            if x - labels[last].1 > (1.0 * PPMM) as i32 {
                labels.push((x, x));
            } else {
                labels[last].1 = x;
            }
        }
        assert!(labels.len() >= 10, "{}", labels.len());
        for i in 1..labels.len() {
            // Each at least 2 mm clear of the one before, and the grid 20 mm.
            assert!(
                labels[i].0 - labels[i - 1].1 >= (2.0 * PPMM) as i32 - 1,
                "{i}"
            );
            let pitch = f64::from((labels[i].0 + labels[i].1) - (labels[i - 1].0 + labels[i - 1].1))
                / (2.0 * PPMM);
            assert!(near(pitch, 20.0, 1.0), "{i}: {pitch}");
        }
    }

    // ---- a sheet to look at ---------------------------------------------------------------------

    #[test]
    fn a_sheet_of_a_long_section_and_service_cross_sections() {
        // What a drafter would plot: the road's long section from CH 20 to 160 across the top, and
        // three cross sections through its services below, all automatic. Written to
        // KATANA_SHEET_PNG to be looked at.
        let mut road = Road::designed();
        road.service("WATER", 55.0, Some(22.0));
        road.service("GAS", 53.3, Some(22.5));
        road.service("POWER", 42.0, Some(30.0));
        road.service("KERB", 46.0, None);
        let mut along = long_section(
            bx(24.0, 160.0, 409.0, 286.0),
            500.0,
            10.0,
            Point2::default(),
        );
        along.source.chainage_from = 20.0;
        along.source.chainage_to = 160.0;
        along.auto_scale = true;
        along.auto_centre = true;
        let mut across = cross_section(
            bx(24.0, 36.0, 409.0, 157.0),
            60.0,
            100.0,
            Point2::default(),
        );
        across.source.stations = vec![40.0, 140.0];
        across.auto_scale = true;
        across.auto_centre = true;
        let (_, stats) = painted(
            &sheet_with(vec![along, across]),
            &road.source(),
            "a_sheet_of_a_long_section_and_service_cross_sections",
        );
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert_eq!(stats.viewports_drawn, 2);
        assert_eq!(stats.section_notes_dropped, 0);
    }

    #[test]
    fn what_cannot_be_drawn_is_reported_not_counted_as_drawn() {
        let road = Road::designed();
        // A range that lies off the plot of a fixed centre: 30 m to 50 m at 1 : 1000, centred
        // 500 m away.
        let mut off = long_section(long_rect(), 1000.0, 10.0, Point2::new(590.0, 25.0));
        off.source.chainage_from = 30.0;
        off.source.chainage_to = 50.0;
        let (_, stats) = painted(
            &sheet_with(vec![off]),
            &road.source(),
            "what_cannot_be_drawn_off",
        );
        assert_eq!(stats.problems.len(), 1);
        assert!(
            stats.problems[0].contains("off the plot"),
            "{}",
            stats.problems[0]
        );
        assert_eq!(stats.viewports_drawn, 0);

        // A viewport too small for any plot.
        let tiny = long_section(
            bx(23.0, 150.0, 40.0, 162.0),
            1000.0,
            10.0,
            Point2::new(90.0, 25.0),
        );
        let (_, stats) = painted(
            &sheet_with(vec![tiny]),
            &road.source(),
            "what_cannot_be_drawn_tiny",
        );
        assert_eq!(stats.problems.len(), 1);
        assert!(
            stats.problems[0].contains("no room"),
            "{}",
            stats.problems[0]
        );
        assert_eq!(stats.viewports_drawn, 0);

        // A chainage that is not a finite number, as a file could hold.
        let mut infinite = long_section(long_rect(), 1000.0, 10.0, Point2::new(90.0, 25.0));
        infinite.source.chainage_to = f64::INFINITY;
        let (_, stats) = painted(
            &sheet_with(vec![infinite]),
            &road.source(),
            "what_cannot_be_drawn_infinite",
        );
        assert_eq!(stats.problems.len(), 1);
        assert!(
            stats.problems[0].contains("not a finite number"),
            "{}",
            stats.problems[0]
        );
        assert_eq!(stats.viewports_drawn, 0);
    }
}
