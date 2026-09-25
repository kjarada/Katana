#include "katana/archive12d/writer.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstddef>
#include <string_view>

#include "katana/archive12d/reader.hpp"
#include "text_utilities.hpp"

namespace katana::archive12d {

namespace {

using detail::formatExactReal;
using detail::formatHexReal;
using detail::formatReal;

// Flags of a nulling block, and values of the other long lists, per line. 16
// is what 12d Model writes; it keeps a quarter-million flags to lines an
// editor will open.
constexpr std::size_t kValuesPerLine = 16;

[[nodiscard]] bool isPlainWord(std::string_view text)
{
    return !text.empty() && std::all_of(text.begin(), text.end(), [](unsigned char ch) {
        return std::isalnum(ch) != 0 || ch == '_';
    });
}

// The model an element is in; empty for tins and super tins, which belong to
// the project rather than to a model.
template <typename T> [[nodiscard]] const std::string& modelOf(const T& element)
{
    return element.header.model;
}
template <> [[nodiscard]] const std::string& modelOf<Tin>(const Tin&)
{
    static const std::string none;
    return none;
}
template <> [[nodiscard]] const std::string& modelOf<SuperTin>(const SuperTin&)
{
    static const std::string none;
    return none;
}
template <> [[nodiscard]] const std::string& modelOf<Trimesh>(const Trimesh& mesh)
{
    return mesh.model;
}

class Writer {
  public:
    explicit Writer(const WriteOptions& options) : options_(options) {}

    std::string run(const Archive& archive);

  private:
    // ---- text ------------------------------------------------------------------

    void line(std::string_view text)
    {
        out_.append(indent_ * 2, ' ');
        out_ += text;
        out_ += '\n';
    }

    void open(std::string_view keyword)
    {
        line(std::string(keyword) + " {");
        ++indent_;
    }

    void close()
    {
        --indent_;
        line("}");
    }

    [[nodiscard]] std::string real(double value) const
    {
        return formatReal(value, options_.decimalPlaces);
    }

    [[nodiscard]] std::string height(const std::optional<double>& z) const
    {
        return z ? real(*z) : real(nullValue_);
    }

    // A name or a word as the manual requires it: bare when it is purely
    // alphanumeric, in quotes otherwise.
    [[nodiscard]] static std::string word(std::string_view text)
    {
        return isPlainWord(text) ? std::string(text) : quoted(text);
    }

    void pair(std::string_view key, std::string_view value)
    {
        std::string text(key);
        // Values line up in a column, as 12d Model's do; it is only
        // legibility, but these files are read by people.
        if (text.size() < 9) {
            text.append(9 - text.size(), ' ');
        }
        text += ' ';
        text += value;
        line(text);
    }

    void field(const Field& item)
    {
        if (item.value.empty() && !item.quoted) {
            line(item.key); // a keyword that had no value when it was read
        } else if (item.quoted || !(isPlainWord(item.value) || parseReal(item.value))) {
            pair(item.key, quoted(item.value));
        } else {
            pair(item.key, item.value);
        }
    }

    void fields(const FieldList& list)
    {
        for (const Field& item : list.fields()) {
            // A string's own null value, read into its extras so that its
            // heights could be told from its nulls, must NOT be written: every
            // null in this file is written with the ONE value at its head.
            if (item.key == "null_value") {
                continue;
            }
            field(item);
        }
    }

    void fieldBlock(std::string_view keyword, const FieldList& list)
    {
        open(keyword);
        fields(list);
        close();
    }

    void attributes(const AttributeList& list)
    {
        if (list.empty()) {
            return;
        }
        open("attributes");
        attributeBody(list);
        close();
    }

