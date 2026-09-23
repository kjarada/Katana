#include <algorithm>
#include <cmath>
#include <map>
#include <numbers>
#include <set>
#include <unordered_map>
#include <utility>

#include "katana/archive12d/domain.hpp"
#include "katana/terrain/super_surface.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "plan_geometry.hpp"
#include "text_utilities.hpp"

namespace katana::archive12d {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::entity::Entity;
using katana::entity::PropertyMap;
using katana::entity::PropertyValue;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Vec2;

constexpr double kPi = std::numbers::pi;

// A list item with a blank in it, as `"..."` with `"` and `\` escaped - the
// 12da rule (manual 1.1), so one splitter serves both.
[[nodiscard]] std::string quotedListItem(std::string_view text)
{
    std::string out = "\"";
    for (const char ch : text) {
        if (ch == '"' || ch == '\\') {
            out += '\\';
        }
        out += ch;
    }
    return out + '"';
}

// How far short of half its chord a radius may fall and still be taken as the
// semicircle it was rounded from, when an arc is solved for its tangents: a
// millimetre, the precision real exports carry.
constexpr double kArcSolveTolerance = 0.001;

// Skipped elements named one by one. Past this they are only counted: ten
// thousand identical lines tell the user nothing the count does not.
constexpr std::size_t kMaxNamedProblems = 20;

[[nodiscard]] double radians(double degrees)
{
    return degrees * kPi / 180.0;
}

// A field of a pit or a pipe as a typed property: a number when it was written
// as one, text otherwise - "diameter 0.375" must not arrive as the text
// "0.375", or nothing can be computed from it.
[[nodiscard]] PropertyValue propertyOf(const Field& field)
{
    if (!field.quoted) {
        if (const auto integer = detail::parseInteger(field.value)) {
            return *integer;
        }
        if (const auto real = parseReal(field.value)) {
            return *real;
        }
    }
    return field.value;
}

void addProperty(PropertyMap& properties, std::string key, PropertyValue value)
{
    if (key.empty()) {
        return;
    }
    // Attribute names need not be unique in 12d. A map needs them to be, and
    // the second "Remarks" is not worth less than the first. try_emplace
    // leaves both of its arguments untouched when the name is already taken,
    // so the qualified name - and the search for a free one - is built only
    // when there is a collision, which almost never happens.
    if (properties.try_emplace(std::move(key), std::move(value)).second) {
        return;
    }
    for (int copy = 2;; ++copy) {
        if (properties.try_emplace(key + " (" + std::to_string(copy) + ")", std::move(value))
                .second) {
            return;
        }
    }
}

void flattenAttributes(const AttributeList& attributes, const std::string& prefix,
                       PropertyMap& properties)
{
    // One buffer for every name under this prefix: the concatenation below is
    // done once per attribute of every element in the archive.
    std::string key;
    for (const Attribute& attribute : attributes) {
        key.assign(prefix);
        if (!prefix.empty()) {
            key += '/';
        }
        key += attribute.name;
        if (const auto* group = std::get_if<AttributeList>(&attribute.value)) {
            flattenAttributes(*group, key, properties);
        } else if (const auto* integer = std::get_if<std::int64_t>(&attribute.value)) {
            addProperty(properties, key, *integer);
        } else if (const auto* real = std::get_if<double>(&attribute.value)) {
            addProperty(properties, key, *real);
        } else if (const auto* text = std::get_if<std::string>(&attribute.value)) {
            addProperty(properties, key, *text);
        }
    }
}

// A per-vertex or per-segment list as one text value: the property model has
// no list type (see HEIGHTS in domain.hpp). Values with a blank in them are
// quoted so that the list can be split again; kMetaPointIds keeps its own
// older comma form.
[[nodiscard]] std::string joinList(const std::vector<std::string>& values)
{
    std::string out;
    for (const std::string& value : values) {
        if (!out.empty()) {
            out += ' ';
        }
        const bool plain = !value.empty() && value.find_first_of(" \t\"") == std::string::npos;
        out += plain ? value : quotedListItem(value);
    }
    return out;
}

[[nodiscard]] std::string joinFlags(const std::vector<bool>& flags)
{
    std::string out;
    for (const bool flag : flags) {
        out += out.empty() ? "" : " ";
        out += flag ? '1' : '0';
    }
    return out;
}

[[nodiscard]] std::string joinReals(const std::vector<double>& values)
{
    std::string out;
    for (const double value : values) {
        out += out.empty() ? "" : " ";
        out += detail::formatExactReal(value);
    }
    return out;
}

class Importer {
  public:
    Importer(const Archive& archive, const ImportOptions& options)
        : archive_(archive), options_(options)
    {
    }

    DomainImport run()
    {
        // Most elements become one entity, and none becomes more than a
        // handful, so the element count is the right order for the first
        // allocation - and it is memory the archive itself already holds.
        result_.entities.reserve(archive_.elements.size());
        for (const Element& element : archive_.elements) {
            const std::string keyword = elementKeyword(element);
            const std::size_t imported =
                std::visit([this](const auto& value) { return import(value); }, element);
            ElementTally& tally = tallyFor(keyword);
            ++tally.read;
            tally.imported += imported == 0 ? 0 : 1;
        }
        buildSuperTins();
        finishWarnings();
        return std::move(result_);
    }

  private:
    // ---- bookkeeping --------------------------------------------------------------

    ElementTally& tallyFor(const std::string& keyword)
    {
        for (ElementTally& tally : result_.tally) {
            if (tally.keyword == keyword) {
                return tally;
            }
        }
        result_.tally.push_back(ElementTally{keyword, 0, 0});
        return result_.tally.back();
    }

    void problem(std::string message)
    {
        if (namedProblems_ < kMaxNamedProblems) {
            result_.warnings.push_back(std::move(message));
        }
        ++namedProblems_;
    }

    [[nodiscard]] Point2 plan(double x, double y) const
    {
        const Vec2 shift = options_.originShift.value_or(Vec2{});
        return Point2(x - shift.x, y - shift.y);
    }
    [[nodiscard]] Point2 plan(const Vertex& vertex) const { return plan(vertex.x, vertex.y); }

    const std::string& layerFor(const std::string& model, const std::string& colour)
    {
        const auto found = layerOfModel_.find(model);
        if (found != layerOfModel_.end()) {
            return found->second;
        }
        katana::entity::Layer layer;
        layer.name = layerPathForModel(model, options_.layerPrefix);
        if (const auto rgb = standardColour(colour)) {
            layer.color = *rgb;
        }
        const bool known = std::any_of(
            result_.layersNeeded.begin(), result_.layersNeeded.end(),
            [&layer](const katana::entity::Layer& other) { return other.name == layer.name; });
        if (!known) {
            result_.layersNeeded.push_back(layer);
        }
        return layerOfModel_.emplace(model, layer.name).first->second;
    }

    // The Katana style for a 12d linestyle name: created once per name. A
    // name Katana cannot hold (empty) means ByLayer. A style named by a
    // symbol block gets the symbol its name suggests at the size the first
    // such block gave; met as a line first, it stays a line style until a
    // symbol block names it.
    katana::entity::Style& styleFor(const std::string& linestyle,
                                    const FieldList* symbolBlock = nullptr)
    {
        static katana::entity::Style byLayer;
        if (linestyle.empty()) {
            return byLayer;
        }
        // Found through an index, NOT by searching stylesNeeded: this is asked
        // once for every entity, and the search it replaces was O(entities x
        // styles). The vector's first-use order is the contract (domain.hpp);
        // the index only says where in it each name landed.
        auto at = styleIndex_.find(linestyle);
        if (at == styleIndex_.end()) {
            katana::entity::Style style;
            style.name = linestyle;
            // The 12d LINESTYLE NAME, kept as the name to draw the line with.
            // A loaded linestyle library is looked up by this, and a project
            // with no library falls back to a plain line - which is what
            // happened before, when this was left as "continuous" and the
            // name was recorded nowhere a renderer would look.
            style.linetype = linestyle;
            style.description = "12d linestyle";
            result_.stylesNeeded.push_back(std::move(style));
            at = styleIndex_.emplace(linestyle, result_.stylesNeeded.size() - 1).first;
        }
        katana::entity::Style& known = result_.stylesNeeded[at->second];
        if (symbolBlock != nullptr && known.symbol.empty()) {
            // The REAL 12d symbol name, not one of the sixteen shapes Katana
            // draws without a library. This used to store the guess and throw
            // the name away, which meant a loaded symbol library could never
            // be matched against it - the symbols were there and nothing
            // could find them. The guess now happens when it is drawn, only
            // if nothing defines the name (entity::builtInSymbolFor).
            known.symbol = linestyle;
            known.symbolSize = std::max(0.0, symbolBlock->real("size").value_or(0.0));
            known.description = "12d symbol";
        }
        return known;
    }

