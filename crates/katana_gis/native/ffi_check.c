/*
 * The check of the GDAL declarations in src/ffi/ (cpl.rs, minixml.rs,
 * gdal.rs, ogr.rs, osr.rs), compiled by build.rs against the headers of the
 * GDAL the crate links. It is no code: every line is a _Static_assert, so a
 * header that changes, or a declaration transcribed wrongly, fails the build
 * here rather than misbehaving at run time.
 *
 *   - Each function the Rust declares, by its type: the Rust signature
 *     written in the header's own typedefs, compared with the header's
 *     declaration by _Generic (C11 6.5.1.1). A parameter's top-level const
 *     is no part of a function's type (C11 6.7.6.3), as in the header's
 *     OCTTransformBounds.
 *   - Each type the Rust takes for another: every enumeration and OGRErr an
 *     int's size (the Rust has a c_int for each), GIntBig 64 bits, bool one
 *     byte.
 *   - Each #[repr(C)] layout's size and offsets (CPLXMLNode, GDALColorEntry),
 *     and the platform's VSIStatBufL, read by offset (cpl.rs).
 *   - Each constant's value. A text constant (a capability's name) is held
 *     by its length: C has no constant comparison of text.
 *
 * The numbers here are the Rust's: change one, change both.
 */

/* What g++ defines for the C++, so the check sees the declarations it does:
 * glibc declares struct stat64 (its VSIStatBufL) only for large-file builds,
 * and S_IFMT and S_IFDIR only beyond strict ISO C. */
#define _GNU_SOURCE 1

#include <stdbool.h>
#include <stddef.h>
#include <sys/stat.h>

#include <cpl_conv.h>
#include <cpl_error.h>
#include <cpl_minixml.h>
#include <cpl_progress.h>
#include <cpl_string.h>
#include <cpl_vsi.h>
#include <gdal.h>
#include <gdal_utils.h>
#include <ogr_api.h>
#include <ogr_core.h>
#include <ogr_srs_api.h>

