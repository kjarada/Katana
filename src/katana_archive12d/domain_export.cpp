#include <algorithm>
#include <cmath>
#include <numbers>
#include <unordered_set>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "text_utilities.hpp"

namespace katana::archive12d {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::entity::Entity;
using katana::entity::PropertyValue;
using katana::geometry::Point2;
using katana::geometry::Vec2;

constexpr double kPi = std::numbers::pi;

// The model alignments are written into. Katana's alignments belong to the
// drawing, not to a layer, and a 12d string must be in some model.
constexpr const char* kAlignmentModel = "alignments";

[[nodiscard]] double degrees(double radiansValue)
{
    const double value = std::fmod(radiansValue * 180.0 / kPi, 360.0);
    return value < 0.0 ? value + 360.0 : value;
}

[[nodiscard]] const std::string* textOf(const katana::entity::PropertyMap& map,
                                        std::string_view key)
{
    const auto found = map.find(key);
    return found == map.end() ? nullptr : std::get_if<std::string>(&found->second);
}

[[nodiscard]] std::optional<double> realOf(const katana::entity::PropertyMap& map,
                                           std::string_view key)
{
    const auto found = map.find(key);
    if (found == map.end()) {
        return std::nullopt;
    }
    if (const auto* real = std::get_if<double>(&found->second)) {
        return *real;
    }
    if (const auto* integer = std::get_if<std::int64_t>(&found->second)) {
        return static_cast<double>(*integer);
    }
    return std::nullopt;
}

// "Asset/Dimensions/Size" -> group Asset { group Dimensions { Size } }: the
// inverse of the flattening import does.
void insertAttribute(AttributeList& attributes, std::string_view path, const PropertyValue& value)
{
    const std::size_t slash = path.find('/');
    if (slash != std::string_view::npos && slash > 0 && slash + 1 < path.size()) {
        const std::string_view groupName = path.substr(0, slash);
        for (Attribute& attribute : attributes) {
            if (attribute.name == groupName) {
                if (auto* members = std::get_if<AttributeList>(&attribute.value)) {
                    insertAttribute(*members, path.substr(slash + 1), value);
                    return;
                }
            }
        }
        Attribute group;
        group.name = std::string(groupName);
        AttributeList members;
        insertAttribute(members, path.substr(slash + 1), value);
        group.value = std::move(members);
        attributes.push_back(std::move(group));
        return;
    }
    Attribute attribute;
    attribute.name = std::string(path);
    if (const auto* flag = std::get_if<bool>(&value)) {
        attribute.value = std::int64_t{*flag ? 1 : 0}; // 12d has no boolean attribute
    } else if (const auto* integer = std::get_if<std::int64_t>(&value)) {
        attribute.value = *integer;
    } else if (const auto* real = std::get_if<double>(&value)) {
        attribute.value = *real;
    } else if (const auto* text = std::get_if<std::string>(&value)) {
        attribute.value = *text;
    }
    attributes.push_back(std::move(attribute));
}

class Exporter {
  public:
    Exporter(const katana::entity::Model& model, const ExportOptions& options)
        : model_(model), options_(options)
    {
    }

    DomainExport run(const std::vector<ExportSurface>& surfaces)
    {
        const std::unordered_set<katana::entity::EntityId> wanted(options_.entities.begin(),
                                                                  options_.entities.end());
        model_.entities.forEach([&](const Entity& entity) {
            if (!wanted.empty() && !wanted.contains(entity.id)) {
                return;
            }
            if (write(entity)) {
                ++result_.entitiesWritten;
            } else {
                ++result_.entitiesSkipped;
            }
        });
        if (dimensions_ != 0) {
            result_.warnings.push_back(std::to_string(dimensions_) +
                                       " dimensions were not written: the 12da format has no "
                                       "dimension element");
        }
        if (options_.includeAlignments && wanted.empty()) {
            for (const katana::entity::Alignment& alignment : model_.alignments.all()) {
                write(alignment);
            }
        }
        for (const ExportSurface& surface : surfaces) {
            if (surface.surface != nullptr && !surface.surface->empty()) {
                write(surface);
            }
        }
        return std::move(result_);
    }