    [[nodiscard]] Entity makeEntity(const StringHeader& header, std::string_view keyword)
    {
        Entity entity;
        entity.layer = layerFor(header.model, header.colour);
        entity.style = styleFor(header.style).name;
        // A 12d string has a colour of its own; there is no ByLayer in 12d.
        // A name Katana has no RGB for is left to the layer, and kept below.
        entity.color = standardColour(header.colour);
        if (options_.attributesAsProperties) {
            flattenAttributes(header.attributes, {}, entity.properties);
        }
        const auto meta = [&entity](std::string_view key, PropertyValue value) {
            entity.metadata.insert_or_assign(std::string(key), std::move(value));
        };
        meta(kMetaElement, std::string(keyword));
        if (!header.name.empty()) {
            meta(kMetaName, header.name);
        }
        if (!header.colour.empty()) {
            meta(kMetaColour, header.colour);
        }
        if (header.chainage != 0.0) {
            meta(kMetaChainage, header.chainage);
        }
        meta(kMetaBreakline, std::string(header.breakline == Breakline::Point ? "point" : "line"));
        if (!options_.sourceName.empty()) {
            meta(kMetaSource, options_.sourceName);
        }
        // Every scalar the string carried that Katana has no field for -
        // weight, time_created, a face's hatching, a plot frame's margins -
        // travels as `12d.x.<key>`, and export writes it back. Nothing scalar
        // is lost between a file and the file exported from it.
        for (const Field& field : header.extras.fields()) {
            if (field.key != "null_value") {
                meta(std::string(kMetaExtraPrefix) + field.key, field.value);
            }
        }
        return entity;
    }

    // Adds `entity` if its geometry is one the model will accept. It must be
    // checked HERE: the caller creates every entity in one atomic command, so
    // a single zero-length string would otherwise fail the whole import.
    bool add(Entity entity, const std::string& what)
    {
        if (const auto status = katana::entity::validate(entity.geometry); !status) {
            problem(what + " skipped: " + status.error().message);
            return false;
        }
        result_.bounds.expand(katana::entity::boundingBox(entity.geometry));
        result_.entities.push_back(std::move(entity));
        return true;
    }

    [[nodiscard]] static std::string describe(const StringHeader& header, std::string_view kind)
    {
        return std::string(kind) + " \"" + header.name + "\" in model \"" + header.model + "\"";
    }

    // ---- strings ----------------------------------------------------------------------

    // The height of a chorded point: the vertex's own, or for a point chording
    // added, interpolated between the vertices either side of it.
    [[nodiscard]] static std::optional<double>
    heightAt(const detail::PlanPoint& point, const std::vector<Vertex>& vertices,
             const std::optional<double>& constantZ)
    {
        const auto heightOf = [&](std::size_t index) -> std::optional<double> {
            const std::optional<double>& z = vertices[index % vertices.size()].z;
            return z ? z : constantZ;
        };
        const auto a = heightOf(point.segment);
        if (point.fraction == 0.0) {
            return a;
        }
        const auto b = heightOf(point.segment + 1);
        if (!a || !b) {
            return std::nullopt;
        }
        return *a + (*b - *a) * point.fraction;
    }

    // Vertices and segments -> a Point, a Line or a Polyline on `entity`.
    bool setStringGeometry(Entity& entity, const std::vector<Vertex>& vertices,
                           const std::vector<Segment>& segments, bool closed,
                           const std::optional<double>& constantZ)
    {
        if (vertices.empty()) {
            return false;
        }
        detail::ChordReport report;
        const auto points =
            detail::chordPlan(vertices, segments, closed, options_.curveTolerance, report);
        chordReport_.merge(report);

        std::vector<std::optional<double>> heights;
        Polyline2 polyline;
        // Exactly one of each per chorded point, so the size is known.
        heights.reserve(points.size());
        polyline.vertices.reserve(points.size());
        polyline.closed = closed && points.size() > 2;
        for (const detail::PlanPoint& point : points) {
            polyline.vertices.push_back(plan(point.point.x, point.point.y));
            heights.push_back(heightAt(point, vertices, constantZ));
        }

        if (polyline.vertices.size() == 1 || !(polyline.length() > 0.0)) {
            // One vertex, or several in the same place: a point.
            entity.geometry = katana::entity::PointGeometry{polyline.vertices.front()};
            heights.resize(1);
        } else if (polyline.vertices.size() == 2 && !polyline.closed) {
            // A Line rather than a two-vertex polyline, as importVector makes
            // it, so that it can still be filleted and extended.
            entity.geometry =
                katana::geometry::Segment2{polyline.vertices[0], polyline.vertices[1]};
        } else {
            entity.geometry = std::move(polyline);
        }
        katana::entity::setHeights(entity.properties, heights);
        return true;
    }

    // Where 12d's anchor point puts the LEFT END OF THE BASELINE, which is
    // what a Katana TextGeometry's position is.
    //
    // justify is "top|middle|bottom" and "-left|-centre|-right" (manual
    // 1.5.8.4.9); every annotation in the sample archives is "bottom-left",
    // which is already Katana's meaning, but the format allows the other
    // eight and a text anchored by its centre sits half its width away.
    // The width is ESTIMATED at 0.6 of the height per character - there is
    // no font here, and the renderer that will draw it has its own metrics;
    // the estimate only has to be better than ignoring justification, which
    // is out by the whole width.
    [[nodiscard]] static Point2 baselineLeft(const Point2& anchor, const std::string& text,
                                             double height, double rotation,
                                             const std::string& justify)
    {
        constexpr double kWidthPerHeight = 0.6;
        const double width =
            static_cast<double>(text.size()) * height * kWidthPerHeight;
        const std::string wanted = detail::lowered(justify);
        double along = 0.0;
        if (wanted.find("centre") != std::string::npos ||
            wanted.find("center") != std::string::npos) {
            along = -0.5 * width;
        } else if (wanted.find("right") != std::string::npos) {
            along = -width;
        }
        // "top" and "middle" are measured from the cap line and the middle of
        // it; the baseline is below both.
        double across = 0.0;
        if (wanted.starts_with("top")) {
            across = -height;
        } else if (wanted.starts_with("middle")) {
            across = -0.5 * height;
        }
        const double c = std::cos(rotation);
        const double sn = std::sin(rotation);
        return Point2(anchor.x + along * c - across * sn, anchor.y + along * sn + across * c);
    }

    // Everything a 12d annotation says about a piece of text, applied to the
    // entity Katana will draw. See kMetaTextPrefix for what is kept rather
    // than applied, and why.
    void applyAnnotation(Entity& entity, katana::entity::TextGeometry& geometry,
                         const FieldList* annotation)
    {
        if (annotation == nullptr) {
            return;
        }
        geometry.rotation = radians(annotation->real("angle").value_or(0.0));
        // The text's own colour, which is not the string's: "no_colour"
        // means take the string's, which the entity already has.
        const std::string colour = annotation->text("colour", annotation->text("text_colour"));
        if (!colour.empty() && colour != "no_colour") {
            if (const auto rgb = standardColour(colour)) {
                entity.color = rgb;
            }
            entity.metadata.insert_or_assign(std::string(kMetaTextPrefix) + "colour", colour);
        }
        if (const std::string style = annotation->text("textstyle"); !style.empty()) {
            entity.style = styleFor(style).name;
        }
        // An offset is in the same units as the SIZE, and only worldsize is
        // model units; a papersize offset is millimetres on a plot, which
        // has no model-unit meaning without a plot scale - the same reason
        // textHeight() falls back. The direction the manual does not give:
        // it is taken as perpendicular to the text, to the left, which is
        // where 12d puts an offset annotation above a line.
        const bool worldUnits = annotation->real("worldsize").value_or(0.0) > 0.0;
        for (const char* key : {"offset", "raise", "slant", "x_factor", "xfactor", "justify",
                                "papersize", "screensize"}) {
            if (const Field* field = annotation->find(key); field != nullptr) {
                entity.metadata.insert_or_assign(std::string(kMetaTextPrefix) + key, field->value);
            }
        }
        const double offset = annotation->real("offset").value_or(0.0);
        if (worldUnits && offset != 0.0) {
            const double c = std::cos(geometry.rotation);
            const double sn = std::sin(geometry.rotation);
            geometry.position = Point2(geometry.position.x - offset * sn,
                                       geometry.position.y + offset * c);
        }
        // `raise` raises the LEVEL: it is a height, not a plan displacement,
        // so it goes onto the elevation the text already carries.
        const double raised = annotation->real("raise").value_or(0.0);
        if (raised != 0.0) {
            const auto found = entity.properties.find(std::string(kElevationProperty));
            if (found != entity.properties.end()) {
                if (const auto* z = std::get_if<double>(&found->second)) {
                    found->second = *z + raised;
                }
            }
        }
        geometry.position = baselineLeft(geometry.position, geometry.text, geometry.height,
                                         geometry.rotation, annotation->text("justify"));
    }

    [[nodiscard]] static double textHeight(const FieldList* annotation)
    {
        // Only `worldsize` is a height in model units. papersize (mm on the
        // plot) and screensize (pixels) have no model-unit equivalent without
        // a plot scale, so they fall back to the model's own default height.
        const double fallback = katana::entity::TextGeometry{}.height;
        if (annotation == nullptr) {
            return fallback;
        }
        const auto world = annotation->real("worldsize");
        return world && *world > 0.0 ? *world : fallback;
    }

