//! What the views show of a surface in its display style, on the command line
//! (include/katana/cad/terrain/surface_display_verbs.hpp,
//! src/katana_cad/terrain/surface_display_verbs.cpp; docs/surface_display.md,
//! "On the command line"): the plan view and the 3D view draw a surface for a
//! person, and these say the same in words for an agent, a script and
//! katana_cli - the legend of its colours, and what the plan view draws of it
//! at a framing.
//!
//! ```text
//! SURFACE LEGEND <name> [3D] [JSON]
//!     The colours of its faces in its style: in plan (plan_faces), or in 3D
//!     (faces3d, over the surface's own heights - a 3D view stretches an
//!     open range over every visible surface, which a verb has no view to
//!     know). One record a band, stop or sector after a head record:
//!       legend surface="ground" style="Elevation Bands" view=plan faces=elevation kind=bands entries=10
//!       entry index=1 colour=#28468C from=24.892000198364258 to=26.284700202941895 label="24.892 - 26.285"
//!     from and to are the edges exactly (the sample terrain's heights are
//!     Float32, so they show their stored values); the label is the rounded
//!     form a person reads. kind is bands, gradient (a smooth ramp or smooth
//!     stops: entries are colours from lowest to highest), sectors (aspect),
//!     flat or none. A band that takes in everything past an edge has no
//!     from or to there. JSON: the same as one object.
//!
//! SURFACE PLAN <name> [AREA x0,y0,x1,y1 | VIEW [<id>]] [pixels=W,H]
//!     What the plan view draws of it (terrain/surface_plan.rs) looking at
//!     AREA (default: the whole surface) through a view of W x H pixels
//!     (default 1600,1000), or at what the window's plan view shows - its own
//!     area at its own size and scale, so what is stroked, tinted or hidden
//!     is what that view drew; pixels=W,H frames the view's area as another
//!     size instead. VIEW EXTENTS is refused: a plan is looked at through an
//!     area. How many triangles are in view, stroked and tinted, the
//!     contours, labels, points and pixels. One record, plus the scope's.
//! ```
//!
//! Both only read, so neither is an undo step. A surface hidden in the
//! drawing is still described (visible=no says so): hiding is the picture's,
//! and the question is what its style would draw. Refused: no such surface
//! (NotFound, naming it), a word neither verb takes, a bad AREA (not four
//! numbers, or one with no width and no height) or pixels=.

use katana_core::error::make_error;
use katana_core::json::OrderedJson;
use katana_core::text::{
    equals_ignoring_case, format_exact_real, lowered, parse_integer, reply_quoted, uppered,
};
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::{Box2, Point2};
use katana_math::cpp;
use katana_terrain::{NamedSurface, TinSurface};

use crate::document::Document;
use crate::scope_verbs::{ScopeSource, ScopeViewProvider, parse_scope_words, resolve_scope};
use crate::terrain::surface_colouring::{SurfaceLegend, surface_colouring_for, surface_legend};
use crate::terrain::surface_plan::{
    SurfacePlanCache, SurfacePlanView, plan_image_grid, surface_plan,
};
use crate::terrain::surface_style::{SurfaceFaces, SurfaceStyle, style_of, surface_faces_word};
use crate::view_transform::ViewTransform;

// The plan view's frame a verb looks through when none is given: the plan
// benchmark's 1600 x 1000 (docs/plan_view.md, "The benchmark").
const DEFAULT_WIDTH_PIXELS: i32 = 1600;
const DEFAULT_HEIGHT_PIXELS: i32 = 1000;
// The largest frame a verb is asked to look through: twice a 4K display each
// way, past any view a person has. The image a frame gets is held to
// IMAGE_PIXEL_CAP whatever the frame: 16 million pixels of 4 bytes, 64
// megabytes at most (an uncapped 7680 x 7680 image would be 236).
const MAXIMUM_PIXELS: i32 = 7680;
// The pixel cap the plan painter gives a surface's image on screen
// (PlanPaintOptions::rasterPixelCap).
const IMAGE_PIXEL_CAP: usize = 16_000_000;

/// What a SURFACE LEGEND reply says, as plain data: whose legend it is, which
/// view it is read for, and the legend. The verb fills it from the drawing;
/// surface_legend_json writes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceLegendReply {
    pub surface: String,
    pub style: String,
    pub three_d: bool,
    pub faces: SurfaceFaces,
    pub legend: SurfaceLegend,
}

fn kind_of(legend: &SurfaceLegend) -> &'static str {
    match legend.faces {
        SurfaceFaces::None => "none",
        SurfaceFaces::Flat => "flat",
        SurfaceFaces::Aspect => "sectors",
        SurfaceFaces::Elevation | SurfaceFaces::Slope => {
            if legend.gradient {
                "gradient"
            } else {
                "bands"
            }
        }
    }
}

