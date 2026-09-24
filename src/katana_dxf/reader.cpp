#include "katana/dxf/reader.hpp"

#include <algorithm>
#include <cmath>
#include <fstream>
#include <iterator>
#include <map>
#include <numbers>
#include <unordered_map>
#include <utility>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/dxf/codes.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/math/numerics.hpp"
#include "tokenizer.hpp"

namespace katana::dxf {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::Geometry;
using katana::entity::Layer;
using katana::entity::Linetype;
using katana::entity::LinetypeElement;
using katana::entity::PointGeometry;
using katana::entity::PropertyMap;
using katana::entity::TextGeometry;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
using detail::Pair;
using detail::toInteger;
using detail::toReal;
using detail::trimmedValue;

namespace {

constexpr double kDegrees = std::numbers::pi / 180.0;
constexpr double kTwoPi = 2.0 * std::numbers::pi;
// A text a person reads is one line; MTEXT's default line pitch is five thirds
// of the text height.
constexpr double kLinePitch = 5.0 / 3.0;
// The width of a character as a fraction of its height, for the few places a
// text's extent must be guessed without a font: the entity layer's figure.
constexpr double kGlyphAspect = 0.6;

std::string upper(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        if (c >= 'a' && c <= 'z') {
            c = static_cast<char>(c - 'a' + 'A');
        }
    }
    return out;
}

bool equalsUpper(std::string_view text, std::string_view upperWord)
{
    if (text.size() != upperWord.size()) {
        return false;
    }
    for (std::size_t i = 0; i < text.size(); ++i) {
        char c = text[i];
        if (c >= 'a' && c <= 'z') {
            c = static_cast<char>(c - 'a' + 'A');
        }
        if (c != upperWord[i]) {
            return false;
        }
    }
    return true;
}

struct Vec3 {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;
};

Vec3 cross(const Vec3& a, const Vec3& b)
{
    return {a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x};
}

Vec3 normalised(const Vec3& v)
{
    const double length = std::sqrt(v.x * v.x + v.y * v.y + v.z * v.z);
    if (!(length > 0.0) || !std::isfinite(length)) {
        return {0.0, 0.0, 1.0};
    }
    return {v.x / length, v.y / length, v.z / length};
}

// x' = a x + b y + tx, y' = c x + d y + ty: where an entity's own coordinates
// land in plan. An object coordinate system, a block insertion, or both.
struct Affine {
    double a = 1.0;
    double b = 0.0;
    double c = 0.0;
    double d = 1.0;
    double tx = 0.0;
    double ty = 0.0;

    [[nodiscard]] Point2 apply(const Point2& p) const
    {
        return Point2(a * p.x + b * p.y + tx, c * p.x + d * p.y + ty);
    }
    [[nodiscard]] Vec2 linear(const Vec2& v) const
    {
        return Vec2(a * v.x + b * v.y, c * v.x + d * v.y);
    }
    [[nodiscard]] double determinant() const { return a * d - b * c; }
    [[nodiscard]] bool isIdentity() const
    {
        return a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0 && tx == 0.0 && ty == 0.0;
    }
    // Rotation, uniform scale and perhaps a mirror: a circle stays a circle.
    [[nodiscard]] bool isSimilarity() const
    {
        const double first = a * a + c * c;
        const double second = b * b + d * d;
        const double dot = a * b + c * d;
        const double scale = std::max(first, second);
        return scale > 0.0 && std::abs(first - second) <= 1e-9 * scale &&
               std::abs(dot) <= 1e-9 * scale;
    }
    [[nodiscard]] double largestScale() const
    {
        return std::sqrt(std::max(a * a + c * c, b * b + d * d));
    }
    // this after `inner`: inner is applied first.
    [[nodiscard]] Affine after(const Affine& inner) const
    {
        Affine out;
        out.a = a * inner.a + b * inner.c;
        out.b = a * inner.b + b * inner.d;
        out.c = c * inner.a + d * inner.c;
        out.d = c * inner.b + d * inner.d;
        out.tx = a * inner.tx + b * inner.ty + tx;
        out.ty = c * inner.tx + d * inner.ty + ty;
        return out;
    }
};

// The object coordinate system of an entity whose extrusion direction is
// `normal`, as a map onto the plan, by the format's arbitrary axis algorithm.
// `z` is the entity's elevation in that system, which moves it in plan only
// when the plane is tilted.
Affine objectToPlan(const Vec3& normalIn, double z)
{
    const Vec3 normal = normalised(normalIn);
    constexpr double kArbitraryAxisLimit = 1.0 / 64.0;
    const Vec3 xAxis = normalised(std::abs(normal.x) < kArbitraryAxisLimit &&
                                          std::abs(normal.y) < kArbitraryAxisLimit
                                      ? cross(Vec3{0.0, 1.0, 0.0}, normal)
                                      : cross(Vec3{0.0, 0.0, 1.0}, normal));
    const Vec3 yAxis = normalised(cross(normal, xAxis));
    Affine out;
    out.a = xAxis.x;
    out.b = yAxis.x;
    out.c = xAxis.y;
    out.d = yAxis.y;
    out.tx = z * normal.x;
    out.ty = z * normal.y;
    return out;
}

bool isPlainExtrusion(const Vec3& normal)
{
    return normal.x == 0.0 && normal.y == 0.0 && normal.z > 0.0;
}

double sweepBetween(double startDegrees, double endDegrees)
{
    double sweep = std::fmod(endDegrees - startDegrees, 360.0);
    if (sweep <= 0.0) {
        sweep += 360.0;
    }
    return sweep * kDegrees;
}

// The arc a bulge describes from `from` to `to`: bulge = tan(angle / 4),
// positive counter-clockwise. nullopt for a straight segment.
std::optional<Arc2> bulgeArc(const Point2& from, const Point2& to, double bulge)
{
    if (bulge == 0.0 || !std::isfinite(bulge)) {
        return std::nullopt;
    }
    const Vec2 chord = to - from;
    const double length = chord.length();
    if (!(length > katana::math::tolerance::kGeometric)) {
        return std::nullopt;
    }
    // Centre off the chord's midpoint along its left normal by
    // (length / 2) (1 - b^2) / (2 b): at b = 1 (a semicircle) the midpoint,
    // small b far to the left, b > 1 (more than half a turn) to the right.
    const Vec2 unit = chord / length;
    const Vec2 left(-unit.y, unit.x);
    const Point2 middle = from + chord * 0.5;
    const Point2 centre = middle + left * (length * 0.5 * (1.0 - bulge * bulge) / (2.0 * bulge));
    const double radius = length * 0.25 * (1.0 + bulge * bulge) / std::abs(bulge);
    const Vec2 start = from - centre;
    return Arc2{centre, radius, std::atan2(start.y, start.x), 4.0 * std::atan(bulge)};
}

// One vertex of a polyline in its own coordinates.
struct Vertex {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;
    double bulge = 0.0;
};

// Everything most entities carry: group 8, 62, 420, 6, 370, 60, 67, 210-230.
struct Common {
    std::string_view layer = "0";
    std::optional<int> colour;
    std::optional<long> trueColour;
    std::string_view linetype;
    std::optional<int> lineweight;
    bool invisible = false;
    bool paperSpace = false;
    Vec3 extrusion{0.0, 0.0, 1.0};

    // True when the pair was one of these.
    bool take(const Pair& pair)
    {
        switch (pair.code) {
        case 8:
            layer = trimmedValue(pair.value);
            return true;
        case 62:
            if (const auto value = toInteger(pair.value)) {
                colour = static_cast<int>(*value);
            }
            return true;
        case 420:
            if (const auto value = toInteger(pair.value)) {
                trueColour = static_cast<long>(*value);
            }
            return true;
        case 6:
            linetype = trimmedValue(pair.value);
            return true;
        case 370:
            if (const auto value = toInteger(pair.value)) {
                lineweight = static_cast<int>(*value);
            }
            return true;
        case 60:
            invisible = toInteger(pair.value).value_or(0) == 1;
            return true;
        case 67:
            paperSpace = toInteger(pair.value).value_or(0) == 1;
            return true;
        case 210:
            extrusion.x = toReal(pair.value).value_or(0.0);
            return true;
        case 220:
            extrusion.y = toReal(pair.value).value_or(0.0);
            return true;
        case 230:
            extrusion.z = toReal(pair.value).value_or(1.0);
            return true;
        default:
            return false;
        }
    }
};

struct Record {
    std::string_view type;
    std::vector<Pair> pairs;
    // VERTEX records of a POLYLINE, ATTRIB records of an INSERT. Only the
    // first childCount are this record's: the vector is reused.
    std::vector<Record> children;
    std::size_t childCount = 0;