#define KATANA_SAME(value, expected) _Static_assert((value) == (expected), #value " is " #expected)
#define KATANA_DECLARED(function, type)                                                            \
    _Static_assert(_Generic(&(function), type: 1, default: 0), #function " is " #type)

/* ---- the types the Rust takes for others ------------------------------------------------- */

KATANA_SAME(sizeof(CPLErr), sizeof(int));
KATANA_SAME(sizeof(CPLErrorNum), sizeof(int));
KATANA_SAME(sizeof(CPLXMLNodeType), sizeof(int));
KATANA_SAME(sizeof(GDALDataType), sizeof(int));
KATANA_SAME(sizeof(GDALRWFlag), sizeof(int));
KATANA_SAME(sizeof(GDALColorInterp), sizeof(int));
KATANA_SAME(sizeof(GDALRIOResampleAlg), sizeof(int));
KATANA_SAME(sizeof(OGRErr), sizeof(int));
KATANA_SAME(sizeof(OGRwkbGeometryType), sizeof(int));
KATANA_SAME(sizeof(OGRFieldType), sizeof(int));
KATANA_SAME(sizeof(OGRFieldSubType), sizeof(int));
KATANA_SAME(sizeof(OSRAxisMappingStrategy), sizeof(int));
KATANA_SAME(sizeof(GIntBig), 8);
KATANA_SAME(sizeof(bool), 1);
KATANA_SAME(sizeof(short), 2);

/* ---- layouts -------------------------------------------------------------------------------- */

KATANA_SAME(sizeof(CPLXMLNode), 4 * sizeof(void *));
KATANA_SAME(offsetof(CPLXMLNode, eType), 0);
KATANA_SAME(offsetof(CPLXMLNode, pszValue), sizeof(void *));
KATANA_SAME(offsetof(CPLXMLNode, psNext), 2 * sizeof(void *));
KATANA_SAME(offsetof(CPLXMLNode, psChild), 3 * sizeof(void *));

KATANA_SAME(sizeof(GDALColorEntry), 8);
KATANA_SAME(offsetof(GDALColorEntry, c1), 0);
KATANA_SAME(offsetof(GDALColorEntry, c2), 2);
KATANA_SAME(offsetof(GDALColorEntry, c3), 4);
KATANA_SAME(offsetof(GDALColorEntry, c4), 6);

/* cpl.rs, stat_layout: the size, and st_mode's and st_size's offset and size. */
#if defined(__linux__) && defined(__x86_64__)
#define KATANA_STAT_SIZE 144
#define KATANA_ST_MODE_OFFSET 24
#define KATANA_ST_MODE_SIZE 4
#define KATANA_ST_SIZE_OFFSET 48
#elif defined(__linux__) && defined(__aarch64__)
#define KATANA_STAT_SIZE 128
#define KATANA_ST_MODE_OFFSET 16
#define KATANA_ST_MODE_SIZE 4
#define KATANA_ST_SIZE_OFFSET 48
#elif defined(__APPLE__)
#define KATANA_STAT_SIZE 144
#define KATANA_ST_MODE_OFFSET 4
#define KATANA_ST_MODE_SIZE 2
#define KATANA_ST_SIZE_OFFSET 96
#elif defined(_WIN32)
#define KATANA_STAT_SIZE 56
#define KATANA_ST_MODE_OFFSET 6
#define KATANA_ST_MODE_SIZE 2
#define KATANA_ST_SIZE_OFFSET 24
#else
#error "VSIStatBufL's layout is written for Linux (x86_64, aarch64), macOS and Windows only"
#endif
KATANA_SAME(sizeof(VSIStatBufL), KATANA_STAT_SIZE);
KATANA_SAME(offsetof(VSIStatBufL, st_mode), KATANA_ST_MODE_OFFSET);
KATANA_SAME(sizeof(((VSIStatBufL *)0)->st_mode), KATANA_ST_MODE_SIZE);
KATANA_SAME(offsetof(VSIStatBufL, st_size), KATANA_ST_SIZE_OFFSET);
KATANA_SAME(sizeof(((VSIStatBufL *)0)->st_size), 8);
KATANA_SAME(S_IFMT, 0170000);
KATANA_SAME(S_IFDIR, 0040000);

/* ---- constants ------------------------------------------------------------------------------ */

KATANA_SAME(TRUE, 1);
KATANA_SAME(FALSE, 0);

KATANA_SAME(CE_None, 0);
KATANA_SAME(CE_Debug, 1);
KATANA_SAME(CE_Warning, 2);
KATANA_SAME(CE_Failure, 3);
KATANA_SAME(CE_Fatal, 4);

KATANA_SAME(CXT_Element, 0);
KATANA_SAME(CXT_Text, 1);
KATANA_SAME(CXT_Attribute, 2);
KATANA_SAME(CXT_Comment, 3);
KATANA_SAME(CXT_Literal, 4);

KATANA_SAME(GDT_Unknown, 0);
KATANA_SAME(GDT_Byte, 1);
KATANA_SAME(GDT_Float64, 7);
KATANA_SAME(GF_Read, 0);
KATANA_SAME(GF_Write, 1);
KATANA_SAME(GCI_GrayIndex, 1);
KATANA_SAME(GCI_PaletteIndex, 2);
KATANA_SAME(GCI_RedBand, 3);
KATANA_SAME(GCI_GreenBand, 4);
KATANA_SAME(GCI_BlueBand, 5);
KATANA_SAME(GCI_AlphaBand, 6);
KATANA_SAME(GRIORA_NearestNeighbour, 0);
KATANA_SAME(GRIORA_Bilinear, 1);
KATANA_SAME(GRIORA_Cubic, 2);
KATANA_SAME(GRIORA_CubicSpline, 3);
KATANA_SAME(GDAL_OF_READONLY, 0x00);
KATANA_SAME(GDAL_OF_UPDATE, 0x01);
KATANA_SAME(GDAL_OF_RASTER, 0x02);
KATANA_SAME(GDAL_OF_VECTOR, 0x04);

KATANA_SAME(sizeof(GDAL_DCAP_OPEN), sizeof("DCAP_OPEN"));
KATANA_SAME(sizeof(GDAL_DCAP_CREATE), sizeof("DCAP_CREATE"));
KATANA_SAME(sizeof(GDAL_DCAP_CREATECOPY), sizeof("DCAP_CREATECOPY"));
KATANA_SAME(sizeof(GDAL_DCAP_VIRTUALIO), sizeof("DCAP_VIRTUALIO"));
KATANA_SAME(sizeof(GDAL_DCAP_RASTER), sizeof("DCAP_RASTER"));
KATANA_SAME(sizeof(GDAL_DCAP_VECTOR), sizeof("DCAP_VECTOR"));
KATANA_SAME(sizeof(GDAL_DCAP_CREATE_LAYER), sizeof("DCAP_CREATE_LAYER"));
KATANA_SAME(sizeof(GDAL_DCAP_CREATE_FIELD), sizeof("DCAP_CREATE_FIELD"));
KATANA_SAME(sizeof(GDAL_DMD_LONGNAME), sizeof("DMD_LONGNAME"));
KATANA_SAME(sizeof(GDAL_DMD_HELPTOPIC), sizeof("DMD_HELPTOPIC"));
KATANA_SAME(sizeof(GDAL_DMD_EXTENSION), sizeof("DMD_EXTENSION"));
KATANA_SAME(sizeof(GDAL_DMD_CONNECTION_PREFIX), sizeof("DMD_CONNECTION_PREFIX"));
KATANA_SAME(sizeof(GDAL_DMD_EXTENSIONS), sizeof("DMD_EXTENSIONS"));
KATANA_SAME(sizeof(GDAL_DMD_CREATIONOPTIONLIST), sizeof("DMD_CREATIONOPTIONLIST"));
KATANA_SAME(sizeof(GDAL_DMD_OPENOPTIONLIST), sizeof("DMD_OPENOPTIONLIST"));
KATANA_SAME(sizeof(GDAL_DMD_CREATIONDATATYPES), sizeof("DMD_CREATIONDATATYPES"));
KATANA_SAME(sizeof(GDAL_DMD_CREATIONFIELDDATATYPES), sizeof("DMD_CREATIONFIELDDATATYPES"));
KATANA_SAME(sizeof(GDAL_DMD_CREATIONFIELDDATASUBTYPES), sizeof("DMD_CREATIONFIELDDATASUBTYPES"));
KATANA_SAME(sizeof(GDAL_DS_LAYER_CREATIONOPTIONLIST), sizeof("DS_LAYER_CREATIONOPTIONLIST"));

KATANA_SAME(OGRERR_NONE, 0);
KATANA_SAME(wkbUnknown, 0);
KATANA_SAME(wkbPoint, 1);
KATANA_SAME(wkbLineString, 2);
KATANA_SAME(wkbPolygon, 3);
KATANA_SAME(wkbMultiPoint, 4);
KATANA_SAME(wkbMultiLineString, 5);
KATANA_SAME(wkbMultiPolygon, 6);
KATANA_SAME(wkbGeometryCollection, 7);
KATANA_SAME(wkbCircularString, 8);
KATANA_SAME(wkbCompoundCurve, 9);
KATANA_SAME(wkbCurvePolygon, 10);
KATANA_SAME(wkbMultiCurve, 11);
KATANA_SAME(wkbMultiSurface, 12);
KATANA_SAME(wkbPolyhedralSurface, 15);
KATANA_SAME(wkbTIN, 16);
KATANA_SAME(wkbTriangle, 17);
KATANA_SAME(wkbLinearRing, 101);
KATANA_SAME(OFTInteger, 0);
KATANA_SAME(OFTReal, 2);
KATANA_SAME(OFTString, 4);
KATANA_SAME(OFTDate, 9);
KATANA_SAME(OFTDateTime, 11);
KATANA_SAME(OFTInteger64, 12);
KATANA_SAME(OFSTNone, 0);
KATANA_SAME(OFSTBoolean, 1);
KATANA_SAME(sizeof(OLCCreateField), sizeof("CreateField"));
KATANA_SAME(sizeof(ODsCCreateLayer), sizeof("CreateLayer"));
KATANA_SAME(sizeof(ODsCTransactions), sizeof("Transactions"));

KATANA_SAME(OAMS_TRADITIONAL_GIS_ORDER, 0);

/* ---- cpl.rs ---------------------------------------------------------------------------------- */

KATANA_DECLARED(CPLGetConfigOption, const char *(*)(const char *, const char *));
KATANA_DECLARED(CPLSetConfigOption, void (*)(const char *, const char *));
KATANA_DECLARED(CPLGetThreadLocalConfigOption, const char *(*)(const char *, const char *));
KATANA_DECLARED(CPLSetThreadLocalConfigOption, void (*)(const char *, const char *));
KATANA_DECLARED(CPLTestBool, bool (*)(const char *));
KATANA_DECLARED(CPLFetchBool, bool (*)(CSLConstList, const char *, bool));
KATANA_DECLARED(CSLDestroy, void (*)(char **));
KATANA_DECLARED(CPLSetErrorHandler, CPLErrorHandler (*)(CPLErrorHandler));
KATANA_DECLARED(CPLQuietErrorHandler, void (*)(CPLErr, CPLErrorNum, const char *));
KATANA_DECLARED(CPLPushErrorHandler, void (*)(CPLErrorHandler));
KATANA_DECLARED(CPLPushErrorHandlerEx, void (*)(CPLErrorHandler, void *));
KATANA_DECLARED(CPLPopErrorHandler, void (*)(void));
KATANA_DECLARED(CPLGetErrorHandlerUserData, void *(*)(void));
KATANA_DECLARED(CPLErrorReset, void (*)(void));
KATANA_DECLARED(CPLGetLastErrorMsg, const char *(*)(void));
KATANA_DECLARED(VSIFree, void (*)(void *));
KATANA_DECLARED(VSIReadDirRecursive, char **(*)(const char *));
KATANA_DECLARED(VSIStatL, int (*)(const char *, VSIStatBufL *));
KATANA_DECLARED(VSIFOpenL, VSILFILE *(*)(const char *, const char *));
KATANA_DECLARED(VSIFReadL, size_t (*)(void *, size_t, size_t, VSILFILE *));
KATANA_DECLARED(VSIFWriteL, size_t (*)(const void *, size_t, size_t, VSILFILE *));
KATANA_DECLARED(VSIFCloseL, int (*)(VSILFILE *));

/* ---- minixml.rs ------------------------------------------------------------------------------ */

KATANA_DECLARED(CPLParseXMLString, CPLXMLNode *(*)(const char *));
KATANA_DECLARED(CPLDestroyXMLNode, void (*)(CPLXMLNode *));
KATANA_DECLARED(CPLGetXMLValue, const char *(*)(const CPLXMLNode *, const char *, const char *));

/* ---- gdal.rs --------------------------------------------------------------------------------- */

KATANA_DECLARED(GDALAllRegister, void (*)(void));
KATANA_DECLARED(GDALOpenEx, GDALDatasetH (*)(const char *, unsigned int, const char *const *,
                                             const char *const *, const char *const *));
KATANA_DECLARED(GDALClose, CPLErr (*)(GDALDatasetH));
KATANA_DECLARED(GDALGetDriverByName, GDALDriverH (*)(const char *));
KATANA_DECLARED(GDALGetDriverCount, int (*)(void));
KATANA_DECLARED(GDALGetDriver, GDALDriverH (*)(int));
KATANA_DECLARED(GDALIdentifyDriverEx, GDALDriverH (*)(const char *, unsigned int,
                                                      const char *const *, const char *const *));
KATANA_DECLARED(GDALGetOutputDriversForDatasetName, char **(*)(const char *, int, bool, bool));
KATANA_DECLARED(GDALCreate, GDALDatasetH (*)(GDALDriverH, const char *, int, int, int, GDALDataType,
                                             CSLConstList));
KATANA_DECLARED(GDALCreateCopy, GDALDatasetH (*)(GDALDriverH, const char *, GDALDatasetH, int,
                                                 CSLConstList, GDALProgressFunc, void *));
KATANA_DECLARED(GDALDeleteDataset, CPLErr (*)(GDALDriverH, const char *));
KATANA_DECLARED(GDALGetDatasetDriver, GDALDriverH (*)(GDALDatasetH));
KATANA_DECLARED(GDALVersionInfo, const char *(*)(const char *));
KATANA_DECLARED(GDALGetDescription, const char *(*)(GDALMajorObjectH));
KATANA_DECLARED(GDALGetMetadata, CSLConstList (*)(GDALMajorObjectH, const char *));
KATANA_DECLARED(GDALGetMetadataItem, const char *(*)(GDALMajorObjectH, const char *, const char *));
KATANA_DECLARED(GDALGetRasterXSize, int (*)(GDALDatasetH));
KATANA_DECLARED(GDALGetRasterYSize, int (*)(GDALDatasetH));
KATANA_DECLARED(GDALGetRasterCount, int (*)(GDALDatasetH));
KATANA_DECLARED(GDALGetRasterBand, GDALRasterBandH (*)(GDALDatasetH, int));
KATANA_DECLARED(GDALGetProjectionRef, const char *(*)(GDALDatasetH));
KATANA_DECLARED(GDALGetSpatialRef, OGRSpatialReferenceH (*)(GDALDatasetH));
KATANA_DECLARED(GDALSetSpatialRef, CPLErr (*)(GDALDatasetH, OGRSpatialReferenceH));
KATANA_DECLARED(GDALGetGeoTransform, CPLErr (*)(GDALDatasetH, double *));
KATANA_DECLARED(GDALSetGeoTransform, CPLErr (*)(GDALDatasetH, const double *));
KATANA_DECLARED(GDALInvGeoTransform, int (*)(const double *, double *));
KATANA_DECLARED(GDALGetRasterNoDataValue, double (*)(GDALRasterBandH, int *));
KATANA_DECLARED(GDALSetRasterNoDataValue, CPLErr (*)(GDALRasterBandH, double));
KATANA_DECLARED(GDALGetRasterDataType, GDALDataType (*)(GDALRasterBandH));
KATANA_DECLARED(GDALGetDataTypeName, const char *(*)(GDALDataType));
KATANA_DECLARED(GDALGetRasterColorInterpretation, GDALColorInterp (*)(GDALRasterBandH));
KATANA_DECLARED(GDALGetRasterColorTable, GDALColorTableH (*)(GDALRasterBandH));
KATANA_DECLARED(GDALGetColorEntryCount, int (*)(GDALColorTableH));
KATANA_DECLARED(GDALGetColorEntry, const GDALColorEntry *(*)(GDALColorTableH, int));
KATANA_DECLARED(GDALRasterIO, CPLErr (*)(GDALRasterBandH, GDALRWFlag, int, int, int, int, void *, int,
                                         int, GDALDataType, int, int));
KATANA_DECLARED(GDALRasterInterpolateAtPoint, CPLErr (*)(GDALRasterBandH, double, double,
                                                         GDALRIOResampleAlg, double *, double *));
KATANA_DECLARED(GDALDatasetGetLayerCount, int (*)(GDALDatasetH));
KATANA_DECLARED(GDALDatasetGetLayer, OGRLayerH (*)(GDALDatasetH, int));
KATANA_DECLARED(GDALDatasetGetLayerByName, OGRLayerH (*)(GDALDatasetH, const char *));
KATANA_DECLARED(GDALDatasetDeleteLayer, OGRErr (*)(GDALDatasetH, int));
KATANA_DECLARED(GDALDatasetCreateLayer, OGRLayerH (*)(GDALDatasetH, const char *, OGRSpatialReferenceH,
                                                      OGRwkbGeometryType, CSLConstList));
KATANA_DECLARED(GDALDatasetTestCapability, int (*)(GDALDatasetH, const char *));
KATANA_DECLARED(GDALDatasetStartTransaction, OGRErr (*)(GDALDatasetH, int));
KATANA_DECLARED(GDALDatasetCommitTransaction, OGRErr (*)(GDALDatasetH));
KATANA_DECLARED(GDALDatasetRollbackTransaction, OGRErr (*)(GDALDatasetH));
KATANA_DECLARED(GDALDatasetExecuteSQL, OGRLayerH (*)(GDALDatasetH, const char *, OGRGeometryH,
                                                     const char *));
KATANA_DECLARED(GDALDatasetReleaseResultSet, void (*)(GDALDatasetH, OGRLayerH));
KATANA_DECLARED(GDALTranslateOptionsNew, GDALTranslateOptions *(*)(char **,
                                                                   GDALTranslateOptionsForBinary *));
KATANA_DECLARED(GDALTranslateOptionsFree, void (*)(GDALTranslateOptions *));
KATANA_DECLARED(GDALTranslate, GDALDatasetH (*)(const char *, GDALDatasetH,
                                                const GDALTranslateOptions *, int *));
KATANA_DECLARED(GDALWarpAppOptionsNew, GDALWarpAppOptions *(*)(char **, GDALWarpAppOptionsForBinary *));
KATANA_DECLARED(GDALWarpAppOptionsSetProgress, void (*)(GDALWarpAppOptions *, GDALProgressFunc,
                                                        void *));
KATANA_DECLARED(GDALWarpAppOptionsFree, void (*)(GDALWarpAppOptions *));
KATANA_DECLARED(GDALWarp, GDALDatasetH (*)(const char *, GDALDatasetH, int, GDALDatasetH *,
                                           const GDALWarpAppOptions *, int *));
KATANA_DECLARED(GDALVectorTranslateOptionsNew,
                GDALVectorTranslateOptions *(*)(char **, GDALVectorTranslateOptionsForBinary *));
KATANA_DECLARED(GDALVectorTranslateOptionsFree, void (*)(GDALVectorTranslateOptions *));
KATANA_DECLARED(GDALVectorTranslate, GDALDatasetH (*)(const char *, GDALDatasetH, int, GDALDatasetH *,
                                                      const GDALVectorTranslateOptions *, int *));
KATANA_DECLARED(GDALBuildVRTOptionsNew, GDALBuildVRTOptions *(*)(char **, GDALBuildVRTOptionsForBinary *));
KATANA_DECLARED(GDALBuildVRTOptionsFree, void (*)(GDALBuildVRTOptions *));
KATANA_DECLARED(GDALBuildVRT, GDALDatasetH (*)(const char *, int, GDALDatasetH *, const char *const *,
                                               const GDALBuildVRTOptions *, int *));

/* ---- ogr.rs ---------------------------------------------------------------------------------- */

KATANA_DECLARED(OGR_GT_Flatten, OGRwkbGeometryType (*)(OGRwkbGeometryType));
KATANA_DECLARED(OGR_GT_HasZ, int (*)(OGRwkbGeometryType));
KATANA_DECLARED(OGR_GT_SetZ, OGRwkbGeometryType (*)(OGRwkbGeometryType));
KATANA_DECLARED(OGRGeometryTypeToName, const char *(*)(OGRwkbGeometryType));
KATANA_DECLARED(OGR_G_CreateGeometry, OGRGeometryH (*)(OGRwkbGeometryType));
KATANA_DECLARED(OGR_G_DestroyGeometry, void (*)(OGRGeometryH));
KATANA_DECLARED(OGR_G_GetGeometryType, OGRwkbGeometryType (*)(OGRGeometryH));
KATANA_DECLARED(OGR_G_IsEmpty, int (*)(OGRGeometryH));
KATANA_DECLARED(OGR_G_Is3D, int (*)(OGRGeometryH));
KATANA_DECLARED(OGR_G_GetLinearGeometry, OGRGeometryH (*)(OGRGeometryH, double, CSLConstList));
KATANA_DECLARED(OGR_G_GetPointCount, int (*)(OGRGeometryH));
KATANA_DECLARED(OGR_G_GetPoint, void (*)(OGRGeometryH, int, double *, double *, double *));
KATANA_DECLARED(OGR_G_SetPoint, OGRErr (*)(OGRGeometryH, int, double, double, double));
KATANA_DECLARED(OGR_G_SetPoint_2D, OGRErr (*)(OGRGeometryH, int, double, double));
KATANA_DECLARED(OGR_G_AddPoint, OGRErr (*)(OGRGeometryH, double, double, double));
KATANA_DECLARED(OGR_G_AddPoint_2D, OGRErr (*)(OGRGeometryH, double, double));
KATANA_DECLARED(OGR_G_GetGeometryCount, int (*)(OGRGeometryH));
KATANA_DECLARED(OGR_G_GetGeometryRef, OGRGeometryH (*)(OGRGeometryH, int));
KATANA_DECLARED(OGR_G_AddGeometryDirectly, OGRErr (*)(OGRGeometryH, OGRGeometryH));
KATANA_DECLARED(OGR_L_GetName, const char *(*)(OGRLayerH));
KATANA_DECLARED(OGR_L_GetGeomType, OGRwkbGeometryType (*)(OGRLayerH));
KATANA_DECLARED(OGR_L_GetSpatialRef, OGRSpatialReferenceH (*)(OGRLayerH));
KATANA_DECLARED(OGR_L_GetLayerDefn, OGRFeatureDefnH (*)(OGRLayerH));
KATANA_DECLARED(OGR_L_ResetReading, void (*)(OGRLayerH));
KATANA_DECLARED(OGR_L_GetNextFeature, OGRFeatureH (*)(OGRLayerH));
KATANA_DECLARED(OGR_L_SetSpatialFilterRect, void (*)(OGRLayerH, double, double, double, double));
KATANA_DECLARED(OGR_L_SetSpatialFilter, void (*)(OGRLayerH, OGRGeometryH));
KATANA_DECLARED(OGR_L_SetAttributeFilter, OGRErr (*)(OGRLayerH, const char *));
KATANA_DECLARED(OGR_L_GetFeatureCount, GIntBig (*)(OGRLayerH, int));
KATANA_DECLARED(OGR_L_TestCapability, int (*)(OGRLayerH, const char *));
KATANA_DECLARED(OGR_L_CreateField, OGRErr (*)(OGRLayerH, OGRFieldDefnH, int));
KATANA_DECLARED(OGR_L_CreateFeature, OGRErr (*)(OGRLayerH, OGRFeatureH));
KATANA_DECLARED(OGR_F_Create, OGRFeatureH (*)(OGRFeatureDefnH));
KATANA_DECLARED(OGR_F_Destroy, void (*)(OGRFeatureH));
KATANA_DECLARED(OGR_F_GetGeometryRef, OGRGeometryH (*)(OGRFeatureH));
KATANA_DECLARED(OGR_F_SetGeometryDirectly, OGRErr (*)(OGRFeatureH, OGRGeometryH));
KATANA_DECLARED(OGR_F_IsFieldSetAndNotNull, int (*)(OGRFeatureH, int));
KATANA_DECLARED(OGR_F_GetFieldAsInteger, int (*)(OGRFeatureH, int));
KATANA_DECLARED(OGR_F_GetFieldAsInteger64, GIntBig (*)(OGRFeatureH, int));
KATANA_DECLARED(OGR_F_GetFieldAsDouble, double (*)(OGRFeatureH, int));
KATANA_DECLARED(OGR_F_GetFieldAsString, const char *(*)(OGRFeatureH, int));
KATANA_DECLARED(OGR_F_GetFieldAsDateTimeEx,
                int (*)(OGRFeatureH, int, int *, int *, int *, int *, int *, float *, int *));
KATANA_DECLARED(OGR_F_SetFieldNull, void (*)(OGRFeatureH, int));
KATANA_DECLARED(OGR_F_SetFieldInteger, void (*)(OGRFeatureH, int, int));
KATANA_DECLARED(OGR_F_SetFieldInteger64, void (*)(OGRFeatureH, int, GIntBig));
KATANA_DECLARED(OGR_F_SetFieldDouble, void (*)(OGRFeatureH, int, double));
KATANA_DECLARED(OGR_F_SetFieldString, void (*)(OGRFeatureH, int, const char *));
KATANA_DECLARED(OGR_F_SetStyleString, void (*)(OGRFeatureH, const char *));
KATANA_DECLARED(OGR_FD_GetFieldCount, int (*)(OGRFeatureDefnH));
KATANA_DECLARED(OGR_FD_GetFieldDefn, OGRFieldDefnH (*)(OGRFeatureDefnH, int));
KATANA_DECLARED(OGR_FD_GetFieldIndex, int (*)(OGRFeatureDefnH, const char *));
KATANA_DECLARED(OGR_Fld_Create, OGRFieldDefnH (*)(const char *, OGRFieldType));
KATANA_DECLARED(OGR_Fld_Destroy, void (*)(OGRFieldDefnH));
KATANA_DECLARED(OGR_Fld_SetSubType, void (*)(OGRFieldDefnH, OGRFieldSubType));
KATANA_DECLARED(OGR_Fld_GetNameRef, const char *(*)(OGRFieldDefnH));
KATANA_DECLARED(OGR_Fld_GetType, OGRFieldType (*)(OGRFieldDefnH));
KATANA_DECLARED(OGR_Fld_GetSubType, OGRFieldSubType (*)(OGRFieldDefnH));

/* ---- osr.rs ---------------------------------------------------------------------------------- */

KATANA_DECLARED(OSRNewSpatialReference, OGRSpatialReferenceH (*)(const char *));
KATANA_DECLARED(OSRDestroySpatialReference, void (*)(OGRSpatialReferenceH));
KATANA_DECLARED(OSRSetAxisMappingStrategy, void (*)(OGRSpatialReferenceH, OSRAxisMappingStrategy));
KATANA_DECLARED(OSRGetDataAxisToSRSAxisMapping, const int *(*)(OGRSpatialReferenceH, int *));
KATANA_DECLARED(OSRImportFromWkt, OGRErr (*)(OGRSpatialReferenceH, char **));
KATANA_DECLARED(OSRSetFromUserInputEx, OGRErr (*)(OGRSpatialReferenceH, const char *, CSLConstList));
KATANA_DECLARED(OSRExportToWkt, OGRErr (*)(OGRSpatialReferenceH, char **));
KATANA_DECLARED(OSRExportToWktEx, OGRErr (*)(OGRSpatialReferenceH, char **, const char *const *));
KATANA_DECLARED(OSRSetWellKnownGeogCS, OGRErr (*)(OGRSpatialReferenceH, const char *));
KATANA_DECLARED(OSRIsGeographic, int (*)(OGRSpatialReferenceH));
KATANA_DECLARED(OSRIsSame, int (*)(OGRSpatialReferenceH, OGRSpatialReferenceH));
KATANA_DECLARED(OSRIsSameEx, int (*)(OGRSpatialReferenceH, OGRSpatialReferenceH, const char *const *));
KATANA_DECLARED(OSRGetAxesCount, int (*)(OGRSpatialReferenceH));
KATANA_DECLARED(OSRDemoteTo2D, OGRErr (*)(OGRSpatialReferenceH, const char *));
KATANA_DECLARED(OSRGetName, const char *(*)(OGRSpatialReferenceH));
KATANA_DECLARED(OSRGetAuthorityName, const char *(*)(OGRSpatialReferenceH, const char *));
KATANA_DECLARED(OSRGetAuthorityCode, const char *(*)(OGRSpatialReferenceH, const char *));
KATANA_DECLARED(OSRAutoIdentifyEPSG, OGRErr (*)(OGRSpatialReferenceH));
KATANA_DECLARED(OSRFindMatches,
                OGRSpatialReferenceH *(*)(OGRSpatialReferenceH, CSLConstList, int *, int **));
KATANA_DECLARED(OSRFreeSRSArray, void (*)(OGRSpatialReferenceH *));
KATANA_DECLARED(OCTNewCoordinateTransformation,
                OGRCoordinateTransformationH (*)(OGRSpatialReferenceH, OGRSpatialReferenceH));
KATANA_DECLARED(OCTDestroyCoordinateTransformation, void (*)(OGRCoordinateTransformationH));
KATANA_DECLARED(OCTTransform, int (*)(OGRCoordinateTransformationH, int, double *, double *, double *));
KATANA_DECLARED(OCTTransformEx,
                int (*)(OGRCoordinateTransformationH, int, double *, double *, double *, int *));
KATANA_DECLARED(OCTTransformBounds,
                int (*)(OGRCoordinateTransformationH, double, double, double, double, double *,
                        double *, double *, double *, int));