    std::size_t addVertexText(const VertexString& string)
    {
        std::size_t added = 0;
        for (std::size_t i = 0; i < string.vertices.size(); ++i) {
            const std::string* text = i < string.vertexText.size() ? &string.vertexText[i]
                                      : string.vertexTextValue     ? &*string.vertexTextValue
                                                                   : nullptr;
            if (text == nullptr || text->empty()) {
                continue;
            }
            const FieldList* annotation = i < string.vertexAnnotations.size()
                                              ? &string.vertexAnnotations[i]
                                          : string.vertexAnnotation ? &*string.vertexAnnotation
                                                                    : nullptr;
            Entity entity = makeEntity(string.header, "vertex text");
            entity.properties.clear(); // the string's attributes belong to the string
            katana::entity::TextGeometry geometry;
            geometry.position = plan(string.vertices[i]);
            geometry.text = *text;
            geometry.height = textHeight(annotation);
            const auto& z = string.vertices[i].z ? string.vertices[i].z : string.constantZ;
            if (z) {
                entity.properties.insert_or_assign(std::string(kElevationProperty), *z);
            }
            applyAnnotation(entity, geometry, annotation);
            entity.geometry = std::move(geometry);
            added += add(std::move(entity), "vertex text of " + describe(string.header, "string"))
                         ? 1
                         : 0;
        }
        return added;
    }

    std::size_t import(const VertexString& string)
    {
        const std::string keyword = "string " + std::string(toString(string.kind));
        Entity entity = makeEntity(string.header, keyword);
        // A face string is a filled region (manual 1.5.4: fill_mode, hatch);
        // it has no `closed` field because it always is.
        const bool closed = string.closed || string.kind == StringKind::Face;
        if (!setStringGeometry(entity, string.vertices, string.segments, closed,
                               string.constantZ)) {
            problem(describe(string.header, keyword) + " skipped: it has no vertices");
            return 0;
        }
        const auto meta = [&entity](std::string key, PropertyValue value) {
            entity.metadata.insert_or_assign(std::move(key), std::move(value));
        };
        if (!string.pointIds.empty()) {
            std::string ids;
            for (const std::string& id : string.pointIds) {
                ids += (ids.empty() ? "" : ",") + id;
            }
            meta(std::string(kMetaPointIds), std::move(ids));
        }
        // Pipe and culvert sizes: one for the string, or one per segment. A
        // per-segment list whose entries are all the same IS one for the
        // string, which is how 12d Model 15 writes a uniform pipe.
        const auto uniform = [](const std::vector<double>& values) {
            return !values.empty() && std::all_of(values.begin(), values.end(),
                                                  [&](double v) { return v == values.front(); });
        };
        if (string.diameter) {
            meta("12d.diameter", *string.diameter);
        } else if (uniform(string.diameters)) {
            meta("12d.diameter", string.diameters.front());
        } else if (!string.diameters.empty()) {
            meta("12d.diameters", joinReals(string.diameters));
        }
        if (string.culvert) {
            meta("12d.culvert_width", (*string.culvert)[0]);
            meta("12d.culvert_height", (*string.culvert)[1]);
        } else if (!string.culverts.empty()) {
            std::vector<double> sizes;
            for (const auto& culvert : string.culverts) {
                sizes.push_back(culvert[0]);
                sizes.push_back(culvert[1]);
            }
            meta("12d.culverts", joinReals(sizes));
        }
        if (!string.justify.empty()) {
            meta("12d.justify", string.justify);
        }
        // Flags and colours per vertex or segment: the entity model has no
        // place for them, so they ride in the metadata and are written back.
        // A string with an invisible or untinable part is also COUNTED, since
        // the drawing shows the whole string.
        const auto flags = [&](std::string_view key, const std::optional<bool>& value,
                               const std::vector<bool>& list) {
            if (value) {
                meta(std::string(key), std::string(*value ? "1" : "0"));
            } else if (!list.empty()) {
                meta(std::string(key), joinFlags(list));
            }
            const bool anyOff = (value && !*value) ||
                                std::any_of(list.begin(), list.end(), [](bool f) { return !f; });
            return anyOff;
        };
        if (flags("12d.vertex_visible", string.vertexVisibleValue, string.vertexVisible) ||
            flags("12d.segment_visible", string.segmentVisibleValue, string.segmentVisible)) {
            ++partlyInvisible_;
        }
        flags("12d.vertex_tinable", string.vertexTinableValue, string.vertexTinable);
        flags("12d.segment_tinable", string.segmentTinableValue, string.segmentTinable);
        if (!string.segmentColours.empty()) {
            meta("12d.segment_colours", joinList(string.segmentColours));
        }
        if (!string.interfaceModes.empty()) {
            std::vector<std::string> modes;
            for (const int mode : string.interfaceModes) {
                modes.push_back(std::to_string(mode));
            }
            meta("12d.interface_modes", joinList(modes));
        }
        // Attributes on vertices and segments become properties keyed by
        // position - `vertex/3/QualityLevel` - the way a group flattens; a
        // one-vertex string's vertex attributes are simply its own.
        if (options_.attributesAsProperties) {
            if (string.vertexAttributes.size() == 1 && string.vertices.size() == 1) {
                flattenAttributes(string.vertexAttributes[0], {}, entity.properties);
            } else {
                for (std::size_t i = 0; i < string.vertexAttributes.size(); ++i) {
                    flattenAttributes(string.vertexAttributes[i], "vertex/" + std::to_string(i + 1),
                                      entity.properties);
                }
            }
            for (std::size_t i = 0; i < string.segmentAttributes.size(); ++i) {
                flattenAttributes(string.segmentAttributes[i], "segment/" + std::to_string(i + 1),
                                  entity.properties);
            }
        }
        importSymbols(string, entity, meta);
        // Per-vertex annotation properties have no Katana form at all; they
        // are counted, so that the import can say what it left.
        if (!string.vertexAnnotations.empty() || !string.segmentAnnotations.empty() ||
            (string.segmentAnnotation && string.segmentText.empty() && !string.segmentTextValue)) {
            ++annotationsLeft_;
        }
        std::size_t added = addOrSplit(std::move(entity), string.header,
                                       describe(string.header, keyword));
        added += addVertexText(string);
        added += addSegmentText(string);
        return added;
    }

    // `breakline point` means the string's vertices are POINTS - separate
    // survey shots that 12d keeps in one string for convenience - and not a
    // line through them. Katana has no multi-point geometry, so each vertex
    // becomes its own point entity carrying the string's layer, style,
    // colour, metadata and properties.
    //
    // Without this they were drawn as a polyline: the vertices were joined
    // up, and a model of standing infrastructure came out as a scribble
    // through it. The flag was read and kept as `12d.breakline` but never
    // acted on. Measured on a 15 MB production archive: 13,326 point strings
    // have a single vertex and were always right; nine have several, and
    // those nine held 649 points.
    [[nodiscard]] std::size_t addOrSplit(Entity entity, const StringHeader& header,
                                         const std::string& what)
    {
        const auto* polyline = std::get_if<Polyline2>(&entity.geometry);
        const auto* segment = std::get_if<katana::geometry::Segment2>(&entity.geometry);
        if (header.breakline != Breakline::Point || (polyline == nullptr && segment == nullptr)) {
            return add(std::move(entity), what) ? std::size_t{1} : 0;
        }
        std::vector<Point2> places;
        if (segment != nullptr) {
            places = {segment->start, segment->end};
        } else {
            places = polyline->vertices;
        }
        // The heights the string carried, one per vertex, so each point keeps
        // its own rather than the list of all of them.
        std::vector<std::string> heights;
        const auto found = entity.properties.find("elevations");
        if (found != entity.properties.end()) {
            if (const auto* list = std::get_if<std::string>(&found->second)) {
                std::size_t at = 0;
                while (at <= list->size()) {
                    const std::size_t space = list->find(' ', at);
                    heights.push_back(list->substr(
                        at, space == std::string::npos ? std::string::npos : space - at));
                    if (space == std::string::npos) {
                        break;
                    }
                    at = space + 1;
                }
            }
        }

        std::size_t added = 0;
        for (std::size_t i = 0; i < places.size(); ++i) {
            Entity point = entity;
            point.geometry = katana::entity::PointGeometry{places[i]};
            point.properties.erase("elevations");
            if (i < heights.size() && heights[i] != "null" && !heights[i].empty()) {
                if (const auto value = parseReal(heights[i])) {
                    point.properties.insert_or_assign(std::string(kElevationProperty),
                                                      PropertyValue(*value));
                }
            }
            // Which vertex of the string this was, so the points can be told
            // apart and put back in order.
            point.metadata.insert_or_assign(std::string(kMetaVertex),
                                            static_cast<std::int64_t>(i + 1));
            added += add(std::move(point), what) ? 1 : 0;
        }
        if (added != 0) {
            pointStringsSplit_ += 1;
            pointsFromStrings_ += added;
        }
        return added;
    }

