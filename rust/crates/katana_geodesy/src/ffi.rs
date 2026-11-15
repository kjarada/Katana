//! The part of PROJ's C API (proj.h) and of Karney's geodesic API (geodesic.h,
//! built into libproj) that the geodesy layer calls: exactly those functions,
//! types and constants, written by hand from the headers of PROJ 9.8.1, the
//! toolchain's.
//!
//! No binding crate: as for the GDAL spike (docs/rust.md), the declarations
//! are of exactly what the C++ uses, so nothing is generated and nothing else
//! of PROJ is reachable. Only `proj_internal` calls these; it states why each
//! call is sound.
//!
//! The C enumerations are `int`-sized, so each is an `c_int` alias with its
//! values as constants: a value PROJ returns that Rust does not list must not
//! be undefined behaviour, as it would be in a Rust `enum`.

#![allow(
    unsafe_code,
    reason = "The declarations of PROJ's C API; every call of them is in proj_internal, each with its SAFETY argument."
)]
#![allow(
    dead_code,
    reason = "PROJ makes these values and Rust only reads them or passes them on: their layout is the header's, member for member, read or not."
)]

use std::ffi::{c_char, c_int, c_void};
use std::marker::{PhantomData, PhantomPinned};

/// `PJ`, PROJ's object: a CRS, a coordinate operation, a coordinate system.
#[repr(C)]
pub(crate) struct Pj {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `PJ_CONTEXT`.
#[repr(C)]
pub(crate) struct PjContext {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `PJ_OBJ_LIST`.
#[repr(C)]
pub(crate) struct PjObjList {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `PJ_OPERATION_FACTORY_CONTEXT`.
#[repr(C)]
pub(crate) struct PjOperationFactoryContext {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `PJ_AREA`; only ever passed as NULL.
#[repr(C)]
pub(crate) struct PjArea {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `PJ_XYZT`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct PjXyzt {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
    pub(crate) t: f64,
}

/// `PJ_XY`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct PjXy {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

/// `PJ_LP`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct PjLp {
    pub(crate) lam: f64,
    pub(crate) phi: f64,
}

/// `PJ_COORD`, the union of four doubles. Only the members the C++ reads or
/// writes are named; every member of the C union lies at offset 0 and none is
/// larger than `v`, so the size (32 bytes) and alignment are the header's.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union PjCoord {
    pub(crate) v: [f64; 4],
    pub(crate) xyzt: PjXyzt,
    pub(crate) xy: PjXy,
    pub(crate) lp: PjLp,
}

/// `PJ_FACTORS` (`struct P5_FACTORS`), every member in the header's order.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct PjFactors {
    pub(crate) meridional_scale: f64,
    pub(crate) parallel_scale: f64,
    pub(crate) areal_scale: f64,
    pub(crate) angular_distortion: f64,
    pub(crate) meridian_parallel_angle: f64,
    pub(crate) meridian_convergence: f64,
    pub(crate) tissot_semimajor: f64,
    pub(crate) tissot_semiminor: f64,
    pub(crate) dx_dlam: f64,
    pub(crate) dx_dphi: f64,
    pub(crate) dy_dlam: f64,
    pub(crate) dy_dphi: f64,
}

/// `PJ_INFO`, every member in the header's order.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct PjInfo {
    pub(crate) major: c_int,
    pub(crate) minor: c_int,
    pub(crate) patch: c_int,
    pub(crate) release: *const c_char,
    pub(crate) version: *const c_char,
    pub(crate) searchpath: *const c_char,
    pub(crate) paths: *const *const c_char,
    pub(crate) path_count: usize,
}

/// `struct geod_geodesic`, every member in the header's order. Filled by
/// `geod_init`, read only by the library.
#[repr(C)]
pub(crate) struct GeodGeodesic {
    pub(crate) a: f64,
    pub(crate) f: f64,
    pub(crate) f1: f64,
    pub(crate) e2: f64,
    pub(crate) ep2: f64,
    pub(crate) n: f64,
    pub(crate) b: f64,
    pub(crate) c2: f64,
    pub(crate) etol2: f64,
    pub(crate) a3x: [f64; 6],
    pub(crate) c3x: [f64; 15],
    pub(crate) c4x: [f64; 21],
}

// The layouts above, checked against the header's: four, twelve and fifty-one
// doubles, and on a 64-bit target PJ_INFO's three ints, padding, four pointers
// and a size_t.
const _: () = assert!(std::mem::size_of::<PjCoord>() == 4 * 8);
const _: () = assert!(std::mem::align_of::<PjCoord>() == std::mem::align_of::<f64>());
const _: () = assert!(std::mem::size_of::<PjFactors>() == 12 * 8);
const _: () = assert!(std::mem::size_of::<GeodGeodesic>() == 51 * 8);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<PjInfo>() == 56);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(PjInfo, release) == 16);

