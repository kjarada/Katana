#pragma once

// The plot style on a QPainter (docs/plotting.md, "Plot styles and output").
//
// The rule itself is the model's - cad::paperColour for a pen and
// cad::paperFillColour for a fill, both in plot.hpp - so that the plan
// painter, the sheet painter and anything drawn later agree on what a colour
// becomes. These are its QColor and QImage forms, for the painters: the sheet
// painter's pens, text and fills all pass through plotInk and plotFill, and
// every image it places (the logo, an image viewport, the 3D snapshot)
// through plotImage.

#include <QColor>
#include <QImage>

#include "katana/cad/plot.hpp"

namespace katana::qt {

// A pen's or a letter's colour on paper under `plot` (cad::paperColour).
[[nodiscard]] QColor plotInk(const QColor& colour, const katana::cad::PlotSettings& plot);

// An area fill's colour on paper (cad::paperFillColour). The paper's own
// white - a knock-out behind a label, the page - is not a fill and is never
// passed through this.
[[nodiscard]] QColor plotFill(const QColor& colour, const katana::cad::PlotSettings& plot);

// An image as the style prints it: unchanged in Colour; in Greyscale and
// Monochrome each pixel the grey of its luminance (cad::luminance), with its
// alpha kept, so a logo's transparent ground stays transparent. A photograph
// is not thresholded to black and white in Monochrome - that loses the
// picture - but printed grey, as a one-ink printer's halftone prints it.
// In ARGB32 when converted; a null image stays null.
[[nodiscard]] QImage plotImage(const QImage& image, const katana::cad::PlotSettings& plot);

// A painted sheet as one grey channel (Format_Grayscale8), each pixel the
// cad::luminance of its colour: what a PNG or TIFF of a greyscale or
// monochrome plot is written as, a third the size of RGB. Computed here
// rather than by QImage::convertToFormat, whose grey follows the image's
// colour space and the Qt version, so the same sheet always gives the same
// bytes. Alpha is ignored: a sheet is opaque. A null image stays null.
[[nodiscard]] QImage greyscaleRaster(const QImage& image);

} // namespace katana::qt
