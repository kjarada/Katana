// The test trust root (test_key.hpp). Deliberately does not call the library's own hex
// reader or keyIdOf: the tests of those compare against THIS, so it must not be built
// from them.

#include "test_key.hpp"

#include <cstdint>
#include <string>

namespace katana::update {

namespace {

int nibble(char c)
{
    if (c >= '0' && c <= '9') {
        return c - '0';
    }
    return c - 'a' + 10; // the constants above are lower-case hexadecimal
}

TrustedKey keyFromHex(std::string_view hex)
{
    TrustedKey key;
    for (std::size_t i = 0; i < key.key.size(); ++i) {
        key.key[i] = static_cast<std::uint8_t>(nibble(hex[2 * i]) * 16 + nibble(hex[2 * i + 1]));
    }
    key.id = std::string(hex.substr(0, 16));
    return key;
}

} // namespace

const Trust& testTrust()
{
    static const Trust trust = [] {
        Trust built;
        built.keys.push_back(keyFromHex(kTestActivePublicHex));
        built.keys.push_back(keyFromHex(kTestSparePublicHex));
        built.placeholder = false;
        return built;
    }();
    return trust;
}

} // namespace katana::update