    void clear()
    {
        pairs.clear();
        childCount = 0;
    }
    Record& addChild()
    {
        if (childCount == children.size()) {
            children.emplace_back();
        }
        Record& child = children[childCount++];
        child.clear();
        return child;
    }
};

// A block's entity, in the block's coordinates, remembering what it takes
// from the insert that places it.
struct BlockEntity {
    Entity entity;
    bool layerFromInsert = false;  // it is on layer 0
    bool colourFromInsert = false; // its colour is ByBlock
};

struct Block {
    std::string name;
    Point2 base;
    double baseZ = 0.0;
    int flags = 0;
    std::vector<Record> records;
    // Filled the first time the block is inserted, then reused: a survey
    // drawing inserts one symbol thousands of times.
    std::vector<BlockEntity> expanded;
    bool built = false;
    bool building = false;
};

// Where the entities being converted are going: straight into the drawing,
// or into a block's expansion, which is then placed insert by insert.
struct Sink {
    std::vector<BlockEntity>* block = nullptr; // null: the drawing
    std::string_view kind;                     // the tally the output counts to
    std::size_t depth = 0;                     // of block nesting
    std::string blockPath;                     // metadata of what it produces
};

struct TextPlacement {
    Point2 position;
    double height = 0.0;
    double rotation = 0.0;
};

class Reader {
  public:
    Reader(std::string_view text, const ImportOptions& options)
        : tokens_(text), options_(options)
    {
    }

    Result<DxfImport> run();

  private:
    // ---- structure ----
    void readBody(Record& record);
    void readChildren(Record& record);
    bool readRecord(Record& record, bool& endOfSection);
    void readHeader();
    void readTables();
    void readLayer(const Record& record);
    void readLinetype(const Record& record);
    void readBlocks();
    void readEntities();

    // ---- conversion ----
    void convert(const Record& record, Sink& sink);
    void emit(Entity entity, const Common& common, Sink& sink,
              const std::vector<std::optional<double>>* heights = nullptr);
    void convertLine(const Record& record, Sink& sink);
    void convertPoint(const Record& record, Sink& sink);
    void convertCircle(const Record& record, Sink& sink, bool arc);
    void convertEllipse(const Record& record, Sink& sink);
    void convertLwPolyline(const Record& record, Sink& sink);
    void convertPolyline(const Record& record, Sink& sink);
    void convertText(const Record& record, Sink& sink, bool attribute,
                     const PropertyMap* properties = nullptr);
    void convertMText(const Record& record, Sink& sink);
    void convertInsert(const Record& record, Sink& sink);
    void convertDimension(const Record& record, Sink& sink);
    void emitPolyline(std::vector<Vertex> vertices, bool closed, bool threeD, const Affine& frame,
                      const Common& common, Sink& sink);
    Block* buildBlock(std::string_view name, std::size_t depth, std::string_view insertKind);
    void place(const Block& block, const Affine& transform, double zScale, double zOffset,
               const Common& insert, const PropertyMap& attributes, Sink& sink);

    // ---- tables ----
    const std::string& layerPath(std::string_view dxfName);
    std::optional<Color> colourOf(const Common& common) const;

    // ---- bookkeeping ----
    EntityTally& tally(std::string_view kind);
    void warn(const std::string& category) { ++warningCounts_[category]; }
    bool full() const { return result_.entities.size() >= options_.maximumEntities; }
    // A block's expansion counts against the same limit: blocks nested ten
    // deep that each insert the one below ten times are ten billion entities
    // before one reaches the drawing.
    bool full(const Sink& sink) const
    {
        return sink.block != nullptr ? sink.block->size() >= options_.maximumEntities : full();
    }
    // True, and said once, when the import has made as many entities as it may.
    bool reportFull(const Sink& sink);

    detail::Tokenizer tokens_;
    const ImportOptions& options_;
    DxfImport result_;
    double defaultTextHeight_ = 2.5;
    std::string codePage_;

