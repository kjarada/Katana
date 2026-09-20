#pragma once

// Coordinate reference systems (PLAN.MD Phase 11).
//
// CoordinateReferenceSystem is an immutable VALUE: the definition text handed
// to PROJ plus the metadata PROJ reported for it when it was created. It holds
// no PROJ handle, therefore it is cheap to copy, safe to share between threads
// and can be stored in any container. PROJ objects are re-instantiated from the
// definition where they are needed (CoordinateTransformer, GridFactorCalculator),
// each inside a private PROJ context - see docs/geodesy.md, "Threading model".
//
// Every factory validates its input with PROJ and fails with
// ErrorCode::InvalidCRS (carrying PROJ's own message in Error::context) when
// the text does not describe a CRS. There is no "unknown CRS" placeholder state.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geodesy/ellipsoid.hpp"
#include "katana/geodesy/units.hpp"

namespace katana::geodesy {

enum class CrsKind {
    Geographic2D,
    Geographic3D,
    Geocentric,
    Projected,
    Vertical,
    Compound,
    LocalEngineering, // site grid without geodetic datum
    Other,
};

[[nodiscard]] std::string_view toString(CrsKind kind);

// One axis as declared by the CRS, in the AUTHORITY's order (EPSG:4326 lists
// latitude first). Transformations do not use this order; see coordinate.hpp.
struct AxisInfo {
    std::string name;         // "Easting", "Geodetic latitude", ...
    std::string abbreviation; // "E", "Lat", ...
    std::string direction;    // "east", "north", "up", "south", ...
    std::string unitName;     // "metre", "US survey foot", "degree", ...
    double unitToSi = 1.0;    // metres (or radians) per unit

    friend bool operator==(const AxisInfo&, const AxisInfo&) = default;
};

// Geographic bounding box in degrees. `west` > `east` when the box crosses the
// antimeridian.
struct GeographicExtent {
    double west = 0.0;
    double south = 0.0;
    double east = 0.0;
    double north = 0.0;

    // Boundary inclusive; `longitude` in [-180, 180]. PROJ evaluates a
    // projection wherever its formulas are defined, even far outside the zone
    // it was designed for, so callers that must reject such input compare it
    // with CoordinateReferenceSystem::areaOfUse() using this test.
    [[nodiscard]] constexpr bool contains(double latitude, double longitude) const
    {
        const bool latitudeInside = latitude >= south && latitude <= north;
        const bool longitudeInside = west <= east ? (longitude >= west && longitude <= east)
                                                  : (longitude >= west || longitude <= east);
        return latitudeInside && longitudeInside;
    }

    friend constexpr bool operator==(const GeographicExtent&, const GeographicExtent&) = default;
};

enum class WktVersion {
    Wkt2_2019, // ISO 19162:2019, lossless; the form Katana stores
    Wkt1Gdal,  // legacy consumers (GeoTIFF tooling, old GIS)
    Wkt1Esri,  // .prj side-car files
};

class CoordinateReferenceSystem {
  public:
    // EPSG code of a CRS, e.g. 4326, 32630, 27700. Codes of other EPSG objects
    // (units, datums, transformations) are rejected.
    [[nodiscard]] static core::Result<CoordinateReferenceSystem> fromEpsg(int code);

    // WKT1 (GDAL / ESRI flavours) or WKT2.
    [[nodiscard]] static core::Result<CoordinateReferenceSystem> fromWkt(std::string_view wkt);

    // PROJ string, e.g. "+proj=utm +zone=30 +datum=WGS84". "+type=crs" is
    // appended when absent so that the text is read as a CRS, not as a bare
    // projection operation.
    [[nodiscard]] static core::Result<CoordinateReferenceSystem>
    fromProjString(std::string_view projString);

    // Anything PROJ accepts: "EPSG:32630", "EPSG:4326+3855", "urn:ogc:def:crs:...",
    // WKT, PROJ strings, PROJJSON.
    [[nodiscard]] static core::Result<CoordinateReferenceSystem>
    fromUserInput(std::string_view text);

    // Site-local plane system (easting, northing) with no geodetic datum. It
    // cannot be related to any other CRS by PROJ; relate it to a projected CRS
    // with a site calibration (SimilarityTransform2D). Does not need PROJ's
    // database. Fails with InvalidArgument when `name` is empty.
    [[nodiscard]] static core::Result<CoordinateReferenceSystem>
    localEngineering(std::string_view name, LengthUnit unit);