    void attributeBody(const AttributeList& list)
    {
        for (const Attribute& attribute : list) {
            if (const auto* group = std::get_if<AttributeList>(&attribute.value)) {
                open("group");
                pair("name", quoted(attribute.name));
                open("attributes"); // written even when empty: a group is its attributes
                attributeBody(*group);
                close();
                close();
            } else if (const auto* integer = std::get_if<std::int64_t>(&attribute.value)) {
                const std::string type =
                    attribute.declaredType.empty() ? "integer" : attribute.declaredType;
                line(type + " " + quoted(attribute.name) + " " + std::to_string(*integer));
            } else if (const auto* number = std::get_if<double>(&attribute.value)) {
                line("real " + quoted(attribute.name) + " " + formatExactReal(*number));
            } else if (const auto* text = std::get_if<std::string>(&attribute.value)) {
                const std::string type =
                    attribute.declaredType.empty() ? "text" : attribute.declaredType;
                line(type + " " + quoted(attribute.name) + " " + quoted(*text));
            }
        }
    }

    // A long list of short values, several to a line.
    template <typename Range, typename Format>
    void valueBlock(std::string_view keyword, const Range& values, Format format)
    {
        open(keyword);
        std::string text;
        std::size_t onLine = 0;
        for (const auto& value : values) {
            if (onLine != 0) {
                text += ' ';
            }
            text += format(value);
            if (++onLine == kValuesPerLine) {
                line(text);
                text.clear();
                onLine = 0;
            }
        }
        if (onLine != 0) {
            line(text);
        }
        close();
    }

    void booleanBlock(std::string_view keyword, const std::vector<bool>& values)
    {
        if (!values.empty()) {
            valueBlock(keyword, values, [](bool value) { return std::string(value ? "1" : "0"); });
        }
    }

    void textBlock(std::string_view keyword, const std::vector<std::string>& values)
    {
        if (!values.empty()) {
            valueBlock(keyword, values, [](const std::string& value) { return quoted(value); });
        }
    }

    void propertiesList(std::string_view keyword, const std::vector<FieldList>& records)
    {
        if (records.empty()) {
            return;
        }
        open(keyword);
        for (const FieldList& record : records) {
            fieldBlock("properties", record);
        }
        close();
    }

    void attributesList(std::string_view keyword, const std::vector<AttributeList>& records)
    {
        if (records.empty()) {
            return;
        }
        open(keyword);
        for (const AttributeList& record : records) {
            open("attributes"); // one per vertex even when empty: position is identity
            attributeBody(record);
            close();
        }
        close();
    }

    // ---- shared pieces of strings --------------------------------------------------

    void model(const std::string& name)
    {
        if (name == currentModel_) {
            return;
        }
        currentModel_ = name;
        const auto record =
            std::find_if(models_->begin(), models_->end(), [&name](const ModelRecord& candidate) {
                return detail::equalsIgnoringCase(candidate.name, name);
            });
        const bool alreadyDeclared =
            std::find(declaredModels_.begin(), declaredModels_.end(), name) !=
            declaredModels_.end();
        if (record != models_->end() && !alreadyDeclared) {
            // The block form is the only one that can carry attributes
            // (manual 1.4.1b); once is enough.
            declaredModels_.push_back(name);
            out_ += '\n';
            open("model");
            pair("name", quoted(name));
            fields(record->extras);
            attributes(record->attributes);
            close();
            return;
        }
        out_ += '\n';
        line("model " + quoted(name));
    }

    void header(const StringHeader& head)
    {
        pair("name", quoted(head.name));
        pair("chainage", real(head.chainage));
        pair("breakline", head.breakline == Breakline::Point ? "point" : "line");
        pair("colour", word(head.colour));
        pair("style", quoted(head.style));
        fields(head.extras);
        attributes(head.attributes);
    }

    void vertexRows(std::string_view keyword, const std::vector<Vertex>& vertices, bool withZ)
    {
        open(keyword);
        for (const Vertex& vertex : vertices) {
            std::string text = real(vertex.x) + " " + real(vertex.y);
            if (withZ) {
                text += " " + height(vertex.z);
            }
            line(text);
        }
        close();
    }