    struct LayerEntry {
        Layer layer;
        std::string dxfName;
    };
    std::vector<LayerEntry> layers_;
    std::unordered_map<std::string, std::size_t> layerIndex_; // upper-case DXF name
    std::string lastLayerName_;
    std::size_t lastLayer_ = 0;
    bool haveLastLayer_ = false;
    std::unordered_map<std::string, std::string> linetypeNames_; // upper-case -> Katana
    std::unordered_map<std::string, Block> blocks_;              // upper-case name
    std::unordered_map<std::string, std::size_t> tallyIndex_;
    std::map<std::string, std::size_t> warningCounts_;
    bool fullReported_ = false;
};

// ---- structure ----------------------------------------------------------------

void Reader::readBody(Record& record)
{
    record.pairs.clear();
    Pair pair;
    while (tokens_.next(pair)) {
        if (pair.code == 0) {
            tokens_.pushBack(pair);
            return;
        }
        record.pairs.push_back(pair);
    }
}

// A POLYLINE's VERTEX records, or an INSERT's ATTRIB records, up to the SEQEND
// that closes them. A missing SEQEND ends the list at the next other record.
void Reader::readChildren(Record& record)
{
    const bool polyline = record.type == "POLYLINE";
    bool attributes = false;
    if (record.type == "INSERT") {
        for (const Pair& pair : record.pairs) {
            if (pair.code == 66) {
                attributes = toInteger(pair.value).value_or(0) == 1;
            }
        }
    }
    if (!polyline && !attributes) {
        return;
    }
    const std::string_view childType = polyline ? "VERTEX" : "ATTRIB";
    Pair pair;
    while (tokens_.next(pair)) {
        if (pair.code != 0) {
            continue;
        }
        const std::string_view type = trimmedValue(pair.value);
        if (type == childType) {
            Record& child = record.addChild();
            child.type = type;
            readBody(child);
            continue;
        }
        if (type == "SEQEND") {
            Record ignored;
            readBody(ignored);
            return;
        }
        tokens_.pushBack(pair);
        warn(std::string(record.type) + " without the SEQEND that ends its " +
             std::string(childType) + " list");
        return;
    }
}

// The next record of a section into `record`: false at the end of the text.
// `endOfSection` is set at ENDSEC (or at an EOF met before one).
bool Reader::readRecord(Record& record, bool& endOfSection)
{
    Pair pair;
    while (tokens_.next(pair)) {
        if (pair.code != 0) {
            continue; // a stray pair between records: nothing can own it
        }
        const std::string_view type = trimmedValue(pair.value);
        if (type == "ENDSEC" || type == "EOF") {
            if (type == "EOF") {
                tokens_.pushBack(pair);
            }
            endOfSection = true;
            return true;
        }
        record.clear();
        record.type = type;
        readBody(record);
        readChildren(record);
        return true;
    }
    return false;
}

void Reader::readHeader()
{
    Pair pair;
    std::string_view variable;
    while (tokens_.next(pair)) {
        if (pair.code == 0) {
            if (trimmedValue(pair.value) != "ENDSEC") {
                tokens_.pushBack(pair);
            }
            return;
        }
        if (pair.code == 9) {
            variable = trimmedValue(pair.value);
            continue;
        }
        if (variable == "$ACADVER" && pair.code == 1) {
            result_.version = std::string(trimmedValue(pair.value));
            result_.release = std::string(releaseName(result_.version));
        } else if (variable == "$DWGCODEPAGE" && pair.code == 3) {
            codePage_ = std::string(trimmedValue(pair.value));
        } else if (variable == "$TEXTSIZE" && pair.code == 40) {
            if (const auto size = toReal(pair.value); size && *size > 0.0) {
                defaultTextHeight_ = *size;
            }
        }
    }
}

void Reader::readTables()
{
    Record record;
    bool end = false;
    while (!end && readRecord(record, end)) {
        if (end) {
            break;
        }
        if (record.type == "LAYER") {
            readLayer(record);
        } else if (record.type == "LTYPE") {
            readLinetype(record);
        }
    }
}

// A layer name as a Katana layer path. One that is already a valid path is
// kept - a "/" in it nests, which is how a file written by an older export
// brings its tree back. Otherwise it is mended level by level: a control
// character becomes "_", the blanks around a level go, and a level that is
// then empty, "." or ".." becomes "_"; past the depth a path may have, the
// levels are joined with "_" instead. Never an invalid path: "_" at worst.
std::string pathForLayerName(std::string_view name)
{
    if (name.empty()) {
        return std::string(katana::entity::kDefaultLayerName);
    }
    if (katana::entity::validateLayerPath(name)) {
        return std::string(name);
    }
    std::vector<std::string> levels;
    for (std::string_view rest = name;;) {
        const std::size_t slash = rest.find('/');
        std::string level(katana::core::trimmed(rest.substr(0, slash)));
        for (char& c : level) {
            const auto byte = static_cast<unsigned char>(c);
            if (byte < 0x20 || byte == 0x7F) {
                c = '_';
            }
        }
        if (level.empty() || level == "." || level == "..") {
            level = "_" + level;
        }
        levels.push_back(std::move(level));
        if (slash == std::string_view::npos) {
            break;
        }
        rest.remove_prefix(slash + 1);
    }
    const char joiner = levels.size() > katana::entity::kMaximumLayerDepth ? '_' : '/';
    std::string fixed;
    for (const std::string& level : levels) {
        if (!fixed.empty()) {
            fixed.push_back(joiner);
        }
        fixed += level;
    }
    if (katana::entity::validateLayerPath(fixed)) {
        return fixed;
    }
    return "_"; // too long, or bytes that are not UTF-8: nothing of it can be kept safely
}

void Reader::readLayer(const Record& record)
{
    std::string_view name;
    Layer layer;
    std::optional<int> colour;
    std::optional<long> rgb;
    int flags = 0;
    std::string_view linetype;
    std::optional<int> weight;
    std::string_view xdataApplication;
    std::optional<std::string> katanaPath;
    for (const Pair& pair : record.pairs) {
        switch (pair.code) {
        case 2:
            name = trimmedValue(pair.value);
            break;
        case 62:
            if (const auto value = toInteger(pair.value)) {
                colour = static_cast<int>(*value);
            }
            break;
        case 420:
            if (const auto value = toInteger(pair.value)) {
                rgb = static_cast<long>(*value);
            }
            break;
        case 70:
            flags = static_cast<int>(toInteger(pair.value).value_or(0));
            break;
        case 6:
            linetype = trimmedValue(pair.value);
            break;
        case 370:
            if (const auto value = toInteger(pair.value)) {
                weight = static_cast<int>(*value);
            }
            break;
        case 1001:
            xdataApplication = trimmedValue(pair.value);
            break;
        case 1000:
            if (xdataApplication == kApplicationName && !katanaPath) {
                const std::string path = plainText(pair.value);
                if (katana::entity::validateLayerPath(path)) {
                    katanaPath = path;
                }
            }
            break;
        default:
            break;
        }
    }
    if (name.empty()) {
        warn("LAYER table entry with no name");
        return;
    }
    const std::string key = upper(name);
    if (layerIndex_.contains(key)) {
        return; // the first definition stands
    }
    layer.name = katanaPath ? *katanaPath : pathForLayerName(plainText(name));
    // A negative colour is the layer switched off; frozen (flag 1) hides it too.
    if (colour) {
        layer.visible = *colour >= 0 && (flags & 1) == 0;
        if (const auto indexed = indexedColour(std::abs(*colour))) {
            layer.color = *indexed;
        }
    } else {
        layer.visible = (flags & 1) == 0;
    }
    if (rgb) {
        layer.color = trueColour(*rgb);
    }
    layer.locked = (flags & 4) != 0;
    if (!linetype.empty() && !equalsUpper(linetype, "CONTINUOUS") &&
        !equalsUpper(linetype, "BYLAYER") && !equalsUpper(linetype, "BYBLOCK")) {
        // Resolved against the LTYPE table once both are read.
        layer.linetype = std::string(linetype);
    }
    if (weight && *weight >= 0) {
        layer.lineWeight = static_cast<double>(*weight) / 100.0;
    }
    layerIndex_.emplace(key, layers_.size());
    layers_.push_back({std::move(layer), std::string(name)});
}

// A DXF pattern as Katana's: runs of dashes (or of gaps) merged, the ends
// joined when they are the same kind, and started on a dash - DXF allows a
// pattern to begin with a gap or hold two dashes together, Katana's must
// alternate. Empty for a pattern that is all dash (continuous) or all gap.
std::vector<LinetypeElement> normalisedPattern(const std::vector<double>& lengths)
{
    std::vector<LinetypeElement> merged;
    for (const double length : lengths) {
        if (!std::isfinite(length)) {
            return {};
        }
        const bool down = length >= 0.0;
        if (!merged.empty() && merged.back().isPenDown() == down) {
            // Two pen-down runs make a dash (a dot and a dash, a dash); two
            // gaps a longer gap.
            merged.back().length += length;
            continue;
        }
        merged.push_back({length});
    }
    if (merged.size() >= 2 && merged.front().isPenDown() == merged.back().isPenDown()) {
        merged.front().length += merged.back().length;
        merged.pop_back();
    }
    const bool anyGap = std::any_of(merged.begin(), merged.end(),
                                    [](const LinetypeElement& e) { return e.isGap(); });
    const bool anyDash = std::any_of(merged.begin(), merged.end(),
                                     [](const LinetypeElement& e) { return e.isPenDown(); });
    if (!anyGap || !anyDash) {
        return {};
    }
    if (merged.front().isGap()) {
        std::rotate(merged.begin(), merged.begin() + 1, merged.end());
    }
    return merged;
}

void Reader::readLinetype(const Record& record)
{
    std::string_view name;
    std::string_view description;
    std::vector<double> lengths;
    for (const Pair& pair : record.pairs) {
        switch (pair.code) {
        case 2:
            name = trimmedValue(pair.value);
            break;
        case 3:
            description = pair.value;
            break;
        case 49:
            lengths.push_back(toReal(pair.value).value_or(0.0));
            break;
        default:
            break;
        }
    }
    if (name.empty() || equalsUpper(name, "BYLAYER") || equalsUpper(name, "BYBLOCK") ||
        equalsUpper(name, "CONTINUOUS")) {
        return;
    }
    const std::string key = upper(name);
    if (linetypeNames_.contains(key)) {
        return;
    }
    Linetype linetype;
    linetype.name = plainText(name);
    linetype.description = plainText(description);
    if (!lengths.empty()) {
        linetype.pattern = normalisedPattern(lengths);
        if (linetype.pattern.empty()) {
            warn("linetype pattern that is all dash or all gap, read as continuous");
        }
    }
    if (!katana::entity::validate(linetype)) {
        warn("linetype Katana cannot draw, read as continuous");
        linetype.pattern.clear();
    }
    if (!katana::entity::isValidUtf8(linetype.name) || !katana::entity::validate(linetype)) {
        return;
    }
    linetypeNames_.emplace(key, linetype.name);
    result_.linetypes.push_back(std::move(linetype));
}

void Reader::readBlocks()
{
    Record record;
    bool end = false;
    Block* current = nullptr;
    while (!end && readRecord(record, end)) {
        if (end) {
            break;
        }
        if (record.type == "BLOCK") {
            Block block;
            for (const Pair& pair : record.pairs) {
                switch (pair.code) {
                case 2:
                    block.name = std::string(trimmedValue(pair.value));
                    break;
                case 10:
                    block.base.x = toReal(pair.value).value_or(0.0);
                    break;
                case 20:
                    block.base.y = toReal(pair.value).value_or(0.0);
                    break;
                case 30:
                    block.baseZ = toReal(pair.value).value_or(0.0);
                    break;
                case 70:
                    block.flags = static_cast<int>(toInteger(pair.value).value_or(0));
                    break;
                default:
                    break;
                }
            }
            const std::string key = upper(block.name);
            if (block.name.empty() || blocks_.contains(key)) {
                current = nullptr; // nameless, or a repeat: its content is not reachable
                continue;
            }
            current = &blocks_.emplace(key, std::move(block)).first->second;
            continue;
        }
        if (record.type == "ENDBLK") {
            current = nullptr;
            continue;
        }
        if (current != nullptr) {
            Record& stored = current->records.emplace_back();
            stored.type = record.type;
            stored.pairs = record.pairs;
            for (std::size_t i = 0; i < record.childCount; ++i) {
                Record& child = stored.addChild();
                child.type = record.children[i].type;
                child.pairs = record.children[i].pairs;
            }
        }
    }
}

void Reader::readEntities()
{
    Record record;
    bool end = false;
    Sink drawing;
    while (!end && readRecord(record, end)) {
        if (end) {
            break;
        }
        drawing.kind = record.type;
        convert(record, drawing);
    }
}

// ---- tables --------------------------------------------------------------------

const std::string& Reader::layerPath(std::string_view dxfName)
{
    if (dxfName.empty()) {
        dxfName = katana::entity::kDefaultLayerName;
    }
    if (haveLastLayer_ && dxfName == lastLayerName_) {
        return layers_[lastLayer_].layer.name;
    }
    const std::string key = upper(dxfName);
    auto found = layerIndex_.find(key);
    if (found == layerIndex_.end()) {
        // Named by an entity and missing from the table: the defaults.
        Layer layer;
        layer.name = pathForLayerName(plainText(dxfName));
        found = layerIndex_.emplace(key, layers_.size()).first;
        layers_.push_back({std::move(layer), std::string(dxfName)});
    }
    lastLayerName_ = std::string(dxfName);
    lastLayer_ = found->second;
    haveLastLayer_ = true;
    return layers_[lastLayer_].layer.name;
}

std::optional<Color> Reader::colourOf(const Common& common) const
{
    if (common.trueColour) {
        return trueColour(*common.trueColour);
    }
    if (common.colour) {
        return indexedColour(std::abs(*common.colour));
    }
    return std::nullopt;
}

EntityTally& Reader::tally(std::string_view kind)
{
    auto found = tallyIndex_.find(std::string(kind));
    if (found == tallyIndex_.end()) {
        found = tallyIndex_.emplace(std::string(kind), result_.tally.size()).first;
        result_.tally.push_back({std::string(kind), 0, 0});
    }
    return result_.tally[found->second];
}

// ---- conversion ----------------------------------------------------------------

bool Reader::reportFull(const Sink& sink)
{
    if (!full(sink)) {
        return false;
    }
    if (!fullReported_) {
        fullReported_ = true;
        warn("the import stopped at " + std::to_string(options_.maximumEntities) +
             " entities, the most one import makes");
    }
    return true;
}

// The entity, validated, onto the sink: into the drawing with its layer and
// colour resolved, or into a block expansion remembering what it inherits.
void Reader::emit(Entity entity, const Common& common, Sink& sink,
                  const std::vector<std::optional<double>>* heights)
{
    if (const auto status = katana::entity::validate(entity.geometry); !status) {
        warn(std::string(sink.kind) + " not imported: " + status.error().message);
        return;
    }
    if (reportFull(sink)) {
        return;
    }
    if (heights != nullptr) {
        katana::entity::setHeights(entity.properties, *heights);
    }
    const bool fromInsert = common.layer.empty() || common.layer == "0";
    const bool byBlock = !common.trueColour && common.colour && *common.colour == kColourByBlock;
    if (!byBlock && !(common.colour && *common.colour == kColourByLayer)) {
        entity.color = colourOf(common);
    }
    entity.visible = !common.invisible;
    if (!common.linetype.empty() && !equalsUpper(common.linetype, "BYLAYER")) {
        const auto name = linetypeNames_.find(upper(common.linetype));
        entity.metadata[std::string(kMetaLinetype)] =
            name != linetypeNames_.end() ? name->second : plainText(common.linetype);
    }
    if (common.lineweight && *common.lineweight >= 0) {
        entity.metadata[std::string(kMetaLineweight)] =
            static_cast<std::int64_t>(*common.lineweight);
    }
    if (!sink.blockPath.empty()) {
        entity.metadata[std::string(kMetaBlock)] = sink.blockPath;
    }
    if (sink.block != nullptr) {
        entity.layer = std::string(common.layer.empty() ? std::string_view("0") : common.layer);
        sink.block->push_back({std::move(entity), fromInsert, byBlock});
        return;
    }
    entity.layer = layerPath(common.layer);
    if (!options_.sourceName.empty()) {
        entity.metadata[std::string(kMetaSource)] = options_.sourceName;
    }
    result_.entities.push_back(std::move(entity));
    ++tally(sink.kind).imported;
}

void Reader::convert(const Record& record, Sink& sink)
{
    const std::string_view type = record.type;
    if (sink.block == nullptr) {
        ++tally(type).read;
        for (const Pair& pair : record.pairs) {
            if (pair.code == 67 && toInteger(pair.value).value_or(0) == 1) {
                warn("paper space entities not imported (a Katana drawing is the model)");
                return;
            }
        }
    }
    if (type == "LINE") {
        convertLine(record, sink);
    } else if (type == "POINT") {
        convertPoint(record, sink);
    } else if (type == "CIRCLE") {
        convertCircle(record, sink, false);
    } else if (type == "ARC") {
        convertCircle(record, sink, true);
    } else if (type == "ELLIPSE") {
        convertEllipse(record, sink);
    } else if (type == "LWPOLYLINE") {
        convertLwPolyline(record, sink);
    } else if (type == "POLYLINE") {
        convertPolyline(record, sink);
    } else if (type == "TEXT") {
        convertText(record, sink, false);
    } else if (type == "MTEXT") {
        convertMText(record, sink);
    } else if (type == "INSERT") {
        convertInsert(record, sink);
    } else if (type == "DIMENSION") {
        convertDimension(record, sink);
    } else if (type == "ATTDEF" || type == "SEQEND" || type == "ENDBLK" ||
               type == "VIEWPORT") {
        // ATTDEF is the template an ATTRIB replaced; a viewport is paper space
        // furniture. Neither is drawing content.
    } else if (sink.block == nullptr) {
        warn(std::string(type) + " entities not imported (Katana has no such entity)");
    }
}

void Reader::convertLine(const Record& record, Sink& sink)
{
    Common common;
    double x1 = 0.0, y1 = 0.0, z1 = 0.0, x2 = 0.0, y2 = 0.0, z2 = 0.0;
    bool bad = false;
    for (const Pair& pair : record.pairs) {
        if (common.take(pair)) {
            continue;
        }
        double* target = nullptr;
        switch (pair.code) {
        case 10: target = &x1; break;
        case 20: target = &y1; break;
        case 30: target = &z1; break;
        case 11: target = &x2; break;
        case 21: target = &y2; break;
        case 31: target = &z2; break;
        default: break;
        }
        if (target != nullptr) {
            const auto value = toReal(pair.value);
            bad = bad || !value;
            *target = value.value_or(0.0);
        }
    }
    if (bad) {
        warn("LINE not imported: a coordinate is not a number");
        return;
    }
    Entity entity;
    const Point2 start(x1, y1);
    const Point2 end(x2, y2);
    if (start.distanceTo(end) <= katana::math::tolerance::kGeometric) {
        // A dot drawn as a line of no length is still a surveyed dot.
        entity.geometry = PointGeometry{start};
        if (z1 != 0.0) {
            const std::vector<std::optional<double>> heights{z1};
            emit(std::move(entity), common, sink, &heights);
        } else {
            emit(std::move(entity), common, sink);
        }
        return;
    }
    entity.geometry = Segment2{start, end};
    if (z1 != 0.0 || z2 != 0.0) {
        const std::vector<std::optional<double>> heights{z1, z2};
        emit(std::move(entity), common, sink, &heights);
    } else {
        emit(std::move(entity), common, sink);
    }
}

void Reader::convertPoint(const Record& record, Sink& sink)
{
    Common common;
    double x = 0.0, y = 0.0, z = 0.0;
    bool bad = false;
    for (const Pair& pair : record.pairs) {
        if (common.take(pair)) {
            continue;
        }
        if (pair.code == 10 || pair.code == 20 || pair.code == 30) {
            const auto value = toReal(pair.value);
            bad = bad || !value;
            (pair.code == 10 ? x : pair.code == 20 ? y : z) = value.value_or(0.0);
        }
    }
    if (bad) {
        warn("POINT not imported: a coordinate is not a number");
        return;
    }
    Entity entity;
    entity.geometry = PointGeometry{Point2(x, y)};
    if (z != 0.0) {
        const std::vector<std::optional<double>> heights{z};
        emit(std::move(entity), common, sink, &heights);
    } else {
        emit(std::move(entity), common, sink);
    }
}

// The curve through `frame` into plan. A similarity keeps it a circle or arc
// (a mirror reverses the arc's direction); anything else - a tilted plane, a
// block scaled unevenly - makes it an ellipse, which is chorded.
std::optional<Geometry> placeCurve(const Arc2& arc, bool full, const Affine& frame,
                                   double tolerance)
{
    if (frame.isIdentity()) {
        return full ? Geometry{Circle2{arc.center, arc.radius}} : Geometry{arc};
    }
    if (frame.isSimilarity()) {
        const double scale = std::sqrt(std::abs(frame.determinant()));
        const Point2 centre = frame.apply(arc.center);
        if (full) {
            return Geometry{Circle2{centre, arc.radius * scale}};
        }
        const double from = frame.determinant() < 0.0 ? arc.endAngle() : arc.startAngle;
        const Vec2 direction = frame.linear(Vec2(std::cos(from), std::sin(from)));
        return Geometry{
            Arc2{centre, arc.radius * scale, std::atan2(direction.y, direction.x), arc.sweep}};
    }
    const double scale = frame.largestScale();
    const double local = scale > 0.0 ? tolerance / scale : tolerance;
    std::vector<Point2> points = full ? katana::geometry::chordCircle(Circle2{arc.center, arc.radius},
                                                                      local)
                                      : katana::geometry::chordArc(arc, local);
    for (Point2& point : points) {
        point = frame.apply(point);
    }
    return Geometry{Polyline2{std::move(points), full}};
}

void Reader::convertCircle(const Record& record, Sink& sink, bool arc)
{
    Common common;
    double x = 0.0, y = 0.0, z = 0.0, radius = 0.0, start = 0.0, end = 360.0;
    bool bad = false;
    for (const Pair& pair : record.pairs) {
        if (common.take(pair)) {
            continue;
        }
        double* target = nullptr;
        switch (pair.code) {
        case 10: target = &x; break;
        case 20: target = &y; break;
        case 30: target = &z; break;
        case 40: target = &radius; break;
        case 50: target = &start; break;
        case 51: target = &end; break;
        default: break;
        }
        if (target != nullptr) {
            const auto value = toReal(pair.value);
            bad = bad || !value;
            *target = value.value_or(0.0);
        }
    }
    const std::string_view kind = arc ? "ARC" : "CIRCLE";
    if (bad) {
        warn(std::string(kind) + " not imported: a value is not a number");
        return;
    }
    const bool plain = isPlainExtrusion(common.extrusion);
    const Affine frame = plain ? Affine{} : objectToPlan(common.extrusion, z);
    if (!plain && !frame.isSimilarity()) {
        warn(std::string(kind) + " in a tilted plane, projected onto the plan as a chorded polyline");
    }
    // An arc that starts where it ends is read as the full turn it draws.
    const bool full = !arc || std::fmod(std::abs(end - start), 360.0) == 0.0;
    const Arc2 curve{Point2(x, y), radius, full ? 0.0 : start * kDegrees,
                     full ? kTwoPi : sweepBetween(start, end)};
    if (!(radius > 0.0)) {
        warn(std::string(kind) + " not imported: its radius is not positive");
        return;
    }
    auto geometry = placeCurve(curve, full, frame, options_.curveTolerance);
    Entity entity;
    entity.geometry = std::move(*geometry);
    // The elevation of a plan circle; a tilted one has none that means anything.
    if (z != 0.0 && (plain || frame.isSimilarity())) {
        const std::vector<std::optional<double>> heights{z};
        emit(std::move(entity), common, sink, &heights);
    } else {
        emit(std::move(entity), common, sink);
    }
}

void Reader::convertEllipse(const Record& record, Sink& sink)
{
    Common common;
    double cx = 0.0, cy = 0.0, mx = 1.0, my = 0.0, mz = 0.0, ratio = 1.0, start = 0.0,
           end = kTwoPi;
    bool bad = false;
    for (const Pair& pair : record.pairs) {
        if (common.take(pair)) {
            continue;
        }
        double* target = nullptr;
        switch (pair.code) {
        case 10: target = &cx; break;
        case 20: target = &cy; break;
        case 11: target = &mx; break;
        case 21: target = &my; break;
        case 31: target = &mz; break;
        case 40: target = &ratio; break;
        case 41: target = &start; break;
        case 42: target = &end; break;
        default: break;
        }
        if (target != nullptr) {
            const auto value = toReal(pair.value);
            bad = bad || !value;
            *target = value.value_or(0.0);
        }
    }
    if (bad || !(ratio > 0.0)) {
        warn("ELLIPSE not imported: a value is not a number");
        return;
    }
    // The ellipse is in world coordinates: centre, major axis, and the minor
    // axis `ratio` times as long, a quarter turn about the extrusion.
    const Vec3 normal = normalised(common.extrusion);
    const Vec3 major{mx, my, mz};
    const Vec3 minorDirection = cross(normal, major);
    const Vec2 majorAxis(mx, my);
    const Vec2 minorAxis(minorDirection.x * ratio, minorDirection.y * ratio);
    const double majorLength = std::sqrt(mx * mx + my * my + mz * mz);
    double sweep = std::fmod(end - start, kTwoPi);
    if (sweep <= 0.0) {
        sweep += kTwoPi;
    }
    const bool full = std::abs(sweep - kTwoPi) <= 1e-9;
    // Chorded as the circle of the major radius would be: the flatter an
    // ellipse the less a chord stands off it, so this is never too coarse.
    const std::size_t count =
        katana::geometry::sagittaChordCount(majorLength, sweep, options_.curveTolerance);
    std::vector<Point2> points;
    points.reserve(count + 1);
    const std::size_t last = full ? count - 1 : count;
    for (std::size_t i = 0; i <= last; ++i) {
        const double t = start + sweep * static_cast<double>(i) / static_cast<double>(count);
        points.push_back(Point2(cx, cy) + majorAxis * std::cos(t) + minorAxis * std::sin(t));
    }
    Entity entity;
    entity.geometry = Polyline2{std::move(points), full};
    emit(std::move(entity), common, sink);
}

void Reader::emitPolyline(std::vector<Vertex> vertices, bool closed, bool threeD,
                          const Affine& frame, const Common& common, Sink& sink)
{
    const std::string_view kind = sink.kind;
    if (vertices.size() == 1) {
        // One vertex draws a dot; that is what it is kept as.
        Entity entity;
        entity.geometry = PointGeometry{frame.apply(Point2(vertices[0].x, vertices[0].y))};
        if (vertices[0].z != 0.0) {
            const std::vector<std::optional<double>> heights{vertices[0].z};
            emit(std::move(entity), common, sink, &heights);
        } else {
            emit(std::move(entity), common, sink);
        }
        return;
    }
    if (vertices.empty()) {
        warn(std::string(kind) + " not imported: it has no vertices");
        return;
    }
    // A closed polyline whose last vertex repeats its first says so twice;
    // once is Katana's form, and the repeat would be a segment of no length.
    if (closed && vertices.size() > 2) {
        const Vertex& first = vertices.front();
        const Vertex& lastVertex = vertices.back();
        if (first.x == lastVertex.x && first.y == lastVertex.y && lastVertex.bulge == 0.0) {
            vertices.pop_back();
        }
    }
    std::vector<Point2> points;
    std::vector<std::optional<double>> heights;
    points.reserve(vertices.size());
    heights.reserve(vertices.size());
    bool anyHeight = false;
    const std::size_t count = vertices.size();
    const std::size_t segments = closed ? count : count - 1;
    const double scale = frame.largestScale();
    const double tolerance = scale > 0.0 ? options_.curveTolerance / scale : options_.curveTolerance;
    for (std::size_t i = 0; i < count; ++i) {
        const Vertex& vertex = vertices[i];
        points.push_back(Point2(vertex.x, vertex.y));
        heights.emplace_back(vertex.z);
        anyHeight = anyHeight || vertex.z != 0.0;
        if (i >= segments || threeD) {
            continue;
        }
        const Vertex& next = vertices[(i + 1) % count];
        const auto arc = bulgeArc(Point2(vertex.x, vertex.y), Point2(next.x, next.y), vertex.bulge);
        if (!arc) {
            continue;
        }
        ++result_.arcsChorded;
        const std::vector<Point2> chords = katana::geometry::chordArc(*arc, tolerance);
        // The ends are the vertices themselves, exactly; only what lies
        // between them is added. Heights along an arc are the vertex's own,
        // since a 2D polyline has one elevation.
        for (std::size_t k = 1; k + 1 < chords.size(); ++k) {
            points.push_back(chords[k]);
            heights.emplace_back(vertex.z);
        }
    }
    if (!frame.isIdentity()) {
        for (Point2& point : points) {
            point = frame.apply(point);
        }
    }
    Entity entity;
    entity.geometry = Polyline2{std::move(points), closed};
    // A 3D polyline's heights are data even where they are zero; a 2D one's
    // elevation of zero is only the plan.
    if (threeD || anyHeight) {
        emit(std::move(entity), common, sink, &heights);
    } else {
        emit(std::move(entity), common, sink);
    }
}

void Reader::convertLwPolyline(const Record& record, Sink& sink)
{
    Common common;
    std::vector<Vertex> vertices;
    double elevation = 0.0;
    int flags = 0;
    bool bad = false;
    for (const Pair& pair : record.pairs) {
        if (common.take(pair)) {
            continue;
        }
        switch (pair.code) {
        case 90:
            // A count is a hint, and a hostile one could ask for any amount
            // of memory: never more than the pairs there are to fill it.
            if (const auto count = toInteger(pair.value); count && *count > 0) {
                vertices.reserve(std::min(static_cast<std::size_t>(*count), record.pairs.size()));
            }
            break;
        case 70:
            flags = static_cast<int>(toInteger(pair.value).value_or(0));
            break;
        case 38:
            elevation = toReal(pair.value).value_or(0.0);
            break;
        case 10: {
            const auto value = toReal(pair.value);
            bad = bad || !value;
            vertices.push_back({value.value_or(0.0), 0.0, 0.0, 0.0});
            break;
        }
        case 20:
            if (!vertices.empty()) {
                const auto value = toReal(pair.value);
                bad = bad || !value;
                vertices.back().y = value.value_or(0.0);
            }
            break;
        case 42:
            if (!vertices.empty()) {
                vertices.back().bulge = toReal(pair.value).value_or(0.0);
            }
            break;
        default:
            break;
        }
    }
    if (bad) {
        warn("LWPOLYLINE not imported: a coordinate is not a number");
        return;
    }
    for (Vertex& vertex : vertices) {
        vertex.z = elevation;
    }
    const bool plain = isPlainExtrusion(common.extrusion);
    const Affine frame = plain ? Affine{} : objectToPlan(common.extrusion, elevation);
    if (!plain && !frame.isSimilarity()) {
        warn("LWPOLYLINE in a tilted plane, projected onto the plan");
    }
    emitPolyline(std::move(vertices), (flags & 1) != 0, false, frame, common, sink);
}

void Reader::convertPolyline(const Record& record, Sink& sink)
{
    Common common;
    int flags = 0;
    double elevation = 0.0;
    for (const Pair& pair : record.pairs) {
        if (common.take(pair)) {
            continue;
        }
        if (pair.code == 70) {
            flags = static_cast<int>(toInteger(pair.value).value_or(0));
        } else if (pair.code == 30) {
            elevation = toReal(pair.value).value_or(0.0);
        }
    }
    if ((flags & (16 | 64)) != 0) {
        warn("POLYLINE meshes not imported (Katana has no mesh entity in the drawing)");
        return;
    }
    const bool threeD = (flags & 8) != 0;
    std::vector<Vertex> vertices;
    vertices.reserve(record.childCount);
    bool bad = false;
    for (std::size_t i = 0; i < record.childCount; ++i) {
        const Record& child = record.children[i];
        Vertex vertex;
        vertex.z = threeD ? 0.0 : elevation;
        int vertexFlags = 0;
        for (const Pair& pair : child.pairs) {
            switch (pair.code) {
            case 10:
            case 20:
            case 30: {
                const auto value = toReal(pair.value);
                bad = bad || !value;
                if (pair.code == 10) {
                    vertex.x = value.value_or(0.0);
                } else if (pair.code == 20) {
                    vertex.y = value.value_or(0.0);
                } else if (threeD) {
                    vertex.z = value.value_or(0.0);
                }
                break;
            }
            case 42:
                vertex.bulge = toReal(pair.value).value_or(0.0);
                break;
            case 70:
                vertexFlags = static_cast<int>(toInteger(pair.value).value_or(0));
                break;
            default:
                break;
            }
        }
        // A spline's frame (control) points are not on the curve; its
        // fitted vertices are.
        if ((vertexFlags & 16) != 0) {
            continue;
        }
        vertices.push_back(vertex);
    }
    if (bad) {
        warn("POLYLINE not imported: a coordinate is not a number");
        return;
    }
    const bool plain = threeD || isPlainExtrusion(common.extrusion);
    const Affine frame = plain ? Affine{} : objectToPlan(common.extrusion, elevation);
    if (!plain && !frame.isSimilarity()) {
        warn("POLYLINE in a tilted plane, projected onto the plan");
    }
    emitPolyline(std::move(vertices), (flags & 1) != 0, threeD, frame, common, sink);
}

// Where a text's left baseline is, from a justified anchor: a guess at its
// width without a font, and so used only when the file gives nothing better.
Point2 leftBaselineFrom(const Point2& anchor, std::string_view text, double height,
                        double rotation, int horizontal, int vertical)
{
    const double width = kGlyphAspect * height * static_cast<double>(text.size());
    double dx = 0.0;
    double dy = 0.0;
    switch (horizontal) {
    case 1: // centre
    case 4: // middle
        dx = -width / 2.0;
        break;
    case 2: // right
        dx = -width;
        break;
    default:
        break;
    }
    if (horizontal == 4) {
        dy = -height / 2.0;
    }
    switch (vertical) {
    case 2: // middle
        dy = -height / 2.0;
        break;
    case 3: // top
        dy = -height;
        break;
    default:
        break;
    }
    return anchor + Vec2(dx, dy).rotated(rotation);
}

void Reader::convertText(const Record& record, Sink& sink, bool attribute,
                         const PropertyMap* properties)
{
    Common common;
    std::string_view raw;
    double x = 0.0, y = 0.0, ax = 0.0, ay = 0.0, height = 0.0, rotation = 0.0, z = 0.0;
    bool haveFirst = false;
    bool haveSecond = false;
    int horizontal = 0;
    int vertical = 0;
    bool bad = false;
    for (const Pair& pair : record.pairs) {
        if (common.take(pair)) {
            continue;
        }
        switch (pair.code) {
        case 1:
            raw = pair.value;
            break;
        case 10:
        case 20:
        case 11:
        case 21:
        case 30:
        case 40:
        case 50: {
            const auto value = toReal(pair.value);
            bad = bad || !value;
            const double v = value.value_or(0.0);
            switch (pair.code) {
            case 10: x = v; haveFirst = true; break;
            case 20: y = v; break;
            case 11: ax = v; haveSecond = true; break;
            case 21: ay = v; break;
            case 30: z = v; break;
            case 40: height = v; break;
            default: rotation = v; break;
            }
            break;
        }
        case 72:
            horizontal = static_cast<int>(toInteger(pair.value).value_or(0));
            break;
        case 73:
            if (!attribute) {
                vertical = static_cast<int>(toInteger(pair.value).value_or(0));
            }
            break;
        case 74:
            if (attribute) {
                vertical = static_cast<int>(toInteger(pair.value).value_or(0));
            }
            break;
        default:
            break;
        }
    }
    const std::string_view kind = attribute ? "ATTRIB" : "TEXT";
    if (bad) {
        warn(std::string(kind) + " not imported: a value is not a number");
        return;
    }
    std::string text = plainText(raw);
    if (text.empty()) {
        return; // nothing to read, and Katana refuses an empty text
    }
    if (!(height > 0.0)) {
        height = defaultTextHeight_;
    }
    const double radians = rotation * kDegrees;
    Point2 position(x, y);
    // Justified text is anchored at the second point. The first is then the
    // left of the baseline as the writing program computed it, which is
    // better than any guess - unless the program did not compute it, which
    // leaves it at the origin or missing.
    const bool justified = (horizontal != 0 && horizontal != 3 && horizontal != 5) || vertical != 0;
    if (justified && haveSecond && (!haveFirst || (x == 0.0 && y == 0.0))) {
        position = leftBaselineFrom(Point2(ax, ay), text, height, radians, horizontal, vertical);
    }
    const bool plain = isPlainExtrusion(common.extrusion);
    const Affine frame = plain ? Affine{} : objectToPlan(common.extrusion, z);
    double finalRotation = radians;
    if (!plain) {
        position = frame.apply(position);
        const Vec2 direction = frame.linear(Vec2(std::cos(radians), std::sin(radians)));
        finalRotation = std::atan2(direction.y, direction.x);
    }
    Entity entity;
    entity.geometry = TextGeometry{position, std::move(text), height, finalRotation};
    if (properties != nullptr) {
        entity.properties = *properties;
    }
    if (z != 0.0 && plain) {
        const std::vector<std::optional<double>> heights{z};
        emit(std::move(entity), common, sink, &heights);
    } else {
        emit(std::move(entity), common, sink);
    }
}

void Reader::convertMText(const Record& record, Sink& sink)
{
    Common common;
    std::string raw;
    std::string_view last;
    double x = 0.0, y = 0.0, z = 0.0, height = 0.0, rotation = 0.0, dx = 0.0, dy = 0.0;
    double spacing = 1.0;
    bool haveDirection = false;
    int attachment = 1;
    bool bad = false;
    for (const Pair& pair : record.pairs) {
        if (common.take(pair)) {
            continue;
        }
        switch (pair.code) {
        case 3:
            raw.append(pair.value); // the text in pieces, before the last
            break;
        case 1:
            last = pair.value;
            break;
        case 10:
        case 20:
        case 30:
        case 40:
        case 50:
        case 11:
        case 21:
        case 44: {
            const auto value = toReal(pair.value);
            bad = bad || !value;
            const double v = value.value_or(0.0);
            switch (pair.code) {
            case 10: x = v; break;
            case 20: y = v; break;
            case 30: z = v; break;
            case 40: height = v; break;
            case 50: rotation = v; break;
            case 11: dx = v; haveDirection = true; break;
            case 21: dy = v; haveDirection = true; break;
            default: spacing = v > 0.0 ? v : 1.0; break;
            }
            break;
        }
        case 71:
            attachment = static_cast<int>(toInteger(pair.value).value_or(1));
            break;
        default:
            break;
        }
    }
    if (bad) {
        warn("MTEXT not imported: a value is not a number");
        return;
    }
    raw.append(last);
    const std::string plain = plainMText(raw);
    if (!(height > 0.0)) {
        height = defaultTextHeight_;
    }
    // The direction vector, where there is one, wins over the angle: it is
    // what the writing program keeps, and the angle is derived from it.
    double radians = rotation * kDegrees;
    if (haveDirection && (dx != 0.0 || dy != 0.0)) {
        radians = std::atan2(dy, dx);
    }
    std::vector<std::string_view> lines;
    for (std::size_t from = 0;;) {
        const std::size_t end = plain.find('\n', from);
        lines.push_back(std::string_view(plain).substr(from, end == std::string::npos
                                                                 ? std::string::npos
                                                                 : end - from));
        if (end == std::string::npos) {
            break;
        }
        from = end + 1;
    }
    const double pitch = kLinePitch * height * spacing;
    const double block = height + pitch * static_cast<double>(lines.size() - 1);
    // Attachment 1-9: top, middle, bottom rows of left, centre, right. The
    // first baseline sits one text height below a top attachment.
    const int row = (std::clamp(attachment, 1, 9) - 1) / 3;
    const int column = (std::clamp(attachment, 1, 9) - 1) % 3;
    double firstBaseline = -height;
    if (row == 1) {
        firstBaseline = block / 2.0 - height;
    } else if (row == 2) {
        firstBaseline = block - height;
    }
    const bool plainExtrusion = isPlainExtrusion(common.extrusion);
    const Affine frame = plainExtrusion ? Affine{} : objectToPlan(common.extrusion, z);
    const Vec2 along(std::cos(radians), std::sin(radians));
    const Vec2 up(-along.y, along.x);
    for (std::size_t i = 0; i < lines.size(); ++i) {
        const std::string_view line = lines[i];
        if (katana::core::trimmed(line).empty()) {
            continue;
        }
        const double width = kGlyphAspect * height * static_cast<double>(line.size());
        const double shift = column == 1 ? -width / 2.0 : column == 2 ? -width : 0.0;
        Point2 position = Point2(x, y) + along * shift +
                          up * (firstBaseline - pitch * static_cast<double>(i));
        double finalRotation = radians;
        if (!plainExtrusion) {
            position = frame.apply(position);
            const Vec2 direction = frame.linear(along);
            finalRotation = std::atan2(direction.y, direction.x);
        }
        Entity entity;
        entity.geometry = TextGeometry{position, std::string(line), height, finalRotation};
        if (z != 0.0 && plainExtrusion) {
            const std::vector<std::optional<double>> heights{z};
            emit(std::move(entity), common, sink, &heights);
        } else {
            emit(std::move(entity), common, sink);
        }
    }
}

// Converts a block's records once, into the block's own coordinates.
Block* Reader::buildBlock(std::string_view name, std::size_t depth, std::string_view insertKind)
{
    const auto found = blocks_.find(upper(name));
    if (found == blocks_.end()) {
        warn(std::string(insertKind) + " of a block the file does not define");
        return nullptr;
    }
    Block& block = found->second;
    if (block.built) {
        return &block;
    }
    if (block.building) {
        warn(std::string(insertKind) + " of a block that contains itself, not expanded");
        return nullptr;
    }
    if (depth > options_.maximumBlockDepth) {
        warn(std::string(insertKind) + " nested deeper than " +
             std::to_string(options_.maximumBlockDepth) + " blocks, not expanded");
        return nullptr;
    }
    if ((block.flags & 4) != 0) {
        warn(std::string(insertKind) + " of an external reference, whose drawing is not read");
    }
    block.building = true;
    Sink inner;
    inner.block = &block.expanded;
    inner.depth = depth;
    for (const Record& record : block.records) {
        inner.kind = record.type;
        convert(record, inner);
    }
    block.building = false;
    block.built = true;
    return &block;
}

// Moves heights in Z with the insert: scaled, then raised.
void placeHeights(PropertyMap& properties, std::size_t vertices, double zScale, double zOffset)
{
    std::vector<std::optional<double>> heights = katana::entity::heightsOf(properties, vertices);
    bool any = false;
    for (auto& height : heights) {
        if (height) {
            *height = *height * zScale + zOffset;
            any = true;
        }
    }
    if (any) {
        katana::entity::setHeights(properties, heights);
    }
}

std::size_t vertexCount(const Geometry& geometry)
{
    if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        return polyline->vertices.size();
    }
    if (std::holds_alternative<Segment2>(geometry)) {
        return 2;
    }
    return 1;
}

