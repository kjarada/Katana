#pragma once

// The spellings of the linework control codes.
//
// A point's field code may carry control codes after its string name: "KB1 ST",
// "FL CL", "KB1 BC", "FN3 JPN 105". What each control DOES is the linework
// processing in the cad layer (cad/linework.hpp); how it is SPELLED is here,
// because a spelling is part of a customisation - a survey team writes "ST" or
// "S" as its field book does - and a customisation is read and written in this
// layer, below cad (entity/customisation.hpp). The type lived in cad until the
// Katana customisation format gave it a file to be kept in; cad still names it
// `cad::LineworkCodes`.

#include <span>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::entity {

// The defaults are spellings common in field practice, chosen so that an
// untouched table does something sensible; no product's file was copied for
// them:
//   ST   start   - "start", the usual two-letter field abbreviation
//   END  end     - written out in most code sets
//   CL   close   - "close", as in "FL CL"
//   BC   arcStart, EC arcEnd - "begin curve" / "end curve", the road-design
//                  abbreviations surveyors already use for tangent points
//   JPN  join    - "join point number": also draw a line from this point to
//                  the point whose number follows ("JPN 105")
//   RECT rectangle - "rectangle": three points make a rectangle
// An empty spelling switches that control off.
struct LineworkCodes {
    std::string start = "ST";
    std::string end = "END";
    std::string close = "CL";
    std::string arcStart = "BC";
    std::string arcEnd = "EC";
    std::string join = "JPN";
    std::string rectangle = "RECT";

    friend bool operator==(const LineworkCodes&, const LineworkCodes&) = default;
};

// One control: the word it is known by and where its spelling is kept.
struct LineworkCodeMember {
    std::string_view name;                 // "arcStart"
    std::string LineworkCodes::* spelling; // &LineworkCodes::arcStart
};

// The seven controls in the order the struct declares them, by the words a
// refusal, the Katana customisation format and a verb name them with: "start",
// "end", "close", "arcStart", "arcEnd", "join", "rectangle". One list, so that
// the three cannot come to disagree about a name.
[[nodiscard]] std::span<const LineworkCodeMember> lineworkCodeMembers();

// Refuses a spelling containing a blank (a code is split on blanks, so it
// could never be matched) and two controls spelled alike (a token that means
// two things means neither). Spellings are compared ignoring ASCII case,
// because tokens are matched that way - see cad::parseFieldCode.
[[nodiscard]] katana::core::Status validate(const LineworkCodes& codes);

} // namespace katana::entity
