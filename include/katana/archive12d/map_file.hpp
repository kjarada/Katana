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

// ---- writing ----------------------------------------------------------------------------------

struct MapFileWriteOptions {
    // `<map_file><version>`. 11.0 is what the reference mapfiles say.
    std::string version = "11.0";
    // `<map_file><comments>`, one `<item>` each - where 12d's mapfiles
    // describe themselves, and where readMapFile finds them again.
    std::vector<std::string> comments{};
};

// A survey map -> 12d mapfile text: XML, as UTF-8. 12d itself writes mapfiles
// as UTF-16LE with a byte order mark; core::encodeUtf16LittleEndian turns
// this text into exactly that, and readMapFile reads either.
//
// Sections are written in 12d's order - map_data, vertex_symbol_data,
// tinable_data, vertex_textstyle_data, the three pipe sections, then the two
// attribute sections - and within a section the rules keep the order they
// have in the map. Among rules of ONE key that order decides every field more
// than one section fills (a comment; an attribute of one name from pipe_data
// and string_attribute_data, or from vertex_pipe_data and vertex_attribute_
// data), so a rule is never written ahead of an earlier rule of its own key:
// when 12d's order would put it there - as it would in a map made by loading
// two mapfiles, or by merging a load into the current map - the sections are
// written again, in the same order, for the rules that must follow, and
// readMapFile reads a repeated section in turn. Rules of DIFFERENT keys may
// be regrouped, which no code can tell: none matches two different keys of
// one specificity. So every code resolves after a round trip exactly as it
// did before, and a map whose every key is in 12d's order - one mapfile as
// 12d writes it - comes back rule for rule, each section written once. A
// value the map does not have is
// written as no element at all, never as a 0: an empty `<rotation/>` and a
// rotation of 0 read the same, but a size of 0 and no size do not mean the
// same thing to a person reading the file.
//
// Fails, naming the rule, rather than writing a file that would not read back
// as the map it was given: a rule holding a field its section cannot carry
// (a map_data rule with a symbol), a pipe with nothing but `active`, an
// attribute that is neither text nor integer or has no name, a value
// beginning or ending in white space (XML text is trimmed on reading) or
// holding a character XML 1.0 cannot carry at all.
[[nodiscard]] katana::core::Result<std::string>
writeMapFile(const katana::entity::SurveyMap& map, const MapFileWriteOptions& options = {});

} // namespace katana::archive12d
