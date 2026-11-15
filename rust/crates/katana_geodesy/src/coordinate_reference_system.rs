//! Coordinate reference systems.
//!
//! `CoordinateReferenceSystem` is an immutable VALUE: the definition text
//! handed to PROJ plus the metadata PROJ reported for it when it was created.
//! It holds no PROJ handle, therefore it is cheap to copy, safe to share
//! between threads and can be stored in any container. PROJ objects are
//! re-instantiated from the definition where they are needed
//! (`CoordinateTransformer`, `GridFactorCalculator`), each inside a private
//! PROJ context - see docs/geodesy.md, "Threading model".
//!
//! Every factory validates its input with PROJ and fails with
//! `ErrorCode::InvalidCrs` (carrying PROJ's own message in `Error::context`)
//! when the text does not describe a CRS. There is no "unknown CRS"
//! placeholder state.

use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};

use crate::ellipsoid::Ellipsoid;
use crate::ffi;
use crate::proj_internal::{self, PjPtr, ProjContext, abbreviate};
use crate::units::{self, LengthUnit, length_unit_from_metres_per_unit, metres_per_unit};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CrsKind {
    Geographic2D,
    Geographic3D,
    Geocentric,
    Projected,
    Vertical,
    Compound,
    /// Site grid without geodetic datum.
    LocalEngineering,
    Other,
}

pub const fn to_string(kind: CrsKind) -> &'static str {
    match kind {
        CrsKind::Geographic2D => "Geographic2D",
        CrsKind::Geographic3D => "Geographic3D",
        CrsKind::Geocentric => "Geocentric",
        CrsKind::Projected => "Projected",
        CrsKind::Vertical => "Vertical",
        CrsKind::Compound => "Compound",
        CrsKind::LocalEngineering => "LocalEngineering",
        CrsKind::Other => "Other",
    }
}

/// One axis as declared by the CRS, in the AUTHORITY's order (EPSG:4326 lists
/// latitude first). Transformations do not use this order; see the
/// `coordinate` module.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisInfo {
    /// "Easting", "Geodetic latitude", ...
    pub name: String,
    /// "E", "Lat", ...
    pub abbreviation: String,
    /// "east", "north", "up", "south", ...
    pub direction: String,
    /// "metre", "US survey foot", "degree", ...
    pub unit_name: String,
    /// Metres (or radians) per unit.
    pub unit_to_si: f64,
}

