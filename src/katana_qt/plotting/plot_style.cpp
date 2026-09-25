#include "plot_style.hpp"

#include <cstdint>

namespace katana::qt {

namespace {

katana::entity::Color modelColour(const QColor& colour)
{
    const QColor rgb = colour.toRgb();
    return {static_cast<std::uint8_t>(rgb.red()), static_cast<std::uint8_t>(rgb.green()),
            static_cast<std::uint8_t>(rgb.blue()), static_cast<std::uint8_t>(rgb.alpha())};
}

QColor qtColour(const katana::entity::Color& colour)
{
    return QColor(colour.r, colour.g, colour.b, colour.a);
}

// The one grey a pixel becomes, whatever the image.
std::uint8_t greyOf(QRgb pixel)
{
    return katana::cad::luminance({static_cast<std::uint8_t>(qRed(pixel)),
                                   static_cast<std::uint8_t>(qGreen(pixel)),
                                   static_cast<std::uint8_t>(qBlue(pixel)), 255});
}

} // namespace

QColor plotInk(const QColor& colour, const katana::cad::PlotSettings& plot)
{
    return qtColour(katana::cad::paperColour(modelColour(colour), plot));
}

QColor plotFill(const QColor& colour, const katana::cad::PlotSettings& plot)
{
    return qtColour(katana::cad::paperFillColour(modelColour(colour), plot));
}

QImage plotImage(const QImage& image, const katana::cad::PlotSettings& plot)
{
    if (image.isNull() || plot.colourMode == katana::cad::PlotColourMode::Colour) {
        return image;
    }
    // Straight (not premultiplied) ARGB, so the luminance is the colour's and
    // not the colour darkened by its own transparency.
    QImage grey = image.convertToFormat(QImage::Format_ARGB32);
    for (int y = 0; y < grey.height(); ++y) {
        auto* row = reinterpret_cast<QRgb*>(grey.scanLine(y));
        for (int x = 0; x < grey.width(); ++x) {
            const std::uint8_t level = greyOf(row[x]);
            row[x] = qRgba(level, level, level, qAlpha(row[x]));
        }
    }
    return grey;
}

QImage greyscaleRaster(const QImage& image)
{
    if (image.isNull()) {
        return image;
    }
    const QImage rgb = image.convertToFormat(QImage::Format_RGB32);
    QImage grey(rgb.size(), QImage::Format_Grayscale8);
    grey.setDotsPerMeterX(image.dotsPerMeterX());
    grey.setDotsPerMeterY(image.dotsPerMeterY());
    for (int y = 0; y < rgb.height(); ++y) {
        const auto* in = reinterpret_cast<const QRgb*>(rgb.constScanLine(y));
        std::uint8_t* out = grey.scanLine(y);
        for (int x = 0; x < rgb.width(); ++x) {
            out[x] = greyOf(in[x]);
        }
    }
    return grey;
}

} // namespace katana::qt