  private:
    [[nodiscard]] Vertex vertex(const Point2& point, std::optional<double> z) const
    {
        const Vec2 shift = options_.originShift.value_or(Vec2{});
        return Vertex{point.x + shift.x, point.y + shift.y, z};
    }

    // The header 12d needs, from what import left in the metadata where there
    // is any, and from the entity itself where there is not.
    [[nodiscard]] StringHeader headerFor(const Entity& entity, Breakline fallback) const
    {
        StringHeader header;
        header.model = entity.layer;
        if (const auto* name = textOf(entity.metadata, kMetaName)) {
            header.name = *name;
        }
        // The entity's own style is the 12d linestyle; ByLayer has no name in
        // 12d, and "1" is its default linestyle (manual 1.4.3). A point that
        // took its symbol's style still writes the linestyle it came with.
        if (const auto* stringStyle = textOf(entity.metadata, kMetaStringStyle)) {
            header.style = *stringStyle;
        } else {
            header.style = entity.style.empty() ? "1" : entity.style;
        }
        header.chainage = realOf(entity.metadata, kMetaChainage).value_or(0.0);
        const auto* breakline = textOf(entity.metadata, kMetaBreakline);
        header.breakline = breakline == nullptr ? fallback
                           : *breakline == "point" ? Breakline::Point
                                                   : Breakline::Line;

        // Colour: the 12d name the entity came with, unless its colour has
        // been changed since, in which case the name would be a lie.
        const auto* named = textOf(entity.metadata, kMetaColour);
        std::optional<katana::entity::Color> rgb = entity.color;
        if (!rgb) {
            if (const katana::entity::Layer* layer = model_.layers.find(entity.layer)) {
                rgb = layer->color;
            }
        }
        // A point that took its colour from its symbol (kMetaSymbolPrefix)
        // has a string colour that is deliberately not the entity's, so the
        // entity's colour must not be used to second-guess the name.
        const bool colourIsTheSymbols =
            textOf(entity.metadata, std::string(kMetaSymbolPrefix) + "colour") != nullptr;
        if (named != nullptr && (colourIsTheSymbols || !entity.color ||
                                 standardColour(*named) == entity.color ||
                                 !standardColour(*named))) {
            header.colour = *named;
        } else if (rgb) {
            header.colour = nearestStandardColour(*rgb);
        } else {
            header.colour = "red"; // the format's own default (manual 1.4.2)
        }

        if (options_.propertiesAsAttributes) {
            for (const auto& [key, value] : entity.properties) {
                // Heights are geometry, and `vertex/3/...` belongs to vertex 3
                // (finish() puts those back where they came from).
                if (key != kElevationProperty && key != kElevationsProperty &&
                    !key.starts_with("vertex/") && !key.starts_with("segment/")) {
                    insertAttribute(header.attributes, key, value);
                }
            }
        }
        // The scalars import kept as `12d.x.<key>`, in the order they were
        // read - std::map keeps keys sorted, which is not that order, but the
        // format does not care where a scalar stands in its block.
        for (const auto& [key, value] : entity.metadata) {
            if (key.starts_with(kMetaExtraPrefix)) {
                if (const auto* text = std::get_if<std::string>(&value)) {
                    header.extras.add(key.substr(kMetaExtraPrefix.size()), *text);
                }
            }
        }
        return header;
    }

    // Properties keyed `<prefix>/<index>/<path>` back into one attribute list
    // per index, 1-based as import wrote them. `count` sizes the result; an
    // index past it - a vertex removed since import - is dropped.
    [[nodiscard]] static std::vector<AttributeList>
    positionalAttributes(const Entity& entity, std::string_view prefix, std::size_t count)
    {
        std::vector<AttributeList> lists;
        for (const auto& [key, value] : entity.properties) {
            if (!key.starts_with(prefix) || key.size() <= prefix.size() + 1 ||
                key[prefix.size()] != '/') {
                continue;
            }
            const std::size_t slash = key.find('/', prefix.size() + 1);
            if (slash == std::string::npos || slash + 1 >= key.size()) {
                continue;
            }
            std::size_t index = 0;
            const auto digits = std::string_view(key).substr(prefix.size() + 1,
                                                             slash - prefix.size() - 1);
            for (const char ch : digits) {
                if (ch < '0' || ch > '9') {
                    index = 0;
                    break;
                }
                index = index * 10 + static_cast<std::size_t>(ch - '0');
            }
            if (index == 0 || index > count) {
                continue;
            }
            if (lists.size() < count) {
                lists.resize(count);
            }
            insertAttribute(lists[index - 1], std::string_view(key).substr(slash + 1), value);
        }
        return lists;
    }