/// Geographic bounding box in degrees. `west > east` when the box crosses the
/// antimeridian.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GeographicExtent {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl GeographicExtent {
    /// Boundary inclusive; `longitude` in [-180, 180]. PROJ evaluates a
    /// projection wherever its formulas are defined, even far outside the zone
    /// it was designed for, so callers that must reject such input compare it
    /// with `CoordinateReferenceSystem::area_of_use` using this test.
    pub fn contains(&self, latitude: f64, longitude: f64) -> bool {
        let latitude_inside = (self.south..=self.north).contains(&latitude);
        let longitude_inside = if self.west <= self.east {
            (self.west..=self.east).contains(&longitude)
        } else {
            longitude >= self.west || longitude <= self.east
        };
        latitude_inside && longitude_inside
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WktVersion {
    /// ISO 19162:2019, lossless; the form Katana stores.
    #[default]
    Wkt2_2019,
    /// Legacy consumers (GeoTIFF tooling, old GIS).
    Wkt1Gdal,
    /// .prj side-car files.
    Wkt1Esri,
}

#[derive(Clone, Debug)]
pub struct CoordinateReferenceSystem {
    definition: String,
    // Canonical single-line WKT2:2019; empty if the export failed.
    wkt: String,
    // PROJ's reason when `wkt` is empty.
    wkt_error: String,
    name: String,
    authority: String,
    code: String,
    horizontal_unit_name: String,
    horizontal_unit_to_si: f64,
    length_unit: Option<LengthUnit>,
    ellipsoid: Option<Ellipsoid>,
    area_of_use: Option<GeographicExtent>,
    axes: Vec<AxisInfo>,
    kind: CrsKind,
    horizontal_is_geographic: bool,
    horizontal_is_projected: bool,
}

// ---- helpers ---------------------------------------------------------------------

fn trimmed(text: &str) -> &str {
    text.trim_matches(|ch: char| matches!(ch, ' ' | '\t' | '\r' | '\n'))
}

fn looks_like_proj_string(text: &str) -> bool {
    text.starts_with('+') || text.starts_with("proj=") || text.starts_with("init=")
}

// A PROJ string without "+type=crs" denotes a coordinate OPERATION (radians
// in, metres out), not a CRS. Katana only deals in CRSs here.
fn as_crs_proj_string(text: &str) -> String {
    let mut definition = text.to_owned();
    if !definition.contains("type=crs") {
        definition.push_str(" +type=crs");
    }
    definition
}

// A BoundCRS (a CRS carrying a +towgs84 / +nadgrids hint) is classified by the
// CRS it wraps.
fn unwrap_bound(context: &ProjContext, crs: &PjPtr) -> Option<PjPtr> {
    if proj_internal::get_type(crs) == ffi::PJ_TYPE_BOUND_CRS {
        return proj_internal::get_source_crs(context, crs);
    }
    None
}

fn kind_of(object_type: ffi::PjType) -> CrsKind {
    match object_type {
        ffi::PJ_TYPE_GEOGRAPHIC_2D_CRS => CrsKind::Geographic2D,
        ffi::PJ_TYPE_GEOGRAPHIC_3D_CRS => CrsKind::Geographic3D,
        ffi::PJ_TYPE_GEOCENTRIC_CRS => CrsKind::Geocentric,
        ffi::PJ_TYPE_PROJECTED_CRS | ffi::PJ_TYPE_DERIVED_PROJECTED_CRS => CrsKind::Projected,
        ffi::PJ_TYPE_VERTICAL_CRS => CrsKind::Vertical,
        ffi::PJ_TYPE_COMPOUND_CRS => CrsKind::Compound,
        ffi::PJ_TYPE_ENGINEERING_CRS => CrsKind::LocalEngineering,
        _ => CrsKind::Other,
    }
}

fn append_axes(context: &ProjContext, crs: &PjPtr, axes: &mut Vec<AxisInfo>) {
    let unwrapped = unwrap_bound(context, crs);
    let base = unwrapped.as_ref().unwrap_or(crs);

    if proj_internal::get_type(base) == ffi::PJ_TYPE_COMPOUND_CRS {
        let mut index = 0;
        while let Some(component) = proj_internal::crs_get_sub_crs(context, base, index) {
            append_axes(context, &component, axes);
            index += 1;
        }
        return;
    }

    let Some(system) = proj_internal::crs_get_coordinate_system(context, base) else {
        return;
    };
    let count = proj_internal::cs_get_axis_count(context, &system);
    for index in 0..count {
        if let Some(axis) = proj_internal::cs_get_axis_info(context, &system, index) {
            axes.push(axis);
        }
    }
}

fn proj_wkt_type(version: WktVersion) -> ffi::PjWktType {
    match version {
        WktVersion::Wkt2_2019 => ffi::PJ_WKT2_2019,
        WktVersion::Wkt1Gdal => ffi::PJ_WKT1_GDAL,
        WktVersion::Wkt1Esri => ffi::PJ_WKT1_ESRI,
    }
}

// Single-line output: the canonical form is stored and compared, not read.
fn export_wkt(context: &ProjContext, crs: &PjPtr, version: WktVersion) -> Result<String> {
    context.clear_diagnostics();
    match proj_internal::as_wkt(context, crs, proj_wkt_type(version)) {
        Some(wkt) => Ok(wkt),
        None => Err(Error::new(
            ErrorCode::Unsupported,
            "PROJ cannot express this CRS in the requested WKT flavour",
        )
        .with_context(context.consume_diagnostics())),
    }
}

fn escape_wkt_quotes(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        escaped.push(ch);
        if ch == '"' {
            escaped.push('"'); // WKT doubles an embedded quote
        }
    }
    escaped
}

// std::from_chars into an int: an optional '-' and decimal digits, all of the
// text, in range. Rust's parse also takes a leading '+', which from_chars does
// not.
fn parse_whole_int(text: &str) -> Option<i32> {
    if text.starts_with('+') {
        return None;
    }
    text.parse().ok()
}

// ---- the builder -------------------------------------------------------------------

// Instantiates the definition in a context of its own and reads the CRS PROJ
// makes of it.
fn from_definition(definition: String) -> Result<CoordinateReferenceSystem> {
    let context = ProjContext::create()?;
    let object = proj_internal::create_crs(&context, &definition)?;
    Ok(from_object(&context, &object, definition))
}