// The C++ `uppered(text)` / `lowered(text)` as text: only ASCII letters change,
// so UTF-8 stays UTF-8.
fn upper(text: &str) -> String {
    String::from_utf8_lossy(&uppered(text)).into_owned()
}

fn lower(text: &str) -> String {
    String::from_utf8_lossy(&lowered(text)).into_owned()
}

fn is(word: &str, keyword: &str) -> bool {
    equals_ignoring_case(word.as_bytes(), keyword.as_bytes())
}

// The surface of that name, with its TIN; None for none, and for one with no
// TIN.
fn surface_named<'a>(
    document: &'a Document,
    name: &str,
) -> Option<(&'a NamedSurface, &'a TinSurface)> {
    let named = document.surfaces().find(name)?;
    named.surface.as_deref().map(|surface| (named, surface))
}

fn no_surface(name: &str) -> Error {
    make_error(
        ErrorCode::NotFound,
        "no surface of that name; SURFACE LIST lists them",
        name,
    )
}

fn legend_verb(document: &Document, args: &[String]) -> Result<String> {
    if args.len() < 2 {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "SURFACE LEGEND <name> [3D] [JSON]",
        ));
    }
    let Some((named, surface)) = surface_named(document, &args[1]) else {
        return Err(no_surface(&args[1]));
    };
    let mut three_d = false;
    let mut json = false;
    for arg in &args[2..] {
        let word = upper(arg);
        if word == "3D" {
            three_d = true;
        } else if word == "JSON" {
            json = true;
        } else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "is not an option of SURFACE LEGEND <name>: 3D, JSON",
                arg.as_str(),
            ));
        }
    }
    let style = style_of(document.surface_styles(), named);
    // A wireframe or hidden surface is drawn in no colour of its faces
    // (docs/surface_display.md, "In 3D"), whatever faces3d says.
    let faces_drawn = !three_d
        || (style.shading != SurfaceStyle::Wireframe && style.shading != SurfaceStyle::Hidden);
    let faces = if !faces_drawn {
        SurfaceFaces::None
    } else if three_d {
        style.faces3d
    } else {
        style.plan_faces
    };
    let colouring = surface_colouring_for(style, faces, surface);
    let legend = surface_legend(&colouring);

    if json {
        return Ok(surface_legend_json(&SurfaceLegendReply {
            surface: named.name.clone(),
            style: style.name.clone(),
            three_d,
            faces,
            legend,
        }));
    }

    let mut reply = format!(
        "legend surface={} style={} view={} faces={} kind={} entries={}",
        reply_quoted(&named.name),
        reply_quoted(&style.name),
        if three_d { "3d" } else { "plan" },
        surface_faces_word(faces),
        kind_of(&legend),
        legend.entries.len()
    );
    for (index, entry) in (1..).zip(&legend.entries) {
        reply += &format!("\nentry index={index} colour={}", entry.colour.to_hex());
        if let Some(from) = entry.from {
            reply += &format!(" from={}", format_exact_real(from));
        }
        if let Some(to) = entry.to {
            reply += &format!(" to={}", format_exact_real(to));
        }
        if !entry.label.is_empty() {
            reply += &format!(" label={}", reply_quoted(&entry.label));
        }
    }
    Ok(reply)
}

fn read_pixels(value: &str) -> Result<(i32, i32)> {
    let comma = value.find(',');
    let width = comma.and_then(|comma| parse_integer(&value[..comma]));
    let height = comma.and_then(|comma| parse_integer(&value[comma + 1..]));
    let maximum = i64::from(MAXIMUM_PIXELS);
    match (width, height) {
        (Some(width), Some(height))
            if (1..=maximum).contains(&width) && (1..=maximum).contains(&height) =>
        {
            Ok((width as i32, height as i32))
        }
        _ => Err(make_error(
            ErrorCode::InvalidArgument,
            format!("pixels= is W,H, two whole numbers from 1 to {MAXIMUM_PIXELS}"),
            value,
        )),
    }
}

fn box_text(area: &Box2) -> String {
    format!(
        "{},{},{},{}",
        format_exact_real(area.min.x),
        format_exact_real(area.min.y),
        format_exact_real(area.max.x),
        format_exact_real(area.max.y)
    )
}

