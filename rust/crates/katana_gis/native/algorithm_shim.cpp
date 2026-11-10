// The calls of GDAL's C++ algorithm framework that the geoprocessing bridge
// (src/processing/bridge.rs, the port of src/katana_io/geo/processing.cpp)
// makes and GDAL's C API cannot: it has no twin for them, or its twin is not
// the same call (src/ffi/algorithm.rs lists which, and how each was measured
// on libgdal 3.13.2). Everything else the bridge calls is the C API; every
// decision the bridge makes is Rust. The C ABI is algorithm_shim.h.
//
// The algorithm is reached through GDALAlgorithmHS::ptr, which
// gdalalgorithm_cpp.h declares in public (inside @cond Doxygen_Suppress). This
// file is compiled against the headers of the GDAL the crate links, so a
// release that changes the struct fails the build here rather than the run.
//
// Every function catches every exception: nothing may unwind into Rust. One
// caught is reported as a CE_Failure through CPLError - where the bridge's
// collector on this thread takes it, as it takes GDAL's own failures - and
// the function answers its failure value.

#include "algorithm_shim.h"

#include <exception>
#include <limits>
#include <memory>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <cpl_conv.h>
#include <cpl_error.h>
#include <cpl_string.h>
#include <gdal_priv.h>
#include <gdalalgorithm.h>

// The C++ constants the Rust copies (src/ffi/algorithm.rs): C has no twin of
// them, so they are held here.
static_assert(std::string_view(GDALAlgorithmRegistry::HIDDEN_ALIAS_SEPARATOR) == "==hide==",
              "HIDDEN_ALIAS_SEPARATOR in src/ffi/algorithm.rs");
static_assert(std::string_view(GDALGlobalAlgorithmRegistry::ROOT_ALG_NAME) == "gdal",
              "ROOT_ALG_NAME in src/ffi/algorithm.rs");
static_assert(GDALAlgorithmArgDecl::UNBOUNDED == std::numeric_limits<int>::max(),
              "UNBOUNDED in src/ffi/algorithm.rs");

namespace {

void report(const char *what)
{
    CPLError(CE_Failure, CPLE_AppDefined, "%s", what);
}

// `call()`, or `failure` with the exception it threw reported.
template <class Result, class Call> Result guarded(Result failure, const Call &call) noexcept
{
    try {
        return call();
    } catch (const std::exception &error) {
        report(error.what());
    } catch (...) {
        report("GDAL's algorithm framework threw something that is no std::exception");
    }
    return failure;
}

GDALAlgorithm *algorithmOf(GDALAlgorithmH handle)
{
    return handle != nullptr ? handle->ptr : nullptr;
}

// The argument as the C++ bridge looks it up: by its long name, a short name
// or an alias, with no suggestion raised for a name that has none.
GDALAlgorithmArg *argOf(GDALAlgorithmH handle, const char *name)
{
    GDALAlgorithm *algorithm = algorithmOf(handle);
    return algorithm != nullptr && name != nullptr ? algorithm->GetArg(name) : nullptr;
}

char **listOf(const std::vector<std::string> &items)
{
    CPLStringList list;
    for (const std::string &item : items) {
        list.AddString(item.c_str());
    }
    return list.StealList();
}

} // namespace

extern "C" GDALAlgorithmH katana_gdal_algorithm_instantiate_sub_algorithm(GDALAlgorithmH algorithm,
                                                                          const char *name)
{
    return guarded(static_cast<GDALAlgorithmH>(nullptr), [&]() -> GDALAlgorithmH {
        const GDALAlgorithm *parent = algorithmOf(algorithm);
        if (parent == nullptr || name == nullptr) {
            return nullptr;
        }
        // Suggestions off, as the C++ bridge asks: the C API's twin asks with
        // them on, which raises an error for a name close to another.
        std::unique_ptr<GDALAlgorithm> sub = parent->InstantiateSubAlgorithm(name, false);
        if (!sub) {
            return nullptr;
        }
        // Owned as GDAL's C API owns what it hands out: GDALAlgorithmRelease
        // deletes it.
        return std::make_unique<GDALAlgorithmHS>(std::move(sub)).release();
    });
}

extern "C" int katana_gdal_algorithm_is_hidden(GDALAlgorithmH algorithm)
{
    return guarded(0, [&]() -> int {
        const GDALAlgorithm *self = algorithmOf(algorithm);
        return self != nullptr && self->IsHidden() ? 1 : 0;
    });
}

extern "C" char **katana_gdal_algorithm_aliases(GDALAlgorithmH algorithm)
{
    return guarded(static_cast<char **>(nullptr), [&]() -> char ** {
        const GDALAlgorithm *self = algorithmOf(algorithm);
        return self != nullptr ? listOf(self->GetAliases()) : nullptr;
    });
}

extern "C" char *katana_gdal_algorithm_suggestion_for_argument_name(GDALAlgorithmH algorithm,
                                                                    const char *name)
{
    return guarded(static_cast<char *>(nullptr), [&]() -> char * {
        const GDALAlgorithm *self = algorithmOf(algorithm);
        if (self == nullptr || name == nullptr) {
            return nullptr;
        }
        return CPLStrdup(self->GetSuggestionForArgumentName(name).c_str());
    });
}

