#include <bit>
#include <charconv>
#include <cstdint>
#include <iostream>
#include <string>
#include <system_error>

#include "katana/core/text.hpp"

int main()
{
    std::string token;
    while (std::getline(std::cin, token)) {
        std::uint64_t bits = 0;
        const auto parsed = std::from_chars(token.data(), token.data() + token.size(), bits, 16);
        if (token.size() != 16 || parsed.ec != std::errc() ||
            parsed.ptr != token.data() + token.size()) {
            std::cerr << "expected one 16-digit hexadecimal double per line\n";
            return 2;
        }
        const double value = std::bit_cast<double>(bits);
        char general[64];
        const auto converted =
            std::to_chars(general, general + sizeof general, value, std::chars_format::general, 12);
        if (converted.ec != std::errc()) {
            std::cerr << "could not format the general-precision value\n";
            return 1;
        }
        std::cout << katana::core::formatExactReal(value) << '\t'
                  << std::string(general, converted.ptr);
        for (const int decimals : {-1, 0, 3, 12, 60, 61}) {
            std::cout << '\t' << katana::core::formatFixed(value, decimals);
        }
        std::cout << '\n';
    }
    return std::cin.bad() || !std::cout ? 1 : 0;
}