    // x y z radius bulge: the geometry of the segment leaving each vertex.
    void polylineRows(const std::vector<Vertex>& vertices, const std::vector<Segment>& segments)
    {
        open("data");
        for (std::size_t i = 0; i < vertices.size(); ++i) {
            const Vertex& vertex = vertices[i];
            const bool arc = i < segments.size() && segments[i].kind == SegmentKind::Arc;
            line(real(vertex.x) + " " + real(vertex.y) + " " + height(vertex.z) + " " +
                 (arc ? real(segments[i].radius) : std::string("0")) + " " +
                 (arc && segments[i].major ? "1" : "0"));
        }
        close();
    }

    // radius_data and major_data when every curved segment is an arc, which is
    // the compact form; geometry_data as soon as one is a transition.
    void segmentGeometry(const std::vector<Segment>& segments, bool alwaysGeometryData)
    {
        if (segments.empty()) {
            return;
        }
        const bool onlyArcs = std::all_of(segments.begin(), segments.end(), [](const Segment& s) {
            return s.kind == SegmentKind::Straight || s.kind == SegmentKind::Arc;
        });
        if (onlyArcs && !alwaysGeometryData) {
            valueBlock("radius_data", segments, [this](const Segment& segment) {
                return segment.kind == SegmentKind::Arc ? real(segment.radius) : std::string("0");
            });
            valueBlock("major_data", segments, [](const Segment& segment) {
                return std::string(segment.kind == SegmentKind::Arc && segment.major ? "1" : "0");
            });
            return;
        }
        open("geometry_data");
        for (const Segment& segment : segments) {
            switch (segment.kind) {
            case SegmentKind::Straight:
                line("straight { }");
                break;
            case SegmentKind::Arc:
                line("arc { radius " + real(segment.radius) + " major " +
                     (segment.major ? "1" : "0") + " }");
                break;
            case SegmentKind::Spiral:
                fieldBlock("spiral", segment.parameters);
                break;
            case SegmentKind::Curve:
                fieldBlock("curve", segment.parameters);
                break;
            case SegmentKind::Parabola:
                fieldBlock("parabola", segment.parameters);
                break;
            }
        }
        close();
    }

    // ---- elements --------------------------------------------------------------------