    // See kMetaSymbolPrefix for the two forms a symbol takes. A string with
    // ONE block takes that symbol as its style however many vertices it has:
    // a 12d vertex symbol goes on EVERY vertex, which is what `mode vertex`
    // means in a library and where cad::styleDrawing puts it.
    //
    // This was once limited to a string of one vertex, and a survey of
    // standing infrastructure paid for it: a 61-vertex string of drill holes,
    // every one of which should carry a "MARK Drill Hole and Wing", was drawn
    // as a polyline through them with no symbols at all.
    //
    // A per-vertex list of MORE than one block - which the format allows and
    // 12d does not write - is still kept whole and counted, because there is
    // no single symbol for the string to be drawn with.
    template <typename Meta>
    void importSymbols(const VertexString& string, Entity& entity, const Meta& meta)
    {
        std::vector<const FieldList*> blocks;
        if (string.symbol) {
            blocks.push_back(&*string.symbol);
        }
        for (const FieldList& block : string.symbols) {
            blocks.push_back(&block);
        }
        if (blocks.empty()) {
            return;
        }
        const FieldList& first = *blocks.front();
        const std::string symbolStyle = first.text("style");
        if (blocks.size() == 1 && !symbolStyle.empty()) {
            const katana::entity::Style& style = styleFor(symbolStyle, &first);
            entity.style = style.name;
            if (string.header.style != symbolStyle) {
                meta(std::string(kMetaStringStyle), string.header.style);
            }
            // What the string LOOKS like is its symbol, so the symbol's
            // colour is the entity's. The string's own colour name stays in
            // 12d.colour: the two are the same name in every sample archive,
            // but the format allows two and neither may be lost.
            const std::string symbolColour = first.text("colour");
            if (const auto rgb = standardColour(symbolColour)) {
                entity.color = rgb;
            }
            for (const Field& field : first.fields()) {
                // The style carries the shape and the size, the entity the
                // colour where it agrees with the string's; the rest of the
                // block is kept only where it says something - 12d writes
                // rotation, offset and raise as 0 on tens of thousands of
                // points, and metadata is not free.
                const bool onStyle = field.key == "style" ||
                                     (field.key == "size" &&
                                      first.real("size").value_or(-1.0) == style.symbolSize);
                const bool onEntity = field.key == "colour" && symbolColour == string.header.colour;
                const bool isDefault = (field.key == "rotation" || field.key == "offset" ||
                                        field.key == "raise") &&
                                       first.real(field.key) == 0.0;
                if (!onStyle && !onEntity && !isDefault) {
                    meta(std::string(kMetaSymbolPrefix) + field.key, field.value);
                }
            }
            return;
        }
        // One list per key, a value per block, in the order the keys first
        // appear; a key a block lacks holds an empty item so the columns
        // stay aligned.
        std::vector<std::string> keys;
        for (const FieldList* block : blocks) {
            for (const Field& field : block->fields()) {
                if (std::find(keys.begin(), keys.end(), field.key) == keys.end()) {
                    keys.push_back(field.key);
                }
            }
        }
        for (const std::string& key : keys) {
            std::vector<std::string> values;
            values.reserve(blocks.size());
            for (const FieldList* block : blocks) {
                values.push_back(block->text(key));
            }
            meta(std::string(kMetaSymbolPrefix) + key, joinList(values));
        }
        ++symbolsLeft_;
    }

    // Text on a segment is drawn at its middle (manual 1.5.8.4.9 gives it an
    // offset and a raise, which need a paper scale Katana does not have here).
    std::size_t addSegmentText(const VertexString& string)
    {
        const std::size_t segments = string.segmentCount();
        std::size_t added = 0;
        for (std::size_t i = 0; i < segments; ++i) {
            const std::string* text = i < string.segmentText.size() ? &string.segmentText[i]
                                      : string.segmentTextValue     ? &*string.segmentTextValue
                                                                    : nullptr;
            if (text == nullptr || text->empty()) {
                continue;
            }
            const FieldList* annotation = i < string.segmentAnnotations.size()
                                              ? &string.segmentAnnotations[i]
                                          : string.segmentAnnotation ? &*string.segmentAnnotation
                                                                     : nullptr;
            const Vertex& a = string.vertices[i];
            const Vertex& b = string.vertices[(i + 1) % string.vertices.size()];
            Entity entity = makeEntity(string.header, "segment text");
            entity.properties.clear();
            katana::entity::TextGeometry geometry;
            geometry.position = plan(0.5 * (a.x + b.x), 0.5 * (a.y + b.y));
            geometry.text = *text;
            geometry.height = textHeight(annotation);
            applyAnnotation(entity, geometry, annotation);
            entity.geometry = std::move(geometry);
            added += add(std::move(entity), "segment text of " + describe(string.header, "string"))
                         ? 1
                         : 0;
        }
        return added;
    }

    std::size_t import(const ArcString& arc)
    {
        Entity entity = makeEntity(arc.header, "string arc");
        const Point2 centre = plan(arc.centre);
        const Point2 start = plan(arc.start);
        const Point2 end = plan(arc.end);
        const double radius =
            arc.radius != 0.0 ? std::fabs(arc.radius) : centre.distanceTo(start);
        const double startAngle = std::atan2(start.y - centre.y, start.x - centre.x);
        const double endAngle = std::atan2(end.y - centre.y, end.x - centre.x);
        // The manual gives `string arc` a radius and says nothing of its sign.
        // It is read as every other radius in the format is (plan_geometry.hpp):
        // positive runs clockwise from start to end. A negative radius, the
        // only way the format has to say otherwise, runs counter-clockwise.
        const bool clockwise = arc.radius >= 0.0;
        const double sweep = clockwise ? -std::fmod(startAngle - endAngle + 4.0 * kPi, 2.0 * kPi)
                                       : std::fmod(endAngle - startAngle + 4.0 * kPi, 2.0 * kPi);
        if (sweep == 0.0) {
            entity.geometry = katana::geometry::Circle2{centre, radius};
        } else {
            entity.geometry = katana::geometry::Arc2{centre, radius, startAngle, sweep};
        }
        katana::entity::setHeights(entity.properties, {arc.start.z, arc.end.z});
        return add(std::move(entity), describe(arc.header, "string arc")) ? 1 : 0;
    }

    std::size_t import(const CircleString& circle)
    {
        const char* keyword = circle.feature ? "string feature" : "string circle";
        Entity entity = makeEntity(circle.header, keyword);
        entity.geometry = katana::geometry::Circle2{plan(circle.centre), std::fabs(circle.radius)};
        katana::entity::setHeights(entity.properties, {circle.centre.z});
        return add(std::move(entity), describe(circle.header, keyword)) ? 1 : 0;
    }

    std::size_t import(const TextString& text)
    {
        Entity entity = makeEntity(text.header, "string text");
        katana::entity::TextGeometry geometry;
        geometry.position = plan(text.position);
        geometry.text = text.text;
        geometry.height = textHeight(&text.annotation);
        katana::entity::setHeights(entity.properties, {text.position.z});
        applyAnnotation(entity, geometry, &text.annotation);
        entity.geometry = std::move(geometry);
        return add(std::move(entity), describe(text.header, "string text")) ? 1 : 0;
    }

    std::size_t import(const PlotFrame& frame)
    {
        // The sheet on the ground: paper millimetres times the plot scale.
        const auto width = frame.fields.real("width");
        const auto height = frame.fields.real("height");
        const double scale = frame.fields.real("scale").value_or(1000.0);
        if (!width || !height || !(*width > 0.0) || !(*height > 0.0) || !(scale > 0.0)) {
            problem(describe(frame.header, "string plot_frame") +
                    " skipped: it has no width, height or scale");
            return 0;
        }
        const Point2 origin = plan(frame.fields.real("xorigin").value_or(0.0),
                                   frame.fields.real("yorigin").value_or(0.0));
        const double w = *width * scale / 1000.0;
        const double h = *height * scale / 1000.0;
        const double rotation = radians(frame.fields.real("rotation").value_or(0.0));
        const Vec2 along(std::cos(rotation), std::sin(rotation));
        const Vec2 up(-along.y, along.x);
        Polyline2 sheet;
        sheet.closed = true;
        sheet.vertices = {origin, origin + along * w, origin + along * w + up * h, origin + up * h};

        Entity entity = makeEntity(frame.header, "string plot_frame");
        entity.geometry = std::move(sheet);
        for (const Field& field : frame.fields.fields()) {
            addProperty(entity.properties, "plot_frame." + field.key, propertyOf(field));
        }
        return add(std::move(entity), describe(frame.header, "string plot_frame")) ? 1 : 0;
    }

    void addRecordPoint(const StringHeader& header, const DrainageRecord& record,
                        const std::string& kind, std::size_t& added)
    {
        const auto x = record.fields.real("x");
        const auto y = record.fields.real("y");
        if (!x || !y) {
            problem(kind + " \"" + record.fields.text("name") + "\" of " +
                    describe(header, "string drainage") + " skipped: it has no x and y");
            return;
        }
        Entity entity = makeEntity(header, "drainage " + kind);
        entity.properties.clear();
        entity.geometry = katana::entity::PointGeometry{plan(*x, *y)};
        for (const Field& field : record.fields.fields()) {
            if (field.key != "x" && field.key != "y" && field.key != "z") {
                addProperty(entity.properties, kind + "." + field.key, propertyOf(field));
            }
        }
        if (options_.attributesAsProperties) {
            flattenAttributes(record.attributes, {}, entity.properties);
        }
        // Manual 1.5.3: `z` is "the z-value of top of node" for a pit, and
        // "internal use only" for a house connection or property control -
        // 12d Model writes 0 there, 30 m below the job. A house connection's
        // height is its adopted level, or its level.
        std::optional<double> elevation;
        if (kind == "pit") {
            elevation = record.fields.real("z");
        } else if (kind == "house_connection") {
            elevation = record.fields.real("adopted_level");
            if (!elevation) {
                elevation = record.fields.real("level");
            }
        }
        if (elevation && *elevation != archive_.nullValue) {
            entity.properties.insert_or_assign(std::string(kElevationProperty), *elevation);
        }
        added += add(std::move(entity), kind + " of " + describe(header, "string drainage")) ? 1 : 0;
    }