fn from_object(
    context: &ProjContext,
    crs: &PjPtr,
    definition: String,
) -> CoordinateReferenceSystem {
    let mut result = CoordinateReferenceSystem::empty();
    result.definition = definition;
    result.name = proj_internal::get_name(crs);

    let unwrapped = unwrap_bound(context, crs);
    let base = unwrapped.as_ref().unwrap_or(crs);
    result.kind = kind_of(proj_internal::get_type(base));
    classify_horizontal(context, base, &mut result);

    // A BoundCRS has no identifier of its own; the CRS it wraps may.
    for candidate in [crs, base] {
        if let Some(authority) = proj_internal::get_id_auth_name(candidate, 0) {
            result.authority = authority;
            result.code = proj_internal::get_id_code(candidate, 0);
            break;
        }
    }

    append_axes(context, base, &mut result.axes);
    if let Some(first) = result.axes.first() {
        result.horizontal_unit_name = first.unit_name.clone();
        result.horizontal_unit_to_si = first.unit_to_si;
        if !result.horizontal_is_geographic {
            result.length_unit = length_unit_from_metres_per_unit(result.horizontal_unit_to_si);
        }
    }

    read_area_of_use(context, crs, &mut result);
    if result.kind != CrsKind::LocalEngineering
        && result.kind != CrsKind::Vertical
        && result.kind != CrsKind::Other
    {
        read_ellipsoid(context, base, &mut result);
    }

    match export_wkt(context, crs, WktVersion::Wkt2_2019) {
        Ok(wkt) => result.wkt = wkt,
        Err(error) => result.wkt_error = error.context,
    }
    context.clear_diagnostics();
    result
}

fn classify_horizontal(
    context: &ProjContext,
    base: &PjPtr,
    result: &mut CoordinateReferenceSystem,
) {
    let mut horizontal = result.kind;
    if result.kind == CrsKind::Compound
        && let Some(component) = proj_internal::crs_get_sub_crs(context, base, 0)
    {
        let unwrapped = unwrap_bound(context, &component);
        horizontal = kind_of(proj_internal::get_type(
            unwrapped.as_ref().unwrap_or(&component),
        ));
    }
    result.horizontal_is_geographic =
        horizontal == CrsKind::Geographic2D || horizontal == CrsKind::Geographic3D;
    result.horizontal_is_projected = horizontal == CrsKind::Projected;
}

fn read_area_of_use(context: &ProjContext, crs: &PjPtr, result: &mut CoordinateReferenceSystem) {
    // PROJ reports -1000 for every bound of an unknown area.
    const UNKNOWN_BOUND: f64 = -1000.0;
    if let Some([west, south, east, north]) = proj_internal::get_area_of_use(context, crs)
        && west != UNKNOWN_BOUND
        && south != UNKNOWN_BOUND
        && east != UNKNOWN_BOUND
        && north != UNKNOWN_BOUND
    {
        result.area_of_use = Some(GeographicExtent {
            west,
            south,
            east,
            north,
        });
    }
}

fn read_ellipsoid(context: &ProjContext, base: &PjPtr, result: &mut CoordinateReferenceSystem) {
    let Some(ellipsoid) = proj_internal::get_ellipsoid(context, base) else {
        return;
    };
    if let Some((semi_major, inverse_flattening)) =
        proj_internal::ellipsoid_parameters(context, &ellipsoid)
    {
        result.ellipsoid = Some(Ellipsoid::new(semi_major, inverse_flattening));
    }
}

// ---- the type ------------------------------------------------------------------------

impl CoordinateReferenceSystem {
    fn empty() -> Self {
        Self {
            definition: String::new(),
            wkt: String::new(),
            wkt_error: String::new(),
            name: String::new(),
            authority: String::new(),
            code: String::new(),
            horizontal_unit_name: String::new(),
            horizontal_unit_to_si: 1.0,
            length_unit: None,
            ellipsoid: None,
            area_of_use: None,
            axes: Vec::new(),
            kind: CrsKind::Other,
            horizontal_is_geographic: false,
            horizontal_is_projected: false,
        }
    }

    /// EPSG code of a CRS, e.g. 4326, 32630, 27700. Codes of other EPSG
    /// objects (units, datums, transformations) are rejected.
    pub fn from_epsg(code: i32) -> Result<CoordinateReferenceSystem> {
        if code <= 0 {
            return Err(
                Error::new(ErrorCode::InvalidCrs, "EPSG codes are positive integers")
                    .with_context(format!("code={code}")),
            );
        }
        from_definition(format!("EPSG:{code}"))
    }

