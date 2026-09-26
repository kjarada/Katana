#pragma once

// Shared by the bridge's two files (processing.cpp, policy.cpp), never
// installed: what the policy table needs to see of a live algorithm.

#include <string>
#include <string_view>
#include <vector>

#include <gdalalgorithm.h>

#include "katana/gis/processing.hpp"

namespace katana::gis::processing::detail {

// The policy of the leaf at `path`, whose instance is `algorithm`, and why
// (AlgorithmInfo::policyReason).
[[nodiscard]] Policy classify(const std::vector<std::string>& path, const GDALAlgorithm& algorithm,
                              std::string& reason);

// One word of a pipeline as GDAL's pipeline argument takes it ("read x !
// slope ! write y", nested steps in [ ]): `step` when it names a step - the
// first word that is not an option after the text's start, a '!' or a '[' -
// and `first` while no '!' or '[' has been passed.
struct PipelineWord {
    std::string text;
    bool step = false;
    bool first = false;
};
[[nodiscard]] std::vector<PipelineWord> pipelineWords(std::string_view pipeline);

// Whether a pipeline, as GDAL's pipeline argument takes it ("read ! slope !
// write", nested steps in [ ]), has a step called `external`, which runs a
// program of the caller's choosing.
[[nodiscard]] bool hasExternalStep(std::string_view pipeline);

// Whether the path is one of the three pipeline algorithms.
[[nodiscard]] bool isPipeline(const std::vector<std::string>& path);

} // namespace katana::gis::processing::detail