// std::clamp(static_cast<int>(std::lround(pixels)), 1, MAXIMUM_PIXELS): lround
// rounds half away from zero, as f64::round does; the cast to int keeps the
// low 32 bits as C++20 defines it, and what lround cannot represent (a NaN, an
// infinity) clamps to 1 either way.
fn view_pixels(pixels: f64) -> i32 {
    ((pixels.round() as i64) as i32).clamp(1, MAXIMUM_PIXELS)
}

fn plan_verb(document: &Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    if args.len() < 2 {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "SURFACE PLAN <name> [AREA x0,y0,x1,y1 | VIEW [id]] [pixels=W,H]",
        ));
    }
    let Some((named, surface)) = surface_named(document, &args[1]) else {
        return Err(no_surface(&args[1]));
    };
    let mut words: Vec<String> = Vec::new();
    let mut width = DEFAULT_WIDTH_PIXELS;
    let mut height = DEFAULT_HEIGHT_PIXELS;
    let mut pixels_given = false;
    for arg in &args[2..] {
        let lowered_word = lower(arg);
        if lowered_word.starts_with("pixels=") {
            (width, height) = read_pixels(&arg[7..])?;
            pixels_given = true;
            continue;
        }
        if lowered_word == "where" {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a surface is not entities: SURFACE PLAN takes AREA or VIEW, and no WHERE",
                arg.as_str(),
            ));
        }
        words.push(arg.clone());
    }

    // The window looked through: the whole surface, AREA, or what the plan
    // view shows - through the one scope parser.
    let mut area = *surface.bounds();
    let mut scope = String::from("scope=surface");
    // VIEW: the plan view's own transform, so the surface is judged at the
    // pixels per unit the view draws it at (absent: the area is fitted to
    // width x height).
    let mut view_frame: Option<ViewTransform> = None;
    if !words.is_empty() {
        let mut at = 0;
        let parsed = parse_scope_words(&words, &mut at)?;
        if at != words.len() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "is not an option of SURFACE PLAN <name>: AREA x0,y0,x1,y1, VIEW [id], \
                 pixels=W,H",
                words[at].as_str(),
            ));
        }
        if parsed.source == ScopeSource::Area {
            area = parsed.area;
            scope = format!("scope=area area={}", box_text(&area));
        } else if parsed.source == ScopeSource::View {
            // EXTENTS drops the view's area (resolve_scope), and a plan is
            // looked at through one: say so, not that the view has none.
            if parsed.extents {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "SURFACE PLAN looks at an area, and VIEW EXTENTS has none: drop EXTENTS for \
                     the view's own area, or give AREA x0,y0,x1,y1",
                    "EXTENTS",
                ));
            }
            let resolved = resolve_scope(&parsed, views)?;
            let Some(view_area) = resolved.scope.area else {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "that view has no plan area; name a plan view, or give AREA",
                    words[0].as_str(),
                ));
            };
            area = view_area;
            scope = format!(
                "scope=view view={} area={}",
                resolved.view.unwrap_or(0),
                box_text(&area)
            );
            // The view's own size and scale, unless pixels= asks for another
            // frame: what the plan view draws, not what a 1600 x 1000 one
            // would.
            if !pixels_given
                && let Some(frame) = resolved.frame
                && frame.scale > 0.0
                && frame.scale.is_finite()
            {
                width = view_pixels(frame.width_pixels);
                height = view_pixels(frame.height_pixels);
                view_frame = Some(frame);
            }
        } else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a surface is not entities: SURFACE PLAN takes AREA x0,y0,x1,y1 or VIEW",
                words[0].as_str(),
            ));
        }
    }

    // The plan view's own frame (VIEW): its box at its scale. Otherwise
    // looking at `area` as a plan view of width x height pixels frames it:
    // centred, at the scale that fits it on both sides.
    let units_per_pixel = match &view_frame {
        Some(frame) => 1.0 / frame.scale,
        None => cpp::max(
            area.width() / f64::from(width),
            area.height() / f64::from(height),
        ),
    };
    // A point has no frame: the counts of a plan that was never worked out
    // would read as "nothing of the surface is there".
    if !(units_per_pixel > 0.0) || !units_per_pixel.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "AREA has no width and no height: a plan view cannot frame a point",
            box_text(&area),
        ));
    }
    let mut visible = area;
    if view_frame.is_none() {
        let centre = area.center();
        visible = Box2::new(
            Point2::new(
                centre.x - 0.5 * f64::from(width) * units_per_pixel,
                centre.y - 0.5 * f64::from(height) * units_per_pixel,
            ),
            Point2::new(
                centre.x + 0.5 * f64::from(width) * units_per_pixel,
                centre.y + 0.5 * f64::from(height) * units_per_pixel,
            ),
        );
    }
    let grid = plan_image_grid(&visible, surface.bounds(), units_per_pixel, IMAGE_PIXEL_CAP);
    let view = SurfacePlanView {
        visible,
        units_per_pixel,
        image_box: grid.r#box,
        image_width: grid.width,
        image_height: grid.height,
        ..SurfacePlanView::default()
    };
    let mut cache = SurfacePlanCache::new();
    let style = style_of(document.surface_styles(), named);
    let plan = surface_plan(named, style, &view, &mut cache);
    let s = &plan.stats;
    let contour_state = |hidden: bool| {
        if !style.contours.show {
            "off"
        } else if hidden {
            "hidden"
        } else {
            "drawn"
        }
    };
    let mut reply = format!(
        "plan surface={} style={} visible={} pixels={width},{height} unitsPerPixel={} \
         triangles={} inView={} edges={} tinted={} faces={} image={}x{} covered={} levels={} \
         contours={} minor={} major={} labels={} points={} pointPixels={} boundary={} \
         breaklines={}",
        reply_quoted(&named.name),
        reply_quoted(&style.name),
        if named.visible { "yes" } else { "no" },
        format_exact_real(units_per_pixel),
        surface.triangle_count(),
        s.triangles_in_view,
        s.edges_stroked,
        s.triangles_tinted,
        surface_faces_word(style.plan_faces),
        plan.image.width,
        plan.image.height,
        s.pixels_covered,
        s.contour_levels,
        s.contour_runs,
        contour_state(s.minor_contours_hidden),
        contour_state(s.major_contours_hidden),
        s.labels,
        s.points_marked,
        s.points_in_image,
        s.boundary_runs,
        s.breakline_runs
    );
    if !plan.problem.is_empty() {
        reply += &format!(" problem={}", reply_quoted(&plan.problem));
    }
    reply += "\n";
    reply += &scope;
    Ok(reply)
}

