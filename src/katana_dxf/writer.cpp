#include "katana/dxf/writer.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <cstdint>
#include <fstream>
#include <numbers>
#include <set>
#include <system_error>
#include <unordered_map>
#include <unordered_set>

#include "katana/core/text.hpp"
#include "katana/dxf/codes.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/dimension_text.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/entity/tables.hpp"
#include "katana/entity/text_block.hpp"

namespace katana::dxf {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

namespace {

constexpr double kDegreesPerRadian = 180.0 / std::numbers::pi;
constexpr double kLinePitch = 5.0 / 3.0; // as the reader: MTEXT's default pitch

// Handles of the objects every file has. A table's records, the layers, the
// linetypes and the entities take handles from kFirstFreeHandle on.
enum Handle : std::uint64_t {
    kBlockRecordTable = 0x1,
    kLayerTable = 0x2,
    kStyleTable = 0x3,
    kLinetypeTable = 0x5,
    kViewTable = 0x6,
    kUcsTable = 0x7,
    kViewportTable = 0x8,
    kApplicationTable = 0x9,
    kDimensionStyleTable = 0xA,
    kRootDictionary = 0xC,
    kGroupDictionary = 0xD,
    kPlotStyleDictionary = 0xE,
    kPlotStylePlaceholder = 0xF,
    kLayoutDictionary = 0x10,
    kMultilineStyleDictionary = 0x11,
    kMultilineStyle = 0x12,
    kActiveViewport = 0x13,
    kLinetypeByBlock = 0x14,
    kLinetypeByLayer = 0x15,
    kLinetypeContinuous = 0x16,
    kStandardStyle = 0x17,
    kApplicationAcad = 0x18,
    kApplicationKatana = 0x19,
    kStandardDimensionStyle = 0x1A,
    kModelSpaceRecord = 0x1B,
    kPaperSpaceRecord = 0x1C,
    kModelSpaceBlock = 0x1D,
    kModelSpaceEnd = 0x1E,
    kPaperSpaceBlock = 0x1F,
    kPaperSpaceEnd = 0x20,
    kModelLayout = 0x21,
    kPaperLayout = 0x22,
    kFirstFreeHandle = 0x30,
};

// A DXF name: at most 255 characters, none the format refuses, and unique
// without regard to letter case, which is how a CAD program compares them.
class SymbolNames {
  public:
    std::string unique(std::string name)
    {
        if (name.size() > 240) {
            name.resize(240);
        }
        std::string candidate = name;
        for (int copy = 2; !used_.insert(upperOf(candidate)).second; ++copy) {
            candidate = name + "~" + std::to_string(copy);
        }
        return candidate;
    }
    void reserve(std::string_view name) { used_.insert(upperOf(name)); }

  private:
    static std::string upperOf(std::string_view text)
    {
        std::string out(text);
        for (char& c : out) {
            if (c >= 'a' && c <= 'z') {
                c = static_cast<char>(c - 'a' + 'A');
            }
        }
        return out;
    }
    std::unordered_set<std::string> used_;
};

std::string symbolName(std::string_view name, bool nested)
{
    std::string out;
    out.reserve(name.size());
    for (const char c : name) {
        const auto byte = static_cast<unsigned char>(c);
        if (c == '/' && nested) {
            out.push_back('$');
        } else if (byte < 0x20 || byte == 0x7F || c == '<' || c == '>' || c == '/' ||
                   c == '\\' || c == '"' || c == ':' || c == ';' || c == '?' || c == '*' ||
                   c == '|' || c == '=' || c == '`') {
            out.push_back('_');
        } else {
            out.push_back(c);
        }
    }
    if (out.empty()) {
        out = "_";
    }
    return encodeText(out);
}

// Every vertex known, at exactly 0: a level the plain groups write as the Z 0
// that means "in plan", so it needs saying beside the entity (Writer::finish).
bool isZeroLevel(const std::vector<std::optional<double>>& heights)
{
    return !heights.empty() && std::all_of(heights.begin(), heights.end(), [](const auto& z) {
        return z.has_value() && *z == 0.0;
    });
}

class Writer {
  public:
    Writer(const katana::entity::Model& model, const ExportOptions& options)
        : model_(model), options_(options)
    {
        if (options.originShift) {
            shift_ = *options.originShift;
        }
    }

    Result<DxfExport> run();

  private:
    // ---- pairs ----
    void code(int groupCode)
    {
        char buffer[8];
        const auto [end, error] = std::to_chars(buffer, buffer + sizeof(buffer), groupCode);
        (void)error;
        const auto length = static_cast<std::size_t>(end - buffer);
        // Right-aligned in three columns, as the format's own writers do.
        for (std::size_t pad = length; pad < 3; ++pad) {
            body_.push_back(' ');
        }
        body_.append(buffer, length);
        body_.push_back('\n');
    }
    void text(int groupCode, std::string_view value)
    {
        code(groupCode);
        body_.append(value);
        body_.push_back('\n');
    }
    void integer(int groupCode, std::int64_t value)
    {
        code(groupCode);
        char buffer[24];
        const auto [end, error] = std::to_chars(buffer, buffer + sizeof(buffer), value);
        (void)error;
        body_.append(buffer, static_cast<std::size_t>(end - buffer));
        body_.push_back('\n');
    }
    // The shortest text that reads back as exactly `value`, with a decimal
    // point where it would have none, since some readers want one in a real.
    void real(int groupCode, double value)
    {
        code(groupCode);
        if (value == 0.0) {
            body_.append("0.0\n"); // and never "-0"
            return;
        }
        char buffer[32];
        const auto [end, error] = std::to_chars(buffer, buffer + sizeof(buffer), value);
        (void)error;
        const std::string_view written(buffer, static_cast<std::size_t>(end - buffer));
        body_.append(written);
        if (written.find_first_of(".eEni") == std::string_view::npos) {
            body_.append(".0");
        }
        body_.push_back('\n');
    }
    void handle(int groupCode, std::uint64_t value)
    {
        code(groupCode);
        char buffer[24];
        const auto [end, error] = std::to_chars(buffer, buffer + sizeof(buffer), value, 16);
        (void)error;
        for (char* c = buffer; c != end; ++c) {
            if (*c >= 'a' && *c <= 'f') {
                *c = static_cast<char>(*c - 'a' + 'A');
            }
        }
        body_.append(buffer, static_cast<std::size_t>(end - buffer));
        body_.push_back('\n');
    }
    void point(int groupCode, const Point2& p, double z = 0.0)
    {
        real(groupCode, p.x + shift_.x);
        real(groupCode + 10, p.y + shift_.y);
        real(groupCode + 20, z);
        extents_.expand(Point2(p.x + shift_.x, p.y + shift_.y));
    }
    std::uint64_t nextHandle() { return handle_++; }

    // ---- sections ----
    void assignNames();
    void writeTables();
    void writeBlocks();
    void writeEntities();
    void writeObjects();
    std::string header() const;