    void write(const VertexString& string)
    {
        model(string.header.model);
        if (string.kind == StringKind::Face || string.kind == StringKind::Interface) {
            writeSimpleVertexString(string);
            return;
        }
        open("string super");
        header(string.header);
        pair("closed", string.closed ? "true" : "false");

        const bool anyHeight = std::any_of(string.vertices.begin(), string.vertices.end(),
                                           [](const Vertex& vertex) { return vertex.z.has_value(); });
        if (anyHeight) {
            vertexRows("data_3d", string.vertices, true);
        } else {
            if (string.constantZ) {
                pair("z", real(*string.constantZ));
            }
            vertexRows("data_2d", string.vertices, false);
        }
        segmentGeometry(string.segments, false);

        if (string.interval) {
            fieldBlock("interval", *string.interval);
        }
        textBlock("colour_data", string.segmentColours);
        textBlock("point_data", string.pointIds);

        if (string.diameter) {
            pair("diameter_value", real(*string.diameter));
        }
        if (!string.diameters.empty()) {
            valueBlock("diameter_data", string.diameters,
                       [this](double value) { return real(value); });
        }
        if (string.culvert) {
            open("culvert_value");
            pair("width", real((*string.culvert)[0]));
            pair("height", real((*string.culvert)[1]));
            close();
        }
        if (!string.culverts.empty()) {
            open("culvert_data");
            for (const auto& culvert : string.culverts) {
                line("properties { width " + real(culvert[0]) + " height " + real(culvert[1]) +
                     " }");
            }
            close();
        }
        if (!string.justify.empty()) {
            pair("justify", word(string.justify));
        }

        const auto flag = [this](std::string_view key, const std::optional<bool>& value) {
            if (value) {
                pair(key, *value ? "1" : "0");
            }
        };
        flag("vertex_tinable_value", string.vertexTinableValue);
        booleanBlock("vertex_tinable_data", string.vertexTinable);
        flag("segment_tinable_value", string.segmentTinableValue);
        booleanBlock("segment_tinable_data", string.segmentTinable);
        flag("vertex_visible_value", string.vertexVisibleValue);
        booleanBlock("vertex_visible_data", string.vertexVisible);
        flag("segment_visible_value", string.segmentVisibleValue);
        booleanBlock("segment_visible_data", string.segmentVisible);

        if (string.vertexTextValue) {
            pair("vertex_text_value", quoted(*string.vertexTextValue));
        }
        textBlock("vertex_text_data", string.vertexText);
        if (string.vertexAnnotation) {
            fieldBlock("vertex_annotate_value", *string.vertexAnnotation);
        }
        propertiesList("vertex_annotate_data", string.vertexAnnotations);
        if (string.segmentTextValue) {
            pair("segment_text_value", quoted(*string.segmentTextValue));
        }
        textBlock("segment_text_data", string.segmentText);
        if (string.segmentAnnotation) {
            fieldBlock("segment_annotate_value", *string.segmentAnnotation);
        }
        propertiesList("segment_annotate_data", string.segmentAnnotations);

        if (string.symbol) {
            fieldBlock("symbol_value", *string.symbol);
        }
        propertiesList("symbol_data", string.symbols);
        attributesList("vertex_attribute_data", string.vertexAttributes);
        attributesList("segment_attribute_data", string.segmentAttributes);
        close();
    }

    // Face and interface strings are current types with a `data` block of
    // their own; they are not super strings and are not written as one.
    void writeSimpleVertexString(const VertexString& string)
    {
        const bool withModes = string.kind == StringKind::Interface;
        open(withModes ? "string interface" : "string face");
        header(string.header);
        open("data");
        for (std::size_t i = 0; i < string.vertices.size(); ++i) {
            const Vertex& vertex = string.vertices[i];
            std::string text = real(vertex.x) + " " + real(vertex.y) + " " + height(vertex.z);
            if (withModes) {
                text += " " + std::to_string(i < string.interfaceModes.size()
                                                 ? string.interfaceModes[i]
                                                 : 0);
            }
            line(text);
        }
        close();
        close();
    }

    void vertexFields(const Vertex& vertex, std::string_view x, std::string_view y,
                      std::string_view z)
    {
        pair(x, real(vertex.x));
        pair(y, real(vertex.y));
        pair(z, height(vertex.z));
    }

    void write(const ArcString& arc)
    {
        model(arc.header.model);
        open("string arc");
        header(arc.header);
        pair("radius", real(arc.radius));
        vertexFields(arc.centre, "xcentre", "ycentre", "zcentre");
        vertexFields(arc.start, "xstart", "ystart", "zstart");
        vertexFields(arc.end, "xend", "yend", "zend");
        close();
    }

    void write(const CircleString& circle)
    {
        model(circle.header.model);
        open(circle.feature ? "string feature" : "string circle");
        header(circle.header);
        pair("radius", real(circle.radius));
        vertexFields(circle.centre, "xcentre", "ycentre", "zcentre");
        close();
    }

    void write(const TextString& text)
    {
        model(text.header.model);
        open("string text");
        header(text.header);
        fields(text.annotation);
        vertexFields(text.position, "x", "y", "z");
        pair("text", quoted(text.text));
        close();
    }

    void write(const PlotFrame& frame)
    {
        model(frame.header.model);
        open("string plot_frame");
        header(frame.header);
        fields(frame.fields);
        close();
    }

    void drainageRecord(std::string_view keyword, const DrainageRecord& record)
    {
        open(keyword);
        fields(record.fields);
        attributes(record.attributes);
        if (!record.vertices.empty()) {
            polylineRows(record.vertices, record.segments);
        }
        close();
    }

