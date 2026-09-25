#include "tiff_writer.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <vector>

#include <QByteArray>
#include <QSaveFile>

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

// TIFF 6.0, section 2: the field types used here.
constexpr std::uint16_t kAscii = 2;
constexpr std::uint16_t kShort = 3;
constexpr std::uint16_t kLong = 4;
constexpr std::uint16_t kRational = 5;

// The uncompressed bytes a strip holds, at most: large enough that Deflate
// has something to work with, small enough that a reader streams the image.
constexpr qsizetype kStripBytes = 256 * 1024;

void put16(QByteArray& out, std::uint16_t value)
{
    out.append(static_cast<char>(value & 0xFFu));
    out.append(static_cast<char>((value >> 8) & 0xFFu));
}

void put32(QByteArray& out, std::uint32_t value)
{
    for (int shift = 0; shift < 32; shift += 8) {
        out.append(static_cast<char>((value >> shift) & 0xFFu));
    }
}

void set32(QByteArray& out, qsizetype at, std::uint32_t value)
{
    for (int i = 0; i < 4; ++i) {
        out[at + i] = static_cast<char>((value >> (8 * i)) & 0xFFu);
    }
}

// One directory entry: its tag, type and count, and its value's bytes in
// file order - inline in the entry when they fit in four, elsewhere when not.
struct Field {
    std::uint16_t tag = 0;
    std::uint16_t type = kShort;
    std::uint32_t count = 1;
    QByteArray value;
};

Field shorts(std::uint16_t tag, std::initializer_list<std::uint16_t> values)
{
    Field field{tag, kShort, static_cast<std::uint32_t>(values.size()), {}};
    for (const std::uint16_t value : values) {
        put16(field.value, value);
    }
    return field;
}

Field longs(std::uint16_t tag, const std::vector<std::uint32_t>& values)
{
    Field field{tag, kLong, static_cast<std::uint32_t>(values.size()), {}};
    for (const std::uint32_t value : values) {
        put32(field.value, value);
    }
    return field;
}

// Dots per inch as a fraction: whole numbers exactly, others to a thousandth.
Field rational(std::uint16_t tag, double value)
{
    Field field{tag, kRational, 1, {}};
    if (value == std::floor(value) && value < 4.0e9) {
        put32(field.value, static_cast<std::uint32_t>(value));
        put32(field.value, 1);
    } else {
        put32(field.value, static_cast<std::uint32_t>(std::lround(std::min(value, 4.0e6) * 1000.0)));
        put32(field.value, 1000);
    }
    return field;
}

} // namespace

Status writeTiff(const QImage& image, const QString& path, double dpi)
{
    if (image.isNull()) {
        return makeError(ErrorCode::InvalidArgument, "there is no image to write");
    }
    if (!(dpi > 0.0) || !std::isfinite(dpi)) {
        return makeError(ErrorCode::InvalidArgument, "the resolution must be positive");
    }
    const bool grey = image.format() == QImage::Format_Grayscale8;
    const QImage pixels = grey ? image : image.convertToFormat(QImage::Format_RGB888);
    const std::uint16_t samples = grey ? 1 : 3;
    const auto width = static_cast<std::uint32_t>(pixels.width());
    const auto height = static_cast<std::uint32_t>(pixels.height());
    const qsizetype rowBytes = static_cast<qsizetype>(width) * samples;
    const auto rowsPerStrip = static_cast<std::uint32_t>(std::max<qsizetype>(kStripBytes / rowBytes, 1));

    QByteArray file;
    file.append("II", 2); // little-endian
    put16(file, 42);
    put32(file, 0); // the directory's offset, once it is known

    // The strips, each a zlib stream: qCompress's output less the four-byte
    // length it puts in front.
    std::vector<std::uint32_t> offsets;
    std::vector<std::uint32_t> counts;
    QByteArray strip;
    for (std::uint32_t row = 0; row < height; row += rowsPerStrip) {
        const std::uint32_t rows = std::min(rowsPerStrip, height - row);
        strip.clear();
        for (std::uint32_t r = 0; r < rows; ++r) {
            strip.append(reinterpret_cast<const char*>(pixels.constScanLine(static_cast<int>(row + r))),
                         rowBytes);
        }
        const QByteArray packed = qCompress(strip).mid(4);
        offsets.push_back(static_cast<std::uint32_t>(file.size()));
        counts.push_back(static_cast<std::uint32_t>(packed.size()));
        file.append(packed);
        if (file.size() > std::numeric_limits<std::int32_t>::max()) {
            return makeError(ErrorCode::FileExportFailure,
                             "the raster is too large for a TIFF file (over 2 GB compressed)",
                             path.toStdString());
        }
    }

    // The directory, its tags in ascending order as TIFF requires, on a word
    // boundary; values longer than four bytes follow it.
    std::vector<Field> fields;
    fields.push_back(longs(256, {width}));  // ImageWidth
    fields.push_back(longs(257, {height})); // ImageLength
    fields.push_back(grey ? shorts(258, {8}) : shorts(258, {8, 8, 8})); // BitsPerSample
    fields.push_back(shorts(259, {8}));                                  // Compression: Deflate
    fields.push_back(shorts(262, {static_cast<std::uint16_t>(grey ? 1 : 2)})); // BlackIsZero, RGB
    fields.push_back(longs(273, offsets));                               // StripOffsets
    fields.push_back(shorts(277, {samples}));                            // SamplesPerPixel
    fields.push_back(longs(278, {rowsPerStrip}));                        // RowsPerStrip
    fields.push_back(longs(279, counts));                                // StripByteCounts
    fields.push_back(rational(282, dpi));                                // XResolution
    fields.push_back(rational(283, dpi));                                // YResolution
    fields.push_back(shorts(284, {1}));                                  // PlanarConfiguration
    fields.push_back(shorts(296, {2}));                                  // ResolutionUnit: inch
    Field software{305, kAscii, 7, QByteArray("Katana", 7)};             // with its NUL
    fields.push_back(software);

    if (file.size() % 2 != 0) {
        file.append('\0');
    }
    const auto directory = static_cast<std::uint32_t>(file.size());
    set32(file, 4, directory);
    std::uint32_t beyond = directory + 2 + 12 * static_cast<std::uint32_t>(fields.size()) + 4;
    QByteArray values;
    put16(file, static_cast<std::uint16_t>(fields.size()));
    for (const Field& field : fields) {
        put16(file, field.tag);
        put16(file, field.type);
        put32(file, field.count);
        if (field.value.size() <= 4) {
            QByteArray inline4 = field.value;
            inline4.append(QByteArray(4 - inline4.size(), '\0'));
            file.append(inline4);
        } else {
            put32(file, beyond + static_cast<std::uint32_t>(values.size()));
            values.append(field.value);
            if (values.size() % 2 != 0) {
                values.append('\0');
            }
        }
    }
    put32(file, 0); // no next directory
    file.append(values);

    QSaveFile out(path);
    if (!out.open(QIODevice::WriteOnly) || out.write(file) != file.size() || !out.commit()) {
        return makeError(ErrorCode::FileExportFailure, "could not write the TIFF file",
                         path.toStdString());
    }
    return {};
}

} // namespace katana::qt