    std::size_t import(const DrainageString& drainage)
    {
        std::size_t added = 0;
        Entity line = makeEntity(drainage.header, "string drainage");
        if (setStringGeometry(line, drainage.vertices, drainage.segments, false, std::nullopt)) {
            if (drainage.outfall) {
                addProperty(line.properties, "outfall", *drainage.outfall);
            }
            if (drainage.flowDirection) {
                addProperty(line.properties, "flow_direction",
                            static_cast<std::int64_t>(*drainage.flowDirection));
            }
            // Pipe i joins pit i to pit i + 1. It has no position of its own,
            // so it travels with the line.
            for (std::size_t i = 0; i < drainage.pipes.size(); ++i) {
                const std::string prefix = "pipe." + std::to_string(i + 1) + ".";
                for (const Field& field : drainage.pipes[i].fields.fields()) {
                    addProperty(line.properties, prefix + field.key, propertyOf(field));
                }
                if (options_.attributesAsProperties) {
                    flattenAttributes(drainage.pipes[i].attributes, prefix + "attributes",
                                      line.properties);
                }
            }
            added += add(std::move(line), describe(drainage.header, "string drainage")) ? 1 : 0;
        }
        for (const DrainageRecord& pit : drainage.pits) {
            addRecordPoint(drainage.header, pit, "pit", added);
        }
        for (const DrainageRecord& connection : drainage.houseConnections) {
            addRecordPoint(drainage.header, connection, "house_connection", added);
        }
        for (const DrainageRecord& control : drainage.propertyControls) {
            if (control.vertices.empty()) {
                addRecordPoint(drainage.header, control, "property_control", added);
                continue;
            }
            Entity entity = makeEntity(drainage.header, "drainage property_control");
            entity.properties.clear();
            if (setStringGeometry(entity, control.vertices, control.segments, false,
                                  std::nullopt)) {
                for (const Field& field : control.fields.fields()) {
                    addProperty(entity.properties, "property_control." + field.key,
                                propertyOf(field));
                }
                if (options_.attributesAsProperties) {
                    flattenAttributes(control.attributes, {}, entity.properties);
                }
                added += add(std::move(entity), "property control of " +
                                                    describe(drainage.header, "string drainage"))
                             ? 1
                             : 0;
            }
        }
        return added;
    }

    // ---- alignments ---------------------------------------------------------------------

    struct Reconstruction {
        katana::geometry::HorizontalAlignment horizontal;
        std::string failure; // non-empty: why the PIs could not be had
    };

    [[nodiscard]] Reconstruction pisFromParts(const SuperAlignment& alignment) const
    {
        Reconstruction out;
        for (const AlignmentPart& part : alignment.horizontalParts) {
            const auto x = part.fields.real("x");
            const auto y = part.fields.real("y");
            if (!x || !y) {
                out.failure = "an IP has no coordinates";
                return out;
            }
            katana::geometry::AlignmentPI pi;
            pi.point = plan(*x, *y);
            pi.radius = std::fabs(part.fields.real("r").value_or(0.0));
            if (part.kind == "spiral") {
                pi.spiralIn = std::fabs(part.fields.real("l1").value_or(0.0));
                pi.spiralOut = std::fabs(part.fields.real("l2").value_or(0.0));
            }
            out.horizontal.pis.push_back(pi);
        }
        return out;
    }

    // The direction of travel entering (`atStart`) or leaving a curved
    // segment, from the segment's own parameters - so that two curves with no
    // straight between them can still be given their tangents.
    [[nodiscard]] static std::optional<double> tangentOf(const Segment& segment, const Point2& a,
                                                         const Point2& b, bool atStart)
    {
        if (segment.kind == SegmentKind::Spiral) {
            const bool leading = segment.parameters.boolean("leading").value_or(true);
            // (a1) belongs to the segment's first vertex when leading, to its
            // second - and pointing backwards - when trailing. See
            // plan_geometry.hpp.
            const auto first = segment.parameters.real(leading ? "a1" : "a2");
            const auto second = segment.parameters.real(leading ? "a2" : "a1");
            const auto& angle = atStart ? first : second;
            if (!angle) {
                return std::nullopt;
            }
            return radians(*angle) + (leading ? 0.0 : kPi);
        }
        if (segment.kind == SegmentKind::Arc) {
            detail::ArcCircle arc;
            if (!detail::solveArc(a, b, segment, kArcSolveTolerance, arc)) {
                return std::nullopt;
            }
            return detail::arcTangentAt(arc, atStart ? a : b);
        }
        return std::nullopt;
    }

    [[nodiscard]] Reconstruction pisFromElements(const SolvedGeometry& data) const
    {
        Reconstruction out;
        const std::size_t vertexCount = data.vertices.size();
        if (vertexCount < 2) {
            out.failure = "its solved geometry has fewer than two vertices";
            return out;
        }
        const auto vertex = [&](std::size_t index) { return plan(data.vertices[index]); };
        const auto kindOf = [&](std::size_t index) {
            return index < data.segments.size() ? data.segments[index].kind
                                                : SegmentKind::Straight;
        };
        const std::size_t segmentCount = vertexCount - 1;
        auto& pis = out.horizontal.pis;
        pis.push_back({vertex(0), 0.0, 0.0, 0.0});

        for (std::size_t i = 0; i < segmentCount;) {
            if (kindOf(i) == SegmentKind::Straight) {
                if (i + 1 < segmentCount && kindOf(i + 1) == SegmentKind::Straight) {
                    pis.push_back({vertex(i + 1), 0.0, 0.0, 0.0}); // two straights: a kink
                }
                ++i;
                continue;
            }
            // One curve: [transition] arc [transition].
            std::size_t k = i;
            double spiralIn = 0.0;
            double spiralOut = 0.0;
            const auto lengthOf = [&](std::size_t index) {
                const FieldList& p = data.segments[index].parameters;
                return std::fabs(p.real("l2").value_or(0.0) - p.real("l1").value_or(0.0));
            };
            const auto isClothoid = [&](std::size_t index) {
                const std::string type =
                    detail::lowered(data.segments[index].parameters.text("type", "clothoid"));
                return type == "clothoid" || type == "natural clothoid";
            };
            if (kindOf(k) == SegmentKind::Curve) {
                out.failure = "it contains an offset transition";
                return out;
            }
            if (kindOf(k) == SegmentKind::Spiral) {
                if (!isClothoid(k)) {
                    out.failure = "its transitions are of type \"" +
                                  data.segments[k].parameters.text("type") +
                                  "\" and Katana's are clothoids";
                    return out;
                }
                spiralIn = lengthOf(k);
                ++k;
            }
            if (k >= segmentCount || kindOf(k) != SegmentKind::Arc) {
                out.failure = "a transition is not followed by an arc";
                return out;
            }
            const double radius = std::fabs(data.segments[k].radius);
            ++k;
            if (k < segmentCount && kindOf(k) == SegmentKind::Spiral) {
                if (!isClothoid(k)) {
                    out.failure = "its transitions are of type \"" +
                                  data.segments[k].parameters.text("type") +
                                  "\" and Katana's are clothoids";
                    return out;
                }
                spiralOut = lengthOf(k);
                ++k;
            }
            if (i == 0) {
                out.failure = "it begins on a curve";
                return out;
            }
            if (k == segmentCount) {
                out.failure = "it ends on a curve";
                return out;
            }
            const auto in = tangentOf(data.segments[i], vertex(i), vertex(i + 1), true);
            const auto away = tangentOf(data.segments[k - 1], vertex(k - 1), vertex(k), false);
            if (!in || !away) {
                out.failure = "a curve does not record its tangents";
                return out;
            }
            // PI: where the tangent entering the curve meets the one leaving it.
            const Vec2 d1(std::cos(*in), std::sin(*in));
            const Vec2 d2(std::cos(*away), std::sin(*away));
            const double cross = d1.x * d2.y - d1.y * d2.x;
            if (std::fabs(cross) < 1e-9) {
                out.failure = "a curve turns through 180 degrees or not at all";
                return out;
            }
            const Vec2 gap = vertex(k) - vertex(i);
            const double t = (gap.x * d2.y - gap.y * d2.x) / cross;
            pis.push_back({vertex(i) + d1 * t, radius, spiralIn, spiralOut});
            i = k;
        }
        pis.push_back({vertex(vertexCount - 1), 0.0, 0.0, 0.0});
        return out;
    }