    void write(const DrainageString& drainage)
    {
        model(drainage.header.model);
        open("string drainage");
        header(drainage.header);
        if (drainage.outfall) {
            pair("outfall", real(*drainage.outfall));
        }
        if (drainage.flowDirection) {
            pair("flow_direction", std::to_string(*drainage.flowDirection));
        }
        polylineRows(drainage.vertices, drainage.segments);
        for (const DrainageRecord& pit : drainage.pits) {
            drainageRecord(drainage.pitsAreVersion2 ? "pit_v2" : "pit", pit);
        }
        for (const DrainageRecord& pipe : drainage.pipes) {
            drainageRecord("pipe", pipe);
        }
        for (const DrainageRecord& control : drainage.propertyControls) {
            drainageRecord("property_control", control);
        }
        for (const DrainageRecord& connection : drainage.houseConnections) {
            drainageRecord("house_connection", connection);
        }
        close();
    }

    void parts(std::string_view keyword, const std::vector<AlignmentPart>& list)
    {
        if (list.empty()) {
            return;
        }
        open(keyword);
        for (const AlignmentPart& part : list) {
            open(part.kind);
            fields(part.fields);
            attributes(part.attributes);
            close();
        }
        close();
    }

    void solvedGeometry(std::string_view keyword, const SolvedGeometry& geometry)
    {
        open(keyword);
        fields(geometry.header);
        pair("closed", geometry.closed ? "true" : "false");
        if (geometry.interval) {
            fieldBlock("interval", *geometry.interval);
        }
        vertexRows("data_2d", geometry.vertices, false);
        segmentGeometry(geometry.segments, true);
        textBlock("colour_data", geometry.segmentColours);
        close();
    }

    void write(const SuperAlignment& alignment)
    {
        model(alignment.header.model);
        open("string super_alignment");
        header(alignment.header);
        pair("closed", alignment.closed ? "true" : "false");
        if (!alignment.spiralType.empty()) {
            pair("spiral_type", quoted(alignment.spiralType));
        }
        if (alignment.validHorizontal) {
            pair("valid_horizontal", *alignment.validHorizontal ? "true" : "false");
        }
        if (alignment.validVertical) {
            pair("valid_vertical", *alignment.validVertical ? "true" : "false");
        }
        if (alignment.diameter) {
            open("pipe_value");
            pair("diameter", real(*alignment.diameter));
            close();
        }
        if (alignment.pipeLength) {
            pair("length", real(*alignment.pipeLength));
        }
        parts("horizontal_parts", alignment.horizontalParts);
        if (alignment.horizontalData) {
            solvedGeometry("horizontal_data", *alignment.horizontalData);
        }
        parts("vertical_parts", alignment.verticalParts);
        if (alignment.verticalData) {
            solvedGeometry("vertical_data", *alignment.verticalData);
        }
        close();
    }