    // ---- entities ----
    std::uint64_t begin(std::string_view type, const Entity& entity, std::string_view subclass,
                        std::uint64_t owner = kModelSpaceRecord);
    void writeEntity(const Entity& entity);
    // The entity as its own geometry, with no drawn shapes looked up: what
    // writeEntity writes each drawn shape with, since a shape keeps its
    // entity's id.
    void writeGeometry(const Entity& entity);
    void writePolyline(const Entity& entity, const Polyline2& polyline);
    void writeCurvePolyline(const Entity& entity, const katana::geometry::CurvePolyline2& polyline);
    void writeEllipse(const Entity& entity, const katana::geometry::Ellipse2& ellipse);
    void writeSpline(const Entity& entity, const katana::geometry::Spline2& spline);
    void writeText(const Entity& entity, const katana::entity::TextGeometry& text);
    void writeDimension(const Entity& entity, const katana::entity::DimensionGeometry& dimension);
    void writeLeader(const Entity& entity, const katana::entity::LeaderGeometry& leader);
    // A text's height in the file: its paper height, or its style's, at the
    // export's annotation scale; else its model height.
    [[nodiscard]] double textHeightOf(const katana::entity::TextGeometry& text) const;
    // Ends the entity begun last with this module's extended data where it
    // has something the entity's groups could not hold: `heights` they
    // cannot carry, a colour no index is.
    void finish(const std::vector<std::optional<double>>* heights = nullptr);
    void textLine(const Entity& entity, const Point2& position, std::string_view line,
                  double height, double rotation, double z, const Point2* centre = nullptr,
                  const std::vector<std::optional<double>>* heights = nullptr);
    void line(const Entity& entity, const Point2& from, const Point2& to);
    std::string_view layerOf(const Entity& entity);
    std::vector<std::optional<double>> heightsOf(const Entity& entity, std::size_t count) const;