    // PVIs from the solved vertical geometry. Returns the reason on failure.
    [[nodiscard]] std::string pvisFromElements(const SolvedGeometry& data,
                                               katana::geometry::VerticalAlignment& vertical,
                                               std::vector<std::string>& notes) const
    {
        const std::size_t count = data.vertices.size();
        if (count < 2) {
            return "its vertical geometry has fewer than two vertices";
        }
        const auto kindOf = [&](std::size_t index) {
            return index < data.segments.size() ? data.segments[index].kind
                                                : SegmentKind::Straight;
        };
        vertical.pvis.push_back({data.vertices[0].x, data.vertices[0].y, 0.0});
        for (std::size_t i = 0; i + 1 < count; ++i) {
            const Vertex& a = data.vertices[i];
            const Vertex& b = data.vertices[i + 1];
            const double length = b.x - a.x;
            if (!(length > 0.0)) {
                return "its vertical geometry goes backwards in chainage";
            }
            if (kindOf(i) == SegmentKind::Straight) {
                if (i + 2 < count && kindOf(i + 1) == SegmentKind::Straight) {
                    vertical.pvis.push_back({b.x, b.y, 0.0});
                }
                continue;
            }
            katana::geometry::ProfilePVI pvi;
            pvi.curveLength = length;
            if (kindOf(i) == SegmentKind::Parabola) {
                const auto chainage = data.segments[i].parameters.real("chainage");
                const auto height = data.segments[i].parameters.real("height");
                if (!chainage || !height) {
                    return "a vertical parabola does not record its VIP";
                }
                pvi.station = *chainage;
                pvi.elevation = *height;
                if (std::fabs(*chainage - 0.5 * (a.x + b.x)) > 1e-3 * length) {
                    notes.push_back("an asymmetric vertical parabola is approximated by a "
                                    "symmetric one of the same length");
                    pvi.station = 0.5 * (a.x + b.x);
                    // Keep the entering grade: the VIP slides along it.
                    const double grade = (*height - a.y) / (*chainage - a.x);
                    pvi.elevation = a.y + grade * (pvi.station - a.x);
                }
            } else if (kindOf(i) == SegmentKind::Arc) {
                // A circular vertical curve. Its end tangents are taken from
                // the circle and a parabola fitted to them: for the grades of
                // a road or railway the two differ by less than a millimetre.
                detail::ArcCircle arc;
                const Point2 start(a.x, a.y);
                if (!detail::solveArc(start, Point2(b.x, b.y), data.segments[i],
                                      kArcSolveTolerance, arc)) {
                    return "a vertical arc cannot span its end points";
                }
                // The entering grade is the arc's tangent there; the VIP of the
                // symmetric parabola sits on it, mid-way.
                const double grade = std::tan(detail::arcTangentAt(arc, start));
                pvi.station = 0.5 * (a.x + b.x);
                pvi.elevation = a.y + grade * (pvi.station - a.x);
                notes.push_back("a circular vertical curve is represented by the parabola with "
                                "the same tangents");
            } else {
                return "its vertical geometry holds a segment that is not a line, parabola or arc";
            }
            vertical.pvis.push_back(pvi);
        }
        const Vertex& last = data.vertices.back();
        vertical.pvis.push_back({last.x, last.y, 0.0});
        return {};
    }

    // PVIs from IP-method vertical parts (and so from an old alignment
    // string's vipdata).
    [[nodiscard]] static std::string pvisFromParts(const SuperAlignment& alignment,
                                                   katana::geometry::VerticalAlignment& vertical,
                                                   std::vector<std::string>& notes)
    {
        const auto& parts = alignment.verticalParts;
        for (std::size_t i = 0; i < parts.size(); ++i) {
            const auto x = parts[i].fields.real("x");
            const auto y = parts[i].fields.real("y");
            if (!x || !y) {
                return "a VIP has no chainage or height";
            }
            katana::geometry::ProfilePVI pvi{*x, *y, 0.0};
            const bool interior = i > 0 && i + 1 < parts.size();
            if (interior && parts[i].kind != "ip") {
                const auto px = parts[i - 1].fields.real("x");
                const auto py = parts[i - 1].fields.real("y");
                const auto nx = parts[i + 1].fields.real("x");
                const auto ny = parts[i + 1].fields.real("y");
                if (!px || !py || !nx || !ny || !(*x > *px) || !(*nx > *x)) {
                    return "its VIPs are not in increasing chainage";
                }
                // Grades in percent, as the K value is defined: K is the
                // length of curve per percent of grade change.
                const double change =
                    std::fabs((*ny - *y) / (*nx - *x) - (*y - *py) / (*x - *px)) * 100.0;
                const std::string& kind = parts[i].kind;
                if (kind == "length") {
                    const auto l1 = parts[i].fields.real("l1");
                    const auto l2 = parts[i].fields.real("l2");
                    pvi.curveLength = parts[i].fields.real("l").value_or(0.0);
                    if (l1 && l2) {
                        pvi.curveLength = *l1 + *l2;
                        notes.push_back("an asymmetric vertical parabola is approximated by a "
                                        "symmetric one of the same length");
                    }
                } else if (kind == "asymmetric") {
                    pvi.curveLength = parts[i].fields.real("l1").value_or(0.0) +
                                      parts[i].fields.real("l2").value_or(0.0);
                    notes.push_back("an asymmetric vertical parabola is approximated by a "
                                    "symmetric one of the same length");
                } else if (kind == "kvalue") {
                    pvi.curveLength = parts[i].fields.real("k").value_or(0.0) * change;
                } else if (kind == "radius" || kind == "arc") {
                    // L = R * |change of grade|, the grade as a ratio.
                    pvi.curveLength = parts[i].fields.real("r").value_or(0.0) * change / 100.0;
                    if (kind == "arc") {
                        notes.push_back("a circular vertical curve is represented by the "
                                        "parabola with the same tangents");
                    }
                }
                pvi.curveLength = std::fabs(pvi.curveLength);
            }
            vertical.pvis.push_back(pvi);
        }
        return {};
    }

