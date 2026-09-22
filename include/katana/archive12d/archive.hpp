#pragma once

// The contents of a 12d Archive file (.12da), as typed values.
//
// A 12da is the text interchange format of 12d Model: a stream of commands
// (`model`, `colour`, `style`, `breakline`, `null`) and brace-delimited
// elements - strings, tins, super tins, trimeshes. This header is the whole
// vocabulary of the "12d Archive File Format" chapter of the 12d Model
// reference manual (V15), element for element; `coverage.hpp` lists them and a
// test holds that list against the reader so the two cannot drift.
//
// TWO THINGS SHAPE THIS MODEL.
//
// 1. 12d Model writes far more than its manual documents. A real export
//    carries `drawables`, `geometry_modifiers`, `equalities`, `extrude_value`,
//    `vertex_uid_data`, `time_created`, `weight` and dozens more, and the
//    parts of a super alignment other than IPs are declared undocumented by
//    the manual itself. So nothing here pretends to be closed:
//      * a scalar `key value` the reader has no member for is kept, in order,
//        in the element's `extras` (a FieldList) - nothing scalar is lost;
//      * a BLOCK it has no member for is counted by path in
//        `Archive::unrecognised`, so an import can say exactly what it did not
//        take rather than implying it took everything (PLAN.MD section 36).
//
// 2. The superseded string types ARE super strings. The manual says of 2d, 3d,
//    4d, pipe and polyline strings that each "has been superseded by the super
//    string", and a super string can express every one of them. They are read
//    into the same `VertexString`, tagged with the kind they came from, so
//    that everything downstream handles one shape of string, not eight.
//
// Coordinates are kept as written. `z` is optional because 12d has a null
// height - the `null` keyword, or the null value (-999 unless a `null` command
// says otherwise) - and a null is not a zero.

#include <array>
#include <cstddef>
#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

namespace katana::archive12d {

// ---- attributes (manual section 1.3) ---------------------------------------

struct Attribute;
using AttributeList = std::vector<Attribute>;

struct Attribute {
    std::string name;
    // A group nests: `group { name "X" attributes { ... } }`.
    std::variant<std::int64_t, double, std::string, AttributeList> value;
    // The type keyword as written, when it is not one of the four the manual
    // documents. Real exports carry `uid "this id" 5292`; keeping the word
    // means it is written back as it was read.
    std::string declaredType;

    friend bool operator==(const Attribute& a, const Attribute& b);
};

// ---- scalar fields ---------------------------------------------------------

struct Field {
    std::string key; // lower case
    std::string value;
    bool quoted = false; // written in double quotes

    friend bool operator==(const Field&, const Field&) = default;
};

// An ordered `key value` record: a pit, a pipe, a plot frame, the properties
// of an annotation, and the fields of any element that this model has no
// member for. Ordered and duplicate-tolerant, because it has to reproduce what
// was read; lookups take the LAST occurrence, which is what 12d does with a
// repeated key.
class FieldList {
  public:
    void add(std::string key, std::string value, bool quoted = false);
    void setReal(std::string_view key, double value);
    void setInteger(std::string_view key, std::int64_t value);
    void setText(std::string_view key, std::string value);

    // Removes every occurrence of `key` and returns the last, which is how a
    // reader moves a field it DOES have a member for out of the catch-all.
    std::optional<Field> take(std::string_view key);

    [[nodiscard]] const Field* find(std::string_view key) const;
    [[nodiscard]] bool contains(std::string_view key) const { return find(key) != nullptr; }
    // nullopt when absent, null, or not a number. A quoted number counts.
    [[nodiscard]] std::optional<double> real(std::string_view key) const;
    [[nodiscard]] std::optional<std::int64_t> integer(std::string_view key) const;
    [[nodiscard]] std::optional<bool> boolean(std::string_view key) const;
    [[nodiscard]] std::string text(std::string_view key, std::string_view fallback = {}) const;

