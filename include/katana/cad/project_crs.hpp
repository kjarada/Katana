#pragma once

// The project's coordinate system: what it is, how it is set, and which
// systems suit a place (docs/cad.md, "The project's coordinate system").
//
// A project keeps its coordinate system as text in its metadata
// (storage::ProjectMetadata::coordinateSystem): "EPSG:7856", or WKT or a PROJ
// string for a system with no code, or nothing at all for local coordinates.
// Everything that meets the outside world needs it - GIS > Online Data has
// nowhere to put a web map without it, GIS imports reproject into it, the
// sheets print it in the title block - so it is set here, once, through ONE
// undoable step (Document::setCoordinateSystem), from the File menu, the
// status bar, the online data dialog or the command line (CRS ...).
//
// What is accepted is anything PROJ accepts (geodesy::CoordinateReferenceSystem
// ::fromUserInput): an EPSG code with or without its "EPSG:", WKT, a PROJ
// string. It is stored as "EPSG:<code>" whenever the system has one, so two
// spellings of one system - "epsg:7856", "7856", its WKT - are one value.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geodesy/coordinate_reference_system.hpp"

namespace katana::cad {

// A system as a list offers it.
struct CrsChoice {
    std::string id;    // "EPSG:7856"
    std::string name;  // "GDA2020 / MGA zone 56", as the EPSG register names it
    std::string group; // "Australia - GDA2020 MGA", for a list's headings

    friend bool operator==(const CrsChoice&, const CrsChoice&) = default;
};

// What a system is, for a label or a reply.
struct CrsDescription {
    std::string id;   // how the project stores it: "EPSG:7856", or the text given
    std::string name; // "GDA2020 / MGA zone 56"
    std::string kind; // "projected", "geographic 2D", ...
    std::string units; // "metre", "degree", ...
    bool projected = false;
    std::optional<geodesy::GeographicExtent> areaOfUse;
};

// `text` read as a coordinate system and returned as the project stores it:
// "EPSG:<code>" when it has one, else the text itself, trimmed. Empty text is
// empty: local coordinates. InvalidCRS for text that names no system.
[[nodiscard]] core::Result<std::string> normaliseCoordinateSystem(std::string_view text);

// What `text` is. InvalidCRS for empty text or text that names no system.
[[nodiscard]] core::Result<CrsDescription> describeCoordinateSystem(std::string_view text);

// The systems a survey or civil project is usually in, grouped: the MGA zones
// of GDA2020 and GDA94 (49 to 56, the mainland), the national Albers and the
// geographic systems of both, WGS 84 and its UTM zones, Web Mercator, and New
// Zealand's and Great Britain's national grids. Any other is typed.
[[nodiscard]] const std::vector<CrsChoice>& commonCoordinateSystems();

// The common systems whose id, name or group holds every word of `words`,
// case-insensitive, in list order. Every one for no words.
[[nodiscard]] std::vector<CrsChoice> findCoordinateSystems(std::string_view words);

// The systems that suit a place, best first: in and around Australia the
// place's GDA2020 MGA zone, then its GDA94 MGA zone; everywhere the WGS 84 UTM
// zone (north or south); and last WGS 84 itself. InvalidArgument for a
// longitude outside -180..180 or a latitude outside -90..90. Near the poles,
// where no UTM zone is defined (beyond 84 N and 80 S), WGS 84 alone.
[[nodiscard]] core::Result<std::vector<CrsChoice>> suggestCoordinateSystems(double longitude,
                                                                           double latitude);

} // namespace katana::cad
