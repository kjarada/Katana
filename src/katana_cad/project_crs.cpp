// The project's coordinate system (include/katana/cad/project_crs.hpp), and
// Document::setCoordinateSystem, kept here with it as the sheet set's members
// are kept with the sheets (plotting/sheet_store.cpp).

#include "katana/cad/project_crs.hpp"

#include <cmath>
#include <functional>
#include <memory>
#include <string>
#include <utility>

#include "katana/cad/document.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

std::string trimmed(std::string_view text)
{
    const auto first = text.find_first_not_of(" \t\r\n");
    if (first == std::string_view::npos) {
        return {};
    }
    const auto last = text.find_last_not_of(" \t\r\n");
    return std::string(text.substr(first, last - first + 1));
}

std::string lower(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        c = katana::core::asciiLower(c);
    }
    return out;
}

bool allDigits(std::string_view text)
{
    return !text.empty() &&
           text.find_first_not_of("0123456789") == std::string_view::npos;
}

// PROJ reads "7856" as nothing; a person typing a code means EPSG's.
std::string asProjInput(std::string_view text)
{
    const std::string t = trimmed(text);
    return allDigits(t) ? "EPSG:" + t : t;
}

Result<geodesy::CoordinateReferenceSystem> parse(std::string_view text)
{
    const std::string input = asProjInput(text);
    if (input.empty()) {
        return makeError(ErrorCode::InvalidCRS, "no coordinate system was given");
    }
    auto crs = geodesy::CoordinateReferenceSystem::fromUserInput(input);
    if (!crs) {
        return makeError(ErrorCode::InvalidCRS,
                         "not a coordinate system: give an EPSG code (EPSG:7856), WKT or a PROJ "
                         "string",
                         input);
    }
    return crs;
}

// The kind as a reply writes it: plain lower-case words, whatever spelling
// geodesy's own toString uses for its logs.
std::string kindWords(geodesy::CrsKind kind)
{
    switch (kind) {
    case geodesy::CrsKind::Geographic2D: return "geographic 2D";
    case geodesy::CrsKind::Geographic3D: return "geographic 3D";
    case geodesy::CrsKind::Geocentric: return "geocentric";
    case geodesy::CrsKind::Projected: return "projected";
    case geodesy::CrsKind::Vertical: return "vertical";
    case geodesy::CrsKind::Compound: return "compound";
    case geodesy::CrsKind::LocalEngineering: return "local engineering";
    case geodesy::CrsKind::Other: return "other";
    }
    return "other";
}

CrsChoice choice(int code, std::string name, std::string group)
{
    return CrsChoice{"EPSG:" + std::to_string(code), std::move(name), std::move(group)};
}

// The zone a longitude falls in: 6 degrees each, zone 1 from 180 W.
int utmZone(double longitude)
{
    const int zone = static_cast<int>(std::floor((longitude + 180.0) / 6.0)) + 1;
    return zone > 60 ? 60 : (zone < 1 ? 1 : zone);
}

// The one step that changes the project's coordinate system: the value before
// and after, so undo and redo are exact.
class ProjectValueCommand final : public katana::commands::Command {
  public:
    using Apply = std::function<void(bool after)>;

    ProjectValueCommand(std::string name, Apply apply)
        : name_(std::move(name)), apply_(std::move(apply))
    {
    }

    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const katana::commands::CommandContext&) const override
    {
        return {};
    }
    [[nodiscard]] Status execute(katana::commands::CommandContext&) override
    {
        apply_(true);
        return {};
    }
    [[nodiscard]] Status undo(katana::commands::CommandContext&) override
    {
        apply_(false);
        return {};
    }
    [[nodiscard]] Status redo(katana::commands::CommandContext&) override
    {
        apply_(true);
        return {};
    }

  private:
    std::string name_;
    Apply apply_;
};

} // namespace

Result<std::string> normaliseCoordinateSystem(std::string_view text)
{
    const std::string input = trimmed(text);
    if (input.empty()) {
        return std::string{};
    }
    auto crs = parse(input);
    if (!crs) {
        return crs.error();
    }
    if (const auto code = crs->epsgCode()) {
        return "EPSG:" + std::to_string(*code);
    }
    return input;
}

Result<CrsDescription> describeCoordinateSystem(std::string_view text)
{
    auto crs = parse(text);
    if (!crs) {
        return crs.error();
    }
    auto id = normaliseCoordinateSystem(text);
    if (!id) {
        return id.error();
    }
    CrsDescription description;
    description.id = std::move(*id);
    description.name = crs->name();
    description.kind = kindWords(crs->kind());
    description.units = crs->horizontalUnitName();
    description.projected = crs->isProjected();
    description.areaOfUse = crs->areaOfUse();
    return description;
}

