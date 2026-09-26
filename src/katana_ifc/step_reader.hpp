#pragma once

// An ISO 10303-21 file read into instances: the parse, with no knowledge of
// any schema. Internal to katana_ifc; import.cpp gives it meaning.
//
// The file is untrusted input, so what it may ask for is bounded: lists
// nest at most kMaximumDepth deep, a reference or an instance number must
// fit in 32 bits, and a structure the parser cannot follow is a
// ParseFailure naming the line rather than a guess - a guess would shift
// every attribute after it.

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::ifc::detail {

inline constexpr std::size_t kMaximumDepth = 64;

struct StepValue {
    enum class Kind : std::uint8_t {
        Null,    // $
        Derived, // *
        Integer,
        Real,
        String,      // decoded to UTF-8
        Enumeration, // .VALUE., without the dots; .T. and .F. are T and F
        Reference,   // #123
        List,
        Typed,  // IFCLABEL('x'): `text` the type, `items` the one value
        Binary, // "0FF": kept as written
    };
    Kind kind = Kind::Null;
    std::int64_t integer = 0;
    double real = 0.0;
    std::uint32_t reference = 0;
    std::string text;
    std::vector<StepValue> items;

    [[nodiscard]] bool isNull() const { return kind == Kind::Null || kind == Kind::Derived; }
    // A number, from an Integer, a Real, or a Typed wrapping one.
    [[nodiscard]] std::optional<double> number() const;
    // The text of a String, an Enumeration, or a Typed wrapping one.
    [[nodiscard]] const std::string* string() const;
};

struct StepInstance {
    std::string type; // upper case, as written
    std::vector<StepValue> arguments;
    std::size_t line = 0;
};

struct StepModel {
    std::string schema; // FILE_SCHEMA's first name: "IFC4X3_ADD2"
    std::vector<std::string> viewDefinitions;
    std::unordered_map<std::uint32_t, StepInstance> instances;
    std::vector<std::uint32_t> order; // instance numbers in file order
    std::size_t complexInstances = 0; // #1=(A()B()); read past, not modelled

    [[nodiscard]] const StepInstance* find(std::uint32_t id) const;
    // The instance `value` refers to, or nullptr.
    [[nodiscard]] const StepInstance* follow(const StepValue& value) const;
};

[[nodiscard]] katana::core::Result<StepModel> parseStep(std::string_view text);

} // namespace katana::ifc::detail