    [[nodiscard]] static std::vector<std::optional<double>> heightsOf(const Entity& entity,
                                                                      std::size_t count)
    {
        return entityHeights(entity, count);
    }

    bool write(const Entity& entity)
    {
        struct Visitor {
            Exporter& self;
            const Entity& entity;

            bool operator()(const katana::entity::PointGeometry& point) const
            {
                VertexString string;
                string.header = self.headerFor(entity, Breakline::Point);
                string.vertices.push_back(self.vertex(point.position, heightsOf(entity, 1)[0]));
                // A point whose style draws a symbol is written with the
                // block 12d draws it from (see kMetaSymbolPrefix): the
                // `symbol_value` form, which is what 12d writes on a point.
                if (const auto* style = self.model_.styles.find(entity.style);
                    style != nullptr && !style->symbol.empty()) {
                    FieldList symbol;
                    symbol.setText("style", style->name);
                    const auto* colour = textOf(entity.metadata, std::string(kMetaSymbolPrefix) + "colour");
                    symbol.setText("colour", colour != nullptr ? *colour : string.header.colour);
                    // The block's fields are kept as the text they were, so a
                    // size that differs from the style's is text too.
                    const auto* size = textOf(entity.metadata, std::string(kMetaSymbolPrefix) + "size");
                    symbol.setReal("size", (size != nullptr ? parseReal(*size) : std::nullopt)
                                               .value_or(style->symbolSize));
                    for (const char* key : {"rotation", "offset", "raise"}) {
                        const auto* kept = textOf(entity.metadata, std::string(kMetaSymbolPrefix) + key);
                        symbol.add(key, kept != nullptr ? *kept : std::string("0"));
                    }
                    string.symbol = std::move(symbol);
                }
                self.finish(entity, std::move(string));
                return true;
            }
            bool operator()(const katana::geometry::Segment2& segment) const
            {
                VertexString string;
                string.header = self.headerFor(entity, Breakline::Line);
                const auto heights = heightsOf(entity, 2);
                string.vertices.push_back(self.vertex(segment.start, heights[0]));
                string.vertices.push_back(self.vertex(segment.end, heights[1]));
                self.finish(entity, std::move(string));
                return true;
            }
            bool operator()(const katana::geometry::Polyline2& polyline) const
            {
                VertexString string;
                string.header = self.headerFor(entity, Breakline::Line);
                // A face or an interface string is written back as one: they
                // are current types with forms of their own (manual 1.5.4,
                // 1.5.6), not super strings.
                if (const auto* element = textOf(entity.metadata, kMetaElement)) {
                    if (*element == "string face") {
                        string.kind = StringKind::Face;
                    } else if (*element == "string interface") {
                        string.kind = StringKind::Interface;
                    }
                }
                string.closed = polyline.closed;
                const auto heights = heightsOf(entity, polyline.vertices.size());
                for (std::size_t i = 0; i < polyline.vertices.size(); ++i) {
                    string.vertices.push_back(self.vertex(polyline.vertices[i], heights[i]));
                }
                self.finish(entity, std::move(string));
                return true;
            }
            bool operator()(const katana::geometry::Arc2& arc) const
            {
                // A super string of two vertices joined by an arc: the current
                // form, and the only one that can say "the larger arc".
                VertexString string;
                string.header = self.headerFor(entity, Breakline::Line);
                const auto heights = heightsOf(entity, 2);
                string.vertices.push_back(self.vertex(arc.startPoint(), heights[0]));
                string.vertices.push_back(self.vertex(arc.endPoint(), heights[1]));
                Segment segment;
                segment.kind = SegmentKind::Arc;
                // Counter-clockwise is a positive sweep here and a NEGATIVE
                // radius in 12d, whose positive turns right.
                segment.radius = arc.sweep > 0.0 ? -arc.radius : arc.radius;
                segment.major = std::fabs(arc.sweep) > kPi;
                string.segments.push_back(std::move(segment));
                self.finish(entity, std::move(string));
                return true;
            }
            bool operator()(const katana::geometry::Circle2& circle) const
            {
                CircleString string;
                string.header = self.headerFor(entity, Breakline::Line);
                string.radius = circle.radius;
                string.centre = self.vertex(circle.center, heightsOf(entity, 1)[0]);
                self.result_.archive.elements.emplace_back(std::move(string));
                return true;
            }
            bool operator()(const katana::entity::TextGeometry& text) const
            {
                TextString string;
                string.header = self.headerFor(entity, Breakline::Point);
                string.text = text.text;
                string.position = self.vertex(text.position, heightsOf(entity, 1)[0]);
                string.annotation.setReal("worldsize", text.height);
                string.annotation.setReal("angle", degrees(text.rotation));
                // Katana anchors text at the left end of its baseline.
                string.annotation.setText("justify", "bottom-left");
                self.result_.archive.elements.emplace_back(std::move(string));
                return true;
            }
            bool operator()(const katana::entity::DimensionGeometry&) const
            {
                ++self.dimensions_;
                return false;
            }
        };
        return std::visit(Visitor{*this, entity}, entity.geometry);
    }