    void write(const LasCloud& cloud)
    {
        model(cloud.header.model);
        open("string las_cloud_data");
        header(cloud.header);
        const bool reference = !cloud.referenceFile.empty();
        open(reference ? "ref_data" : "data");
        if (!cloud.categories.empty()) {
            valueBlock("categories", cloud.categories,
                       [](bool value) { return std::string(value ? "true" : "false"); });
        }
        if (reference) {
            pair("file_name", quoted(cloud.referenceFile));
        } else {
            pair("format", cloud.format.empty() ? "v12_p0" : cloud.format);
        }
        if (!cloud.range.empty()) {
            fieldBlock("range", cloud.range);
        }
        if (!reference) {
            open("points_" + (cloud.format.empty() ? std::string("v12_p0") : cloud.format));
            const int format = cloud.pointFormat;
            const bool modern = format >= 6;
            const bool hasTime = modern || format == 1 || format == 3 || format == 4 || format == 5;
            const bool hasColour =
                format == 2 || format == 3 || format == 5 || format == 7 || format == 8 ||
                format == 10;
            for (const LasPoint& point : cloud.points) {
                std::string text = "p { x " + real(point.x) + " y " + real(point.y) + " z " +
                                   real(point.z) + " i " + std::to_string(point.intensity) +
                                   " rn " + std::to_string(point.returnNumber) + " rc " +
                                   std::to_string(point.returnCount);
                if (modern) {
                    text += " cf " + std::to_string(point.classificationFlags) + " sc " +
                            std::to_string(point.scannerChannel);
                }
                text += " sd " + std::to_string(point.scanDirection) + " fe " +
                        std::to_string(point.flightLineEdge) + " cl " +
                        std::to_string(point.classification);
                if (modern) {
                    text += " ud " + std::to_string(point.userData) + " sr " +
                            std::to_string(point.scanAngle);
                } else {
                    text += " sr " + std::to_string(point.scanAngle) + " ud " +
                            std::to_string(point.userData);
                }
                text += " id " + std::to_string(point.pointSourceId);
                if (hasTime) {
                    text += " t " + formatExactReal(point.gpsTime);
                }
                if (hasColour) {
                    text += " c " + std::to_string(point.colour);
                }
                if (format == 8 || format == 10) {
                    text += " ir " + std::to_string(point.nearInfrared);
                }
                line(text + " }");
            }
            close();
        }
        close();
        close();
    }

    void write(const Tin& tin)
    {
        // A full_tin's neighbours are mandatory (manual 1.4.7.1). A tin that
        // has none - one Katana built, say - is written in the format that
        // does not need them.
        const bool full = tin.full && tin.neighbours.size() == tin.triangles.size() &&
                          tin.visible.size() == tin.triangles.size();
        out_ += '\n';
        open(full ? "full_tin" : "tin");
        pair("name", quoted(tin.name));
        fields(tin.extras);
        attributes(tin.attributes);

        open("points");
        for (const Vertex& point : tin.points) {
            if (options_.hexFloatTins) {
                line(formatHexReal(point.x) + " " + formatHexReal(point.y) + " " +
                     formatHexReal(point.z.value_or(nullValue_)));
            } else {
                line(real(point.x) + " " + real(point.y) + " " + height(point.z));
            }
        }
        close();

        // A `tin` lists visible triangles only; the colours block follows the
        // same order, so it is filtered alongside.
        std::vector<std::size_t> written;
        for (std::size_t i = 0; i < tin.triangles.size(); ++i) {
            if (full || tin.isSurfaceTriangle(i)) {
                written.push_back(i);
            }
        }
        open("triangles");
        for (const std::size_t i : written) {
            const auto& triangle = tin.triangles[i];
            line(std::to_string(triangle[0] + 1) + " " + std::to_string(triangle[1] + 1) + " " +
                 std::to_string(triangle[2] + 1));
        }
        close();

        if (full) {
            open("neighbours");
            for (const auto& row : tin.neighbours) {
                std::string text;
                for (std::size_t k = 0; k < 3; ++k) {
                    text += (k == 0 ? "" : " ") +
                            std::to_string(row[k] == Tin::kNoNeighbour ? 0u : row[k] + 1);
                }
                line(text);
            }
            close();
            valueBlock("nulling", tin.visible,
                       [](bool visible) { return std::string(visible ? "2" : "1"); });
        }
        if (!tin.colour.empty()) {
            pair("colour", word(tin.colour));
        }
        // Per-triangle colours (manual 1.4.7, both forms), one per triangle
        // written. A list of the wrong length cannot be lined up and is left
        // out; the reader said so when it read it.
        if (tin.colours.size() == tin.triangles.size()) {
            std::vector<std::string> colours;
            colours.reserve(written.size());
            for (const std::size_t i : written) {
                colours.push_back(tin.colours[i]);
            }
            valueBlock("colours", colours, [](const std::string& colour) {
                return colour == "-1" ? colour : word(colour);
            });
        }
        if (!tin.input.empty() || !tin.inputModels.empty()) {
            open("input");
            fields(tin.input);
            textBlock("models", tin.inputModels);
            close();
        }
        close();
    }

