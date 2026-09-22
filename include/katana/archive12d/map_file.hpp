#pragma once

// A 12d mapfile -> entity::SurveyMap (PLAN.MD 20.3, slice 3).
//
// The mapfile is XML, and 12d writes it UTF-16 little-endian with a byte
// order mark, so the bytes go through decodeText first exactly as a 12da's
// do. The extension is not a guide: of the two files this was built against
// one is `.mapfile` and the other is `.4d`, which is also the extension of
// the linestyle libraries. What makes a file a mapfile is that it holds a
// `<map_file>` element.
//
//   <xml12d>
//     <map_file>
//       <map_data>
//         <item>
//           <key>WM*</key>
//           <model>SURVEY SERVICES</model>
//           <colour>sui water potable</colour>
//           <breakline>Line</breakline>
//           <linestyle>WATR Main</linestyle>
//           <weight>0</weight>
//         </item>
//
// Every element inside an `<item>` that this does not know is counted and
// named, the same promise readArchive and readStyleLibrary make.

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/archive12d/domain.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::archive12d {

struct MapFileRead {
    katana::entity::SurveyMap map{};
    // By element name: how many were read, how many were understood.
    std::vector<ElementTally> tally{};
    std::vector<std::string> warnings{};
    // `<map_file><comments>`, which is where these files describe themselves.
    std::vector<std::string> comments{};
    std::string version{}; // `<map_file><version>`
};

// Fails when the text is not XML, or is XML with no `<map_file>` in it. A
// single rule that cannot be read is skipped with a warning instead, so one
// bad `<item>` does not cost the other 1,627.
[[nodiscard]] katana::core::Result<MapFileRead> readMapFile(std::string_view text);

// Reads into an existing map, so a customisation made of several mapfiles
// loads as one. The EARLIER file wins a field both set - see
// entity::SurveyMap::add for why.
[[nodiscard]] katana::core::Result<MapFileRead> readMapFileInto(katana::entity::SurveyMap map,
                                                                std::string_view text);

} // namespace katana::archive12d