    [[nodiscard]] bool empty() const { return fields_.empty(); }
    [[nodiscard]] std::size_t size() const { return fields_.size(); }
    [[nodiscard]] const std::vector<Field>& fields() const { return fields_; }

    friend bool operator==(const FieldList&, const FieldList&) = default;

  private:
    std::vector<Field> fields_;
};

// ---- geometry --------------------------------------------------------------

struct Vertex {
    double x = 0.0;
    double y = 0.0;
    std::optional<double> z; // nullopt: a null height

    friend bool operator==(const Vertex&, const Vertex&) = default;
};

enum class SegmentKind { Straight, Arc, Spiral, Curve, Parabola };

// The geometry between two consecutive vertices (manual 1.5.8.2).
struct Segment {
    SegmentKind kind = SegmentKind::Straight;
    // Arc. SIGNED: a positive radius turns to the RIGHT travelling from the
    // first vertex to the second, a negative one to the left. The manual's
    // wording ("+ve is above the line connecting the vertices") is ambiguous
    // about which way is up; this was settled against 12d Model's own output,
    // where all 18 arcs that follow a straight in `test Super Alignment.12da`
    // agree - 10 positive all turning right, 8 negative all turning left.
    double radius = 0.0;
    // Arc: take the larger of the two arcs of this radius between the vertices.
    bool major = false;
    // Spiral: type leading l1 r1 a1 l2 r2 a2. Curve: type leading xorigin
    // yorigin radius length start end angle offset mvalue. Parabola (vertical
    // geometry only): chainage height - the VIP of the curve.
    FieldList parameters;

    friend bool operator==(const Segment&, const Segment&) = default;
};

enum class Breakline { Point, Line };

// What every string carries.
struct StringHeader {
    std::string name;
    std::string model;  // resolved: the string's own, else the current `model` command
    std::string colour; // resolved likewise; a NAME - the RGB lives in the 12d project
    std::string style;
    double chainage = 0.0;
    Breakline breakline = Breakline::Line;
    AttributeList attributes;
    // Scalar fields with no member above: weight, time_created, time_updated,
    // and whatever else 12d Model wrote.
    FieldList extras;

    friend bool operator==(const StringHeader&, const StringHeader&) = default;
};

enum class StringKind { Super, TwoD, ThreeD, FourD, Pipe, Polyline, Face, Interface };

[[nodiscard]] std::string_view toString(StringKind kind);

// `string super`, and every string type that is a list of vertices.
//
// Per-vertex lists hold one entry per vertex; per-segment lists one per
// segment - n-1 for an open string of n vertices, n for a closed one, and the
// manual allows n for an open string too (the last is kept for when the string
// is closed). A list that is empty means "not given", not "all default".
struct VertexString {
    StringKind kind = StringKind::Super;
    StringHeader header;
    bool closed = false;
    // The one height of a `data_2d` string (`z value`) or a 2d string.
    std::optional<double> constantZ;
    std::vector<Vertex> vertices;
    std::vector<Segment> segments; // empty: every segment is straight

    std::vector<std::string> pointIds; // vertex ids; alphanumeric
    std::optional<std::string> vertexTextValue;
    std::vector<std::string> vertexText;
    std::optional<std::string> segmentTextValue;
    std::vector<std::string> segmentText;
    std::vector<std::string> segmentColours;

    // Pipes and culverts: one size for the string, or one per segment. The
    // manual says a string has one or the other; 12d Model writes both on a
    // culvert, and both are kept.
    std::optional<double> diameter;
    std::vector<double> diameters;
    std::optional<std::array<double, 2>> culvert; // width, height
    std::vector<std::array<double, 2>> culverts;
    std::string justify; // bottom|invert|top|obvert|centre; empty: not given

    std::optional<bool> vertexTinableValue;
    std::vector<bool> vertexTinable;
    std::optional<bool> segmentTinableValue;
    std::vector<bool> segmentTinable;
    std::optional<bool> vertexVisibleValue;
    std::vector<bool> vertexVisible;
    std::optional<bool> segmentVisibleValue;
    std::vector<bool> segmentVisible;