/// The reply as JSON, one object (the JSON word of SURFACE LEGEND): surface,
/// style, view, faces, kind, title, entries. The C++ writes a byte of a name
/// or label that is not UTF-8 as U+FFFD and never throws; a String holds no
/// such byte.
pub fn surface_legend_json(reply: &SurfaceLegendReply) -> String {
    let entries = reply.legend.entries.iter().map(|entry| {
        OrderedJson::object_of([
            ("colour", OrderedJson::from(entry.colour.to_hex())),
            (
                "from",
                entry.from.map_or(OrderedJson::Null, OrderedJson::from),
            ),
            ("to", entry.to.map_or(OrderedJson::Null, OrderedJson::from)),
            ("label", OrderedJson::from(entry.label.as_str())),
        ])
    });
    let out = OrderedJson::object_of([
        ("surface", OrderedJson::from(reply.surface.as_str())),
        ("style", OrderedJson::from(reply.style.as_str())),
        (
            "view",
            OrderedJson::from(if reply.three_d { "3d" } else { "plan" }),
        ),
        ("faces", OrderedJson::from(surface_faces_word(reply.faces))),
        ("kind", OrderedJson::from(kind_of(&reply.legend))),
        ("title", OrderedJson::from(reply.legend.title.as_str())),
        ("entries", OrderedJson::array_of(entries)),
    ]);
    // A reply: compact, as the other JSON replies are.
    out.dump()
}

/// SURFACE followed by LEGEND or PLAN (any case): this module's lines. The
/// other SURFACE words are other modules'.
pub fn is_surface_display_verb(verb: &str, args: &[String]) -> bool {
    if !is(verb, "SURFACE") || args.is_empty() {
        return false;
    }
    is(&args[0], "LEGEND") || is(&args[0], "PLAN")
}

/// `args` are the words after SURFACE. VIEW asks `views`; headless there is
/// none, and VIEW is refused naming AREA.
pub fn run_surface_display_verb(
    document: &Document,
    args: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    if args.is_empty() {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "SURFACE LEGEND|PLAN <name> ...",
        ));
    }
    if is(&args[0], "LEGEND") {
        return legend_verb(document, args);
    }
    if is(&args[0], "PLAN") {
        return plan_verb(document, args, views);
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        "SURFACE LEGEND|PLAN <name> ...",
        args[0].as_str(),
    ))
}

/// The lines HELP prints for them.
pub fn surface_display_verb_help() -> String {
    String::from(
        "Display   SURFACE LEGEND name [3D] [JSON]   the colours of its faces in its style: one
          record a band, stop or sector (3D: faces3d over its own heights)
          SURFACE PLAN name [AREA x0,y0,x1,y1 | VIEW [id]] [pixels=W,H]   what the plan
          view draws of it there: triangles in view, stroked and tinted, contours,
          labels, points (docs/surface_display.md)
",
    )
}
