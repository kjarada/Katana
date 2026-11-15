/*
 * The check of the declarations in src/ffi/algorithm.rs, compiled by build.rs
 * against the headers of the GDAL the crate links, as native/ffi_check.c
 * checks K1's. It is no code: every line is a _Static_assert, so a header
 * that changes, or a declaration transcribed wrongly, fails the build here
 * rather than misbehaving at run time.
 *
 *   - Each function the Rust declares, by its type: the Rust signature
 *     written in the header's own typedefs, compared with the header's
 *     declaration by _Generic (C11 6.5.1.1). The shim's functions are
 *     compared with native/algorithm_shim.h, whose declarations the shim's
 *     definitions must match.
 *   - Each type the Rust takes for another: the enumerations and
 *     GDALArgDatasetType an int's size, GIntBig 64 bits, size_t a pointer's
 *     size (the Rust's usize), bool one byte.
 *   - Each constant's value. The C++-only constants (HIDDEN_ALIAS_SEPARATOR,
 *     ROOT_ALG_NAME, UNBOUNDED) are held by static_assert in
 *     native/algorithm_shim.cpp.
 *
 * The numbers here are the Rust's: change one, change both.
 */

/* What g++ defines for the C++, as native/ffi_check.c has it. */
#define _GNU_SOURCE 1

#include <stdbool.h>
#include <stddef.h>

#include <cpl_conv.h>
#include <cpl_error.h>
#include <cpl_multiproc.h>
#include <cpl_vsi.h>
#include <gdal.h>
#include <gdalalgorithm_c.h>
#include <ogr_api.h>
#include <ogr_srs_api.h>

#include "algorithm_shim.h"