    const katana::entity::Model& model_;
    const ExportOptions& options_;
    Vec2 shift_{0.0, 0.0};
    std::string body_;
    std::uint64_t handle_ = kFirstFreeHandle;
    katana::geometry::Box2 extents_;
    DxfExport result_;
    std::unordered_map<std::string, std::string> layerNames_;    // Katana path -> DXF
    std::unordered_map<std::string, std::string> linetypeNames_; // Katana -> DXF
    // The table records in the order they are written, with their DXF names.
    std::vector<std::pair<const katana::entity::Linetype*, std::string>> linetypes_;
    std::vector<std::pair<const katana::entity::Layer*, std::string>> layers_;
    std::size_t partialHeights_ = 0;
    // Labels drawn out by the front end that drew nothing (no room at the
    // scale), counted for the warning.
    std::size_t drawnNothing_ = 0;
    // Labels, and dimensions of a kind other than aligned, not written
    // (writeEntity says why), for the export's warning.
    std::size_t labelsSkipped_ = 0;
    std::size_t dimensionsSkipped_ = 0;
    // The entity begun last has a colour no index is exactly.
    std::optional<katana::entity::Color> exactColour_;
    std::size_t lastColourKey_ = 0;
    int lastColourIndex_ = kColourForeground;
    bool haveLastColour_ = false;
};

std::string Writer::header() const
{
    // Written last, into a string of its own, because $HANDSEED - the next
    // free handle - is known only once everything else has one.
    std::string out;
    const auto group = [&](std::string_view groupCode, std::string_view value) {
        out.append(groupCode);
        out.push_back('\n');
        out.append(value);
        out.push_back('\n');
    };
    const auto realText = [](double value) {
        char buffer[32];
        const auto [end, error] = std::to_chars(buffer, buffer + sizeof(buffer), value);
        (void)error;
        std::string text(buffer, static_cast<std::size_t>(end - buffer));
        if (text.find_first_of(".eEni") == std::string::npos) {
            text += ".0";
        }
        return text;
    };
    const bool none = extents_.empty();
    const Point2 low = none ? Point2(0.0, 0.0) : extents_.min;
    const Point2 high = none ? Point2(0.0, 0.0) : extents_.max;
    char seed[24];
    const auto [seedEnd, seedError] = std::to_chars(seed, seed + sizeof(seed), handle_, 16);
    (void)seedError;
    std::string handseed(seed, static_cast<std::size_t>(seedEnd - seed));
    for (char& c : handseed) {
        if (c >= 'a' && c <= 'f') {
            c = static_cast<char>(c - 'a' + 'A');
        }
    }
    group("  0", "SECTION");
    group("  2", "HEADER");
    group("  9", "$ACADVER");
    group("  1", "AC1015");
    group("  9", "$ACADMAINTVER");
    group(" 70", "6");
    group("  9", "$DWGCODEPAGE");
    group("  3", "ANSI_1252");
    group("  9", "$INSBASE");
    group(" 10", "0.0");
    group(" 20", "0.0");
    group(" 30", "0.0");
    group("  9", "$EXTMIN");
    group(" 10", realText(low.x));
    group(" 20", realText(low.y));
    group(" 30", "0.0");
    group("  9", "$EXTMAX");
    group(" 10", realText(high.x));
    group(" 20", realText(high.y));
    group(" 30", "0.0");
    group("  9", "$LIMMIN");
    group(" 10", realText(low.x));
    group(" 20", realText(low.y));
    group("  9", "$LIMMAX");
    group(" 10", realText(high.x));
    group(" 20", realText(high.y));
    group("  9", "$LTSCALE");
    group(" 40", "1.0");
    group("  9", "$TEXTSIZE");
    group(" 40", "2.5");
    group("  9", "$TEXTSTYLE");
    group("  7", "Standard");
    group("  9", "$CLAYER");
    group("  8", "0");
    group("  9", "$CELTYPE");
    group("  6", "ByLayer");
    group("  9", "$CECOLOR");
    group(" 62", "256");
    group("  9", "$DIMSTYLE");
    group("  2", "Standard");
    group("  9", "$HANDSEED");
    group("  5", handseed);
    group("  9", "$MEASUREMENT");
    group(" 70", "1");
    // Metres: survey coordinates are, and a drawing with no unit set is
    // scaled by nothing when it is inserted into one that has.
    group("  9", "$INSUNITS");
    group(" 70", "6");
    group("  9", "$LWDISPLAY");
    group("290", "1");
    group("  0", "ENDSEC");
    // CLASSES: the object classes the OBJECTS section below uses.
    group("  0", "SECTION");
    group("  2", "CLASSES");
    const auto objectClass = [&](std::string_view dxfName, std::string_view className) {
        group("  0", "CLASS");
        group("  1", dxfName);
        group("  2", className);
        group("  3", "ObjectDBX Classes");
        group(" 90", "0");
        group("280", "0");
        group("281", "0");
    };
    objectClass("ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault");
    objectClass("ACDBPLACEHOLDER", "AcDbPlaceHolder");
    objectClass("LAYOUT", "AcDbLayout");
    group("  0", "ENDSEC");
    return out;
}

// The DXF name of every linetype and layer, before anything is written: the
// entities, written first, name their layers, and the tables, written after
// them, list the same names under the same handles.
void Writer::assignNames()
{
    SymbolNames linetypeSymbols;
    linetypeSymbols.reserve("ByBlock");
    linetypeSymbols.reserve("ByLayer");
    linetypeSymbols.reserve("Continuous");
    model_.linetypes.forEach([&](const katana::entity::Linetype& linetype) {
        if (linetype.isContinuous()) {
            return;
        }
        std::string name = linetypeSymbols.unique(symbolName(linetype.name, false));
        linetypeNames_.emplace(linetype.name, name);
        linetypes_.emplace_back(&linetype, std::move(name));
    });
    // Every layer the drawing has, used or not - an empty layer is still part
    // of how the drawing is organised. Layer 0 first, under its own name.
    const std::set<std::string, std::less<>> only(options_.layers.begin(), options_.layers.end());
    SymbolNames layerSymbols;
    layerSymbols.reserve("0");
    for (const std::string& path : model_.layers.names()) {
        const bool zero = path == katana::entity::kDefaultLayerName;
        if (!only.empty() && !zero && !only.contains(path)) {
            continue;
        }
        const katana::entity::Layer* layer = model_.layers.find(path);
        if (layer == nullptr) {
            continue;
        }
        std::string name = zero ? std::string("0") : layerSymbols.unique(layerNameFor(path));
        layerNames_.emplace(path, name);
        layers_.emplace_back(layer, std::move(name));
    }
}

void Writer::writeTables()
{
    text(0, "SECTION");
    text(2, "TABLES");
    const auto tableHead = [&](std::string_view name, std::uint64_t own, std::int64_t count) {
        text(0, "TABLE");
        text(2, name);
        handle(5, own);
        handle(330, 0);
        text(100, "AcDbSymbolTable");
        integer(70, count);
    };
    const auto record = [&](std::string_view type, std::uint64_t own, std::uint64_t table,
                            std::string_view subclass) {
        text(0, type);
        handle(5, own);
        handle(330, table);
        text(100, "AcDbSymbolTableRecord");
        text(100, subclass);
    };

    // VPORT: the one active viewport, looking at the extents.
    tableHead("VPORT", kViewportTable, 1);
    record("VPORT", kActiveViewport, kViewportTable, "AcDbViewportTableRecord");
    text(2, "*Active");
    integer(70, 0);
    real(10, 0.0);
    real(20, 0.0);
    real(11, 1.0);
    real(21, 1.0);
    const Point2 centre = extents_.empty() ? Point2(0.0, 0.0) : extents_.center();
    real(12, centre.x);
    real(22, centre.y);
    real(13, 0.0);
    real(23, 0.0);
    real(14, 10.0);
    real(24, 10.0);
    real(15, 10.0);
    real(25, 10.0);
    real(16, 0.0);
    real(26, 0.0);
    real(36, 1.0);
    real(17, 0.0);
    real(27, 0.0);
    real(37, 0.0);
    real(40, extents_.empty() ? 100.0 : std::max(1.0, extents_.height() * 1.1));
    real(41, extents_.empty() ? 1.5 : std::clamp(extents_.width() / std::max(extents_.height(), 1e-9), 0.1, 10.0));
    real(42, 50.0);
    real(43, 0.0);
    real(44, 0.0);
    real(50, 0.0);
    real(51, 0.0);
    integer(71, 0);
    integer(72, 1000);
    integer(73, 1);
    integer(74, 3);
    integer(75, 0);
    integer(76, 0);
    integer(77, 0);
    integer(78, 0);
    text(0, "ENDTAB");

    // LTYPE: the three every file has, then the drawing's patterns.
    tableHead("LTYPE", kLinetypeTable, static_cast<std::int64_t>(3 + linetypes_.size()));
    const auto builtIn = [&](std::string_view name, std::uint64_t own, std::string_view text3) {
        record("LTYPE", own, kLinetypeTable, "AcDbLinetypeTableRecord");
        text(2, name);
        integer(70, 0);
        text(3, text3);
        integer(72, 65);
        integer(73, 0);
        real(40, 0.0);
    };
    builtIn("ByBlock", kLinetypeByBlock, "");
    builtIn("ByLayer", kLinetypeByLayer, "");
    builtIn("Continuous", kLinetypeContinuous, "Solid line");
    for (const auto& [linetype, name] : linetypes_) {
        record("LTYPE", nextHandle(), kLinetypeTable, "AcDbLinetypeTableRecord");
        text(2, name);
        integer(70, 0);
        text(3, encodeText(linetype->description));
        integer(72, 65);
        integer(73, static_cast<std::int64_t>(linetype->pattern.size()));
        real(40, linetype->patternLength());
        for (const katana::entity::LinetypeElement& element : linetype->pattern) {
            real(49, element.length);
            integer(74, 0);
        }
        ++result_.linetypesWritten;
    }
    text(0, "ENDTAB");

    tableHead("LAYER", kLayerTable, static_cast<std::int64_t>(layers_.size()));
    for (const auto& [layer, name] : layers_) {
        record("LAYER", nextHandle(), kLayerTable, "AcDbLayerTableRecord");
        text(2, name);
        integer(70, layer->locked ? 4 : 0);
        const int colour = nearestIndexedColour(layer->color);
        integer(62, layer->visible ? colour : -colour);
        const auto linetype = linetypeNames_.find(layer->linetype);
        text(6, linetype != linetypeNames_.end() ? std::string_view(linetype->second)
                                                  : std::string_view("Continuous"));
        integer(370, nearestLineweight(layer->lineWeight));
        handle(390, kPlotStylePlaceholder);
        // The path, where the name could not hold it, and the colour, where
        // no index is it, for this reader to take back: other programs pass
        // extended data by without reading it.
        const bool exact = indexedColour(colour) == layer->color;
        if (name != layer->name || !exact) {
            text(1001, kApplicationName);
            if (name != layer->name) {
                text(1000, kExtendedPath);
                text(1000, encodeText(layer->name));
            }
            if (!exact) {
                text(1000, kExtendedColour);
                text(1000, layer->color.toHex());
            }
        }
        ++result_.layersWritten;
    }
    text(0, "ENDTAB");

    tableHead("STYLE", kStyleTable, 1);
    record("STYLE", kStandardStyle, kStyleTable, "AcDbTextStyleTableRecord");
    text(2, "Standard");
    integer(70, 0);
    real(40, 0.0);
    real(41, 1.0);
    real(50, 0.0);
    integer(71, 0);
    real(42, 2.5);
    text(3, "txt");
    text(4, "");
    text(0, "ENDTAB");

    tableHead("VIEW", kViewTable, 0);
    text(0, "ENDTAB");
    tableHead("UCS", kUcsTable, 0);
    text(0, "ENDTAB");

    tableHead("APPID", kApplicationTable, 2);
    record("APPID", kApplicationAcad, kApplicationTable, "AcDbRegAppTableRecord");
    text(2, "ACAD");
    integer(70, 0);
    record("APPID", kApplicationKatana, kApplicationTable, "AcDbRegAppTableRecord");
    text(2, kApplicationName);
    integer(70, 0);
    text(0, "ENDTAB");

    tableHead("DIMSTYLE", kDimensionStyleTable, 1);
    text(100, "AcDbDimStyleTable");
    text(0, "DIMSTYLE");
    handle(105, kStandardDimensionStyle); // a dimension style's handle is 105, not 5
    handle(330, kDimensionStyleTable);
    text(100, "AcDbSymbolTableRecord");
    text(100, "AcDbDimStyleTableRecord");
    text(2, "Standard");
    integer(70, 0);
    real(40, 1.0);
    real(41, 2.5);
    real(42, 0.625);
    real(43, 3.75);
    real(44, 1.25);
    real(140, 2.5);
    real(141, 2.5);
    real(147, 0.625);
    integer(77, 1);
    integer(271, 3);
    text(0, "ENDTAB");

    tableHead("BLOCK_RECORD", kBlockRecordTable, 2);
    record("BLOCK_RECORD", kModelSpaceRecord, kBlockRecordTable, "AcDbBlockTableRecord");
    text(2, "*Model_Space");
    handle(340, kModelLayout);
    record("BLOCK_RECORD", kPaperSpaceRecord, kBlockRecordTable, "AcDbBlockTableRecord");
    text(2, "*Paper_Space");
    handle(340, kPaperLayout);
    text(0, "ENDTAB");
    text(0, "ENDSEC");
}

void Writer::writeBlocks()
{
    text(0, "SECTION");
    text(2, "BLOCKS");
    const auto block = [&](std::string_view name, std::uint64_t begin, std::uint64_t end,
                           std::uint64_t owner) {
        text(0, "BLOCK");
        handle(5, begin);
        handle(330, owner);
        text(100, "AcDbEntity");
        text(8, "0");
        text(100, "AcDbBlockBegin");
        text(2, name);
        integer(70, 0);
        real(10, 0.0);
        real(20, 0.0);
        real(30, 0.0);
        text(3, name);
        text(1, "");
        text(0, "ENDBLK");
        handle(5, end);
        handle(330, owner);
        text(100, "AcDbEntity");
        text(8, "0");
        text(100, "AcDbBlockEnd");
    };
    block("*Model_Space", kModelSpaceBlock, kModelSpaceEnd, kModelSpaceRecord);
    block("*Paper_Space", kPaperSpaceBlock, kPaperSpaceEnd, kPaperSpaceRecord);
    text(0, "ENDSEC");
}

void Writer::writeObjects()
{
    text(0, "SECTION");
    text(2, "OBJECTS");
    const auto dictionary = [&](std::uint64_t own, std::uint64_t owner) {
        text(0, "DICTIONARY");
        handle(5, own);
        handle(330, owner);
        text(100, "AcDbDictionary");
        integer(281, 1);
    };
    dictionary(kRootDictionary, 0);
    text(3, "ACAD_GROUP");
    handle(350, kGroupDictionary);
    text(3, "ACAD_LAYOUT");
    handle(350, kLayoutDictionary);
    text(3, "ACAD_MLINESTYLE");
    handle(350, kMultilineStyleDictionary);
    text(3, "ACAD_PLOTSTYLENAME");
    handle(350, kPlotStyleDictionary);
    dictionary(kGroupDictionary, kRootDictionary);
    dictionary(kLayoutDictionary, kRootDictionary);
    text(3, "Layout1");
    handle(350, kPaperLayout);
    text(3, "Model");
    handle(350, kModelLayout);
    dictionary(kMultilineStyleDictionary, kRootDictionary);
    text(3, "Standard");
    handle(350, kMultilineStyle);
    text(0, "ACDBDICTIONARYWDFLT");
    handle(5, kPlotStyleDictionary);
    handle(330, kRootDictionary);
    text(100, "AcDbDictionary");
    integer(281, 1);
    text(3, "Normal");
    handle(350, kPlotStylePlaceholder);
    text(100, "AcDbDictionaryWithDefault");
    handle(340, kPlotStylePlaceholder);
    text(0, "ACDBPLACEHOLDER");
    handle(5, kPlotStylePlaceholder);
    handle(330, kPlotStyleDictionary);
    text(0, "MLINESTYLE");
    handle(5, kMultilineStyle);
    handle(330, kMultilineStyleDictionary);
    text(100, "AcDbMlineStyle");
    text(2, "Standard");
    integer(70, 0);
    text(3, "");
    integer(62, 256);
    real(51, 90.0);
    real(52, 90.0);
    integer(71, 2);
    real(49, 0.5);
    integer(62, 256);
    text(6, "BYLAYER");
    real(49, -0.5);
    integer(62, 256);
    text(6, "BYLAYER");
    const auto layout = [&](std::uint64_t own, std::string_view name, int flags, int order,
                            std::uint64_t record) {
        text(0, "LAYOUT");
        handle(5, own);
        handle(330, kLayoutDictionary);
        text(100, "AcDbPlotSettings");
        text(1, "");
        text(4, "A3");
        text(6, "");
        real(40, 7.5);
        real(41, 20.0);
        real(42, 7.5);
        real(43, 20.0);
        real(44, 420.0);
        real(45, 297.0);
        real(46, 0.0);
        real(47, 0.0);
        real(48, 0.0);
        real(49, 0.0);
        real(140, 0.0);
        real(141, 0.0);
        real(142, 1.0);
        real(143, 1.0);
        integer(70, flags);
        integer(72, 1);
        integer(73, 0);
        integer(74, 5);
        text(7, "");
        integer(75, 16);
        integer(76, 0);
        integer(77, 2);
        integer(78, 300);
        real(147, 1.0);
        real(148, 0.0);
        real(149, 0.0);
        text(100, "AcDbLayout");
        text(1, name);
        integer(70, 1);
        integer(71, order);
        real(10, 0.0);
        real(20, 0.0);
        real(11, 420.0);
        real(21, 297.0);
        real(12, 0.0);
        real(22, 0.0);
        real(32, 0.0);
        real(14, 1e20);
        real(24, 1e20);
        real(34, 1e20);
        real(15, -1e20);
        real(25, -1e20);
        real(35, -1e20);
        real(146, 0.0);
        real(13, 0.0);
        real(23, 0.0);
        real(33, 0.0);
        real(16, 1.0);
        real(26, 0.0);
        real(36, 0.0);
        real(17, 0.0);
        real(27, 1.0);
        real(37, 0.0);
        integer(76, 1);
        handle(330, record);
    };
    layout(kModelLayout, "Model", 1024, 0, kModelSpaceRecord);
    layout(kPaperLayout, "Layout1", 0, 1, kPaperSpaceRecord);
    text(0, "ENDSEC");
}

std::string_view Writer::layerOf(const Entity& entity)
{
    const auto found = layerNames_.find(entity.layer);
    if (found != layerNames_.end()) {
        return found->second;
    }
    // A layer the table does not have (the model would not allow one, but a
    // name is cheap to make safe): the name, made legal.
    return layerNames_.emplace(entity.layer, layerNameFor(entity.layer)).first->second;
}

std::uint64_t Writer::begin(std::string_view type, const Entity& entity, std::string_view subclass,
                            std::uint64_t owner)
{
    exactColour_.reset();
    const std::uint64_t own = nextHandle();
    text(0, type);
    handle(5, own);
    handle(330, owner);
    text(100, "AcDbEntity");
    text(8, layerOf(entity));
    if (entity.color) {
        const katana::entity::Color& colour = *entity.color;
        const std::size_t key = (static_cast<std::size_t>(colour.r) << 16) |
                                (static_cast<std::size_t>(colour.g) << 8) | colour.b;
        if (!haveLastColour_ || key != lastColourKey_) {
            lastColourIndex_ = nearestIndexedColour(colour);
            lastColourKey_ = key;
            haveLastColour_ = true;
        }
        integer(62, lastColourIndex_);
        if (indexedColour(lastColourIndex_) != colour) {
            exactColour_ = colour;
        }
    }
    if (const auto linetype = entity.metadata.find(kMetaLinetype);
        linetype != entity.metadata.end()) {
        const auto name = linetypeNames_.find(katana::entity::toString(linetype->second));
        if (name != linetypeNames_.end()) {
            text(6, name->second);
        }
    }
    if (const auto weight = entity.metadata.find(kMetaLineweight);
        weight != entity.metadata.end()) {
        if (const auto* value = std::get_if<std::int64_t>(&weight->second)) {
            integer(370, nearestLineweight(static_cast<double>(*value) / 100.0));
        }
    }
    if (!entity.visible) {
        integer(60, 1);
    }
    text(100, subclass);
    ++result_.dxfEntitiesWritten;
    return own;
}

std::vector<std::optional<double>> Writer::heightsOf(const Entity& entity, std::size_t count) const
{
    if (entity.properties.empty()) {
        return std::vector<std::optional<double>>(count);
    }
    return katana::entity::heightsOf(entity.properties, count);
}

void Writer::line(const Entity& entity, const Point2& from, const Point2& to)
{
    begin("LINE", entity, "AcDbLine");
    point(10, from);
    point(11, to);
    finish();
}

void Writer::textLine(const Entity& entity, const Point2& position, std::string_view lineText,
                      double height, double rotation, double z, const Point2* centre,
                      const std::vector<std::optional<double>>* heights)
{
    begin("TEXT", entity, "AcDbText");
    point(10, position, z);
    real(40, height);
    text(1, encodeText(lineText));
    // The degrees that read back as exactly this rotation, where it is a
    // turn or less, as every drawn text's is.
    double degrees = exactDegrees(rotation);
    if (degrees < 0.0 || degrees >= 360.0) {
        degrees = std::fmod(degrees, 360.0);
        if (degrees < 0.0) {
            degrees += 360.0;
        }
    }
    if (degrees != 0.0) {
        real(50, degrees);
    }
    if (centre != nullptr) {
        integer(72, 1);
        point(11, *centre, z);
    }
    text(100, "AcDbText");
    finish(heights);
}

void Writer::writeText(const Entity& entity, const katana::entity::TextGeometry& geometry)
{
    const auto heights = heightsOf(entity, 1);
    const double z = heights.front().value_or(0.0);
    const auto* besides = isZeroLevel(heights) ? &heights : nullptr;
    // One TEXT a line: TEXT holds one, and a Text holding a break is lines.
    const double height = textHeightOf(geometry);
    const Vec2 down = Vec2(std::sin(geometry.rotation), -std::cos(geometry.rotation)) *
                      (kLinePitch * height);
    std::string_view rest = geometry.text;
    Point2 position = geometry.position;
    if (geometry.justify != katana::entity::TextJustify::BottomLeft) {
        // The block's first baseline-left, by the estimate the model's box
        // uses (entity/text_block.hpp): TEXT's own alignment points would
        // need the reader's font to agree with ours.
        katana::entity::TextGeometry sized = geometry;
        sized.height = height;
        const auto corners = katana::entity::estimatedTextCorners(sized);
        position = corners[3] + Vec2(std::sin(geometry.rotation), -std::cos(geometry.rotation)) *
                                    height;
    }
    for (;;) {
        const std::size_t end = rest.find('\n');
        std::string_view lineText = rest.substr(0, end);
        if (!lineText.empty() && lineText.back() == '\r') {
            lineText.remove_suffix(1);
        }
        if (!lineText.empty()) {
            textLine(entity, position, lineText, height, geometry.rotation, z, nullptr, besides);
        }
        if (end == std::string_view::npos) {
            break;
        }
        rest.remove_prefix(end + 1);
        position = position + down;
    }
}

// This module's extended data, under its registered application: what the
// entity's own groups cannot hold, for this reader to take back. Other
// programs pass it by.
//
// HEIGHTS the entity's own groups cannot hold:
//  - known at some vertices and not others. The entity itself goes in plan,
//    because the format has no "no height" and a vertex written at Z 0 would
//    be a false level - a surface built from the file would dive to the datum
//    there.
//  - at the two ends of an arc that climbs: ARC has one Z. In plan too, for
//    the same reason.
//  - a level of exactly 0 at every vertex. The groups say Z 0, which is how
//    every program writes "in plan", and so how this reader takes it; a
//    surveyed level of 0 would come back as no level at all.
// The list is the `elevations` property's text ("31.25 null 32.5"), in pieces
// of at most 250 characters, since R2000 holds an extended data string to
// 255. One height at every vertex is that one height alone, which the reader
// takes for every vertex, as the `elevation` property is. Only the first two
// kinds lose anything in another program, and the export's warning counts
// them.
//
// A COLOUR that no index is: R2000 has no true colour, so the entity carries
// the nearest index for everyone else and its own colour here.
void Writer::finish(const std::vector<std::optional<double>>* heights)
{
    if (heights == nullptr && !exactColour_) {
        return;
    }
    text(1001, kApplicationName);
    if (heights != nullptr) {
        const bool one = !heights->empty() &&
                         std::all_of(heights->begin(), heights->end(), [&](const auto& height) {
                             return height.has_value() && *height == *heights->front();
                         });
        const std::size_t count = one ? 1 : heights->size();
        text(1000, kExtendedHeights);
        text(1002, "{");
        std::string piece;
        for (std::size_t i = 0; i < count; ++i) {
            const auto& height = (*heights)[i];
            std::string token = "null";
            if (height) {
                char buffer[32];
                const auto [end, error] = std::to_chars(buffer, buffer + sizeof(buffer), *height);
                (void)error;
                token.assign(buffer, static_cast<std::size_t>(end - buffer));
            }
            if (!piece.empty() && piece.size() + 1 + token.size() > 250) {
                text(1000, piece);
                piece.clear();
            }
            if (!piece.empty()) {
                piece.push_back(' ');
            }
            piece += token;
        }
        if (!piece.empty()) {
            text(1000, piece);
        }
        text(1002, "}");
        if (!one) {
            ++partialHeights_; // written in plan: other programs see no height
        }
    }
    if (exactColour_) {
        text(1000, kExtendedColour);
        text(1000, exactColour_->toHex());
        exactColour_.reset();
    }
}

void Writer::writePolyline(const Entity& entity, const Polyline2& polyline)
{
    const std::size_t count = polyline.vertices.size();
    const auto heights = heightsOf(entity, count);
    bool anyHeight = false;
    bool everyHeight = true;
    bool oneHeight = true;
    for (const auto& height : heights) {
        anyHeight = anyHeight || height.has_value();
        everyHeight = everyHeight && height.has_value();
        oneHeight = oneHeight && height.has_value() && *height == heights.front().value_or(0.0);
    }
    if (!anyHeight || oneHeight || !everyHeight) {
        // Every vertex at one height, or none: an LWPOLYLINE, with the closed
        // flag. Never a HATCH - a closed boundary is a line, not a fill.
        begin("LWPOLYLINE", entity, "AcDbPolyline");
        integer(90, static_cast<std::int64_t>(count));
        integer(70, polyline.closed ? 1 : 0);
        const bool zeroLevel = oneHeight && anyHeight && *heights.front() == 0.0;
        if (oneHeight && anyHeight && !zeroLevel) {
            real(38, *heights.front());
        }
        for (const Point2& vertex : polyline.vertices) {
            real(10, vertex.x + shift_.x);
            real(20, vertex.y + shift_.y);
            extents_.expand(Point2(vertex.x + shift_.x, vertex.y + shift_.y));
        }
        finish((anyHeight && !everyHeight) || zeroLevel ? &heights : nullptr);
        return;
    }
    // Heights that differ: a 3D polyline, the only kind with a Z per vertex.
    const std::uint64_t owner = begin("POLYLINE", entity, "AcDb3dPolyline");
    integer(66, 1);
    real(10, 0.0);
    real(20, 0.0);
    real(30, 0.0);
    integer(70, 8 | (polyline.closed ? 1 : 0));
    finish(); // on the POLYLINE, before its vertices
    for (std::size_t i = 0; i < count; ++i) {
        begin("VERTEX", entity, "AcDbVertex", owner);
        text(100, "AcDb3dPolylineVertex");
        point(10, polyline.vertices[i], *heights[i]);
        integer(70, 32);
    }
    const std::uint64_t end = nextHandle();
    text(0, "SEQEND");
    handle(5, end);
    handle(330, owner);
    text(100, "AcDbEntity");
    text(8, layerOf(entity));
}

// A polyline with arcs: an LWPOLYLINE with each arc's bulge (group 42), the
// format's own way of saying it. Heights one and the same at every vertex go
// in its elevation; any other heights go beside it as this module's extended
// data, as a straight polyline's partial heights do, since the LWPOLYLINE has
// one elevation and the 3D POLYLINE, which has a Z per vertex, has no arcs.
void Writer::writeCurvePolyline(const Entity& entity,
                                const katana::geometry::CurvePolyline2& polyline)
{
    const std::size_t count = polyline.vertices.size();
    const auto heights = polyline.heights();
    const bool anyHeight = polyline.hasHeights();
    bool oneHeight = anyHeight;
    for (const auto& height : heights) {
        oneHeight = oneHeight && height.has_value() && *height == *heights.front();
    }
    begin("LWPOLYLINE", entity, "AcDbPolyline");
    integer(90, static_cast<std::int64_t>(count));
    integer(70, polyline.closed ? 1 : 0);
    const bool zeroLevel = oneHeight && *heights.front() == 0.0;
    if (oneHeight && !zeroLevel) {
        real(38, *heights.front());
    }
    for (std::size_t i = 0; i < count; ++i) {
        const auto& vertex = polyline.vertices[i];
        real(10, vertex.position.x + shift_.x);
        real(20, vertex.position.y + shift_.y);
        const bool startsSegment = polyline.closed || i + 1 < count;
        if (startsSegment && vertex.bulge != 0.0) {
            real(42, vertex.bulge);
        }
        extents_.expand(Point2(vertex.position.x + shift_.x, vertex.position.y + shift_.y));
    }
    for (const auto& piece : polyline.segments()) {
        if (const auto* arc = std::get_if<Arc2>(&piece)) {
            const auto box = arc->boundingBox();
            extents_.expand(Point2(box.min.x + shift_.x, box.min.y + shift_.y));
            extents_.expand(Point2(box.max.x + shift_.x, box.max.y + shift_.y));
        }
    }
    finish((anyHeight && !oneHeight) || zeroLevel ? &heights : nullptr);
}

// ELLIPSE: centre, the major axis's end relative to it, the ratio, and the
// start and end parameters in radians - Ellipse2's own fields.
void Writer::writeEllipse(const Entity& entity, const katana::geometry::Ellipse2& ellipse)
{
    const auto heights = heightsOf(entity, 1);
    begin("ELLIPSE", entity, "AcDbEllipse");
    point(10, ellipse.center, heights.front().value_or(0.0));
    real(11, ellipse.majorAxis.x);
    real(21, ellipse.majorAxis.y);
    real(31, 0.0);
    real(40, std::min(ellipse.ratio, 1.0));
    real(41, ellipse.startParameter);
    real(42, ellipse.isFull() ? ellipse.startParameter + 2.0 * std::numbers::pi
                              : ellipse.endParameter());
    finish(isZeroLevel(heights) ? &heights : nullptr);
    const auto box = ellipse.boundingBox();
    extents_.expand(Point2(box.min.x + shift_.x, box.min.y + shift_.y));
    extents_.expand(Point2(box.max.x + shift_.x, box.max.y + shift_.y));
}

// SPLINE, planar: degree, knots, weights when rational, control points, and
// the fit points it was drawn through when it has them.
void Writer::writeSpline(const Entity& entity, const katana::geometry::Spline2& spline)
{
    begin("SPLINE", entity, "AcDbSpline");
    real(210, 0.0);
    real(220, 0.0);
    real(230, 1.0);
    integer(70, 8 | (spline.isRational() ? 4 : 0));
    integer(71, spline.degree);
    integer(72, static_cast<std::int64_t>(spline.knots.size()));
    integer(73, static_cast<std::int64_t>(spline.controlPoints.size()));
    integer(74, static_cast<std::int64_t>(spline.fitPoints.size()));
    real(42, 1e-10);
    real(43, 1e-10);
    if (!spline.fitPoints.empty()) {
        real(44, 1e-10);
    }
    for (const double knot : spline.knots) {
        real(40, knot);
    }
    for (const double weight : spline.weights) {
        real(41, weight);
    }
    for (const Point2& p : spline.controlPoints) {
        point(10, p);
    }
    for (const Point2& p : spline.fitPoints) {
        point(11, p);
    }
    finish();
}

// A dimension as the lines and text it draws: the format's DIMENSION needs a
// picture block beside it that every reader draws instead of the entity, so
// writing the picture is writing what the reader will see, without the half
// that some readers get wrong.
double Writer::textHeightOf(const katana::entity::TextGeometry& text) const
{
    double paper = text.paperHeight;
    if (!(paper > 0.0) && !text.style.empty()) {
        if (const auto* style = model_.textStyles.find(text.style); style != nullptr) {
            paper = style->paperHeight;
        }
    }
    return paper > 0.0 ? katana::entity::annotationModelSize(paper, options_.annotationScale)
                       : text.height;
}

// A leader as the lines and text it draws at the export's annotation scale:
// the line through its vertices, a landing, and its note's lines beside the
// landing. No arrowhead - a filled one would be a SOLID with nothing to say
// it belongs to the line - and no callout frame.
void Writer::writeLeader(const Entity& entity, const katana::entity::LeaderGeometry& leader)
{
    const auto& v = leader.vertices;
    for (std::size_t i = 0; i + 1 < v.size(); ++i) {
        line(entity, v[i], v[i + 1]);
    }
    const double scale = options_.annotationScale;
    const Vec2 last = v.back() - v[v.size() - 2];
    const double sign = last.x < 0.0 ? -1.0 : 1.0;
    Point2 end = v.back();
    if (leader.landing > 0.0) {
        end = end + Vec2(sign * katana::entity::annotationModelSize(leader.landing, scale), 0.0);
        line(entity, v.back(), end);
    }
    // A smart leader's note is read off its target (docs/annotation.md,
    // "Smart leaders"). This layer cannot see survey coding, so a {code}
    // field is absent here; a front end hands the drawn leader instead
    // (ExportOptions::drawn), which has it.
    const std::string text = katana::entity::leaderNote(model_, leader);
    if (text.empty()) {
        return;
    }
    double paper = leader.paperHeight;
    if (!(paper > 0.0)) {
        const auto* style = model_.textStyles.find(
            leader.style.empty() ? katana::entity::kDefaultTextStyleName : leader.style);
        paper = style != nullptr && style->paperHeight > 0.0 ? style->paperHeight : 2.5;
    }
    katana::entity::TextGeometry note;
    note.text = text;
    note.height = katana::entity::annotationModelSize(paper, scale);
    note.position = end + Vec2(sign * 0.5 * note.height, 0.0);
    note.justify = sign > 0.0 ? katana::entity::TextJustify::MiddleLeft
                              : katana::entity::TextJustify::MiddleRight;
    writeText(entity, note);
}

void Writer::writeDimension(const Entity& entity, const katana::entity::DimensionGeometry& dimension)
{
    if (dimension.kind != katana::entity::DimensionKind::Aligned) {
        ++dimensionsSkipped_;
        ++result_.entitiesSkipped;
        return;
    }
    const katana::entity::Layer* layer = model_.layers.find(entity.layer);
    const std::string styleName = layer != nullptr && !layer->dimensionStyle.empty()
                                      ? layer->dimensionStyle
                                      : std::string(katana::entity::kDefaultDimensionStyleName);
    const katana::entity::DimensionStyle* found = model_.dimensionStyles.find(styleName);
    katana::entity::DimensionStyle style = found != nullptr ? *found
                                                            : katana::entity::DimensionStyle{};
    if (style.paperSized) {
        for (double* size : {&style.textHeight, &style.textGap, &style.extensionOffset,
                             &style.extensionBeyond, &style.arrowSize}) {
            *size = katana::entity::annotationModelSize(*size, options_.annotationScale);
        }
    }
    const double length = dimension.measurement();
    const Vec2 along = (dimension.end - dimension.start) / length;
    const Vec2 left(-along.y, along.x);
    const double side = dimension.offset >= 0.0 ? 1.0 : -1.0;
    const Point2 a = dimension.start + left * dimension.offset;
    const Point2 b = dimension.end + left * dimension.offset;
    if (std::abs(dimension.offset) > style.extensionOffset) {
        line(entity, dimension.start + left * (side * style.extensionOffset),
             a + left * (side * style.extensionBeyond));
        line(entity, dimension.end + left * (side * style.extensionOffset),
             b + left * (side * style.extensionBeyond));
    }
    line(entity, a, b);
    // Architectural ticks: a slash of the arrow size across each end.
    const Vec2 tick = (along + left) * (style.arrowSize * 0.5 / std::sqrt(2.0));
    line(entity, a - tick, a + tick);
    line(entity, b - tick, b + tick);
    const std::string label =
        katana::entity::dimensionLabel(length, dimension.textOverride, style);
    if (!label.empty()) {
        // Read from below or from the right, never upside down.
        double rotation = std::atan2(along.y, along.x);
        Vec2 up = left;
        Vec2 forward = along;
        if (rotation > std::numbers::pi / 2.0 + 1e-9 || rotation <= -std::numbers::pi / 2.0) {
            rotation += std::numbers::pi;
            up = up * -1.0;
            forward = forward * -1.0;
        }
        const Point2 middle = (a + b) * 0.5 + up * style.textGap;
        const double width = 0.6 * style.textHeight * static_cast<double>(label.size());
        const Point2 start = middle - forward * (width / 2.0);
        textLine(entity, start, label, style.textHeight, rotation, 0.0, &middle);
    }
}

void Writer::writeEntity(const Entity& entity)
{
    // Annotation the front end drew out for this file (ExportOptions::drawn):
    // written as its shapes, on its layer and in its colour.
    if (options_.drawn != nullptr) {
        if (const auto found = options_.drawn->find(entity.id); found != options_.drawn->end()) {
            if (found->second.empty()) {
                ++drawnNothing_;
            }
            Entity shape = entity;
            shape.properties.clear(); // an annotation's shapes have no surveyed heights
            for (const katana::entity::Geometry& geometry : found->second) {
                shape.geometry = geometry;
                writeGeometry(shape);
            }
            return;
        }
    }
    writeGeometry(entity);
}

void Writer::writeGeometry(const Entity& entity)
{
    std::visit(
        [&](const auto& shape) {
            using T = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<T, katana::entity::PointGeometry>) {
                const auto heights = heightsOf(entity, 1);
                begin("POINT", entity, "AcDbPoint");
                point(10, shape.position, heights.front().value_or(0.0));
                finish(isZeroLevel(heights) ? &heights : nullptr);
            } else if constexpr (std::is_same_v<T, Segment2>) {
                const auto heights = heightsOf(entity, 2);
                // One end's height without the other's: in plan, with the
                // heights beside it (finish says why).
                const bool both = heights[0].has_value() && heights[1].has_value();
                begin("LINE", entity, "AcDbLine");
                point(10, shape.start, both ? *heights[0] : 0.0);
                point(11, shape.end, both ? *heights[1] : 0.0);
                finish((!both && (heights[0] || heights[1])) || isZeroLevel(heights) ? &heights
                                                                                     : nullptr);
            } else if constexpr (std::is_same_v<T, Arc2>) {
                const bool full = std::abs(shape.sweep) >= 2.0 * std::numbers::pi - 1e-12;
                // Every arc in the file runs counter-clockwise: a clockwise
                // one is the same curve from its other end, and its heights
                // are in that order too.
                const double from = shape.sweep >= 0.0 ? shape.startAngle
                                                       : shape.startAngle + shape.sweep;
                // A height at each end, as the model keeps them for an arc
                // on a grade. One Z holds them only when they are the same.
                auto heights = heightsOf(entity, 2);
                if (shape.sweep < 0.0) {
                    std::swap(heights[0], heights[1]);
                }
                const bool oneLevel = heights[0].has_value() && heights[0] == heights[1];
                const double z = oneLevel ? *heights[0] : 0.0;
                begin(full ? "CIRCLE" : "ARC", entity, "AcDbCircle");
                point(10, shape.center, z);
                real(40, shape.radius);
                if (!full) {
                    text(100, "AcDbArc");
                    double start = std::fmod(from * kDegreesPerRadian, 360.0);
                    if (start < 0.0) {
                        start += 360.0;
                    }
                    double end = std::fmod(start + std::abs(shape.sweep) * kDegreesPerRadian,
                                           360.0);
                    real(50, start);
                    real(51, end);
                }
                const bool any = heights[0].has_value() || heights[1].has_value();
                finish(any && (!oneLevel || z == 0.0) ? &heights : nullptr);
                const double r = shape.radius;
                extents_.expand(Point2(shape.center.x + shift_.x - r, shape.center.y + shift_.y - r));
                extents_.expand(Point2(shape.center.x + shift_.x + r, shape.center.y + shift_.y + r));
            } else if constexpr (std::is_same_v<T, Circle2>) {
                const auto heights = heightsOf(entity, 1);
                begin("CIRCLE", entity, "AcDbCircle");
                point(10, shape.center, heights.front().value_or(0.0));
                real(40, shape.radius);
                finish(isZeroLevel(heights) ? &heights : nullptr);
                const double r = shape.radius;
                extents_.expand(Point2(shape.center.x + shift_.x - r, shape.center.y + shift_.y - r));
                extents_.expand(Point2(shape.center.x + shift_.x + r, shape.center.y + shift_.y + r));
            } else if constexpr (std::is_same_v<T, Polyline2>) {
                writePolyline(entity, shape);
            } else if constexpr (std::is_same_v<T, katana::entity::TextGeometry>) {
                writeText(entity, shape);
            } else if constexpr (std::is_same_v<T, katana::entity::DimensionGeometry>) {
                writeDimension(entity, shape);
            } else if constexpr (std::is_same_v<T, katana::entity::LeaderGeometry>) {
                writeLeader(entity, shape);
            } else if constexpr (std::is_same_v<T, katana::geometry::CurvePolyline2>) {
                writeCurvePolyline(entity, shape);
            } else if constexpr (std::is_same_v<T, katana::geometry::Ellipse2>) {
                writeEllipse(entity, shape);
            } else if constexpr (std::is_same_v<T, katana::geometry::Spline2>) {
                writeSpline(entity, shape);
            } else if constexpr (std::is_same_v<T, katana::entity::LabelGeometry>) {
                // A label's words and place are worked out for a view by the
                // placer, which this module cannot see; counted and said, never
                // written as something it is not.
                ++labelsSkipped_;
                ++result_.entitiesSkipped;
            } else {
                static_assert(false, "the DXF writer has no case for this geometry kind");
            }
        },
        entity.geometry);
}

void Writer::writeEntities()
{
    text(0, "SECTION");
    text(2, "ENTITIES");
    const std::unordered_set<katana::entity::EntityId> chosen(options_.entities.begin(),
                                                              options_.entities.end());
    const std::set<std::string, std::less<>> only(options_.layers.begin(), options_.layers.end());
    model_.entities.forEach([&](const Entity& entity) {
        if (!chosen.empty() && !chosen.contains(entity.id)) {
            return;
        }
        if (!only.empty() && !only.contains(entity.layer)) {
            return;
        }
        // A label or a dimension the file cannot hold is counted skipped,
        // not written as well.
        const std::size_t skipped = result_.entitiesSkipped;
        writeEntity(entity);
        if (result_.entitiesSkipped == skipped) {
            ++result_.entitiesWritten;
        }
    });
    text(0, "ENDSEC");
}

Result<DxfExport> Writer::run()
{
    if (options_.originShift &&
        (!std::isfinite(options_.originShift->x) || !std::isfinite(options_.originShift->y))) {
        return makeError(ErrorCode::InvalidArgument, "the origin shift is not finite");
    }
    assignNames();
    // Entities first, into a buffer of their own, so that the extents the
    // header and the viewport state are known when those are written - ahead
    // of them in the file. The table records keep the handles before them.
    handle_ = kFirstFreeHandle + linetypes_.size() + layers_.size();
    body_.reserve(64 * 1024 + model_.entities.size() * 256);
    writeEntities();
    std::string entities = std::move(body_);
    const std::uint64_t afterEntities = handle_;
    handle_ = kFirstFreeHandle;
    body_ = std::string();
    writeTables();
    std::string tables = std::move(body_);
    handle_ = afterEntities;
    body_ = std::string();
    writeBlocks();
    std::string blocks = std::move(body_);
    body_ = std::string();
    writeObjects();
    std::string objects = std::move(body_);

    if (labelsSkipped_ != 0) {
        result_.warnings.push_back(
            std::to_string(labelsSkipped_) +
            " labels were not written: their text and place are worked out for a view, which "
            "the file has no form for");
    }
    if (drawnNothing_ != 0) {
        result_.warnings.push_back(
            std::to_string(drawnNothing_) + " labels had no room at 1:" +
            katana::core::formatExactReal(options_.annotationScale) +
            " and were not written, as the plan view leaves them out at that scale");
    }
    if (dimensionsSkipped_ != 0) {
        result_.warnings.push_back(
            std::to_string(dimensionsSkipped_) +
            " linear, angular, radial and ordinate dimensions were not written: only aligned "
            "dimensions are written, as the lines and text they draw");
    }
    if (partialHeights_ != 0) {
        result_.warnings.push_back(
            std::to_string(partialHeights_) +
            " entities with heights the format cannot hold - at only some vertices, or "
            "different at the two ends of an arc - were written in plan, since the format "
            "has no \"no height\" and a Z of 0 would be a false level; their heights go "
            "beside them as extended data, which Katana reads back");
    }
    std::string out = header();
    out.reserve(out.size() + tables.size() + blocks.size() + entities.size() + objects.size() + 8);
    out += tables;
    out += blocks;
    out += entities;
    out += objects;
    out += "  0\nEOF\n";
    result_.bytesWritten = out.size();
    result_.text = std::move(out);
    return std::move(result_);
}

} // namespace

