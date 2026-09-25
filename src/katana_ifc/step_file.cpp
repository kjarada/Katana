#include "step_file.hpp"

#include <cmath>

namespace katana::ifc::detail {

void Args::separate()
{
    if (!text_.empty()) {
        text_.push_back(',');
    }
}

Args& Args::ref(Id id)
{
    separate();
    text_ += '#';
    text_ += std::to_string(id);
    return *this;
}

Args& Args::refOrNull(Id id)
{
    return id == 0 ? null() : ref(id);
}

Args& Args::null()
{
    separate();
    text_ += '$';
    return *this;
}

Args& Args::derived()
{
    separate();
    text_ += '*';
    return *this;
}

Args& Args::string(std::string_view utf8)
{
    separate();
    text_ += stepString(utf8);
    return *this;
}

Args& Args::stringOrNull(std::string_view utf8)
{
    return utf8.empty() ? null() : string(utf8);
}

Args& Args::real(double value)
{
    nonFinite_ = nonFinite_ || !std::isfinite(value);
    separate();
    text_ += stepReal(value);
    return *this;
}

Args& Args::integer(long long value)
{
    separate();
    text_ += std::to_string(value);
    return *this;
}

Args& Args::enumeration(std::string_view value)
{
    separate();
    text_ += '.';
    text_ += value;
    text_ += '.';
    return *this;
}

Args& Args::enumerationOrNull(std::string_view value)
{
    return value.empty() ? null() : enumeration(value);
}

Args& Args::boolean(bool value)
{
    separate();
    text_ += value ? ".T." : ".F.";
    return *this;
}

Args& Args::refs(const std::vector<Id>& ids)
{
    separate();
    text_ += '(';
    for (std::size_t i = 0; i < ids.size(); ++i) {
        if (i > 0) {
            text_ += ',';
        }
        text_ += '#';
        text_ += std::to_string(ids[i]);
    }
    text_ += ')';
    return *this;
}

Args& Args::reals(std::initializer_list<double> values)
{
    return reals(std::vector<double>(values));
}

Args& Args::reals(const std::vector<double>& values)
{
    separate();
    text_ += '(';
    for (std::size_t i = 0; i < values.size(); ++i) {
        nonFinite_ = nonFinite_ || !std::isfinite(values[i]);
        if (i > 0) {
            text_ += ',';
        }
        text_ += stepReal(values[i]);
    }
    text_ += ')';
    return *this;
}

Args& Args::typed(std::string_view type, std::string_view encoded)
{
    separate();
    text_ += typedValue(type, encoded);
    return *this;
}

Args& Args::raw(std::string_view encoded)
{
    separate();
    text_ += encoded;
    return *this;
}

std::string listOf(const std::vector<std::string>& items)
{
    std::string out = "(";
    for (std::size_t i = 0; i < items.size(); ++i) {
        if (i > 0) {
            out += ',';
        }
        out += items[i];
    }
    out += ')';
    return out;
}

std::string typedValue(std::string_view type, std::string_view encoded)
{
    std::string out;
    out.reserve(type.size() + encoded.size() + 2);
    for (const char c : type) {
        out.push_back(c >= 'a' && c <= 'z' ? static_cast<char>(c - 'a' + 'A') : c);
    }
    out += '(';
    out += encoded;
    out += ')';
    return out;
}

Id StepFile::add(std::string_view entity, const Args& args)
{
    const Id id = next_++;
    nonFinite_ = nonFinite_ || args.nonFinite();
    data_ += '#';
    data_ += std::to_string(id);
    data_ += '=';
    for (const char c : entity) {
        data_.push_back(c >= 'a' && c <= 'z' ? static_cast<char>(c - 'a' + 'A') : c);
    }
    data_ += '(';
    data_ += args.text();
    data_ += ");\n";
    ++counts_[std::string(entity)];
    return id;
}

} // namespace katana::ifc::detail