const std::vector<CrsChoice>& commonCoordinateSystems()
{
    static const std::vector<CrsChoice> list = [] {
        std::vector<CrsChoice> out;
        const std::string gda2020 = "Australia - GDA2020 MGA";
        for (int zone = 49; zone <= 56; ++zone) {
            out.push_back(choice(7800 + zone, "GDA2020 / MGA zone " + std::to_string(zone), gda2020));
        }
        const std::string gda94 = "Australia - GDA94 MGA";
        for (int zone = 49; zone <= 56; ++zone) {
            out.push_back(choice(28300 + zone, "GDA94 / MGA zone " + std::to_string(zone), gda94));
        }
        const std::string national = "Australia - national";
        out.push_back(choice(9473, "GDA2020 / Australian Albers", national));
        out.push_back(choice(3577, "GDA94 / Australian Albers", national));
        out.push_back(choice(7844, "GDA2020", national));
        out.push_back(choice(4283, "GDA94", national));
        const std::string world = "World";
        out.push_back(choice(4326, "WGS 84", world));
        out.push_back(choice(3857, "WGS 84 / Pseudo-Mercator", world));
        out.push_back(choice(2193, "NZGD2000 / New Zealand Transverse Mercator 2000", "New Zealand"));
        out.push_back(choice(27700, "OSGB36 / British National Grid", "Great Britain"));
        for (int zone = 1; zone <= 60; ++zone) {
            out.push_back(choice(32600 + zone, "WGS 84 / UTM zone " + std::to_string(zone) + "N",
                                 "World - WGS 84 UTM north"));
        }
        for (int zone = 1; zone <= 60; ++zone) {
            out.push_back(choice(32700 + zone, "WGS 84 / UTM zone " + std::to_string(zone) + "S",
                                 "World - WGS 84 UTM south"));
        }
        return out;
    }();
    return list;
}

std::vector<CrsChoice> findCoordinateSystems(std::string_view words)
{
    std::vector<std::string> wanted;
    std::string word;
    for (const char c : lower(words)) {
        if (c == ' ' || c == '\t' || c == ',') {
            if (!word.empty()) {
                wanted.push_back(std::move(word));
                word.clear();
            }
        } else {
            word.push_back(c);
        }
    }
    if (!word.empty()) {
        wanted.push_back(std::move(word));
    }
    std::vector<CrsChoice> found;
    for (const CrsChoice& entry : commonCoordinateSystems()) {
        const std::string haystack = lower(entry.id + " " + entry.name + " " + entry.group);
        bool all = true;
        for (const std::string& w : wanted) {
            all = all && haystack.find(w) != std::string::npos;
        }
        if (all) {
            found.push_back(entry);
        }
    }
    return found;
}

Result<std::vector<CrsChoice>> suggestCoordinateSystems(double longitude, double latitude)
{
    if (!std::isfinite(longitude) || !std::isfinite(latitude) || longitude < -180.0 ||
        longitude > 180.0 || latitude < -90.0 || latitude > 90.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "a place is a longitude from -180 to 180 and a latitude from -90 to 90");
    }
    std::vector<CrsChoice> out;
    const int zone = utmZone(longitude);
    // Australia and its waters, where MGA is what a survey is in. GDA2020's
    // MGA runs from zone 46 to 59, GDA94's from 48 to 58.
    const bool australia = latitude >= -60.0 && latitude <= -8.0 && longitude >= 90.0;
    if (australia && zone >= 46 && zone <= 59) {
        out.push_back(choice(7800 + zone, "GDA2020 / MGA zone " + std::to_string(zone),
                             "Australia - GDA2020 MGA"));
    }
    if (australia && zone >= 48 && zone <= 58) {
        out.push_back(choice(28300 + zone, "GDA94 / MGA zone " + std::to_string(zone),
                             "Australia - GDA94 MGA"));
    }
    // UTM is defined from 80 S to 84 N.
    if (latitude >= -80.0 && latitude <= 84.0) {
        const bool north = latitude >= 0.0;
        out.push_back(choice((north ? 32600 : 32700) + zone,
                             "WGS 84 / UTM zone " + std::to_string(zone) + (north ? "N" : "S"),
                             north ? "World - WGS 84 UTM north" : "World - WGS 84 UTM south"));
    }
    out.push_back(choice(4326, "WGS 84", "World"));
    return out;
}

Status Document::setCoordinateSystem(std::string_view text, std::string stepName)
{
    auto stored = normaliseCoordinateSystem(text);
    if (!stored) {
        return stored.error();
    }
    if (*stored == metadata_.coordinateSystem) {
        return {}; // nothing changed, and nothing to undo
    }
    auto apply = [this, before = metadata_.coordinateSystem,
                  after = std::move(*stored)](bool toAfter) {
        metadata_.coordinateSystem = toAfter ? after : before;
    };
    return execute(std::make_unique<ProjectValueCommand>(std::move(stepName), std::move(apply)));
}

} // namespace katana::cad