// The geometry through an insert's transform.
std::optional<Geometry> placeGeometry(const Geometry& geometry, const Affine& transform,
                                      double tolerance)
{
    if (transform.isIdentity()) {
        return geometry;
    }
    struct Visitor {
        const Affine& t;
        double tolerance;
        std::optional<Geometry> operator()(const PointGeometry& point) const
        {
            return Geometry{PointGeometry{t.apply(point.position)}};
        }
        std::optional<Geometry> operator()(const Segment2& segment) const
        {
            return Geometry{Segment2{t.apply(segment.start), t.apply(segment.end)}};
        }
        std::optional<Geometry> operator()(const Arc2& arc) const
        {
            return placeCurve(arc, false, t, tolerance);
        }
        std::optional<Geometry> operator()(const Circle2& circle) const
        {
            return placeCurve(Arc2{circle.center, circle.radius, 0.0, kTwoPi}, true, t,
                              tolerance);
        }
        std::optional<Geometry> operator()(const Polyline2& polyline) const
        {
            Polyline2 out = polyline;
            for (Point2& vertex : out.vertices) {
                vertex = t.apply(vertex);
            }
            return Geometry{std::move(out)};
        }
        std::optional<Geometry> operator()(const TextGeometry& text) const
        {
            TextGeometry out = text;
            out.position = t.apply(text.position);
            const Vec2 along = t.linear(Vec2(std::cos(text.rotation), std::sin(text.rotation)));
            const Vec2 up = t.linear(Vec2(-std::sin(text.rotation), std::cos(text.rotation)));
            out.rotation = std::atan2(along.y, along.x);
            out.height = text.height * up.length();
            return Geometry{std::move(out)};
        }
        std::optional<Geometry> operator()(const katana::entity::DimensionGeometry& dimension) const
        {
            katana::entity::DimensionGeometry out = dimension;
            out.start = t.apply(dimension.start);
            out.end = t.apply(dimension.end);
            const double scale = std::sqrt(std::abs(t.determinant()));
            out.offset = dimension.offset * scale * (t.determinant() < 0.0 ? -1.0 : 1.0);
            return Geometry{std::move(out)};
        }
    };
    return std::visit(Visitor{transform, tolerance}, geometry);
}

