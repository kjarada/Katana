#pragma once

// Road templates (docs/road_design.md, "Road templates"): the cross-section a
// corridor sweeps along an alignment and its design profile.
//
// A template is a list of COMPONENTS out from the centreline on each side,
// in order: traffic lanes, shoulders, kerbs, footpaths, verges, medians,
// table drains, and last a batter that carries the edge to the ground. Each
// has a width and a crossfall, the layer and colour its 3D objects are shown
// in, and - where it is paved or covered - its PAVEMENT COURSES from the top
// down, each a solid of its own in the road model (cad/bim/bim_object.hpp).
//
// It is plain data. The project keeps the drawing's templates as versioned
// JSON under the metadata key "road_templates" (Document::roadTemplates,
// src/katana_cad/road/road_document.cpp), every edit is one undo step through
// the ROADTEMPLATE verbs (cad/road/road_verbs.hpp), and nothing built from a
// template is stored: the corridor's sections, strings, surfaces and solids
// are made from it again each time (cad/road/road_section.hpp), so none can
// disagree with the template it came from.
//
// SIGNS. An offset is positive to the LEFT of the alignment looking along
// increasing station, as SolvedAlignment::pointAtStationOffset has it. A
// crossfall is in PERCENT and is the change of level going OUTWARD from the
// centreline: -3 falls 3 % away from it (a crowned lane), +2.5 rises (a
// footpath rising to the boundary). A batter slope is horizontal run per unit
// rise, as cad::Assembly has it: 2 is 1V:2H.
//
// The old one-carriageway cad::Assembly (cad/corridor.hpp) is a template like
// any other (templateFromAssembly), so there is one corridor engine.

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"

namespace katana::cad {
struct Assembly;
}

namespace katana::cad::road {

enum class ComponentKind { Lane, Shoulder, Kerb, Footpath, Verge, Median, Drain, Batter };
enum class KerbShape { Barrier, Mountable, KerbAndGutter, EdgeStrip };
// Which way a kerb's face looks. INWARD, the default, is a kerb on the outer
// edge of a carriageway: its lip is on the carriageway side, nearer the
// centreline, and it rises going outward to its top - the face looks in, to
// the traffic. OUTWARD is a kerb on the inner edge of a carriageway, a median's
// or an island's: the kerb is highest where it starts, against the median, and
// falls going outward to its lip, so the carriageway beyond it is lower and
// the face looks out, to the traffic on that side.
enum class KerbFacing { Inward, Outward };
enum class Side { Left, Right };

// The words the verbs and the JSON use: "lane", "shoulder", "kerb",
// "footpath", "verge", "median", "drain", "batter"; "barrier", "mountable",
// "kerbgutter", "edgestrip"; "inward", "outward"; "left", "right". Read in any
// case.
[[nodiscard]] std::string_view toString(ComponentKind kind);
[[nodiscard]] std::optional<ComponentKind> componentKindFrom(std::string_view text);
[[nodiscard]] std::string_view toString(KerbShape shape);
[[nodiscard]] std::optional<KerbShape> kerbShapeFrom(std::string_view text);
[[nodiscard]] std::string_view toString(KerbFacing facing);
[[nodiscard]] std::optional<KerbFacing> kerbFacingFrom(std::string_view text);
[[nodiscard]] std::string_view toString(Side side);
[[nodiscard]] std::optional<Side> sideFrom(std::string_view text);

// One course of a pavement, a footpath or a verge: a layer of material of a
// constant thickness under the component's surface.
struct PavementCourse {
    // A word (letters, digits, '-', '_', '.'): it names the course in a verb
    // and in its object's id, "corridor/Main/left/lane/AC14/1".
    std::string name;
    double thickness = 0.0; // metres, measured vertically
    std::string material;   // "asphalt", "crushed rock", "concrete", "topsoil" ...
    // The IfcCourse predefined type the IFC export writes (IFC 4.3
    // IfcCourseTypeEnum: ARMOUR, BALLASTBED, CORE, FILTER, PAVEMENT,
    // PROTECTION, USERDEFINED, NOTDEFINED).
    std::string ifcType = "PAVEMENT";
    katana::entity::Color colour{};