    /// WKT1 (GDAL / ESRI flavours) or WKT2.
    pub fn from_wkt(wkt: &str) -> Result<CoordinateReferenceSystem> {
        let text = trimmed(wkt);
        // Every WKT CRS is KEYWORD[...]; this keeps "EPSG:4326" and PROJ
        // strings, which PROJ would happily accept, out of the WKT entry point.
        if !text.contains(['[', '(']) || looks_like_proj_string(text) {
            return Err(Error::new(ErrorCode::InvalidCrs, "the text is not WKT")
                .with_context(format!("wkt='{}'", abbreviate(text))));
        }
        from_definition(text.to_owned())
    }

    /// PROJ string, e.g. "+proj=utm +zone=30 +datum=WGS84". "+type=crs" is
    /// appended when absent so that the text is read as a CRS, not as a bare
    /// projection operation.
    pub fn from_proj_string(proj_string: &str) -> Result<CoordinateReferenceSystem> {
        let text = trimmed(proj_string);
        if !looks_like_proj_string(text) {
            return Err(Error::new(
                ErrorCode::InvalidCrs,
                "the text is not a PROJ string (expected '+proj=...')",
            )
            .with_context(format!("projString='{}'", abbreviate(text))));
        }
        from_definition(as_crs_proj_string(text))
    }