    std::size_t import(const SuperAlignment& alignment)
    {
        const std::string keyword = alignment.source == AlignmentSource::Alignment
                                        ? "string alignment"
                                    : alignment.source == AlignmentSource::Pipeline
                                        ? "string pipeline"
                                        : "string super_alignment";
        const std::string what = describe(alignment.header, keyword);
        std::size_t added = 0;

        // The definition Katana can hold, if there is one.
        Reconstruction reconstruction;
        if (alignment.horizontalIsIpOnly()) {
            reconstruction = pisFromParts(alignment);
        } else if (alignment.horizontalData) {
            reconstruction = pisFromElements(*alignment.horizontalData);
        } else {
            reconstruction.failure = "it has neither IPs nor solved geometry";
        }
        const std::string spiral = detail::lowered(alignment.spiralType);
        const bool usesSpirals =
            std::any_of(reconstruction.horizontal.pis.begin(), reconstruction.horizontal.pis.end(),
                        [](const auto& pi) { return pi.spiralIn > 0.0 || pi.spiralOut > 0.0; });
        std::string approximation;
        if (reconstruction.failure.empty() && alignment.horizontalIsIpOnly() && usesSpirals &&
            !spiral.empty() && spiral != "clothoid" && spiral != "natural clothoid") {
            // Defined by IPs, with a transition Katana does not have. Where 12d
            // also wrote the solved geometry (below) that is checked and the
            // alignment is refused if it differs; here there is nothing to
            // check against, and an alignment with clothoids of the same
            // lengths - out by centimetres on a tight railway curve - is worth
            // far more than no alignment. It is said, per alignment.
            approximation = " its transitions are of type \"" + alignment.spiralType +
                            "\" and are represented by clothoids of the same length, which "
                            "differ from them by up to a few centimetres";
        }
        reconstruction.horizontal.startStation = alignment.header.chainage;

        std::optional<katana::geometry::SolvedAlignment> solved;
        if (reconstruction.failure.empty()) {
            auto attempt = katana::geometry::solveAlignment(reconstruction.horizontal);
            if (!attempt) {
                reconstruction.failure = attempt.error().message;
            } else {
                solved = std::move(*attempt);
            }
        }
        if (solved && alignment.horizontalData) {
            // The check that makes the reconstruction trustworthy: every
            // vertex 12d recorded must lie on what Katana solved.
            const Polyline2 line = solved->toPolyline(options_.alignmentTolerance * 0.1);
            double worst = 0.0;
            for (const Vertex& vertex : alignment.horizontalData->vertices) {
                worst = std::max(worst, line.distanceTo(plan(vertex)).value_or(0.0));
            }
            if (worst > options_.alignmentTolerance) {
                reconstruction.failure =
                    "the alignment Katana solves from its PIs departs from 12d's by " +
                    detail::formatReal(worst, 3);
                solved.reset();
            }
        }

        // The centreline as a polyline - always, so that an alignment Katana
        // cannot define is still on the drawing.
        Entity entity = makeEntity(alignment.header, keyword);
        bool haveLine = false;
        if (alignment.horizontalData) {
            haveLine = setStringGeometry(entity, alignment.horizontalData->vertices,
                                         alignment.horizontalData->segments,
                                         alignment.horizontalData->closed, std::nullopt);
        } else if (solved) {
            entity.geometry = solved->toPolyline(options_.curveTolerance);
            haveLine = true;
        }
        if (haveLine) {
            if (alignment.diameter) {
                entity.metadata.insert_or_assign("12d.diameter", *alignment.diameter);
            }
            added += add(std::move(entity), what) ? 1 : 0;
        }

        if (!solved) {
            // Nothing solved and nothing recorded: the tangent polygon through
            // the IPs is still worth drawing, and is what the designer placed.
            if (!haveLine && !reconstruction.horizontal.pis.empty()) {
                Polyline2 polygon;
                for (const auto& pi : reconstruction.horizontal.pis) {
                    polygon.vertices.push_back(pi.point);
                }
                Entity fallback = makeEntity(alignment.header, keyword);
                fallback.geometry = polygon.vertices.size() == 1
                                        ? katana::entity::Geometry(
                                              katana::entity::PointGeometry{polygon.vertices[0]})
                                        : katana::entity::Geometry(std::move(polygon));
                haveLine = add(std::move(fallback), what);
                problem(what + (haveLine ? " is imported as the polygon through its IPs only: "
                                         : " skipped: ") +
                        reconstruction.failure);
                return added + (haveLine ? 1 : 0);
            }
            problem(what + (haveLine ? " is imported as a polyline only: " : " skipped: ") +
                    reconstruction.failure);
            return added;
        }
        if (!approximation.empty()) {
            problem(what + ":" + approximation);
        }

        katana::entity::Alignment record;
        record.name = uniqueAlignmentName(alignment.header.name);
        record.description = "Imported from 12d model \"" + alignment.header.model + "\"";
        record.horizontal = reconstruction.horizontal;

        std::vector<std::string> notes;
        katana::geometry::VerticalAlignment vertical;
        std::string verticalFailure;
        if (alignment.verticalData && alignment.verticalData->vertices.size() >= 2) {
            verticalFailure = pvisFromElements(*alignment.verticalData, vertical, notes);
        } else if (alignment.verticalIsIpOnly() && alignment.verticalParts.size() >= 2) {
            verticalFailure = pvisFromParts(alignment, vertical, notes);
        }
        if (verticalFailure.empty() && vertical.pvis.size() >= 2) {
            auto profile = katana::geometry::solveProfile(vertical);
            if (!profile) {
                verticalFailure = profile.error().message;
            } else if (alignment.verticalData) {
                // The same check the plan geometry gets: every vertex 12d
                // recorded must lie on the grade line Katana solves.
                double worst = 0.0;
                for (const Vertex& vertex : alignment.verticalData->vertices) {
                    const auto height = profile->elevationAt(vertex.x);
                    worst = std::max(worst, height ? std::fabs(*height - vertex.y)
                                                   : options_.alignmentTolerance * 2.0);
                }
                if (worst > options_.alignmentTolerance) {
                    verticalFailure = "the grade line Katana solves from its VIPs departs from "
                                      "12d's by " +
                                      detail::formatReal(worst, 3);
                }
            }
            if (verticalFailure.empty()) {
                record.vertical = vertical;
            }
        }
        if (!verticalFailure.empty()) {
            problem(what + ": its vertical geometry was not imported: " + verticalFailure);
        }
        for (const std::string& note : notes) {
            if (record.vertical && noted_.insert(note).second) {
                result_.warnings.push_back(note);
            }
        }
        if (const auto status = katana::entity::validate(record); !status) {
            problem(what + " is imported as a polyline only: " + status.error().message);
            return added;
        }
        result_.alignments.push_back(std::move(record));
        return added + 1;
    }

    [[nodiscard]] std::string uniqueAlignmentName(const std::string& wanted)
    {
        const std::string base = wanted.empty() ? "Alignment" : wanted;
        std::string name = base;
        for (int copy = 2; !alignmentNames_.insert(detail::lowered(name)).second; ++copy) {
            name = base + " (" + std::to_string(copy) + ")";
        }
        return name;
    }

    // ---- surfaces, clouds, meshes -----------------------------------------------------------

