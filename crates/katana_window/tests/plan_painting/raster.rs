//! The raster canvas the painted twins draw on, and the one-page PDF the plot twins write.
//!
//! The C++ tests paint into a QImage through a QPainter and read it back; the Rust twins paint
//! into a canvas Image through the raster backend (style_painter's RasterBackend) and read it
//! back the same way. That backend is C's tiny-skia canvas, written beside this wave and not in
//! this worktree, so it cannot be named here yet (never code against an unbuilt API). SWAP: at
//! the integration the integrator points `backend()` - ONE line - at the adapter of C's raster
//! canvas to RasterBackend, and `pdf_writer()` - one line - at the adapter of C's PDF canvas to
//! PdfPageWriter. Until then every painted twin stops at the first paint, saying so; the twins of
//! pure logic run.

use std::path::Path;
use std::sync::Arc;

use katana_window::canvas::{Canvas, FontMetrics, Image};
use katana_window::customisation::style_painter::{CanvasRasterBackend, RasterBackend};
use katana_window::plan_painter::{CanvasPdfWriter, PdfPageWriter};

/// The raster backend every painted twin paints through: the window's own.
pub fn backend() -> Arc<dyn RasterBackend> {
    Arc::new(CanvasRasterBackend)
}

/// The one-page PDF writer the plot twins write through: the window's own, its file written
/// readable (uncompressed and spaced), as the C++ tests read QPdfWriter's.
pub fn pdf_writer() -> Box<dyn PdfPageWriter> {
    Box::new(CanvasPdfWriter { readable: true })
}

/// Paints `paint` onto `image` through the raster canvas, as `QPainter painter(&image)` and the
/// painting that follows it do in the C++.
pub fn paint_on(image: &mut Image, mut paint: impl FnMut(&mut dyn Canvas)) {
    backend().paint_image(image, &mut paint);
}

/// The raster canvas's font measures, as a QFontMetricsF of a font is.
pub fn metrics() -> Arc<dyn FontMetrics + Send + Sync> {
    backend().metrics()
}

/// Whether a file exists and holds anything (QFileInfo::size() > 0).
pub fn written(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.len() > 0)
}