    friend bool operator==(const PavementCourse&, const PavementCourse&) = default;
};

struct Component {
    // A word, unique on its side of the template: "lane", "lane2", "kerb".
    std::string name;
    ComponentKind kind = ComponentKind::Lane;
    Side side = Side::Left;
    // Metres across, horizontally. A kerb's is its top (the back of the
    // kerb past the face); the whole kerb is wider (kerbWidth). Not used by
    // a batter, which runs to the ground.
    double width = 3.5;
    // Percent, outward (header). Not used by a kerb, whose shape is its own,
    // nor by a batter.
    double crossfall = -3.0;
    // Lanes and shoulders by default: their crossfall is the corridor's
    // superelevation table's for the station, where it has one
    // (cad/road/corridor_model.hpp).
    bool superelevated = false;
    // The drawing layer its objects are shown on, and their colour.
    std::string layer;
    katana::entity::Color colour{};
    // Top down. None for a kerb (the kerb is its own solid) and a batter.
    std::vector<PavementCourse> courses;

    // ---- a kerb ---------------------------------------------------------------
    KerbShape shape = KerbShape::Barrier;
    // Which way the face looks (KerbFacing). An edge strip has no face and
    // takes only the default.
    KerbFacing faces = KerbFacing::Inward;
    // The face's height above the lip (the pavement edge), metres. An edge
    // strip is flush and has none.
    double height = 0.150;
    // A kerb: how far below the lip its base is. A table drain: how far its
    // invert is below its inner edge. An edge strip: its thickness.
    double depth = 0.150;
    // Kerb and gutter: the channel's width and its fall towards the kerb
    // face, percent.
    double gutterWidth = 0.300;
    double gutterFall = 6.0;
    std::string material; // a kerb's; a course has its own

    // ---- a batter -------------------------------------------------------------
    // Run per rise in cut (the batter climbing to the ground) and in fill
    // (falling to it); the farthest out it may reach before the section is
    // given up as not meeting the ground; and benches: a flat of benchWidth
    // every benchHeight of rise or fall (0 for none).
    double cutSlope = 2.0;
    double fillSlope = 4.0;
    double maximumWidth = 50.0;
    double benchHeight = 0.0;
    double benchWidth = 0.0;

    friend bool operator==(const Component&, const Component&) = default;
};

struct RoadTemplate {
    // Any text but a '/', a '"' or a line break; found ignoring case.
    std::string name;
    std::string description;
    // Every component of both sides, each side's in order out from the
    // centreline: the n-th left component is the n-th one whose side is
    // Left.
    std::vector<Component> components;

    // The components of one side, in order outward.
    [[nodiscard]] std::vector<const Component*> side(Side which) const;
    // Its index in `components`, or nullopt: name compared ignoring case.
    [[nodiscard]] std::optional<std::size_t> find(Side which, std::string_view name) const;

    friend bool operator==(const RoadTemplate&, const RoadTemplate&) = default;
};

struct RoadTemplateSet {
    std::vector<RoadTemplate> templates; // in the order they were made

    [[nodiscard]] const RoadTemplate* find(std::string_view name) const;
    [[nodiscard]] RoadTemplate* find(std::string_view name);