void Reader::place(const Block& block, const Affine& transform, double zScale, double zOffset,
                   const Common& insert, const PropertyMap& attributes, Sink& sink)
{
    for (const BlockEntity& item : block.expanded) {
        if (reportFull(sink)) {
            return;
        }
        auto geometry = placeGeometry(item.entity.geometry, transform, options_.curveTolerance);
        if (!geometry) {
            continue;
        }
        Entity entity = item.entity;
        entity.geometry = std::move(*geometry);
        for (const auto& [key, value] : attributes) {
            entity.properties.emplace(key, value); // the entity's own value wins
        }
        if (zScale != 1.0 || zOffset != 0.0) {
            placeHeights(entity.properties, vertexCount(entity.geometry), zScale, zOffset);
        }
        // What the entity inherits from this insert: its layer when it is on
        // layer 0, its colour when that is ByBlock.
        Common common = insert;
        if (!item.layerFromInsert) {
            common.layer = item.entity.layer;
        }
        std::string blockPath = sink.blockPath.empty() ? block.name
                                                       : sink.blockPath + "/" + block.name;
        const auto nested = item.entity.metadata.find(kMetaBlock);
        if (nested != item.entity.metadata.end()) {
            blockPath += "/" + katana::entity::toString(nested->second);
        }
        entity.metadata[std::string(kMetaBlock)] = std::move(blockPath);
        const bool insertByBlock =
            !insert.trueColour && insert.colour && *insert.colour == kColourByBlock;
        if (sink.block != nullptr) {
            entity.layer = std::string(common.layer.empty() ? std::string_view("0") : common.layer);
            if (item.colourFromInsert) {
                entity.color = colourOf(insert);
                if (insert.colour && *insert.colour == kColourByLayer) {
                    entity.color.reset();
                }
            }
            sink.block->push_back({std::move(entity), item.layerFromInsert &&
                                                          (insert.layer.empty() || insert.layer == "0"),
                                   item.colourFromInsert && insertByBlock});
            continue;
        }
        entity.layer = layerPath(common.layer);
        if (item.colourFromInsert) {
            const bool insertByLayer =
                !insert.trueColour && (!insert.colour || *insert.colour == kColourByLayer ||
                                       *insert.colour == kColourByBlock);
            if (insertByLayer) {
                // ByBlock under a ByLayer insert is the INSERT's layer colour,
                // which is not this entity's layer's unless it is on it.
                const std::string insertLayer = layerPath(insert.layer);
                if (insertLayer != entity.layer) {
                    entity.color = layers_[lastLayer_].layer.color;
                } else {
                    entity.color.reset();
                }
            } else {
                entity.color = colourOf(insert);
            }
        }
        if (!options_.sourceName.empty()) {
            entity.metadata[std::string(kMetaSource)] = options_.sourceName;
        }
        result_.entities.push_back(std::move(entity));
        ++tally(sink.kind).imported;
    }
}