    std::size_t import(const Tin& tin)
    {
        const std::string what = std::string(tin.full ? "full_tin" : "tin") + " \"" + tin.name + "\"";
        ImportedSurface surface;
        surface.name = tin.name.empty() ? "tin" : tin.name;
        surface.colour = tin.colour;
        surface.trianglesInFile = tin.triangles.size();

        constexpr std::uint32_t kUnused = 0xFFFFFFFFu;
        std::vector<std::uint32_t> remap(tin.points.size(), kUnused);
        std::vector<katana::geometry::Point3> vertices;
        std::vector<katana::terrain::TinTriangle> triangles;
        for (std::size_t i = 0; i < tin.triangles.size(); ++i) {
            const auto& triangle = tin.triangles[i];
            const bool hasHeights = tin.points[triangle[0]].z && tin.points[triangle[1]].z &&
                                    tin.points[triangle[2]].z;
            const bool distinct = triangle[0] != triangle[1] && triangle[1] != triangle[2] &&
                                  triangle[2] != triangle[0];
            // A triangle with a null height has no surface to give: 12d treats
            // it as null too.
            if (!tin.isSurfaceTriangle(i) || !hasHeights || !distinct) {
                ++surface.trianglesNulled;
                continue;
            }
            const katana::geometry::Triangle2 shape{plan(tin.points[triangle[0]]),
                                                    plan(tin.points[triangle[1]]),
                                                    plan(tin.points[triangle[2]])};
            // Nor has one with no area in PLAN: there is no ground under it to
            // interpolate over. Worse, which way round such a triangle was
            // listed is noise rather than information - its doubled area is
            // below the rounding error of computing it - so keeping it would
            // hand TinSurface an edge direction chosen by the last bits of a
            // subtraction, and one inside-out triangle refuses the whole
            // surface. Real 12d tins carry them: `plot_PW_example_data.12da`
            // has seven, the smallest with a doubled area of 4.9e-15 m^2 over
            // coordinates of 6.2e6 m.
            if (shape.isDegenerate()) {
                ++surface.trianglesNulled;
                continue;
            }
            katana::terrain::TinTriangle mapped{};
            for (std::size_t k = 0; k < 3; ++k) {
                std::uint32_t& slot = remap[triangle[k]];
                if (slot == kUnused) {
                    slot = static_cast<std::uint32_t>(vertices.size());
                    const Vertex& point = tin.points[triangle[k]];
                    const Point2 p = plan(point);
                    vertices.emplace_back(p.x, p.y, *point.z);
                }
                mapped[k] = slot;
            }
            // 12d lists a triangle clockwise seen from above; TinSurface wants
            // it counter-clockwise. The sign is MEASURED rather than assumed,
            // because writers other than 12d get this wrong, and a surface
            // built inside out fails as a whole.
            if (shape.signedArea() < 0.0) {
                std::swap(mapped[1], mapped[2]);
            }
            triangles.push_back(mapped);
        }
        if (triangles.empty()) {
            problem(what + " skipped: it has no visible triangles");
            return 0;
        }
        auto built = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles));
        if (!built) {
            problem(what + " skipped: " + built.error().describe());
            return 0;
        }
        surface.surface = std::move(*built);
        result_.bounds.expand(surface.surface.bounds());
        result_.surfaces.push_back(std::move(surface));
        if (!tin.colours.empty()) {
            ++colouredTins_;
        }
        return 1;
    }

    std::size_t import(const SuperTin& superTin)
    {
        // Nothing to build: a super tin is a ranking of tins, and the tins
        // themselves are what carries the surface.
        result_.superTins.push_back(superTin);
        return 1;
    }

    // A trimesh becomes a mesh in the session, beside the surfaces. Its
    // vertices come across whole - 12d's indices are one-based and the
    // reader has already subtracted - and a face that names a vertex which
    // does not exist is dropped and counted rather than costing the mesh.
    std::size_t import(const Trimesh& trimesh)
    {
        ImportedMesh imported;
        imported.name = trimesh.name.empty() ? "trimesh" : trimesh.name;
        imported.layer = layerPathForModel(trimesh.model, options_.layerPrefix);
        imported.colourName = trimesh.colour;
        imported.color = standardColour(trimesh.colour);
        imported.edgesInFile = trimesh.edges.size();
        imported.vertexInfosInFile = trimesh.vertexInfos.size();
        imported.edgeInfosInFile = trimesh.edgeInfos.size();
        if (options_.attributesAsProperties) {
            flattenAttributes(trimesh.attributes, {}, imported.properties);
        }

        imported.mesh.vertices.reserve(trimesh.vertices.size());
        for (const Vertex& vertex : trimesh.vertices) {
            // A mesh vertex with no height is not a vertex: a face that used
            // it is dropped below, because it names a point in space.
            imported.mesh.vertices.emplace_back(vertex.x, vertex.y, vertex.z.value_or(0.0));
        }
        std::size_t dropped = 0;
        const std::uint32_t count = static_cast<std::uint32_t>(trimesh.vertices.size());
        for (std::size_t i = 0; i < trimesh.faces.size(); ++i) {
            const auto& face = trimesh.faces[i];
            const bool named = face[0] < count && face[1] < count && face[2] < count;
            const bool distinct = face[0] != face[1] && face[1] != face[2] && face[2] != face[0];
            const bool solid = named && trimesh.vertices[face[0]].z &&
                               trimesh.vertices[face[1]].z && trimesh.vertices[face[2]].z;
            if (!named || !distinct || !solid) {
                ++dropped;
                continue;
            }
            imported.mesh.faces.push_back(face);
            // face_flags are ONE-based into face_infos, 0 meaning none
            // (manual 1.4.9); a flag out of range is treated as none.
            std::string colour;
            if (i < trimesh.faceFlags.size()) {
                const std::uint32_t flag = trimesh.faceFlags[i];
                if (flag != 0 && flag <= trimesh.faceInfos.size()) {
                    colour = trimesh.faceInfos[flag - 1].colour;
                }
            }
            imported.faceColourNames.push_back(colour);
            imported.faceColors.push_back(colour.empty() ? std::nullopt : standardColour(colour));
        }
        // All or nothing: a table of face colours with a gap in it would be
        // read by position, so if no face was coloured it is no table.
        if (std::all_of(imported.faceColourNames.begin(), imported.faceColourNames.end(),
                        [](const std::string& colour) { return colour.empty(); })) {
            imported.faceColourNames.clear();
            imported.faceColors.clear();
        }
        if (dropped != 0) {
            trimeshFacesDropped_ += dropped;
        }
        if (imported.mesh.faces.empty()) {
            ++trimeshesEmpty_;
            return 0;
        }
        result_.meshes.push_back(std::move(imported));
        return 1;
    }

    std::size_t import(const LasCloud& cloud)
    {
        ImportedCloud imported;
        imported.name = cloud.header.name.empty() ? "12d cloud" : cloud.header.name;
        imported.referenceFile = cloud.referenceFile;
        imported.points.reserve(cloud.points.size());
        for (const LasPoint& point : cloud.points) {
            const Point2 p = plan(point.x, point.y);
            imported.points.push_back({p.x, p.y, point.z, point.intensity, point.classification});
            result_.bounds.expand(p);
        }
        if (imported.points.empty() && imported.referenceFile.empty()) {
            problem(describe(cloud.header, "string las_cloud_data") + " skipped: it has no points");
            return 0;
        }
        result_.clouds.push_back(std::move(imported));
        return 1;
    }

    // A super tin is a ranking of tins, and the tins are read as separate
    // surfaces - so this can only be done once every element has been, and
    // it is: the list may name a tin that appears later in the file.
    void buildSuperTins()
    {
        // The members stay in the list as well. A tin and a super tin are
        // separate objects in 12d, and a user who asked for both should get
        // both rather than have the parts swallowed by the whole.
        const std::size_t members = result_.surfaces.size();
        for (const SuperTin& superTin : result_.superTins) {
            std::vector<const katana::terrain::TinSurface*> ranked;
            std::vector<std::string> missing;
            for (const std::string& name : superTin.tins) {
                const auto found =
                    std::find_if(result_.surfaces.begin(), result_.surfaces.begin() +
                                                               static_cast<std::ptrdiff_t>(members),
                                 [&name](const ImportedSurface& surface) {
                                     // Names are compared without case (manual 1.4.8).
                                     return detail::lowered(surface.name) == detail::lowered(name);
                                 });
                if (found == result_.surfaces.begin() + static_cast<std::ptrdiff_t>(members)) {
                    missing.push_back(name);
                    continue;
                }
                ranked.push_back(&found->surface);
            }
            if (!missing.empty()) {
                std::string names;
                for (const std::string& name : missing) {
                    names += (names.empty() ? "" : ", ") + name;
                }
                problem("super tin \"" + superTin.name + "\" names " +
                        std::to_string(missing.size()) + " tins the archive does not carry (" +
                        names + "); it is built from the rest");
            }
            if (ranked.empty()) {
                continue;
            }
            auto combined = katana::terrain::combineSurfaces(ranked);
            if (!combined) {
                problem("super tin \"" + superTin.name +
                        "\" could not be built: " + combined.error().describe());
                continue;
            }
            ImportedSurface surface;
            surface.name = superTin.name.empty() ? "super tin" : superTin.name;
            surface.colour = superTin.colour;
            surface.trianglesInFile = combined->triangleCount();
            surface.surface = std::move(*combined);
            result_.bounds.expand(surface.surface.bounds());
            result_.surfaces.push_back(std::move(surface));
            ++superTinsBuilt_;
        }
    }

    void finishWarnings()
    {
        auto& warnings = result_.warnings;
        if (namedProblems_ > kMaxNamedProblems) {
            warnings.push_back(std::to_string(namedProblems_ - kMaxNamedProblems) +
                               " further elements had problems and are not listed");
        }
        if (trimeshesEmpty_ != 0) {
            warnings.push_back(std::to_string(trimeshesEmpty_) +
                               " trimeshes have no triangle that names three distinct vertices "
                               "with heights, and are not imported");
        }
        if (trimeshFacesDropped_ != 0) {
            warnings.push_back(std::to_string(trimeshFacesDropped_) +
                               " mesh faces were dropped: they name a vertex that does not "
                               "exist, repeat one, or have no height");
        }
        if (partlyInvisible_ != 0) {
            warnings.push_back(std::to_string(partlyInvisible_) +
                               " strings have invisible vertices or segments, which are drawn "
                               "(the flags are kept in the entity metadata)");
        }
        if (symbolsLeft_ != 0) {
            warnings.push_back(std::to_string(symbolsLeft_) +
                               " strings carry a DIFFERENT symbol per vertex, which are kept and "
                               "written back but not drawn: a string is drawn with one symbol, "
                               "from its style, and there is no one symbol here");
        }
        if (annotationsLeft_ != 0) {
            warnings.push_back(std::to_string(annotationsLeft_) +
                               " strings carry per-vertex or per-segment annotation settings "
                               "beyond the text height and angle that were taken");
        }
        if (pointStringsSplit_ != 0) {
            problem(std::to_string(pointStringsSplit_) +
                    " strings marked `breakline point` held more than one vertex and became " +
                    std::to_string(pointsFromStrings_) +
                    " points, which is what that flag means");
        }
        if (colouredTins_ != 0) {
            warnings.push_back(std::to_string(colouredTins_) +
                               " surfaces carry per-triangle colours, which a surface here does "
                               "not have");
        }
        if (chordReport_.arcs + chordReport_.transitions != 0) {
            warnings.push_back(std::to_string(chordReport_.arcs) + " arcs and " +
                               std::to_string(chordReport_.transitions) +
                               " transitions were chorded to within " +
                               detail::formatReal(options_.curveTolerance, 6) + " of the curve");
        }
        for (const auto& [type, adjustment] : chordReport_.adjustmentByType) {
            warnings.push_back("transitions of type \"" + type +
                               "\" are drawn as clothoids and moved by up to " +
                               detail::formatReal(adjustment, 3) +
                               " to meet the end points 12d recorded");
        }
        if (chordReport_.straightened != 0) {
            warnings.push_back(std::to_string(chordReport_.straightened) +
                               " curved segments could not be reproduced from their parameters "
                               "and are drawn as straight lines");
        }
        for (const auto& [path, count] : archive_.unrecognised) {
            warnings.push_back("not read: " + path + " (" + std::to_string(count) + ")");
        }
        warnings.insert(warnings.begin(), archive_.warnings.begin(), archive_.warnings.end());
    }

    const Archive& archive_;
    ImportOptions options_;
    DomainImport result_;
    std::map<std::string, std::string> layerOfModel_;
    // Linestyle name -> its place in result_.stylesNeeded; see styleFor.
    std::unordered_map<std::string, std::size_t> styleIndex_;
    std::set<std::string> alignmentNames_;
    std::set<std::string> noted_;
    detail::ChordReport chordReport_;
    std::size_t namedProblems_ = 0;
    std::size_t superTinsBuilt_ = 0;
    std::size_t trimeshesEmpty_ = 0;
    std::size_t trimeshFacesDropped_ = 0;
    std::size_t partlyInvisible_ = 0;
    std::size_t symbolsLeft_ = 0;
    std::size_t annotationsLeft_ = 0;
    std::size_t colouredTins_ = 0;
    std::size_t pointStringsSplit_ = 0;
    std::size_t pointsFromStrings_ = 0;
};

} // namespace

katana::core::Result<DomainImport> toDomain(const Archive& archive, const ImportOptions& options)
{
    if (!(options.curveTolerance > 0.0) || !std::isfinite(options.curveTolerance)) {
        return makeError(ErrorCode::InvalidArgument, "the curve tolerance must be positive");
    }
    if (!(options.alignmentTolerance > 0.0) || !std::isfinite(options.alignmentTolerance)) {
        return makeError(ErrorCode::InvalidArgument, "the alignment tolerance must be positive");
    }
    if (!options.layerPrefix.empty()) {
        if (const auto status = katana::entity::validateLayerPath(options.layerPrefix); !status) {
            return status.error();
        }
    }
    if (options.originShift && !(std::isfinite(options.originShift->x) &&
                                 std::isfinite(options.originShift->y))) {
        return makeError(ErrorCode::InvalidArgument, "the origin shift is not finite");
    }
    Importer importer(archive, options);
    return importer.run();
}

} // namespace katana::archive12d