    // The symbol blocks a line's vertices carried, kept as one list per key
    // (see kMetaSymbolPrefix), go back as one block (`symbol_value`) or one
    // per vertex (`symbol_data`) by the length of the lists. Lists of
    // unequal length are the sign of an edit that broke them, and are not
    // written: a symbol block with a missing field is one 12d cannot draw.
    void restoreSymbolBlocks(const Entity& entity, VertexString& string) const
    {
        std::vector<std::pair<std::string, std::vector<std::string>>> columns;
        for (const auto& [key, value] : entity.metadata) {
            if (!key.starts_with(kMetaSymbolPrefix)) {
                continue;
            }
            if (const auto* text = std::get_if<std::string>(&value)) {
                columns.emplace_back(key.substr(kMetaSymbolPrefix.size()), splitList(*text));
            }
        }
        if (columns.empty()) {
            return;
        }
        const std::size_t count = columns.front().second.size();
        const bool aligned = std::all_of(columns.begin(), columns.end(), [count](const auto& column) {
            return column.second.size() == count;
        });
        if (!aligned || count == 0) {
            return;
        }
        std::vector<FieldList> blocks(count);
        for (const auto& [key, values] : columns) {
            for (std::size_t i = 0; i < count; ++i) {
                if (!values[i].empty()) {
                    blocks[i].add(key, values[i], key == "style" || key == "colour");
                }
            }
        }
        if (count == 1) {
            string.symbol = std::move(blocks.front());
        } else {
            string.symbols = std::move(blocks);
        }
    }

