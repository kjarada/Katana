#pragma once

// The reply records of the geoprocessing verbs (docs/geoprocessing.md,
// "Replies"): one record per line, a word saying what it is and then
// key=value fields in a fixed order, a value quoted when it holds a blank, a
// quote or an '=' (cad::recordValue). One definition, so HELP, LIST, a run's
// reply and the MCP tools that read them back cannot come to differ.

#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/gis/processing.hpp"

namespace katana::app::geo {

// cad::recordValue: plain, or quoted and escaped; nothing for an empty value.
[[nodiscard]] std::string value(std::string_view text);
// A length or a time to 3 decimals, locale-independent: "0.017".
[[nodiscard]] std::string fixed3(double number);

// gdal algorithm="raster hillshade" policy=safe seconds=0.017 cancelled=no
[[nodiscard]] std::string gdalRecord(const katana::gis::processing::AlgorithmInfo& info,
                                     double seconds);
// algorithm path="raster hillshade" policy=safe aliases= description="..."
[[nodiscard]] std::string algorithmRecord(const katana::gis::processing::AlgorithmInfo& info);
// group path="raster" algorithms=47
[[nodiscard]] std::string groupRecord(const katana::gis::processing::AlgorithmInfo& info,
                                      std::size_t algorithms);
// arg name=zfactor short=z aliases= type=real required=no positional=no
//     category=Base default=1 min=0 min_inclusive=no max= max_inclusive=
//     choices= count=0..1 dataset= accepts= description="..."
[[nodiscard]] std::string argRecord(const katana::gis::processing::ArgSpec& arg);
// binding arg=input kinds=raster accepts=name,object sources=raster,surface,file
[[nodiscard]] std::string bindingRecord(const katana::gis::processing::ArgSpec& arg);
// gdal version=3.13.2 release="Iowa City" proj=9.7.1 geos=3.14.1
//      raster_drivers=148 vector_drivers=82 algorithms=121
[[nodiscard]] std::string versionRecord(const katana::gis::processing::Versions& versions);
// text lines=<n>, then the n lines verbatim.
[[nodiscard]] std::string textRecord(std::string_view text);
// warning text="..."
[[nodiscard]] std::string warningRecord(std::string_view text);

// The Katana sources a dataset argument can be bound from: drawing (a
// vector), raster and surface (a raster), and file (either).
[[nodiscard]] std::string sourcesFor(const katana::gis::processing::ArgSpec& arg);

// A record read back: what katana_gdal_run hands an agent as structured
// data, from the very text the command line prints.
struct Record {
    std::string kind;
    std::vector<std::pair<std::string, std::string>> fields;
    std::vector<std::string> body; // a text record's lines
    [[nodiscard]] std::optional<std::string> get(std::string_view key) const;
};

// The records of a reply, a text record with the lines it says it has.
// Lines that are not records (a word with no fields) are records of their
// own kind with no fields.
[[nodiscard]] std::vector<Record> parseRecords(std::string_view text);

} // namespace katana::app::geo
