// GDAL's algorithms and arguments as JSON (schema.hpp).

#include "schema.hpp"

#include <string>
#include <vector>

#include "replies.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
using Json = nlohmann::json;

namespace {

Json scalarJson(const gp::Scalar& value)
{
    return std::visit([](const auto& held) { return Json(held); }, value);
}

Json nullIfEmpty(const std::string& text)
{
    return text.empty() ? Json(nullptr) : Json(text);
}

Json boundJson(const std::optional<gp::Bound>& bound)
{
    if (!bound) {
        return nullptr;
    }
    return Json{{"value", bound->value}, {"inclusive", bound->inclusive}};
}

std::vector<std::string> accepts(const gp::ArgSpec& arg)
{
    std::vector<std::string> ways;
    if (arg.acceptsName) {
        ways.emplace_back("name");
    }
    if (arg.acceptsObject) {
        ways.emplace_back("object");
    }
    return ways;
}

std::vector<std::string> split(const std::string& text)
{
    std::vector<std::string> parts;
    std::size_t start = 0;
    while (start < text.size()) {
        const std::size_t comma = text.find(',', start);
        const std::size_t end = comma == std::string::npos ? text.size() : comma;
        if (end > start) {
            parts.push_back(text.substr(start, end - start));
        }
        start = end + 1;
    }
    return parts;
}

// One value of the argument's type: a list's items, or the scalar itself.
Json elementSchema(const gp::ArgSpec& arg, gp::ArgType type)
{
    Json schema;
    switch (type) {
    case gp::ArgType::Boolean:
        schema["type"] = "boolean";
        break;
    case gp::ArgType::Integer:
    case gp::ArgType::IntegerList:
        schema["type"] = "integer";
        break;
    case gp::ArgType::Real:
    case gp::ArgType::RealList:
        schema["type"] = "number";
        break;
    case gp::ArgType::String:
    case gp::ArgType::StringList:
    case gp::ArgType::Dataset:
    case gp::ArgType::DatasetList:
        schema["type"] = "string";
        if (!arg.choices.empty()) {
            schema["enum"] = arg.choices;
        }
        break;
    }
    if (type == gp::ArgType::Integer || type == gp::ArgType::IntegerList ||
        type == gp::ArgType::Real || type == gp::ArgType::RealList) {
        if (arg.min) {
            schema[arg.min->inclusive ? "minimum" : "exclusiveMinimum"] = arg.min->value;
        }
        if (arg.max) {
            schema[arg.max->inclusive ? "maximum" : "exclusiveMaximum"] = arg.max->value;
        }
    }
    return schema;
}

bool isList(gp::ArgType type)
{
    return type == gp::ArgType::StringList || type == gp::ArgType::IntegerList ||
           type == gp::ArgType::RealList;
}

} // namespace

Json algorithmJson(const gp::AlgorithmInfo& info)
{
    return Json{{"path", info.path},
                {"name", gp::pathText(info.path)},
                {"description", info.description},
                {"aliases", info.aliases},
                {"policy", std::string(gp::toString(info.policy))},
                {"url", info.helpUrl},
                {"container", info.container}};
}

Json argumentJson(const gp::ArgSpec& arg)
{
    Json json{{"name", arg.name},
              {"short_name", nullIfEmpty(arg.shortName)},
              {"aliases", arg.aliases},
              {"type", std::string(gp::toString(arg.type))},
              {"required", arg.required},
              {"positional", arg.positional},
              {"category", arg.category},
              {"default", arg.defaultValue ? scalarJson(*arg.defaultValue) : Json(nullptr)},
              {"choices", arg.choices},
              {"min", boundJson(arg.min)},
              {"max", boundJson(arg.max)},
              {"count",
               {{"min", arg.minCount},
                {"max", arg.maxCount < 0 ? Json(nullptr) : Json(arg.maxCount)}}},
              {"depends_on", arg.dependsOn},
              {"exclusion_group", nullIfEmpty(arg.exclusionGroup)},
              {"dependency_group", nullIfEmpty(arg.dependencyGroup)},
              {"input", arg.isInput},
              {"output", arg.isOutput},
              {"description", arg.description}};
    if (arg.isDataset()) {
        json["dataset"] = Json{{"kinds", split(gp::datasetKindsText(arg.datasetKinds))},
                               {"accepts", accepts(arg)},
                               {"update", arg.datasetUpdate},
                               {"sources", split(sourcesFor(arg))}};
    } else {
        json["dataset"] = nullptr;
    }
    return json;
}

Json argumentsSchema(const gp::AlgorithmSpec& spec)
{
    Json properties = Json::object();
    for (const gp::ArgSpec& arg : spec.args) {
        if (arg.isDataset()) {
            continue;
        }
        Json schema;
        if (isList(arg.type)) {
            schema["type"] = "array";
            schema["items"] = elementSchema(arg, arg.type);
            if (arg.minCount > 0) {
                schema["minItems"] = arg.minCount;
            }
            if (arg.maxCount >= 0) {
                schema["maxItems"] = arg.maxCount;
            }
        } else {
            schema = elementSchema(arg, arg.type);
        }
        if (arg.defaultValue) {
            schema["default"] = scalarJson(*arg.defaultValue);
        }
        schema["description"] = arg.description;
        properties[arg.name] = std::move(schema);
    }
    return Json{{"type", "object"}, {"properties", properties}, {"additionalProperties", false}};
}