    void finish(const Entity& entity, VertexString string)
    {
        if (const auto* ids = textOf(entity.metadata, kMetaPointIds)) {
            std::vector<std::string> parts;
            std::size_t start = 0;
            while (start <= ids->size()) {
                std::size_t end = ids->find(',', start);
                if (end == std::string::npos) {
                    end = ids->size();
                }
                parts.push_back(ids->substr(start, end - start));
                start = end + 1;
            }
            if (parts.size() == string.vertices.size()) {
                string.pointIds = std::move(parts);
            }
        }
        const std::size_t vertices = string.vertices.size();
        const std::size_t segments = string.segmentCount();
        // A list only fits the geometry it was read from; one of another
        // length describes vertices that no longer exist and is left out.
        const auto reals = [&](std::string_view key, std::size_t count) {
            std::vector<double> values;
            if (const auto* text = textOf(entity.metadata, key)) {
                for (const std::string& item : splitList(*text)) {
                    if (const auto value = parseReal(item)) {
                        values.push_back(*value);
                    }
                }
            }
            if (values.size() != count) {
                values.clear();
            }
            return values;
        };
        const auto flags = [&](std::string_view key, std::optional<bool>& one,
                               std::vector<bool>& list, std::size_t count) {
            const auto* text = textOf(entity.metadata, key);
            if (text == nullptr) {
                return;
            }
            const auto items = splitList(*text);
            if (items.size() == 1 && count != 1) {
                one = items[0] != "0";
                return;
            }
            if (items.size() == count) {
                for (const std::string& item : items) {
                    list.push_back(item != "0");
                }
            }
        };

        if (const auto diameter = realOf(entity.metadata, "12d.diameter")) {
            string.diameter = *diameter;
        } else {
            string.diameters = reals("12d.diameters", segments);
        }
        const auto width = realOf(entity.metadata, "12d.culvert_width");
        const auto height = realOf(entity.metadata, "12d.culvert_height");
        if (width && height) {
            string.culvert = std::array<double, 2>{*width, *height};
        } else {
            const auto sizes = reals("12d.culverts", 2 * segments);
            for (std::size_t i = 0; i + 1 < sizes.size(); i += 2) {
                string.culverts.push_back({sizes[i], sizes[i + 1]});
            }
        }
        if (const auto* justify = textOf(entity.metadata, "12d.justify")) {
            string.justify = *justify;
        }
        flags("12d.vertex_visible", string.vertexVisibleValue, string.vertexVisible, vertices);
        flags("12d.segment_visible", string.segmentVisibleValue, string.segmentVisible, segments);
        flags("12d.vertex_tinable", string.vertexTinableValue, string.vertexTinable, vertices);
        flags("12d.segment_tinable", string.segmentTinableValue, string.segmentTinable, segments);
        if (const auto* colours = textOf(entity.metadata, "12d.segment_colours")) {
            auto items = splitList(*colours);
            if (items.size() == segments || items.size() == vertices) {
                string.segmentColours = std::move(items);
            }
        }
        if (string.kind == StringKind::Interface) {
            const auto modes = reals("12d.interface_modes", vertices);
            for (const double mode : modes) {
                string.interfaceModes.push_back(static_cast<int>(mode));
            }
            string.interfaceModes.resize(vertices, 0);
        }
        if (options_.propertiesAsAttributes) {
            string.vertexAttributes = positionalAttributes(entity, "vertex", vertices);
            string.segmentAttributes = positionalAttributes(entity, "segment", segments);
        }
        if (!string.symbol) {
            restoreSymbolBlocks(entity, string);
        }
        result_.archive.elements.emplace_back(std::move(string));
    }

    void write(const katana::entity::Alignment& alignment)
    {
        auto solved = katana::geometry::solveAlignment(alignment.horizontal);
        if (!solved) {
            result_.warnings.push_back("alignment \"" + alignment.name +
                                       "\" was not written: " + solved.error().message);
            return;
        }
        SuperAlignment out;
        out.header.name = alignment.name;
        out.header.model = kAlignmentModel;
        out.header.colour = "red";
        out.header.style = "1";
        out.header.chainage = alignment.horizontal.startStation;
        // Katana's transition is the exact Euler spiral, which is what 12d
        // calls the natural clothoid; its plain "clothoid" is an approximation.
        out.spiralType = "natural clothoid";
        out.validHorizontal = true;
        const Vec2 shift = options_.originShift.value_or(Vec2{});

        std::int64_t partId = 0;
        for (const katana::geometry::AlignmentPI& pi : alignment.horizontal.pis) {
            AlignmentPart part;
            const bool spirals = pi.radius > 0.0 && (pi.spiralIn > 0.0 || pi.spiralOut > 0.0);
            part.kind = spirals ? "spiral" : pi.radius > 0.0 ? "arc" : "ip";
            part.fields.setInteger("id", partId += 100); // manual 1.5.9.1.2: multiples of 100
            if (part.kind != "ip") {
                part.fields.setReal("r", pi.radius);
            }
            if (spirals) {
                part.fields.setReal("l1", pi.spiralIn);
                part.fields.setReal("l2", pi.spiralOut);
            }
            part.fields.setReal("x", pi.point.x + shift.x);
            part.fields.setReal("y", pi.point.y + shift.y);
            out.horizontalParts.push_back(std::move(part));
        }

        SolvedGeometry& plan = out.horizontalData.emplace();
        plan.header.setText("name", alignment.name);
        for (const katana::geometry::AlignmentElement& element : solved->elements()) {
            Segment segment;
            Point2 start;
            if (const auto* line = std::get_if<katana::geometry::Segment2>(&element.shape)) {
                start = line->start;
            } else if (const auto* arc = std::get_if<katana::geometry::Arc2>(&element.shape)) {
                start = arc->startPoint();
                segment.kind = SegmentKind::Arc;
                segment.radius = arc->sweep > 0.0 ? -arc->radius : arc->radius;
                segment.major = std::fabs(arc->sweep) > kPi;
            } else if (const auto* spiral =
                           std::get_if<katana::geometry::Spiral2>(&element.shape)) {
                start = spiral->start;
                segment = spiralSegment(*spiral);
            }
            plan.vertices.push_back(vertex(start, std::nullopt));
            plan.segments.push_back(std::move(segment));
        }
        if (const auto end = solved->pointAtStation(solved->endStation())) {
            plan.vertices.push_back(vertex(*end, std::nullopt));
        }

        if (alignment.vertical) {
            writeVertical(*alignment.vertical, partId, out);
        }
        result_.archive.elements.emplace_back(std::move(out));
        ++result_.alignmentsWritten;
    }

