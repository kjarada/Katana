#pragma once

// A sheet raster as a TIFF file (docs/plotting.md, "Plot styles and output").
//
// Written here rather than through QImageWriter: Qt's TIFF support is a
// plugin (qtiff) that an installation may not have - the toolchain this
// project builds with does not - and a plot format that works on one machine
// and fails on the next is not a format. A baseline TIFF 6.0 file is a header,
// the pixel strips and one directory of tags, so it is written directly:
//
//   8-bit grey (BlackIsZero) for a Grayscale8 image, 24-bit RGB otherwise;
//   strips of about 256 KiB, each compressed with Deflate (compression 8,
//   through qCompress), which every TIFF reader in use decodes; the
//   resolution in dots per inch, so the raster prints at the scale it was
//   plotted at; and "Katana" as the software. Little-endian, one image.
//
// The same image gives the same bytes, so a plot is reproducible.

#include <QImage>
#include <QString>

#include "katana/core/error.hpp"

namespace katana::qt {

// Writes `image` to `path` at `dpi`. InvalidArgument for a null image or a
// resolution that is not positive; FileExportFailure when the file cannot be
// written.
[[nodiscard]] katana::core::Status writeTiff(const QImage& image, const QString& path, double dpi);

} // namespace katana::qt