    void write(const SuperTin& superTin)
    {
        out_ += '\n';
        open("super_tin");
        pair("name", quoted(superTin.name));
        fields(superTin.extras);
        attributes(superTin.attributes);
        if (!superTin.colour.empty()) {
            pair("colour", word(superTin.colour));
        }
        open("tins"); // mandatory, so written even when empty
        for (const std::string& name : superTin.tins) {
            line(quoted(name));
        }
        close();
        close();
    }

    void infos(std::string_view keyword, const std::vector<TrimeshInfo>& list)
    {
        if (list.empty()) {
            return;
        }
        open(keyword);
        for (const TrimeshInfo& info : list) {
            line(std::to_string(info.flag) + " " + std::to_string(info.key) + " " +
                 word(info.colour) + " " + quoted(info.name));
        }
        close();
    }

    void write(const Trimesh& mesh)
    {
        model(mesh.model);
        open("primitive_3d");
        pair("name", quoted(mesh.name));
        pair("colour", word(mesh.colour));
        fields(mesh.extras);
        attributes(mesh.attributes);
        open("trimesh_3d");
        if (!mesh.info.empty()) {
            fieldBlock("info", mesh.info);
        }
        vertexRows("vertices", mesh.vertices, true);
        open("faces");
        for (const auto& face : mesh.faces) {
            line(std::to_string(face[0] + 1) + " " + std::to_string(face[1] + 1) + " " +
                 std::to_string(face[2] + 1));
        }
        close();
        if (!mesh.edges.empty()) {
            open("edges");
            for (const auto& edge : mesh.edges) {
                line(std::to_string(edge[0] + 1) + " " + std::to_string(edge[1] + 1));
            }
            close();
        }
        if (mesh.blend) {
            pair("blend", real(*mesh.blend));
        }
        const auto flags = [this](std::string_view keyword,
                                  const std::vector<std::uint32_t>& values) {
            if (!values.empty()) {
                valueBlock(keyword, values,
                           [](std::uint32_t value) { return std::to_string(value); });
            }
        };
        infos("vertex_infos", mesh.vertexInfos);
        flags("vertex_flags", mesh.vertexFlags);
        infos("edge_infos", mesh.edgeInfos);
        flags("edge_flags", mesh.edgeFlags);
        infos("face_infos", mesh.faceInfos);
        flags("face_flags", mesh.faceFlags);
        close();
        close();
    }

