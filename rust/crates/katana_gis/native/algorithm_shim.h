/*
 * The C ABI of native/algorithm_shim.cpp: the calls of GDAL's C++ algorithm
 * framework the bridge makes that GDAL's C API has no twin for, or whose C
 * twin is not the same call (src/ffi/algorithm.rs says which, and how that
 * was measured). Included by the shim, so its definitions must match these
 * declarations, and by native/algorithm_check.c, which holds the Rust's
 * declarations to them.
 *
 * Every function takes the algorithm and, for an argument, its name, looked
 * up as the C++ looks it up: GetArg(name), no suggestions. Every one catches
 * every exception, reports it as a CE_Failure through CPLError and answers
 * its failure value (NULL, 0). A list (CSLDestroy) or a text (VSIFree) it
 * returns is the caller's to free.
 */

#ifndef KATANA_ALGORITHM_SHIM_H
#define KATANA_ALGORITHM_SHIM_H

#include <stddef.h>

#include <gdal.h>
#include <gdalalgorithm_c.h>

#ifdef __cplusplus
extern "C" {
#endif

GDALAlgorithmH katana_gdal_algorithm_instantiate_sub_algorithm(GDALAlgorithmH algorithm,
                                                               const char *name);
int katana_gdal_algorithm_is_hidden(GDALAlgorithmH algorithm);
char **katana_gdal_algorithm_aliases(GDALAlgorithmH algorithm);
char *katana_gdal_algorithm_suggestion_for_argument_name(GDALAlgorithmH algorithm,
                                                         const char *name);
int katana_gdal_algorithm_validate_arguments(GDALAlgorithmH algorithm);
char *katana_gdal_algorithm_arg_name(GDALAlgorithmH algorithm, const char *name);
char **katana_gdal_arg_auto_complete_choices(GDALAlgorithmH algorithm, const char *argument,
                                             const char *prefix);
int katana_gdal_arg_min_value(GDALAlgorithmH algorithm, const char *argument, double *value,
                              int *inclusive);
int katana_gdal_arg_max_value(GDALAlgorithmH algorithm, const char *argument, double *value,
                              int *inclusive);
char **katana_gdal_arg_metadata_keys(GDALAlgorithmH algorithm, const char *argument);
int katana_gdal_arg_set_dataset_name(GDALAlgorithmH algorithm, const char *argument,
                                     const char *name);
int katana_gdal_arg_set_dataset_list(GDALAlgorithmH algorithm, const char *argument, size_t count,
                                     const char *const *names, const GDALDatasetH *datasets);
char **katana_gdal_arg_dataset_list_names(GDALAlgorithmH algorithm, const char *argument);

#ifdef __cplusplus
}
#endif

#endif /* KATANA_ALGORITHM_SHIM_H */