void Reader::convertInsert(const Record& record, Sink& sink)
{
    Common common;
    std::string_view name;
    double x = 0.0, y = 0.0, z = 0.0, sx = 1.0, sy = 1.0, sz = 1.0, rotation = 0.0;
    double columnSpacing = 0.0, rowSpacing = 0.0;
    std::int64_t columns = 1, rows = 1;
    bool bad = false;
    for (const Pair& pair : record.pairs) {
        if (common.take(pair)) {
            continue;
        }
        switch (pair.code) {
        case 2:
            name = trimmedValue(pair.value);
            break;
        case 10:
        case 20:
        case 30:
        case 41:
        case 42:
        case 43:
        case 50:
        case 44:
        case 45: {
            const auto value = toReal(pair.value);
            bad = bad || !value;
            const double v = value.value_or(0.0);
            switch (pair.code) {
            case 10: x = v; break;
            case 20: y = v; break;
            case 30: z = v; break;
            case 41: sx = v; break;
            case 42: sy = v; break;
            case 43: sz = v; break;
            case 50: rotation = v; break;
            case 44: columnSpacing = v; break;
            default: rowSpacing = v; break;
            }
            break;
        }
        case 70:
            columns = std::max<std::int64_t>(1, toInteger(pair.value).value_or(1));
            break;
        case 71:
            rows = std::max<std::int64_t>(1, toInteger(pair.value).value_or(1));
            break;
        default:
            break;
        }
    }
    if (bad) {
        warn("INSERT not imported: a value is not a number");
        return;
    }
    // The attributes: every one is a property of what the insert makes; a
    // visible one is also a Text, where the writing program put it.
    PropertyMap attributes;
    for (std::size_t i = 0; i < record.childCount; ++i) {
        const Record& child = record.children[i];
        std::string tag;
        std::string value;
        for (const Pair& pair : child.pairs) {
            if (pair.code == 2) {
                tag = plainText(trimmedValue(pair.value));
            } else if (pair.code == 1) {
                value = plainText(pair.value);
            }
        }
        if (!tag.empty()) {
            attributes.insert_or_assign(std::move(tag), std::move(value));
        }
    }
    for (std::size_t i = 0; i < record.childCount; ++i) {
        const Record& child = record.children[i];
        bool invisible = false;
        for (const Pair& pair : child.pairs) {
            if (pair.code == 70) {
                invisible = (toInteger(pair.value).value_or(0) & 1) != 0;
            }
        }
        if (!invisible) {
            Sink attributeSink = sink;
            convertText(child, attributeSink, true, &attributes);
        }
    }
    Block* block = buildBlock(name, sink.depth + 1, "INSERT");
    const bool plain = isPlainExtrusion(common.extrusion);
    const Affine frame = plain ? Affine{} : objectToPlan(common.extrusion, z);
    const double radians = rotation * kDegrees;
    const double cosine = std::cos(radians);
    const double sine = std::sin(radians);
    const bool nothingDrawn = block == nullptr || block->expanded.empty();
    if (columns > 100'000 || rows > 100'000 || columns * rows > 100'000) {
        warn("MINSERT of more than 100 000 copies, only the first placed");
        columns = 1;
        rows = 1;
    }
    for (std::int64_t row = 0; row < rows; ++row) {
        for (std::int64_t column = 0; column < columns; ++column) {
            // insertion + rotation * (scale * (p - base) + array offset)
            const Vec2 offset(static_cast<double>(column) * columnSpacing,
                              static_cast<double>(row) * rowSpacing);
            Affine local;
            local.a = cosine * sx;
            local.b = -sine * sy;
            local.c = sine * sx;
            local.d = cosine * sy;
            const Point2 base = block != nullptr ? block->base : Point2(0.0, 0.0);
            const Vec2 moved(-sx * base.x + offset.x, -sy * base.y + offset.y);
            local.tx = x + cosine * moved.x - sine * moved.y;
            local.ty = y + sine * moved.x + cosine * moved.y;
            const Affine transform = frame.after(local);
            if (nothingDrawn) {
                if (!attributes.empty() && row == 0 && column == 0) {
                    // The data of an insert that draws nothing - a block of
                    // attributes only - stays in the drawing on a point.
                    Entity entity;
                    entity.geometry = PointGeometry{transform.apply(
                        block != nullptr ? block->base : Point2(0.0, 0.0))};
                    entity.properties = attributes;
                    if (z != 0.0) {
                        const std::vector<std::optional<double>> heights{z};
                        emit(std::move(entity), common, sink, &heights);
                    } else {
                        emit(std::move(entity), common, sink);
                    }
                }
                continue;
            }
            const double zOffset = z - sz * (block != nullptr ? block->baseZ : 0.0);
            place(*block, transform, sz, zOffset, common, attributes, sink);
        }
    }
}