    // A spiral as 12d describes one: from its straight end, whichever end of
    // the segment that is. See plan_geometry.hpp for where that was learned.
    [[nodiscard]] static Segment spiralSegment(const katana::geometry::Spiral2& spiral)
    {
        Segment segment;
        segment.kind = SegmentKind::Spiral;
        FieldList& p = segment.parameters;
        const bool leading = std::fabs(spiral.startCurvature) <= std::fabs(spiral.endCurvature);
        const auto radiusOf = [](double curvature) {
            return curvature == 0.0 ? 0.0 : -1.0 / curvature; // positive turns right in 12d
        };
        p.setText("type", "natural clothoid");
        p.setInteger("leading", leading ? 1 : 0);
        p.setReal("l1", 0.0);
        if (leading) {
            p.setReal("r1", radiusOf(spiral.startCurvature));
            p.setReal("a1", degrees(spiral.startDirection));
            p.setReal("l2", spiral.length);
            p.setReal("r2", radiusOf(spiral.endCurvature));
            p.setReal("a2", degrees(spiral.endDirection()));
        } else {
            // Travelled backwards every turn changes hand, so the radii change
            // sign as well as the directions turning about. (0 stays +0: a
            // "-0" in the file would be read correctly and look like a bug.)
            const auto reversed = [&radiusOf](double curvature) {
                const double radius = radiusOf(curvature);
                return radius == 0.0 ? 0.0 : -radius;
            };
            p.setReal("r1", reversed(spiral.endCurvature));
            p.setReal("a1", degrees(spiral.endDirection() + kPi));
            p.setReal("l2", spiral.length);
            p.setReal("r2", reversed(spiral.startCurvature));
            p.setReal("a2", degrees(spiral.startDirection + kPi));
        }
        return segment;
    }

    void writeVertical(const katana::geometry::VerticalAlignment& vertical, std::int64_t& partId,
                       SuperAlignment& out)
    {
        auto profile = katana::geometry::solveProfile(vertical);
        if (!profile) {
            result_.warnings.push_back("the profile of alignment \"" + out.header.name +
                                       "\" was not written: " + profile.error().message);
            return;
        }
        out.validVertical = true;
        for (const katana::geometry::ProfilePVI& pvi : vertical.pvis) {
            AlignmentPart part;
            part.kind = pvi.curveLength > 0.0 ? "length" : "ip";
            part.fields.setInteger("id", partId += 100);
            if (pvi.curveLength > 0.0) {
                part.fields.setReal("l", pvi.curveLength);
            }
            part.fields.setReal("x", pvi.station);
            part.fields.setReal("y", pvi.elevation);
            out.verticalParts.push_back(std::move(part));
        }
        SolvedGeometry& grade = out.verticalData.emplace();
        grade.header.setText("name", out.header.name);
        for (const katana::geometry::ProfileElement& element : profile->elements()) {
            grade.vertices.push_back(
                Vertex{element.startStation, element.startElevation, std::nullopt});
            Segment segment;
            if (element.kind == katana::geometry::ProfileElementKind::Curve &&
                element.pvi < vertical.pvis.size()) {
                segment.kind = SegmentKind::Parabola;
                segment.parameters.setReal("chainage", vertical.pvis[element.pvi].station);
                segment.parameters.setReal("height", vertical.pvis[element.pvi].elevation);
            }
            grade.segments.push_back(std::move(segment));
        }
        if (!profile->elements().empty()) {
            const auto& last = profile->elements().back();
            grade.vertices.push_back(
                Vertex{last.startStation + last.length, last.endElevation(), std::nullopt});
        }
    }

