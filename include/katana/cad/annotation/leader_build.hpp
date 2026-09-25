#pragma once

// Leaders and balloons made from what a person or an agent gives
// (docs/annotation.md, "Leaders and callouts"): what the LEADER and BALLOON
// verbs share with the Leader and Balloon tools, so that a balloon typed and
// a balloon drawn are numbered alike.

#include <string>

#include "katana/entity/model.hpp"

namespace katana::cad::annotation {

// The number a new balloon takes: one more than the highest whole number any
// balloon (a leader with a circle callout) in `model` shows, "1" when there
// is none. A balloon whose note is not a whole number ("A", "3a") does not
// count. BALLOON's n= and the Balloon tool's Number option give another.
[[nodiscard]] std::string nextBalloonNumber(const katana::entity::Model& model);

} // namespace katana::cad::annotation