    // How vertex and segment text is drawn: angle offset raise textstyle slant
    // xfactor worldsize|papersize|screensize justify colour. A 4d string's
    // string-level text properties arrive as `vertexAnnotation`.
    std::optional<FieldList> vertexAnnotation;
    std::vector<FieldList> vertexAnnotations;
    std::optional<FieldList> segmentAnnotation;
    std::vector<FieldList> segmentAnnotations;
    // style colour size rotation offset raise
    std::optional<FieldList> symbol;
    std::vector<FieldList> symbols;

    std::vector<AttributeList> vertexAttributes;
    std::vector<AttributeList> segmentAttributes;
    std::optional<FieldList> interval; // chord_arc distance

    // Interface strings: -1 cut, 0 on the surface, 1 fill, per vertex.
    std::vector<int> interfaceModes;

    [[nodiscard]] std::size_t segmentCount() const;

    friend bool operator==(const VertexString&, const VertexString&) = default;
};

// `string arc`: a helix in general - the ends may differ in height.
struct ArcString {
    StringHeader header;
    double radius = 0.0; // signed as Segment::radius is
    Vertex centre;
    Vertex start;
    Vertex end;

    friend bool operator==(const ArcString&, const ArcString&) = default;
};

// `string circle`, and `string feature` - a circle with a height at its
// centre, which is what 12d calls a feature.
struct CircleString {
    StringHeader header;
    bool feature = false;
    double radius = 0.0;
    Vertex centre;

    friend bool operator==(const CircleString&, const CircleString&) = default;
};

struct TextString {
    StringHeader header;
    std::string text;
    Vertex position;
    // angle offset raise textstyle slant xfactor worldsize|papersize|screensize
    // justify, and the undocumented text_colour, border_style, whiteout.
    FieldList annotation;

    friend bool operator==(const TextString&, const TextString&) = default;
};

struct PlotFrame {
    StringHeader header;
    // title_file border viewport title_1 title_2 plot_file text_size sheet_code
    // width height scale rotation xorigin yorigin *_margin plotter textstyle
    FieldList fields;

    friend bool operator==(const PlotFrame&, const PlotFrame&) = default;
};

// One pit, pipe, property control or house connection of a drainage string.
struct DrainageRecord {
    FieldList fields;
    AttributeList attributes;
    // A property control has geometry of its own.
    std::vector<Vertex> vertices;
    std::vector<Segment> segments;

    friend bool operator==(const DrainageRecord&, const DrainageRecord&) = default;
};

// `string drainage` - the manual's water string.
struct DrainageString {
    StringHeader header;
    std::optional<double> outfall;
    // 0: the string runs from downstream to upstream.
    std::optional<int> flowDirection;
    std::vector<Vertex> vertices;
    std::vector<Segment> segments;
    std::vector<DrainageRecord> pits; // in order along the string
    bool pitsAreVersion2 = false;     // written as pit_v2
    std::vector<DrainageRecord> pipes; // pipe i joins pit i to pit i + 1
    std::vector<DrainageRecord> propertyControls;
    std::vector<DrainageRecord> houseConnections;

    friend bool operator==(const DrainageString&, const DrainageString&) = default;
};

// One construction step of a super alignment. For the IP method - the only
// one the manual documents - `kind` is ip|arc|spiral horizontally and
// ip|kvalue|length|radius|asymmetric|arc vertically. Any other kind is one of
// 12d Model's undocumented methods (computator, floating_arc_end_radius_length
// ...): its scalar fields are kept and its presence means the alignment is not
// defined by IPs alone.
struct AlignmentPart {
    std::string kind;
    FieldList fields;
    AttributeList attributes;