    friend bool operator==(const RoadTemplateSet&, const RoadTemplateSet&) = default;
};

// ---- the defaults -------------------------------------------------------------------

// A component of `kind` as ROADTEMPLATE ADD makes one before its options:
// its width, crossfall, layer, colour and courses (docs/road_design.md,
// "Defaults, and where they come from"), named after its kind.
[[nodiscard]] Component defaultComponent(ComponentKind kind, Side side);

// A course as COURSE ADD makes one: its colour from its material (asphalt
// dark grey, concrete pale grey, crushed rock and gravel tan, sand pale
// tan, topsoil brown; else mid grey).
[[nodiscard]] PavementCourse makeCourse(std::string name, double thickness, std::string material,
                                        std::string ifcType = "PAVEMENT");

// What ROADTEMPLATE DEFAULTS adds: a two-lane rural road with sealed
// shoulders and batters; an urban street with kerb and gutter, footpaths
// and verges; a divided road with a median, mountable kerbs and batters.
[[nodiscard]] std::vector<RoadTemplate> standardTemplates();

// The old one-carriageway assembly as a template: a lane of its half width
// each side at its crossfall (Assembly's positive-falls-away made outward
// percent), then a batter with its slopes and maximum width. No courses, so
// its formation is its finished surface, as the assembly's was.
[[nodiscard]] RoadTemplate templateFromAssembly(const cad::Assembly& assembly);

// ---- limits -----------------------------------------------------------------------------

// The most a template's length may be - a width, a kerb's or drain's depth, a
// course's thickness, a batter's maximum width - in metres. A sanity bound,
// not a design rule, with no standard behind the number: the road a template
// describes is metres to tens of metres across, and a section is sampled every
// half metre across its whole span, so a width typed in the wrong unit would
// cost time linear in it and, past about 4.5e15 m (where adding 0.5 no longer
// moves a double), never end. 1 km is far beyond any real component and a few
// thousand samples at most.
inline constexpr double kMaximumComponentLength = 1000.0;
// The most benches a batter may make across its maximum width. A batter with a
// bench every 6 to 10 m of rise has a handful; each bench is two named points
// a section, and the cut and fill areas cost more than linearly in a
// section's points, so a bench every millimetre - a typo - is refused instead
// of taking the session with it (the same reason as kMaximumSections, a
// corridor's most sections, road_section.cpp).
inline constexpr std::size_t kMaximumBenches = 100;

// ---- checks ---------------------------------------------------------------------------

// A template name, a corridor name: not empty, no '/', '"' or control
// character. InvalidArgument naming `what`.
[[nodiscard]] katana::core::Status checkName(std::string_view what, std::string_view name);
// A component or course name: a word of letters, digits, '-', '_' and '.'.
[[nodiscard]] katana::core::Status checkWord(std::string_view what, std::string_view name);

// Everything a section needs to be built: its name; component names words,
// unique on each side; finite widths and crossfalls - a width positive, a
// crossfall within +-100 % - kerb dimensions positive (a flush edge strip's
// height excepted), a drain's depth not negative; every length at most
// kMaximumComponentLength; at most one batter a side and nothing outside it;
// batter slopes and its maximum width positive, benches both given or neither
// and no more than kMaximumBenches across the maximum width; course names
// words unique in their component, thicknesses positive, and no courses on a
// kerb or a batter.
// InvalidArgument naming the component (left:lane) and what is wrong.
[[nodiscard]] katana::core::Status checkTemplate(const RoadTemplate& roadTemplate);

// The keys ROADTEMPLATE ADD and SET take for a component of `kind`: what its
// JSON keeps (name, layer, colour for all; width, crossfall and superelevated
// for all but a batter; a kerb's shape, faces, height, depth, gutter,
// gutterfall and material; a drain's depth; a batter's cut, fill, maxwidth,
// benchheight and benchwidth). `color` and `thickness` are read as `colour`
// and `depth` and not listed. A key a kind does not keep changes nothing, so
// the verbs refuse it rather than reply "updated".
[[nodiscard]] std::vector<std::string_view> componentKeys(ComponentKind kind);

// ---- a kerb's shape ------------------------------------------------------------------

// One vertical of a solid's cross-section: its distance out from where it
// starts, and its top and bottom levels relative to that start.
struct ShapeColumn {
    double out = 0.0;
    double top = 0.0;
    double bottom = 0.0;

    friend bool operator==(const ShapeColumn&, const ShapeColumn&) = default;
};

// The kerb's cross-section from where the previous component ends (out 0,
// level 0) outward, a column at each change of its top. For a kerb facing
// INWARD that is its lip (the pavement's edge): the lip, the gutter's invert,
// the top of the face, the back. For one facing OUTWARD it is the same kerb
// mirrored: the back, the top of the face, the gutter's invert, the lip - its
// levels measured from the back, so the kerb falls to its lip. The base is
// `depth` below the lip (an edge strip: below its top), level under a barrier
// or a mountable kerb and following the channel under a gutter. The last
// column is where the next component starts: the back of the kerb, or its
// lip.
//
//   barrier      face `height` high, battered 1 in 6, then `width` of top
//   mountable    face rising `height` over 1.5 x height out, then `width`
//   kerbgutter   the gutter, `gutterWidth` falling `gutterFall` %, then a
//                barrier face and top
//   edgestrip    flush: `width` at the component's crossfall, `depth` thick;
//                it has no face and takes no facing
//
// The face is never vertical: a vertical face gives the finished surface two
// levels at one point, which a TIN cannot hold.
[[nodiscard]] std::vector<ShapeColumn> kerbShape(const Component& kerb);
// How far out the kerb reaches from its lip: its last column's `out`.
[[nodiscard]] double kerbWidth(const Component& kerb);

// ---- JSON ------------------------------------------------------------------------------

// The set as the project stores it: {"format":"katana-road-templates",
// "version":1,"templates":[...]} with every double exact. Read back: a set
// equal to the one written. ParseFailure for text that is not one (naming
// what), Unsupported for a newer version, and checkTemplate's refusal for a
// template that cannot be built.
inline constexpr int kRoadTemplatesVersion = 1;
[[nodiscard]] katana::core::Result<std::string> roadTemplatesToJson(const RoadTemplateSet& set);
[[nodiscard]] katana::core::Result<RoadTemplateSet> roadTemplatesFromJson(std::string_view text);

} // namespace katana::cad::road