    [[nodiscard]] const std::string& name() const { return name_; }
    [[nodiscard]] CrsKind kind() const { return kind_; }

    // True when the horizontal coordinates are latitude/longitude, respectively
    // easting/northing. For a Compound CRS this describes its horizontal
    // component; both are false for geocentric, vertical and local systems.
    [[nodiscard]] bool isGeographic() const { return horizontalIsGeographic_; }
    [[nodiscard]] bool isProjected() const { return horizontalIsProjected_; }

    // Identifier declared by the definition ("EPSG", "32630"). Empty when the
    // definition carries none (typical for PROJ strings).
    [[nodiscard]] const std::string& authority() const { return authority_; }
    [[nodiscard]] const std::string& code() const { return code_; }
    [[nodiscard]] std::optional<int> epsgCode() const;

    // Axes in the authority's order; a Compound CRS lists the horizontal axes
    // followed by the vertical axis.
    [[nodiscard]] const std::vector<AxisInfo>& axes() const { return axes_; }

    // Unit of the first (horizontal) axis: angular for geographic, linear for
    // projected / geocentric / local systems.
    [[nodiscard]] const std::string& horizontalUnitName() const { return horizontalUnitName_; }
    [[nodiscard]] double horizontalUnitToSi() const { return horizontalUnitToSi_; }
    // The same unit as a LengthUnit when it is linear and one Katana knows.
    [[nodiscard]] std::optional<LengthUnit> lengthUnit() const { return lengthUnit_; }

    [[nodiscard]] const std::optional<Ellipsoid>& ellipsoid() const { return ellipsoid_; }

    // Region in which the authority declares the CRS valid, when known.
    [[nodiscard]] const std::optional<GeographicExtent>& areaOfUse() const { return areaOfUse_; }

    // The text PROJ instantiates this CRS from ("EPSG:32630", the WKT, ...).
    [[nodiscard]] const std::string& definition() const { return definition_; }

    // Wkt2_2019 is produced once at creation and returned from the cache; the
    // other flavours are exported on demand. Fails with Unsupported when PROJ
    // cannot express the CRS in the requested flavour (e.g. a 3D geographic CRS
    // in WKT1).
    [[nodiscard]] core::Result<std::string> toWkt(WktVersion version = WktVersion::Wkt2_2019) const;

    // Geodetic equivalence as judged by PROJ: same datum, projection parameters
    // and axes, names and identifiers ignored. `ignoreAxisOrder` additionally
    // treats latitude-first and longitude-first geographic CRSs as equivalent.
    [[nodiscard]] core::Result<bool> isEquivalentTo(const CoordinateReferenceSystem& other,
                                                    bool ignoreAxisOrder = false) const;

    // Textual identity of the definition (or of the canonical WKT2). It is exact
    // and cheap but stricter than isEquivalentTo(): "EPSG:32630" and the
    // equivalent PROJ string compare unequal.
    friend bool operator==(const CoordinateReferenceSystem& a, const CoordinateReferenceSystem& b);

  private:
    CoordinateReferenceSystem() = default;

    struct Builder; // defined in the .cpp; fills the fields from a PROJ object

    std::string definition_;
    std::string wkt_;      // canonical single-line WKT2:2019; empty if export failed
    std::string wktError_; // PROJ's reason when wkt_ is empty
    std::string name_;
    std::string authority_;
    std::string code_;
    std::string horizontalUnitName_;
    double horizontalUnitToSi_ = 1.0;
    std::optional<LengthUnit> lengthUnit_;
    std::optional<Ellipsoid> ellipsoid_;
    std::optional<GeographicExtent> areaOfUse_;
    std::vector<AxisInfo> axes_;
    CrsKind kind_ = CrsKind::Other;
    bool horizontalIsGeographic_ = false;
    bool horizontalIsProjected_ = false;
};

// Diagnostics of the PROJ installation the module is running against. Fails
// with NotFound when PROJ cannot open its resource database (proj.db); every
// CRS factory except localEngineering() fails the same way in that situation.
struct ProjRuntimeInfo {
    std::string version;      // "9.7.1"
    std::string databasePath; // resolved location of proj.db
    std::string searchPaths;  // where PROJ looks for proj.db and grid files
};

[[nodiscard]] core::Result<ProjRuntimeInfo> projRuntimeInfo();

} // namespace katana::geodesy