    friend bool operator==(const AlignmentPart&, const AlignmentPart&) = default;
};

// The solved geometry of a super alignment: horizontally (x, y) vertices,
// vertically (chainage, height), with the segments between them.
struct SolvedGeometry {
    FieldList header; // name chainage breakline colour style closed ...
    bool closed = false;
    std::vector<Vertex> vertices;
    std::vector<Segment> segments; // empty: every segment is straight
    std::optional<FieldList> interval; // chord_arc distance
    // 12d Model colours the elements of a solved alignment by kind - straights
    // one colour, arcs another - with the super string's per-segment block.
    std::vector<std::string> segmentColours;

    friend bool operator==(const SolvedGeometry&, const SolvedGeometry&) = default;
};

enum class AlignmentSource { SuperAlignment, Alignment, Pipeline };

// `string super_alignment`, and the superseded `alignment` and `pipeline`
// strings, whose hipdata and vipdata ARE an IP-method definition and are read
// into `horizontalParts` and `verticalParts` as one.
struct SuperAlignment {
    AlignmentSource source = AlignmentSource::SuperAlignment;
    StringHeader header;
    bool closed = false;
    std::string spiralType; // empty: not given, which 12d reads as clothoid
    std::optional<bool> validHorizontal;
    std::optional<bool> validVertical;
    std::vector<AlignmentPart> horizontalParts;
    std::vector<AlignmentPart> verticalParts;
    std::optional<SolvedGeometry> horizontalData;
    std::optional<SolvedGeometry> verticalData;
    // Pipeline strings only.
    std::optional<double> diameter;
    std::optional<double> pipeLength;

    // True when every part is one of the documented IP-method kinds.
    [[nodiscard]] bool horizontalIsIpOnly() const;
    [[nodiscard]] bool verticalIsIpOnly() const;

    friend bool operator==(const SuperAlignment&, const SuperAlignment&) = default;
};

// One LAS point record. Fields the record format does not carry are zero;
// `format` on the cloud says which are meaningful.
struct LasPoint {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;
    std::uint16_t intensity = 0;
    std::uint8_t returnNumber = 0;
    std::uint8_t returnCount = 0;
    std::uint8_t classificationFlags = 0; // formats 6+
    std::uint8_t scannerChannel = 0;      // formats 6+
    std::uint8_t scanDirection = 0;
    std::uint8_t flightLineEdge = 0;
    std::uint8_t classification = 0;
    std::int16_t scanAngle = 0;
    std::uint8_t userData = 0;
    std::uint16_t pointSourceId = 0;
    double gpsTime = 0.0;
    std::uint64_t colour = 0; // LAS colour packed by 12d into 64 bits
    std::uint8_t nearInfrared = 0;

    friend bool operator==(const LasPoint&, const LasPoint&) = default;
};

// `string las_cloud_data`: points inline, or a reference to a LAS file.
struct LasCloud {
    StringHeader header;
    std::string format; // v10_p0 ... v14_p10; empty for a reference
    int pointFormat = 0; // the Y of vX_pY
    std::vector<bool> categories;
    FieldList range; // xmin xmax ymin ymax
    std::vector<LasPoint> points;
    std::string referenceFile; // ref_data: file_name

    friend bool operator==(const LasCloud&, const LasCloud&) = default;
};

// `tin` and `full_tin` (manual 1.4.7).
//
// Indices are ZERO-BASED here; the file's are one-based. A `full_tin` begins
// with four construction points - the corners of a rectangle around the data -
// and lists every triangle including the nulled ones, with `nulling` saying
// which are visible. A `tin` lists visible triangles only and has neither.
// Triangles are CLOCKWISE seen from above, the opposite of terrain::TinSurface.
struct Tin {
    bool full = false;
    std::string name;
    std::string colour;
    AttributeList attributes;
    FieldList extras;
    std::vector<Vertex> points;
    std::vector<std::array<std::uint32_t, 3>> triangles;
    // full_tin only; 0 in the file (no neighbour) is kNoNeighbour here.
    std::vector<std::array<std::uint32_t, 3>> neighbours;
    // full_tin only, one per triangle: true when the triangle is VISIBLE (the
    // file's 2). Anything else in the file - 1, and the 0 the 12d API reports
    // for construction triangles - is not.
    std::vector<bool> visible;
    // Per-triangle colour names; "-1" means the base colour.
    std::vector<std::string> colours;
    FieldList input; // preserve_strings remove_bubbles weed_tin ...
    std::vector<std::string> inputModels;