#define KATANA_SAME(value, expected) _Static_assert((value) == (expected), #value " is " #expected)
#define KATANA_DECLARED(function, type)                                                            \
    _Static_assert(_Generic(&(function), type: 1, default: 0), #function " is " #type)

/* ---- the types the Rust takes for others ------------------------------------------------- */

KATANA_SAME(sizeof(GDALAlgorithmArgType), sizeof(int));
KATANA_SAME(sizeof(GDALArgDatasetType), sizeof(int));
KATANA_SAME(sizeof(GDALColorInterp), sizeof(int));
KATANA_SAME(sizeof(GDALDataType), sizeof(int));
KATANA_SAME(sizeof(CPLErr), sizeof(int));
KATANA_SAME(sizeof(CPLErrorNum), sizeof(int));
KATANA_SAME(sizeof(GIntBig), 8);
KATANA_SAME(sizeof(size_t), sizeof(void *));
KATANA_SAME(sizeof(bool), 1);

/* ---- constants ------------------------------------------------------------------------------ */

KATANA_SAME(GAAT_BOOLEAN, 0);
KATANA_SAME(GAAT_STRING, 1);
KATANA_SAME(GAAT_INTEGER, 2);
KATANA_SAME(GAAT_REAL, 3);
KATANA_SAME(GAAT_DATASET, 4);
KATANA_SAME(GAAT_STRING_LIST, 5);
KATANA_SAME(GAAT_INTEGER_LIST, 6);
KATANA_SAME(GAAT_REAL_LIST, 7);
KATANA_SAME(GAAT_DATASET_LIST, 8);
KATANA_SAME(GADV_NAME, 1);
KATANA_SAME(GADV_OBJECT, 2);
KATANA_SAME(GDAL_OF_MULTIDIM_RASTER, 0x10);
KATANA_SAME(GCI_Undefined, 0);
KATANA_SAME(CPLE_FileIO, 3);
KATANA_SAME(CPLE_OpenFailed, 4);

/* ---- gdalalgorithm_c.h ------------------------------------------------------------------------ */

KATANA_DECLARED(GDALGetGlobalAlgorithmRegistry, GDALAlgorithmRegistryH (*)(void));
KATANA_DECLARED(GDALAlgorithmRegistryRelease, void (*)(GDALAlgorithmRegistryH));
KATANA_DECLARED(GDALAlgorithmRegistryInstantiateAlg,
                GDALAlgorithmH (*)(GDALAlgorithmRegistryH, const char *));
KATANA_DECLARED(GDALAlgorithmRelease, void (*)(GDALAlgorithmH));
KATANA_DECLARED(GDALAlgorithmGetName, const char *(*)(GDALAlgorithmH));
KATANA_DECLARED(GDALAlgorithmGetDescription, const char *(*)(GDALAlgorithmH));
KATANA_DECLARED(GDALAlgorithmGetLongDescription, const char *(*)(GDALAlgorithmH));
KATANA_DECLARED(GDALAlgorithmGetHelpFullURL, const char *(*)(GDALAlgorithmH));
KATANA_DECLARED(GDALAlgorithmHasSubAlgorithms, bool (*)(GDALAlgorithmH));
KATANA_DECLARED(GDALAlgorithmGetSubAlgorithmNames, char **(*)(GDALAlgorithmH));
KATANA_DECLARED(GDALAlgorithmParseCommandLineArguments, bool (*)(GDALAlgorithmH, CSLConstList));
KATANA_DECLARED(GDALAlgorithmRun, bool (*)(GDALAlgorithmH, GDALProgressFunc, void *));
KATANA_DECLARED(GDALAlgorithmFinalize, bool (*)(GDALAlgorithmH));
KATANA_DECLARED(GDALAlgorithmGetUsageAsJSON, char *(*)(GDALAlgorithmH));
KATANA_DECLARED(GDALAlgorithmGetArgNames, char **(*)(GDALAlgorithmH));
KATANA_DECLARED(GDALAlgorithmGetArgNonConst, GDALAlgorithmArgH (*)(GDALAlgorithmH, const char *));
KATANA_DECLARED(GDALAlgorithmArgRelease, void (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetName, const char *(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetType, GDALAlgorithmArgType (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetDescription, const char *(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetShortName, const char *(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetAliases, char **(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetMetaVar, const char *(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetCategory, const char *(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgIsPositional, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgIsRequired, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetMinCount, int (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetMaxCount, int (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetPackedValuesAllowed, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetRepeatedArgAllowed, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetChoices, char **(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetMetadataItem, char **(*)(GDALAlgorithmArgH, const char *));
KATANA_DECLARED(GDALAlgorithmArgIsExplicitlySet, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgHasDefaultValue, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetDefaultAsBoolean, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetDefaultAsString, const char *(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetDefaultAsInteger, int (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetDefaultAsDouble, double (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetDefaultAsStringList, char **(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetDefaultAsIntegerList,
                const int *(*)(GDALAlgorithmArgH, size_t *));
KATANA_DECLARED(GDALAlgorithmArgGetDefaultAsDoubleList,
                const double *(*)(GDALAlgorithmArgH, size_t *));
KATANA_DECLARED(GDALAlgorithmArgIsHidden, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgIsHiddenForAPI, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgIsAvailableInPipelineStep, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgIsInput, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgIsOutput, bool (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetMutualExclusionGroup, const char *(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetMutualDependencyGroup, const char *(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetAsString, const char *(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetAsDatasetValue, GDALArgDatasetValueH (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetAsInteger, int (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetDirectDependencies, char **(*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgSetAsBoolean, bool (*)(GDALAlgorithmArgH, bool));
KATANA_DECLARED(GDALAlgorithmArgSetAsString, bool (*)(GDALAlgorithmArgH, const char *));
KATANA_DECLARED(GDALAlgorithmArgSetDataset, bool (*)(GDALAlgorithmArgH, GDALDatasetH));
KATANA_DECLARED(GDALAlgorithmArgSetAsInteger, bool (*)(GDALAlgorithmArgH, int));
KATANA_DECLARED(GDALAlgorithmArgSetAsDouble, bool (*)(GDALAlgorithmArgH, double));
KATANA_DECLARED(GDALAlgorithmArgSetAsStringList, bool (*)(GDALAlgorithmArgH, CSLConstList));
KATANA_DECLARED(GDALAlgorithmArgSetAsIntegerList,
                bool (*)(GDALAlgorithmArgH, size_t, const int *));
KATANA_DECLARED(GDALAlgorithmArgSetAsDoubleList,
                bool (*)(GDALAlgorithmArgH, size_t, const double *));
KATANA_DECLARED(GDALAlgorithmArgGetDatasetType, GDALArgDatasetType (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALAlgorithmArgGetDatasetInputFlags, int (*)(GDALAlgorithmArgH));
KATANA_DECLARED(GDALArgDatasetValueRelease, void (*)(GDALArgDatasetValueH));
KATANA_DECLARED(GDALArgDatasetValueGetName, const char *(*)(GDALArgDatasetValueH));
KATANA_DECLARED(GDALArgDatasetValueGetDatasetIncreaseRefCount,
                GDALDatasetH (*)(GDALArgDatasetValueH));

/* ---- gdal.h, cpl_multiproc.h, cpl_vsi.h, ogr_api.h, ogr_srs_api.h -------------------------- */

KATANA_DECLARED(GDALReleaseDataset, int (*)(GDALDatasetH));
KATANA_DECLARED(GDALGetDataTypeByName, GDALDataType (*)(const char *));
KATANA_DECLARED(GDALDataTypeIsFloating, int (*)(GDALDataType));
KATANA_DECLARED(GDALSetRasterColorInterpretation, CPLErr (*)(GDALRasterBandH, GDALColorInterp));
KATANA_DECLARED(CPLGetPID, GIntBig (*)(void));
KATANA_DECLARED(VSIMkdirRecursive, int (*)(const char *, long));
KATANA_DECLARED(VSIRmdirRecursive, int (*)(const char *));
KATANA_DECLARED(OGR_G_AssignSpatialReference, void (*)(OGRGeometryH, OGRSpatialReferenceH));
KATANA_DECLARED(OGRGetGEOSVersion, bool (*)(int *, int *, int *));
KATANA_DECLARED(OSRGetPROJVersion, void (*)(int *, int *, int *));

/* ---- the shim, native/algorithm_shim.h ------------------------------------------------------ */

KATANA_DECLARED(katana_gdal_algorithm_instantiate_sub_algorithm,
                GDALAlgorithmH (*)(GDALAlgorithmH, const char *));
KATANA_DECLARED(katana_gdal_algorithm_is_hidden, int (*)(GDALAlgorithmH));
KATANA_DECLARED(katana_gdal_algorithm_aliases, char **(*)(GDALAlgorithmH));
KATANA_DECLARED(katana_gdal_algorithm_suggestion_for_argument_name,
                char *(*)(GDALAlgorithmH, const char *));
KATANA_DECLARED(katana_gdal_algorithm_validate_arguments, int (*)(GDALAlgorithmH));
KATANA_DECLARED(katana_gdal_algorithm_arg_name, char *(*)(GDALAlgorithmH, const char *));
KATANA_DECLARED(katana_gdal_arg_auto_complete_choices,
                char **(*)(GDALAlgorithmH, const char *, const char *));
KATANA_DECLARED(katana_gdal_arg_min_value,
                int (*)(GDALAlgorithmH, const char *, double *, int *));
KATANA_DECLARED(katana_gdal_arg_max_value,
                int (*)(GDALAlgorithmH, const char *, double *, int *));
KATANA_DECLARED(katana_gdal_arg_metadata_keys, char **(*)(GDALAlgorithmH, const char *));
KATANA_DECLARED(katana_gdal_arg_set_dataset_name,
                int (*)(GDALAlgorithmH, const char *, const char *));
KATANA_DECLARED(katana_gdal_arg_set_dataset_list,
                int (*)(GDALAlgorithmH, const char *, size_t, const char *const *,
                        const GDALDatasetH *));
KATANA_DECLARED(katana_gdal_arg_dataset_list_names, char **(*)(GDALAlgorithmH, const char *));