Json sourceSchema(unsigned kinds)
{
    // Only the sources an argument of these kinds reads (sourcesFor): a
    // raster input offered the drawing's scope took drawing data the binder
    // then had to refuse. A file is offered to every kind.
    const bool vector = kinds == 0 || (kinds & gp::DatasetKind::Vector) != 0;
    const bool raster = kinds == 0 || (kinds & gp::DatasetKind::Raster) != 0;
    Json properties = Json::object();
    std::vector<std::string> forms;
    if (vector) {
        forms.emplace_back("drawing data by scope ({scope, area?, layers?, only?, where?})");
        properties["scope"] = {{"type", "string"},
                               {"enum", {"selection", "drawing", "area", "layers"}},
                               {"description",
                                "Which drawing data: the selection, the whole drawing, a window "
                                "(area) or named layers (layers)."}};
        properties["area"] = {{"type", "array"},
                              {"items", {{"type", "number"}}},
                              {"minItems", 4},
                              {"maxItems", 4},
                              {"description", "x0, y0, x1, y1, with scope area."}};
        properties["layers"] = {{"type", "array"},
                                {"items", {{"type", "string"}}},
                                {"description", "Layer paths, with scope layers."}};
        properties["only"] = {
            {"type", "boolean"},
            {"description", "With scope layers: those layers only, not the layers beneath."}};
        properties["where"] = {{"type", "array"},
                               {"items", {{"type", "string"}}},
                               {"description",
                                "Filter conditions, as WHERE takes them: TYPE=polyline, "
                                "LAYER=pattern, STYLE=, COLOUR=, PROP=key[:pattern], TEXT=, DRAWN. "
                                "Without scope they filter the selection, as WHERE alone does."}};
    }
    if (raster) {
        forms.emplace_back("a reference raster ({raster})");
        forms.emplace_back("a surface ({surface, cell?})");
        properties["raster"] = {
            {"type", {"integer", "string"}},
            {"description", "A reference raster's id or name (REFS lists them)."}};
        properties["surface"] = {{"type", "string"}, {"description", "A surface's name."}};
        properties["cell"] = {
            {"type", "number"},
            {"exclusiveMinimum", 0},
            {"description", "With surface: the grid cell; suggested from its extent when absent."}};
    }
    forms.emplace_back("a file ({file, layer?})");
    properties["file"] = {{"type", "string"}, {"description", "A file, a /vsi path or a URL."}};
    properties["layer"] = {{"type", "string"}, {"description", "With file: the file's layer."}};
    std::string description = "One source: ";
    for (std::size_t i = 0; i < forms.size(); ++i) {
        description += (i == 0 ? "" : i + 1 == forms.size() ? " or " : ", ") + forms[i];
    }
    return Json{{"type", "object"},
                {"description", description + "."},
                {"properties", std::move(properties)},
                {"additionalProperties", false}};
}

Json inputsSchema(const gp::AlgorithmSpec& spec)
{
    Json properties = Json::object();
    std::vector<std::string> required;
    for (const gp::ArgSpec& arg : spec.args) {
        if (!arg.isDataset() || !arg.isInput || arg.isOutput) {
            continue;
        }
        Json schema;
        if (arg.type == gp::ArgType::DatasetList && arg.maxCount != 1) {
            schema = Json{{"oneOf",
                           {sourceSchema(arg.datasetKinds),
                            Json{{"type", "array"}, {"items", sourceSchema(arg.datasetKinds)}}}}};
        } else {
            schema = sourceSchema(arg.datasetKinds);
        }
        schema["description"] = arg.description + " Accepts " + sourcesFor(arg) + ".";
        properties[arg.name] = std::move(schema);
        if (arg.required) {
            required.push_back(arg.name);
        }
    }
    Json schema{{"type", "object"}, {"properties", properties}, {"additionalProperties", false}};
    if (!required.empty()) {
        schema["x-required"] = required; // bound by FROM or by GDAL's own words
    }
    return schema;
}

Json describeJson(const gp::AlgorithmSpec& spec)
{
    Json algorithm = algorithmJson(spec.info);
    algorithm["long_description"] = spec.longDescription;
    Json arguments = Json::array();
    for (const gp::ArgSpec& arg : spec.args) {
        arguments.push_back(argumentJson(arg));
    }
    Json usage;
    try {
        usage = Json::parse(spec.usageJson);
    } catch (const Json::parse_error&) {
        usage = spec.usageJson;
    }
    return Json{{"algorithm", algorithm},
                {"arguments", arguments},
                {"arguments_schema", argumentsSchema(spec)},
                {"inputs_schema", inputsSchema(spec)},
                {"gdal_usage", usage}};
}

} // namespace katana::app::geo