    static constexpr std::uint32_t kNoNeighbour = 0xFFFFFFFFu;

    // Whether triangle `index` is part of the surface: visible, and not a
    // construction triangle of a full_tin (one touching its first four points).
    [[nodiscard]] bool isSurfaceTriangle(std::size_t index) const;

    friend bool operator==(const Tin&, const Tin&) = default;
};

// `super_tin`: an ordered list of tins by name. Later members override earlier
// ones where they overlap.
struct SuperTin {
    std::string name;
    std::string colour;
    AttributeList attributes;
    FieldList extras;
    std::vector<std::string> tins;

    friend bool operator==(const SuperTin&, const SuperTin&) = default;
};

struct TrimeshInfo {
    std::int64_t flag = 0;
    std::int64_t key = 0;
    std::string colour;
    std::string name;

    friend bool operator==(const TrimeshInfo&, const TrimeshInfo&) = default;
};

// `primitive_3d { trimesh_3d { ... } }`: a closed or open mesh of triangles,
// counter-clockwise seen from OUTSIDE. Indices zero-based here.
struct Trimesh {
    std::string name;
    std::string model;
    std::string colour;
    AttributeList attributes;
    FieldList extras;
    std::vector<Vertex> vertices;
    std::vector<std::array<std::uint32_t, 3>> faces;
    std::vector<std::array<std::uint32_t, 2>> edges;
    FieldList info; // flag key colour name
    std::optional<double> blend;
    // Optional per-element presentation: a table of infos, and for each
    // vertex, edge or face a ONE-based index into it, 0 for none (the
    // manual's example, 1.4.9: two infos, flags 2 0 1 2 0).
    std::vector<TrimeshInfo> vertexInfos;
    std::vector<std::uint32_t> vertexFlags;
    std::vector<TrimeshInfo> edgeInfos;
    std::vector<std::uint32_t> edgeFlags;
    std::vector<TrimeshInfo> faceInfos;
    std::vector<std::uint32_t> faceFlags;

    friend bool operator==(const Trimesh&, const Trimesh&) = default;
};

using Element = std::variant<VertexString, ArcString, CircleString, TextString, PlotFrame,
                             DrainageString, SuperAlignment, LasCloud, Tin, SuperTin, Trimesh>;

// The keyword an element is written under: "string super", "full_tin" ...
[[nodiscard]] std::string elementKeyword(const Element& element);

// A model that was declared with a block of its own (`model { name attributes }`),
// which is how 12d Model attaches attributes to a model.
struct ModelRecord {
    std::string name;
    AttributeList attributes;
    FieldList extras;

    friend bool operator==(const ModelRecord&, const ModelRecord&) = default;
};

struct Archive {
    // The `// key value` settings 12d Model writes at the head of a file as
    // comments: archive_version, decimal_places, output_hex_floats ... They are
    // comments, so nothing depends on them, but a reader who wants to know
    // which 12d wrote a file should not have to open it in an editor.
    std::vector<Field> headerSettings;
    AttributeList projectAttributes;
    std::vector<ModelRecord> models;
    // Every model named by a command or an element, in first-use order.
    std::vector<std::string> modelNames;
    std::vector<Element> elements;
    double nullValue = -999.0;

    // Blocks the reader has no member for, by path ("string super_alignment/
    // drawables"), with how many times each was seen.
    std::map<std::string, std::size_t> unrecognised;
    std::vector<std::string> warnings;

    friend bool operator==(const Archive&, const Archive&) = default;
};

} // namespace katana::archive12d