extern "C" int katana_gdal_algorithm_validate_arguments(GDALAlgorithmH algorithm)
{
    return guarded(0, [&]() -> int {
        GDALAlgorithm *self = algorithmOf(algorithm);
        return self != nullptr && self->ValidateArguments() ? 1 : 0;
    });
}

extern "C" char *katana_gdal_algorithm_arg_name(GDALAlgorithmH algorithm, const char *name)
{
    return guarded(static_cast<char *>(nullptr), [&]() -> char * {
        const GDALAlgorithmArg *arg = argOf(algorithm, name);
        return arg != nullptr ? CPLStrdup(arg->GetName().c_str()) : nullptr;
    });
}

extern "C" char **katana_gdal_arg_auto_complete_choices(GDALAlgorithmH algorithm,
                                                        const char *argument, const char *prefix)
{
    return guarded(static_cast<char **>(nullptr), [&]() -> char ** {
        const GDALAlgorithmArg *arg = argOf(algorithm, argument);
        if (arg == nullptr) {
            return nullptr;
        }
        return listOf(arg->GetAutoCompleteChoices(prefix != nullptr ? prefix : ""));
    });
}

extern "C" int katana_gdal_arg_min_value(GDALAlgorithmH algorithm, const char *argument,
                                         double *value, int *inclusive)
{
    return guarded(0, [&]() -> int {
        const GDALAlgorithmArg *arg = argOf(algorithm, argument);
        if (arg == nullptr || value == nullptr || inclusive == nullptr) {
            return 0;
        }
        const std::pair<double, bool> bound = arg->GetMinValue();
        *value = bound.first;
        *inclusive = bound.second ? 1 : 0;
        return 1;
    });
}

extern "C" int katana_gdal_arg_max_value(GDALAlgorithmH algorithm, const char *argument,
                                         double *value, int *inclusive)
{
    return guarded(0, [&]() -> int {
        const GDALAlgorithmArg *arg = argOf(algorithm, argument);
        if (arg == nullptr || value == nullptr || inclusive == nullptr) {
            return 0;
        }
        const std::pair<double, bool> bound = arg->GetMaxValue();
        *value = bound.first;
        *inclusive = bound.second ? 1 : 0;
        return 1;
    });
}

extern "C" char **katana_gdal_arg_metadata_keys(GDALAlgorithmH algorithm, const char *argument)
{
    return guarded(static_cast<char **>(nullptr), [&]() -> char ** {
        const GDALAlgorithmArg *arg = argOf(algorithm, argument);
        if (arg == nullptr) {
            return nullptr;
        }
        CPLStringList keys;
        for (const auto &entry : arg->GetMetadata()) {
            keys.AddString(entry.first.c_str());
        }
        return keys.StealList();
    });
}

extern "C" int katana_gdal_arg_set_dataset_name(GDALAlgorithmH algorithm, const char *argument,
                                                const char *name)
{
    return guarded(0, [&]() -> int {
        GDALAlgorithmArg *arg = argOf(algorithm, argument);
        return arg != nullptr && name != nullptr && arg->SetDatasetName(name) ? 1 : 0;
    });
}

extern "C" int katana_gdal_arg_set_dataset_list(GDALAlgorithmH algorithm, const char *argument,
                                                size_t count, const char *const *names,
                                                const GDALDatasetH *datasets)
{
    return guarded(0, [&]() -> int {
        GDALAlgorithmArg *arg = argOf(algorithm, argument);
        if (arg == nullptr) {
            return 0;
        }
        std::vector<GDALArgDatasetValue> list;
        for (size_t i = 0; i < count; ++i) {
            if (datasets != nullptr && datasets[i] != nullptr) {
                // The value references the dataset, as the C++ bridge's
                // GDALArgDatasetValue(GDALDataset*) does.
                list.emplace_back(GDALDataset::FromHandle(datasets[i]));
            } else {
                list.emplace_back(
                    std::string(names != nullptr && names[i] != nullptr ? names[i] : ""));
            }
        }
        // Set with a moved list: writing into the argument's own list does
        // not mark it set (the C++ bridge's own note).
        return arg->Set(std::move(list)) ? 1 : 0;
    });
}

extern "C" char **katana_gdal_arg_dataset_list_names(GDALAlgorithmH algorithm,
                                                     const char *argument)
{
    return guarded(static_cast<char **>(nullptr), [&]() -> char ** {
        GDALAlgorithmArg *arg = argOf(algorithm, argument);
        if (arg == nullptr || arg->GetType() != GAAT_DATASET_LIST) {
            return nullptr;
        }
        CPLStringList names;
        for (const GDALArgDatasetValue &each : arg->Get<std::vector<GDALArgDatasetValue>>()) {
            names.AddString(each.GetName().c_str());
        }
        return names.StealList();
    });
}