/// `PJ_LOG_FUNCTION`.
pub(crate) type PjLogFunction = Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>;

/// `PJ_LOG_LEVEL`.
pub(crate) type PjLogLevel = c_int;
pub(crate) const PJ_LOG_ERROR: PjLogLevel = 1;

/// `PJ_DIRECTION`.
pub(crate) type PjDirection = c_int;
pub(crate) const PJ_FWD: PjDirection = 1;
pub(crate) const PJ_INV: PjDirection = -1;

/// `PJ_TYPE`: the enumerators' positions in proj.h, which assigns none
/// explicitly (PJ_TYPE_UNKNOWN is 0).
pub(crate) type PjType = c_int;
pub(crate) const PJ_TYPE_GEOCENTRIC_CRS: PjType = 10;
pub(crate) const PJ_TYPE_GEOGRAPHIC_2D_CRS: PjType = 12;
pub(crate) const PJ_TYPE_GEOGRAPHIC_3D_CRS: PjType = 13;
pub(crate) const PJ_TYPE_VERTICAL_CRS: PjType = 14;
pub(crate) const PJ_TYPE_PROJECTED_CRS: PjType = 15;
pub(crate) const PJ_TYPE_COMPOUND_CRS: PjType = 16;
pub(crate) const PJ_TYPE_ENGINEERING_CRS: PjType = 18;
pub(crate) const PJ_TYPE_BOUND_CRS: PjType = 19;
pub(crate) const PJ_TYPE_CONVERSION: PjType = 21;
pub(crate) const PJ_TYPE_DERIVED_PROJECTED_CRS: PjType = 28;

/// `PJ_COMPARISON_CRITERION`.
pub(crate) type PjComparisonCriterion = c_int;
pub(crate) const PJ_COMP_EQUIVALENT: PjComparisonCriterion = 1;
pub(crate) const PJ_COMP_EQUIVALENT_EXCEPT_AXIS_ORDER_GEOGCRS: PjComparisonCriterion = 2;

/// `PJ_WKT_TYPE`: PJ_WKT2_2018 and PJ_WKT2_2018_SIMPLIFIED are aliases with
/// explicit values, so the members after them keep counting from 2 and 3.
pub(crate) type PjWktType = c_int;
pub(crate) const PJ_WKT2_2019: PjWktType = 2;
pub(crate) const PJ_WKT1_GDAL: PjWktType = 4;
pub(crate) const PJ_WKT1_ESRI: PjWktType = 5;

/// `PJ_PROJ_STRING_TYPE`.
pub(crate) type PjProjStringType = c_int;
pub(crate) const PJ_PROJ_5: PjProjStringType = 0;

/// `PROJ_GRID_AVAILABILITY_USE`.
pub(crate) type ProjGridAvailabilityUse = c_int;
pub(crate) const PROJ_GRID_AVAILABILITY_USED_FOR_SORTING: ProjGridAvailabilityUse = 0;

/// `PROJ_SPATIAL_CRITERION`.
pub(crate) type ProjSpatialCriterion = c_int;
pub(crate) const PROJ_SPATIAL_CRITERION_PARTIAL_INTERSECTION: ProjSpatialCriterion = 1;