void Reader::convertDimension(const Record& record, Sink& sink)
{
    Common common;
    std::string_view name;
    for (const Pair& pair : record.pairs) {
        if (common.take(pair)) {
            continue;
        }
        if (pair.code == 2) {
            name = trimmedValue(pair.value);
        }
    }
    if (name.empty()) {
        warn("DIMENSION with no picture block, not imported");
        return;
    }
    // The picture block is drawn where it stands: its entities are already
    // in world coordinates.
    Block* block = buildBlock(name, sink.depth + 1, "DIMENSION");
    if (block == nullptr) {
        return;
    }
    Affine transform;
    transform.tx = -block->base.x;
    transform.ty = -block->base.y;
    place(*block, transform, 1.0, 0.0, common, {}, sink);
}

void translate(Geometry& geometry, const Vec2& shift)
{
    std::visit(
        [&](auto& shape) {
            using T = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<T, PointGeometry> || std::is_same_v<T, TextGeometry>) {
                shape.position = shape.position - shift;
            } else if constexpr (std::is_same_v<T, Segment2> ||
                                 std::is_same_v<T, katana::entity::DimensionGeometry>) {
                shape.start = shape.start - shift;
                shape.end = shape.end - shift;
            } else if constexpr (std::is_same_v<T, Arc2> || std::is_same_v<T, Circle2>) {
                shape.center = shape.center - shift;
            } else {
                for (Point2& vertex : shape.vertices) {
                    vertex = vertex - shift;
                }
            }
        },
        geometry);
}