    void write(const ExportSurface& surface)
    {
        // The `tin` form: visible triangles only, no neighbours to get wrong.
        // The manual recommends it to "most software packages".
        Tin tin;
        tin.name = surface.name.empty() ? "surface" : surface.name;
        tin.colour = "green";
        const Vec2 shift = options_.originShift.value_or(Vec2{});
        for (const auto& point : surface.surface->vertices()) {
            tin.points.push_back(Vertex{point.x + shift.x, point.y + shift.y, point.z});
        }
        for (const auto& triangle : surface.surface->triangles()) {
            // Counter-clockwise here, clockwise in 12d.
            tin.triangles.push_back({triangle[0], triangle[2], triangle[1]});
        }
        result_.archive.elements.emplace_back(std::move(tin));
        ++result_.surfacesWritten;
    }

    const katana::entity::Model& model_;
    ExportOptions options_;
    DomainExport result_;
    std::size_t dimensions_ = 0;
};

} // namespace

std::vector<std::string> splitList(std::string_view text)
{
    std::vector<std::string> items;
    std::size_t i = 0;
    while (i < text.size()) {
        if (text[i] == ' ' || text[i] == '\t') {
            ++i;
            continue;
        }
        std::string item;
        if (text[i] == '"') {
            ++i;
            while (i < text.size() && text[i] != '"') {
                if (text[i] == '\\' && i + 1 < text.size()) {
                    ++i;
                }
                item += text[i++];
            }
            ++i; // the closing quote, if there was one
        } else {
            while (i < text.size() && text[i] != ' ' && text[i] != '\t') {
                item += text[i++];
            }
        }
        items.push_back(std::move(item));
    }
    return items;
}

std::vector<std::optional<double>> entityHeights(const katana::entity::Entity& entity,
                                                  std::size_t count)
{
    std::vector<std::optional<double>> heights(count);
    if (const auto* list = textOf(entity.properties, kElevationsProperty)) {
        std::vector<std::optional<double>> parsed;
        std::size_t start = 0;
        while (start < list->size()) {
            std::size_t end = list->find(' ', start);
            if (end == std::string::npos) {
                end = list->size();
            }
            if (end > start) {
                // "null", or anything else that is not a number, is no height.
                parsed.push_back(parseReal(std::string_view(*list).substr(start, end - start)));
            }
            start = end + 1;
        }
        if (parsed.size() == count) {
            return parsed;
        }
    }
    if (const auto z = realOf(entity.properties, kElevationProperty)) {
        std::fill(heights.begin(), heights.end(), *z);
    }
    return heights;
}

katana::core::Result<DomainExport> fromDomain(const katana::entity::Model& model,
                                              const std::vector<ExportSurface>& surfaces,
                                              const ExportOptions& options)
{
    if (!(options.curveTolerance > 0.0) || !std::isfinite(options.curveTolerance)) {
        return makeError(ErrorCode::InvalidArgument, "the curve tolerance must be positive");
    }
    if (options.originShift && !(std::isfinite(options.originShift->x) &&
                                 std::isfinite(options.originShift->y))) {
        return makeError(ErrorCode::InvalidArgument, "the origin shift is not finite");
    }
    Exporter exporter(model, options);
    return exporter.run(surfaces);
}

} // namespace katana::archive12d