    /// Anything PROJ accepts: "EPSG:32630", "EPSG:4326+3855",
    /// "urn:ogc:def:crs:...", WKT, PROJ strings, PROJJSON.
    pub fn from_user_input(input: &str) -> Result<CoordinateReferenceSystem> {
        let text = trimmed(input);
        if text.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidCrs,
                "the CRS definition is empty",
            ));
        }
        if looks_like_proj_string(text) {
            return from_definition(as_crs_proj_string(text));
        }
        from_definition(text.to_owned())
    }

    /// Site-local plane system (easting, northing) with no geodetic datum. It
    /// cannot be related to any other CRS by PROJ; relate it to a projected
    /// CRS with a site calibration (`SimilarityTransform2D`). Does not need
    /// PROJ's database. Fails with `InvalidArgument` when `name` is empty.
    pub fn local_engineering(name: &str, unit: LengthUnit) -> Result<CoordinateReferenceSystem> {
        let text = trimmed(name);
        if text.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a local engineering CRS needs a name",
            ));
        }

        let unit_name = units::to_string(unit).to_owned();
        let unit_to_si = metres_per_unit(unit);
        let quoted_name = escape_wkt_quotes(text);
        // The shortest decimal text that parses back to exactly the unit's size.
        let length_unit = format!(
            "LENGTHUNIT[\"{unit_name}\",{}]",
            format_exact_real(unit_to_si)
        );

        let mut result = CoordinateReferenceSystem::empty();
        // Written in the layout PROJ itself exports for an ENGCRS, so that the
        // text survives a round trip through from_wkt().
        result.wkt = format!(
            "ENGCRS[\"{quoted_name}\",EDATUM[\"{quoted_name} datum\"],CS[Cartesian,2],\
             AXIS[\"(E)\",east,ORDER[1],{length_unit}],AXIS[\"(N)\",north,ORDER[2],{length_unit}]]"
        );
        result.definition = result.wkt.clone();
        result.name = text.to_owned();
        result.kind = CrsKind::LocalEngineering;
        result.horizontal_unit_name = unit_name.clone();
        result.horizontal_unit_to_si = unit_to_si;
        result.length_unit = Some(unit);
        result.axes = vec![
            AxisInfo {
                name: String::from("Easting"),
                abbreviation: String::from("E"),
                direction: String::from("east"),
                unit_name: unit_name.clone(),
                unit_to_si,
            },
            AxisInfo {
                name: String::from("Northing"),
                abbreviation: String::from("N"),
                direction: String::from("north"),
                unit_name,
                unit_to_si,
            },
        ];
        Ok(result)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> CrsKind {
        self.kind
    }

    /// True when the horizontal coordinates are latitude/longitude. For a
    /// Compound CRS this describes its horizontal component; false for
    /// geocentric, vertical and local systems.
    pub fn is_geographic(&self) -> bool {
        self.horizontal_is_geographic
    }

    /// True when the horizontal coordinates are easting/northing, as for
    /// `is_geographic`.
    pub fn is_projected(&self) -> bool {
        self.horizontal_is_projected
    }

    /// Identifier declared by the definition ("EPSG", "32630"). Empty when the
    /// definition carries none (typical for PROJ strings).
    pub fn authority(&self) -> &str {
        &self.authority
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn epsg_code(&self) -> Option<i32> {
        if self.authority != "EPSG" || self.code.is_empty() {
            return None;
        }
        parse_whole_int(&self.code)
    }

    /// Axes in the authority's order; a Compound CRS lists the horizontal
    /// axes followed by the vertical axis.
    pub fn axes(&self) -> &[AxisInfo] {
        &self.axes
    }

    /// Unit of the first (horizontal) axis: angular for geographic, linear for
    /// projected / geocentric / local systems.
    pub fn horizontal_unit_name(&self) -> &str {
        &self.horizontal_unit_name
    }

    pub fn horizontal_unit_to_si(&self) -> f64 {
        self.horizontal_unit_to_si
    }

    /// The same unit as a `LengthUnit` when it is linear and one Katana knows.
    pub fn length_unit(&self) -> Option<LengthUnit> {
        self.length_unit
    }

    pub fn ellipsoid(&self) -> Option<Ellipsoid> {
        self.ellipsoid
    }

    /// Region in which the authority declares the CRS valid, when known.
    pub fn area_of_use(&self) -> Option<GeographicExtent> {
        self.area_of_use
    }

    /// The text PROJ instantiates this CRS from ("EPSG:32630", the WKT, ...).
    pub fn definition(&self) -> &str {
        &self.definition
    }

    /// Wkt2_2019 is produced once at creation and returned from the cache;
    /// the other flavours are exported on demand. Fails with `Unsupported`
    /// when PROJ cannot express the CRS in the requested flavour (e.g. a 3D
    /// geographic CRS in WKT1).
    pub fn to_wkt(&self, version: WktVersion) -> Result<String> {
        if version == WktVersion::Wkt2_2019 {
            if self.wkt.is_empty() {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "PROJ could not export this CRS as WKT2:2019",
                )
                .with_context(self.wkt_error.clone()));
            }
            return Ok(self.wkt.clone());
        }
        let context = ProjContext::create()?;
        let object = proj_internal::create_crs(&context, &self.definition)?;
        export_wkt(&context, &object, version)
    }

    /// Geodetic equivalence as judged by PROJ: same datum, projection
    /// parameters and axes, names and identifiers ignored.
    /// `ignore_axis_order` additionally treats latitude-first and
    /// longitude-first geographic CRSs as equivalent.
    pub fn is_equivalent_to(
        &self,
        other: &CoordinateReferenceSystem,
        ignore_axis_order: bool,
    ) -> Result<bool> {
        if self == other {
            return Ok(true);
        }
        let context = ProjContext::create()?;
        let mine = proj_internal::create_crs(&context, &self.definition)?;
        let theirs = proj_internal::create_crs(&context, &other.definition)?;
        let criterion = if ignore_axis_order {
            ffi::PJ_COMP_EQUIVALENT_EXCEPT_AXIS_ORDER_GEOGCRS
        } else {
            ffi::PJ_COMP_EQUIVALENT
        };
        Ok(proj_internal::is_equivalent_to(
            &context, &mine, &theirs, criterion,
        ))
    }
}

/// Textual identity of the definition (or of the canonical WKT2). It is exact
/// and cheap but stricter than `is_equivalent_to`: "EPSG:32630" and the
/// equivalent PROJ string compare unequal.
impl PartialEq for CoordinateReferenceSystem {
    fn eq(&self, other: &Self) -> bool {
        self.definition == other.definition || (!self.wkt.is_empty() && self.wkt == other.wkt)
    }
}

/// Diagnostics of the PROJ installation the crate is running against.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjRuntimeInfo {
    /// "9.8.1".
    pub version: String,
    /// Resolved location of proj.db.
    pub database_path: String,
    /// Where PROJ looks for proj.db and grid files.
    pub search_paths: String,
}

/// Fails with `NotFound` when PROJ cannot open its resource database
/// (proj.db); every CRS factory except `local_engineering` fails the same way
/// in that situation.
pub fn proj_runtime_info() -> Result<ProjRuntimeInfo> {
    let context = ProjContext::create()?;
    let info = proj_internal::library_info();
    Ok(ProjRuntimeInfo {
        version: info.version,
        database_path: proj_internal::database_path(&context),
        search_paths: info.search_path,
    })
}