Result<DxfImport> Reader::run()
{
    Pair pair;
    bool sawSection = false;
    bool sawEof = false;
    while (tokens_.next(pair)) {
        if (pair.code != 0) {
            continue;
        }
        const std::string_view word = trimmedValue(pair.value);
        if (word == "EOF") {
            sawEof = true;
            break;
        }
        if (word != "SECTION") {
            continue;
        }
        sawSection = true;
        if (!tokens_.next(pair)) {
            break;
        }
        const std::string_view section = pair.code == 2 ? trimmedValue(pair.value) : "";
        if (section == "HEADER") {
            readHeader();
        } else if (section == "TABLES") {
            readTables();
        } else if (section == "BLOCKS") {
            readBlocks();
        } else if (section == "ENTITIES") {
            readEntities();
        } else {
            // CLASSES, OBJECTS, THUMBNAILIMAGE, ACDSDATA: nothing drawn.
            Record ignored;
            bool end = false;
            while (!end && readRecord(ignored, end)) {
            }
        }
    }
    if (tokens_.failed()) {
        if (!sawSection) {
            return makeError(ErrorCode::ParseFailure, "not a DXF file: " + tokens_.failure());
        }
        return makeError(ErrorCode::ParseFailure,
                         "the DXF file is damaged: " + tokens_.failure() +
                             " - nothing after it can be read reliably");
    }
    if (!sawSection) {
        return makeError(ErrorCode::ParseFailure, "not a DXF file: it has no SECTION");
    }
    if (!sawEof) {
        result_.warnings.push_back(
            tokens_.truncated()
                ? "the file ends in the middle of a group: it is truncated, and what was read "
                  "before the end is imported"
                : "the file has no EOF marker: it may be truncated, and what was read is imported");
    }

    // Layers: the linetype each names resolved against LTYPE (continuous
    // where the table does not have it), in file order. Two DXF names can
    // become one path - " Kerb" and "Kerb" once the blank is trimmed - and
    // one layer is what they then are: the first stands.
    std::unordered_map<std::string, bool> listed;
    for (LayerEntry& entry : layers_) {
        Layer& layer = entry.layer;
        if (!listed.emplace(layer.name, true).second) {
            continue;
        }
        if (layer.linetype != katana::entity::kContinuousLinetype) {
            const auto found = linetypeNames_.find(upper(layer.linetype));
            if (found != linetypeNames_.end()) {
                layer.linetype = found->second;
            } else {
                warn("layer linetype the LTYPE table does not define, drawn continuous");
                layer.linetype = std::string(katana::entity::kContinuousLinetype);
            }
        }
        result_.layers.push_back(layer);
    }
    if (options_.originShift) {
        for (Entity& entity : result_.entities) {
            translate(entity.geometry, *options_.originShift);
        }
    }
    for (const Entity& entity : result_.entities) {
        result_.bounds.expand(katana::entity::boundingBox(entity.geometry));
    }
    if (result_.arcsChorded != 0) {
        std::string tolerance = katana::core::formatExactReal(options_.curveTolerance);
        result_.warnings.push_back(std::to_string(result_.arcsChorded) +
                                   " polyline arc segments chorded to within " + tolerance +
                                   " (Katana polylines have no arc segments yet)");
    }
    if (!codePage_.empty() && codePage_ != "ANSI_1252" && !codePage_.starts_with("UTF")) {
        // Recorded for the caller; the decoding decision was made on the bytes.
        result_.warnings.push_back("the file declares code page " + codePage_ +
                                   "; text that is not UTF-8 was read as Windows-1252");
    }
    for (const auto& [category, count] : warningCounts_) {
        result_.warnings.push_back(std::to_string(count) + " x " + category);
    }
    return std::move(result_);
}

} // namespace

std::string_view releaseName(std::string_view acadver)
{
    static constexpr std::pair<std::string_view, std::string_view> kReleases[] = {
        {"AC1006", "R10"},   {"AC1009", "R12"},   {"AC1012", "R13"},   {"AC1014", "R14"},
        {"AC1015", "R2000"}, {"AC1018", "R2004"}, {"AC1021", "R2007"}, {"AC1024", "R2010"},
        {"AC1027", "R2013"}, {"AC1032", "R2018"},
    };
    for (const auto& [code, name] : kReleases) {
        if (acadver == code) {
            return name;
        }
    }
    return {};
}

bool isDxfPath(const std::filesystem::path& path)
{
    const std::string extension = path.extension().string();
    return extension.size() == 4 && equalsUpper(extension, ".DXF");
}

Result<DxfImport> readDxf(std::string_view bytes, const ImportOptions& options)
{
    if (!(options.curveTolerance > 0.0) || !std::isfinite(options.curveTolerance)) {
        return makeError(ErrorCode::InvalidArgument, "the curve tolerance must be positive");
    }
    if (bytes.starts_with("AutoCAD Binary DXF")) {
        return makeError(ErrorCode::Unsupported,
                         "binary DXF is not read; save the drawing as ASCII DXF");
    }
    if (bytes.starts_with("\xEF\xBB\xBF")) {
        bytes.remove_prefix(3);
    }
    // UTF-8 as it is (every release from R2007 writes it, and ASCII is UTF-8);
    // anything else is an older release in its code page.
    std::string decoded;
    bool recoded = false;
    if (!katana::core::isValidUtf8(bytes)) {
        auto text = katana::core::decodeTextAs(bytes, katana::core::TextEncoding::Windows1252);
        if (!text) {
            return text.error();
        }
        decoded = std::move(text->text);
        bytes = decoded;
        recoded = true;
    }
    Reader reader(bytes, options);
    auto result = reader.run();
    if (result && recoded) {
        result->warnings.insert(result->warnings.begin(),
                                "the file is not UTF-8; it was read as Windows-1252");
    }
    return result;
}

Result<DxfImport> readDxfFile(const std::filesystem::path& path, const ImportOptions& options)
{
    std::ifstream stream(path, std::ios::binary);
    if (!stream) {
        return makeError(ErrorCode::FileImportFailure, "cannot open the file",
                         path.string());
    }
    stream.seekg(0, std::ios::end);
    const std::streamoff size = stream.tellg();
    if (size < 0) {
        return makeError(ErrorCode::FileImportFailure, "cannot read the file", path.string());
    }
    stream.seekg(0, std::ios::beg);
    std::string bytes(static_cast<std::size_t>(size), '\0');
    if (size > 0 && !stream.read(bytes.data(), size)) {
        return makeError(ErrorCode::FileImportFailure, "cannot read the file", path.string());
    }
    auto result = readDxf(bytes, options);
    if (!result) {
        katana::core::Error error = result.error();
        error.context = path.string();
        return error;
    }
    return result;
}

} // namespace katana::dxf