#[link(name = "proj")]
#[allow(clippy::too_many_arguments, reason = "The signatures are PROJ's.")]
unsafe extern "C" {
    // ---- contexts ------------------------------------------------------------
    pub(crate) fn proj_context_create() -> *mut PjContext;
    pub(crate) fn proj_context_destroy(ctx: *mut PjContext) -> *mut PjContext;
    pub(crate) fn proj_context_set_search_paths(
        ctx: *mut PjContext,
        count_paths: c_int,
        paths: *const *const c_char,
    );
    pub(crate) fn proj_context_set_enable_network(ctx: *mut PjContext, enabled: c_int) -> c_int;
    pub(crate) fn proj_context_get_database_path(ctx: *mut PjContext) -> *const c_char;
    pub(crate) fn proj_log_level(ctx: *mut PjContext, log_level: PjLogLevel) -> PjLogLevel;
    pub(crate) fn proj_log_func(ctx: *mut PjContext, app_data: *mut c_void, logf: PjLogFunction);
    pub(crate) fn proj_context_errno(ctx: *mut PjContext) -> c_int;
    pub(crate) fn proj_context_errno_string(ctx: *mut PjContext, err: c_int) -> *const c_char;
    pub(crate) fn proj_info() -> PjInfo;

    // ---- objects and transformation -------------------------------------------
    pub(crate) fn proj_create(ctx: *mut PjContext, definition: *const c_char) -> *mut Pj;
    pub(crate) fn proj_create_crs_to_crs_from_pj(
        ctx: *mut PjContext,
        source_crs: *const Pj,
        target_crs: *const Pj,
        area: *mut PjArea,
        options: *const *const c_char,
    ) -> *mut Pj;
    pub(crate) fn proj_normalize_for_visualization(ctx: *mut PjContext, obj: *const Pj) -> *mut Pj;
    pub(crate) fn proj_destroy(p: *mut Pj) -> *mut Pj;
    pub(crate) fn proj_trans(p: *mut Pj, direction: PjDirection, coord: PjCoord) -> PjCoord;
    pub(crate) fn proj_trans_generic(
        p: *mut Pj,
        direction: PjDirection,
        x: *mut f64,
        sx: usize,
        nx: usize,
        y: *mut f64,
        sy: usize,
        ny: usize,
        z: *mut f64,
        sz: usize,
        nz: usize,
        t: *mut f64,
        st: usize,
        nt: usize,
    ) -> usize;
    pub(crate) fn proj_coord(x: f64, y: f64, z: f64, t: f64) -> PjCoord;
    pub(crate) fn proj_errno(p: *const Pj) -> c_int;
    pub(crate) fn proj_errno_reset(p: *const Pj) -> c_int;
    pub(crate) fn proj_factors(p: *mut Pj, lp: PjCoord) -> PjFactors;

    // ---- ISO 19111 ------------------------------------------------------------
    pub(crate) fn proj_get_type(obj: *const Pj) -> PjType;
    pub(crate) fn proj_is_equivalent_to_with_ctx(
        ctx: *mut PjContext,
        obj: *const Pj,
        other: *const Pj,
        criterion: PjComparisonCriterion,
    ) -> c_int;
    pub(crate) fn proj_is_crs(obj: *const Pj) -> c_int;
    pub(crate) fn proj_get_name(obj: *const Pj) -> *const c_char;
    pub(crate) fn proj_get_id_auth_name(obj: *const Pj, index: c_int) -> *const c_char;
    pub(crate) fn proj_get_id_code(obj: *const Pj, index: c_int) -> *const c_char;
    pub(crate) fn proj_get_area_of_use(
        ctx: *mut PjContext,
        obj: *const Pj,
        out_west_lon_degree: *mut f64,
        out_south_lat_degree: *mut f64,
        out_east_lon_degree: *mut f64,
        out_north_lat_degree: *mut f64,
        out_area_name: *mut *const c_char,
    ) -> c_int;
    pub(crate) fn proj_as_wkt(
        ctx: *mut PjContext,
        obj: *const Pj,
        wkt_type: PjWktType,
        options: *const *const c_char,
    ) -> *const c_char;
    pub(crate) fn proj_as_proj_string(
        ctx: *mut PjContext,
        obj: *const Pj,
        string_type: PjProjStringType,
        options: *const *const c_char,
    ) -> *const c_char;
    pub(crate) fn proj_get_source_crs(ctx: *mut PjContext, obj: *const Pj) -> *mut Pj;

    pub(crate) fn proj_create_operation_factory_context(
        ctx: *mut PjContext,
        authority: *const c_char,
    ) -> *mut PjOperationFactoryContext;
    pub(crate) fn proj_operation_factory_context_destroy(ctx: *mut PjOperationFactoryContext);
    pub(crate) fn proj_operation_factory_context_set_desired_accuracy(
        ctx: *mut PjContext,
        factory_ctx: *mut PjOperationFactoryContext,
        accuracy: f64,
    );
    pub(crate) fn proj_operation_factory_context_set_area_of_interest(
        ctx: *mut PjContext,
        factory_ctx: *mut PjOperationFactoryContext,
        west_lon_degree: f64,
        south_lat_degree: f64,
        east_lon_degree: f64,
        north_lat_degree: f64,
    );
    pub(crate) fn proj_operation_factory_context_set_spatial_criterion(
        ctx: *mut PjContext,
        factory_ctx: *mut PjOperationFactoryContext,
        criterion: ProjSpatialCriterion,
    );
    pub(crate) fn proj_operation_factory_context_set_grid_availability_use(
        ctx: *mut PjContext,
        factory_ctx: *mut PjOperationFactoryContext,
        grid_use: ProjGridAvailabilityUse,
    );
    pub(crate) fn proj_operation_factory_context_set_allow_ballpark_transformations(
        ctx: *mut PjContext,
        factory_ctx: *mut PjOperationFactoryContext,
        allow: c_int,
    );
    pub(crate) fn proj_create_operations(
        ctx: *mut PjContext,
        source_crs: *const Pj,
        target_crs: *const Pj,
        operation_context: *const PjOperationFactoryContext,
    ) -> *mut PjObjList;
    pub(crate) fn proj_list_get_count(result: *const PjObjList) -> c_int;
    pub(crate) fn proj_list_get(
        ctx: *mut PjContext,
        result: *const PjObjList,
        index: c_int,
    ) -> *mut Pj;
    pub(crate) fn proj_list_destroy(result: *mut PjObjList);

    pub(crate) fn proj_crs_get_geodetic_crs(ctx: *mut PjContext, crs: *const Pj) -> *mut Pj;
    pub(crate) fn proj_crs_get_sub_crs(
        ctx: *mut PjContext,
        crs: *const Pj,
        index: c_int,
    ) -> *mut Pj;
    pub(crate) fn proj_crs_get_coordinate_system(ctx: *mut PjContext, crs: *const Pj) -> *mut Pj;
    pub(crate) fn proj_cs_get_axis_count(ctx: *mut PjContext, cs: *const Pj) -> c_int;
    pub(crate) fn proj_cs_get_axis_info(
        ctx: *mut PjContext,
        cs: *const Pj,
        index: c_int,
        out_name: *mut *const c_char,
        out_abbrev: *mut *const c_char,
        out_direction: *mut *const c_char,
        out_unit_conv_factor: *mut f64,
        out_unit_name: *mut *const c_char,
        out_unit_auth_name: *mut *const c_char,
        out_unit_code: *mut *const c_char,
    ) -> c_int;
    pub(crate) fn proj_get_ellipsoid(ctx: *mut PjContext, obj: *const Pj) -> *mut Pj;
    pub(crate) fn proj_ellipsoid_get_parameters(
        ctx: *mut PjContext,
        ellipsoid: *const Pj,
        out_semi_major_metre: *mut f64,
        out_semi_minor_metre: *mut f64,
        out_is_semi_minor_computed: *mut c_int,
        out_inv_flattening: *mut f64,
    ) -> c_int;

    pub(crate) fn proj_coordoperation_is_instantiable(
        ctx: *mut PjContext,
        coordoperation: *const Pj,
    ) -> c_int;
    pub(crate) fn proj_coordoperation_has_ballpark_transformation(
        ctx: *mut PjContext,
        coordoperation: *const Pj,
    ) -> c_int;
    pub(crate) fn proj_coordoperation_requires_per_coordinate_input_time(
        ctx: *mut PjContext,
        coordoperation: *const Pj,
    ) -> c_int;
    pub(crate) fn proj_coordoperation_get_grid_used_count(
        ctx: *mut PjContext,
        coordoperation: *const Pj,
    ) -> c_int;
    pub(crate) fn proj_coordoperation_get_grid_used(
        ctx: *mut PjContext,
        coordoperation: *const Pj,
        index: c_int,
        out_short_name: *mut *const c_char,
        out_full_name: *mut *const c_char,
        out_package_name: *mut *const c_char,
        out_url: *mut *const c_char,
        out_direct_download: *mut c_int,
        out_open_license: *mut c_int,
        out_available: *mut c_int,
    ) -> c_int;
    pub(crate) fn proj_coordoperation_get_accuracy(ctx: *mut PjContext, obj: *const Pj) -> f64;

    // ---- geodesic.h -------------------------------------------------------------
    pub(crate) fn geod_init(g: *mut GeodGeodesic, a: f64, f: f64);
    pub(crate) fn geod_direct(
        g: *const GeodGeodesic,
        lat1: f64,
        lon1: f64,
        azi1: f64,
        s12: f64,
        plat2: *mut f64,
        plon2: *mut f64,
        pazi2: *mut f64,
    );
    pub(crate) fn geod_inverse(
        g: *const GeodGeodesic,
        lat1: f64,
        lon1: f64,
        lat2: f64,
        lon2: f64,
        ps12: *mut f64,
        pazi1: *mut f64,
        pazi2: *mut f64,
    );
}
