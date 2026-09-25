#pragma once

// The DATA section of an ISO 10303-21 file, built one instance at a time.
// Internal to katana_ifc.
//
// An instance is written the moment it is added, so everything it refers to
// must already exist: a reference is always to a lower number, and nothing is
// patched afterwards. The builders are written in that order (a point before
// the curve through it), which is also the order a person reading the file
// expects.

#include <cstdint>
#include <initializer_list>
#include <map>
#include <string>
#include <string_view>
#include <vector>

#include "katana/ifc/step.hpp"

namespace katana::ifc::detail {

using Id = std::uint32_t;

// An attribute list, built left to right in the schema's order. Every
// method appends one attribute.
class Args {
  public:
    Args& ref(Id id);
    Args& refOrNull(Id id); // 0 is "no instance": $
    Args& null();           // $: an optional attribute left out
    Args& derived();        // *: an attribute the schema derives
    Args& string(std::string_view utf8);
    Args& stringOrNull(std::string_view utf8); // $ for empty
    Args& real(double value);
    Args& integer(long long value);
    Args& enumeration(std::string_view value); // .VALUE.
    Args& enumerationOrNull(std::string_view value);
    Args& boolean(bool value);                 // .T. / .F.
    Args& refs(const std::vector<Id>& ids);    // (#1,#2)
    Args& reals(std::initializer_list<double> values);
    Args& reals(const std::vector<double>& values);
    // A typed value of a SELECT: IFCLENGTHMEASURE(1.5)
    Args& typed(std::string_view type, std::string_view encoded);
    Args& raw(std::string_view encoded); // already encoded

    [[nodiscard]] const std::string& text() const { return text_; }
    [[nodiscard]] bool nonFinite() const { return nonFinite_; }

  private:
    void separate();
    std::string text_;
    bool nonFinite_ = false;
};

// A list of already-encoded items: (a,b,c).
[[nodiscard]] std::string listOf(const std::vector<std::string>& items);
// A value typed by its IFC defined type: IFCLABEL('x').
[[nodiscard]] std::string typedValue(std::string_view type, std::string_view encoded);

class StepFile {
  public:
    // `entity` is the schema's spelling ("IfcPipeSegment"); it is written in
    // upper case, as ISO 10303-21 requires.
    Id add(std::string_view entity, const Args& args);

    [[nodiscard]] std::size_t size() const { return next_ - 1; }
    [[nodiscard]] const std::string& data() const { return data_; }
    [[nodiscard]] bool nonFinite() const { return nonFinite_; }
    // Instances added of each entity type, by the schema's spelling.
    [[nodiscard]] const std::map<std::string, std::size_t>& counts() const { return counts_; }

  private:
    std::string data_;
    Id next_ = 1;
    bool nonFinite_ = false;
    std::map<std::string, std::size_t> counts_;
};

} // namespace katana::ifc::detail