    WriteOptions options_;
    std::string out_;
    std::size_t indent_ = 0;
    double nullValue_ = -999.0;
    std::string currentModel_;
    const std::vector<ModelRecord>* models_ = nullptr;
    std::vector<std::string> declaredModels_;
};

// The null value the file is written with. It must equal no real height in
// the archive: a height read under one null value and written under another
// would otherwise come back as null - silently, from a plain read and write.
// The archive's own value is kept when it is safe; failing that, sentinels
// far below any elevation on Earth (the Mariana Trench is -11 km) are tried.
[[nodiscard]] double safeNullValue(const Archive& archive)
{
    std::vector<double> heights;
    const auto collectAll = [&heights](const std::vector<Vertex>& vertices) {
        for (const Vertex& vertex : vertices) {
            if (vertex.z) {
                heights.push_back(*vertex.z);
            }
        }
    };
    const auto collectOne = [&heights](const std::optional<double>& z) {
        if (z) {
            heights.push_back(*z);
        }
    };
    struct Visitor {
        decltype(collectAll)& collect;
        decltype(collectOne)& one;
        void operator()(const VertexString& s) const
        {
            collect(s.vertices);
            one(s.constantZ);
        }
        void operator()(const ArcString& a) const
        {
            one(a.centre.z);
            one(a.start.z);
            one(a.end.z);
        }
        void operator()(const CircleString& c) const { one(c.centre.z); }
        void operator()(const TextString& t) const { one(t.position.z); }
        void operator()(const PlotFrame&) const {}
        void operator()(const DrainageString& d) const
        {
            collect(d.vertices);
            for (const DrainageRecord& control : d.propertyControls) {
                collect(control.vertices);
            }
        }
        void operator()(const SuperAlignment&) const {}
        void operator()(const LasCloud&) const {}
        void operator()(const Tin& t) const { collect(t.points); }
        void operator()(const SuperTin&) const {}
        void operator()(const Trimesh& m) const { collect(m.vertices); }
    };
    for (const Element& element : archive.elements) {
        std::visit(Visitor{collectAll, collectOne}, element);
    }
    std::sort(heights.begin(), heights.end());
    const auto collides = [&heights](double candidate) {
        return std::binary_search(heights.begin(), heights.end(), candidate);
    };
    if (!collides(archive.nullValue)) {
        return archive.nullValue;
    }
    for (const double candidate : {-999.0, -9999.0, -99999.0, -999999.0, -9999999.0}) {
        if (!collides(candidate)) {
            return candidate;
        }
    }
    // Every sentinel is a real height somewhere: take the first free integer
    // below the lowest of them, which no survey reaches.
    double candidate = std::floor(heights.front()) - 1.0;
    while (collides(candidate)) {
        candidate -= 1.0;
    }
    return candidate;
}

std::string Writer::run(const Archive& archive)
{
    nullValue_ = safeNullValue(archive);
    models_ = &archive.models;

    std::size_t start = 0;
    while (start <= options_.banner.size() && !options_.banner.empty()) {
        const std::size_t end = options_.banner.find('\n', start);
        line("// " + options_.banner.substr(start, end == std::string::npos ? end : end - start));
        if (end == std::string::npos) {
            break;
        }
        start = end + 1;
    }
    for (const Field& setting : archive.headerSettings) {
        line("// " + setting.key + " " + (setting.quoted ? quoted(setting.value) : setting.value));
    }
    out_ += '\n';
    // The null VALUE, not the `null` keyword 12d Model 15 also accepts: the
    // value is the documented form (manual 1.4.5) and every version reads it.
    line("null " + real(nullValue_));

    if (!archive.projectAttributes.empty()) {
        out_ += '\n';
        open("project_attributes");
        attributeBody(archive.projectAttributes);
        close();
    }
    // A model that holds nothing still exists, and a model declared with
    // attributes must keep them; both are written even if no element follows.
    for (const ModelRecord& record : archive.models) {
        model(record.name);
    }
    for (const std::string& name : archive.modelNames) {
        const bool used = std::any_of(
            archive.elements.begin(), archive.elements.end(), [&name](const Element& element) {
                return std::visit(
                    [&name](const auto& value) { return detail::equalsIgnoringCase(modelOf(value), name); },
                    element);
            });
        if (!used) {
            model(name);
        }
    }
    for (const Element& element : archive.elements) {
        if (!std::holds_alternative<Tin>(element) && !std::holds_alternative<SuperTin>(element)) {
            out_ += '\n';
        }
        std::visit([this](const auto& value) { write(value); }, element);
    }
    return std::move(out_);
}

} // namespace

std::string quoted(std::string_view text)
{
    std::string out;
    out.reserve(text.size() + 2);
    out += '"';
    for (const char ch : text) {
        if (ch == '"' || ch == '\\') {
            out += '\\';
        }
        out += ch;
    }
    out += '"';
    return out;
}

std::string writeArchive(const Archive& archive, const WriteOptions& options)
{
    Writer writer(options);
    return writer.run(archive);
}

} // namespace katana::archive12d
