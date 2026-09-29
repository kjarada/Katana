// Force-included by tools/check_float_to_string.py into a -fsyntax-only
// pass; never part of the build. It makes every to_string of a
// floating-point value fail to compile, so the compiler lists them (the
// script says why that matters).
#pragma once

#include <string>
#include <type_traits>
#include <utility>

// Libraries with to_string functions of their own, read before the macro
// below exists, so that their include guards keep it out of them. The
// script leaves them out on a second try, for a file whose own quoted()
// meets the std::quoted they bring.
#if !defined(KATANA_CHECK_NO_PREINCLUDE)
#include <bitset>
#if __has_include(<boost/multiprecision/cpp_int.hpp>)
#include <boost/multiprecision/cpp_int.hpp>
#endif
#if __has_include(<nlohmann/json.hpp>)
#include <nlohmann/json.hpp>
#endif
#endif

namespace katana_check_detail {
template <class T>
constexpr bool isReal = std::is_floating_point_v<std::remove_cvref_t<T>>;
}

// std::to_string(x), and a using-declaration of it. A lambda as a default
// template argument is a new type at every call, so each call is its own
// instantiation and each is reported, not only a file's first.
namespace std {
template <class T, auto = [] {}>
string katana_to_string_check(T value)
{
    static_assert(!katana_check_detail::isReal<T>, "KATANA_TO_STRING_OF_FLOATING_POINT");
    return to_string(value);
}
} // namespace std

// An unqualified to_string(x): std's, or another library's found by
// argument-dependent lookup.
template <auto = [] {}, class... A>
decltype(auto) katana_to_string_check(A&&... args)
{
    static_assert(!(sizeof...(A) == 1 && (katana_check_detail::isReal<A> && ...)),
                  "KATANA_TO_STRING_OF_FLOATING_POINT");
    using std::to_string;
    return to_string(std::forward<A>(args)...);
}

// Variadic, so that a member to_string taking more than one argument
// (std::bitset's) keeps its count when it is renamed with its header.
#define to_string(...) katana_to_string_check(__VA_ARGS__)