std::string layerNameFor(std::string_view layerPath)
{
    return symbolName(layerPath, true);
}

Result<DxfExport> writeDxf(const katana::entity::Model& model, const ExportOptions& options)
{
    Writer writer(model, options);
    return writer.run();
}

Result<DxfExport> writeDxfFile(const katana::entity::Model& model,
                               const std::filesystem::path& path, const ExportOptions& options)
{
    auto written = writeDxf(model, options);
    if (!written) {
        return written;
    }
    // Beside the target first, then over it: a write that fails part way
    // leaves the previous file, not half of a new one.
    std::filesystem::path temporary = path;
    temporary += ".partial";
    {
        std::ofstream file(temporary, std::ios::binary | std::ios::trunc);
        file.write(written->text.data(), static_cast<std::streamsize>(written->text.size()));
        file.close();
        if (!file) {
            std::error_code ignored;
            std::filesystem::remove(temporary, ignored);
            return makeError(ErrorCode::FileExportFailure, "the file could not be written",
                             path.string());
        }
    }
    std::error_code error;
    std::filesystem::rename(temporary, path, error);
    if (error) {
        std::error_code ignored;
        std::filesystem::remove(temporary, ignored);
        return makeError(ErrorCode::FileExportFailure,
                         "the file could not be replaced: " + error.message(), path.string());
    }
    written->text.clear();
    written->text.shrink_to_fit();
    return written;
}

} // namespace katana::dxf
