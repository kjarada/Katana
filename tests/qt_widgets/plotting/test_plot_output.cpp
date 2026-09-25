// A sheet set plotted to files and to a printer (src/katana_qt/plotting/
// plot_output and tiff_writer): which files each format writes and what they
// are called, their pages and pixels, progress and cancel, and the requests
// refused before anything is written.

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <tuple>
#include <vector>

#include <QByteArray>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QImageReader>
#include <QPrinter>
#include <QTemporaryDir>

#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/page_setup.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/entity/model.hpp"
#include "plotting/plot_output.hpp"
#include "plotting/tiff_writer.hpp"

using katana::cad::PlotColourMode;
using katana::core::ErrorCode;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::qt::PlotFormat;
using katana::qt::PlotReport;
using katana::qt::PlotRequest;
using katana::qt::SheetPaintCache;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

namespace {

// Three A3 sheets, ONE, TWO and THREE, of set C, each a plan of a red line.
struct Fixture {
    Fixture()
    {
        Entity line;
        line.geometry = Segment2{Point2(-50.0, 0.0), Point2(50.0, 20.0)};
        line.color = Color{255, 0, 0, 255};
        EXPECT_TRUE(model.entities.add(std::move(line)).ok());
        source.plan.model = &model;
        set.defaults.setNumber = "C";
        const char* names[] = {"ONE", "TWO", "THREE"};
        for (int i = 0; i < 3; ++i) {
            plotting::Sheet sheet;
            sheet.id = "s" + std::to_string(i + 1);
            sheet.name = names[i];
            plotting::Viewport plan;
            plan.id = "vp" + std::to_string(i + 1);
            plan.kind = plotting::ViewportKind::Plan;
            plan.rect = Box2(Point2(30.0, 40.0), Point2(400.0, 280.0));
            plan.scale = 500.0;
            sheet.viewports.push_back(plan);
            set.sheets.push_back(sheet);
        }
    }

    [[nodiscard]] katana::core::Result<PlotReport> plot(const PlotRequest& request,
                                                        const katana::qt::PlotProgress& progress = {})
    {
        return katana::qt::plotSheets(set, request, source, cache, progress);
    }

    Model model;
    SheetSource source;
    plotting::SheetSet set;
    SheetPaintCache cache;
};

PlotRequest request(PlotFormat format, const QString& destination, std::string sheets = {})
{
    PlotRequest out;
    out.format = format;
    out.destination = destination;
    out.sheets = std::move(sheets);
    return out;
}

QByteArray contents(const QString& path)
{
    QFile file(path);
    return file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray();
}

// The /Page objects of a PDF, not counting the /Pages tree.
int pdfPages(const QByteArray& pdf)
{
    int pages = 0;
    for (qsizetype at = pdf.indexOf("/Type /Page"); at >= 0; at = pdf.indexOf("/Type /Page", at + 1)) {
        pages += pdf.mid(at + 11, 1) != "s" ? 1 : 0;
    }
    return pages;
}

QStringList filesIn(const QString& folder)
{
    return QDir(folder).entryList(QDir::Files, QDir::Name);
}

// A baseline TIFF as tiff_writer writes it, read back: its tags' values
// (a rational as numerator and denominator) and its pixels, the strips
// inflated and joined.
struct Tiff {
    std::map<std::uint16_t, std::vector<std::uint32_t>> tags;
    std::string software;
    QByteArray pixels;
    [[nodiscard]] std::uint32_t tag(std::uint16_t id) const { return tags.at(id).at(0); }
};

std::optional<Tiff> readTiff(const QByteArray& file)
{
    if (file.size() < 8 || file.left(4) != QByteArray("II*\0", 4)) {
        return std::nullopt;
    }
    const auto u16 = [&file](qsizetype at) {
        return static_cast<std::uint16_t>(static_cast<std::uint8_t>(file[at]) |
                                          static_cast<std::uint8_t>(file[at + 1]) << 8);
    };
    const auto u32 = [&](qsizetype at) {
        return static_cast<std::uint32_t>(u16(at)) | static_cast<std::uint32_t>(u16(at + 2)) << 16;
    };
    Tiff tiff;
    const std::uint32_t directory = u32(4);
    const std::uint16_t entries = u16(directory);
    for (std::uint16_t i = 0; i < entries; ++i) {
        const qsizetype entry = directory + 2 + 12 * i;
        const std::uint16_t id = u16(entry);
        const std::uint16_t type = u16(entry + 2);
        const std::uint32_t count = u32(entry + 4);
        const std::uint32_t size = type == 3 ? 2 : type == 4 ? 4 : type == 5 ? 8 : 1;
        const qsizetype at = size * count <= 4 ? entry + 8 : u32(entry + 8);
        std::vector<std::uint32_t> values;
        for (std::uint32_t n = 0; n < count; ++n) {
            if (type == 3) {
                values.push_back(u16(at + 2 * n));
            } else if (type == 4) {
                values.push_back(u32(at + 4 * n));
            } else if (type == 5) {
                values.push_back(u32(at + 8 * n));
                values.push_back(u32(at + 8 * n + 4));
            }
        }
        if (type == 2) {
            tiff.software = std::string(file.constData() + at, count - 1);
        }
        tiff.tags[id] = values;
    }
    const std::uint32_t width = tiff.tag(256);
    const std::uint32_t height = tiff.tag(257);
    const std::uint32_t rowBytes = width * tiff.tag(277);
    const std::uint32_t rowsPerStrip = tiff.tag(278);
    const auto& offsets = tiff.tags.at(273);
    const auto& counts = tiff.tags.at(279);
    for (std::size_t s = 0; s < offsets.size(); ++s) {
        const std::uint32_t rows = std::min(rowsPerStrip, height - static_cast<std::uint32_t>(s) * rowsPerStrip);
        const std::uint32_t expected = rows * rowBytes;
        // qUncompress wants the inflated size in front, big-endian.
        QByteArray packed;
        for (int shift = 24; shift >= 0; shift -= 8) {
            packed.append(static_cast<char>((expected >> shift) & 0xFFu));
        }
        packed.append(file.mid(offsets[s], counts[s]));
        const QByteArray strip = qUncompress(packed);
        if (strip.size() != static_cast<qsizetype>(expected)) {
            return std::nullopt;
        }
        tiff.pixels.append(strip);
    }
    return tiff;
}

// The pixels of `image` as a TIFF holds them: rows of RGB, or of grey.
QByteArray tiffPixelsOf(const QImage& image)
{
    const bool grey = image.format() == QImage::Format_Grayscale8;
    const QImage rows = grey ? image : image.convertToFormat(QImage::Format_RGB888);
    QByteArray out;
    for (int y = 0; y < rows.height(); ++y) {
        out.append(reinterpret_cast<const char*>(rows.constScanLine(y)),
                   static_cast<qsizetype>(rows.width()) * (grey ? 1 : 3));
    }
    return out;
}

} // namespace

TEST(PlotOutput, FormatsAreNamedReadAndGivenTheirExtension)
{
    for (const PlotFormat format : {PlotFormat::Pdf, PlotFormat::PdfPerSheet, PlotFormat::Png, PlotFormat::Tiff}) {
        EXPECT_EQ(katana::qt::plotFormatFrom(katana::qt::toString(format)), format);
    }
    EXPECT_EQ(katana::qt::plotFormatFrom("TIF"), PlotFormat::Tiff);
    EXPECT_EQ(katana::qt::plotFormatFrom("pdf-per-sheet"), PlotFormat::PdfPerSheet);
    EXPECT_FALSE(katana::qt::plotFormatFrom("jpeg").has_value());
    EXPECT_EQ(katana::qt::plotFormatExtension(PlotFormat::Tiff), ".tif");
    EXPECT_EQ(katana::qt::plotFormatExtension(PlotFormat::PdfPerSheet), ".pdf");
    EXPECT_FALSE(katana::qt::plotsFilePerSheet(PlotFormat::Pdf));
    EXPECT_TRUE(katana::qt::plotsFilePerSheet(PlotFormat::Png));
}

TEST(PlotOutput, ARequestComesFromThePageSetupAndGoesBackToIt)
{
    plotting::PageSetup setup;
    setup.colourMode = PlotColourMode::Monochrome;
    setup.lineWeightScale = 0.7;
    setup.dpi = 150.0;
    setup.fileNamePattern = "{number} {name}";
    setup.filePerSheet = true;
    const PlotRequest asked = katana::qt::plotRequestFor(setup, "C:/out", "2-3");
    EXPECT_EQ(asked.format, PlotFormat::PdfPerSheet);
    EXPECT_EQ(asked.colourMode, PlotColourMode::Monochrome);
    EXPECT_EQ(asked.lineWeightScale, 0.7);
    EXPECT_EQ(asked.dpi, 150.0);
    EXPECT_EQ(asked.fileNamePattern, "{number} {name}");
    EXPECT_EQ(asked.sheets, "2-3");
    EXPECT_EQ(asked.destination, "C:/out");
    // And back: the same setup; one PDF turns the file per sheet off; a
    // raster leaves it as it was.
    EXPECT_EQ(katana::qt::pageSetupFor(asked, plotting::PageSetup{}), setup);
    PlotRequest single = asked;
    single.format = PlotFormat::Pdf;
    EXPECT_FALSE(katana::qt::pageSetupFor(single, setup).filePerSheet);
    PlotRequest raster = asked;
    raster.format = PlotFormat::Png;
    EXPECT_TRUE(katana::qt::pageSetupFor(raster, setup).filePerSheet);
    EXPECT_FALSE(katana::qt::pageSetupFor(raster, plotting::PageSetup{}).filePerSheet);
}

TEST(PlotOutput, OnePdfHoldsTheChosenSheetsInTheOrderChosen)
{
    Fixture fixture;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    const QString path = dir.filePath("nested/folder/set.pdf"); // made when missing
    const auto report = fixture.plot(request(PlotFormat::Pdf, path, "3,1"));
    ASSERT_TRUE(report.ok()) << report.error().describe();
    EXPECT_EQ(report->files, (std::vector<QString>{path}));
    EXPECT_EQ(report->sheets, (std::vector<std::size_t>{2, 0}));
    EXPECT_EQ(report->sheetsAsked, 2u);
    EXPECT_FALSE(report->cancelled);
    EXPECT_TRUE(report->problems.empty());
    const QByteArray pdf = contents(path);
    EXPECT_TRUE(pdf.startsWith("%PDF"));
    EXPECT_EQ(pdfPages(pdf), 2);
    EXPECT_EQ(report->summary(request(PlotFormat::Pdf, path)),
              QString("Plotted 2 sheets to %1.").arg(QDir::toNativeSeparators(path)));

    // Everything: three pages.
    ASSERT_TRUE(fixture.plot(request(PlotFormat::Pdf, path)).ok());
    EXPECT_EQ(pdfPages(contents(path)), 3);
    // No temporary file is left beside it.
    EXPECT_EQ(filesIn(dir.filePath("nested/folder")), QStringList{"set.pdf"});
}

TEST(PlotOutput, APdfASheetIsNamedByThePatternInTheFolder)
{
    Fixture fixture;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    const QString folder = dir.filePath("sheets");
    const PlotRequest asked = request(PlotFormat::PdfPerSheet, folder);
    const auto planned = katana::qt::plannedFiles(fixture.set, asked);
    ASSERT_TRUE(planned.ok()) << planned.error().describe();
    const auto report = fixture.plot(asked);
    ASSERT_TRUE(report.ok()) << report.error().describe();
    EXPECT_EQ(report->files, *planned);
    // The default pattern: the set number, the sheet number to two places,
    // the sheet's name.
    EXPECT_EQ(filesIn(folder), (QStringList{"C01 ONE.pdf", "C02 TWO.pdf", "C03 THREE.pdf"}));
    for (const QString& file : report->files) {
        EXPECT_EQ(pdfPages(contents(file)), 1) << file.toStdString();
    }
    EXPECT_EQ(report->summary(asked), QString("Plotted 3 sheets to 3 PDF files in %1.")
                                          .arg(QDir::toNativeSeparators(folder)));

    // Another pattern, some of the sheets.
    PlotRequest named = request(PlotFormat::PdfPerSheet, dir.filePath("named"), "s2,3");
    named.fileNamePattern = "{id} of {N} - {name}";
    ASSERT_TRUE(fixture.plot(named).ok());
    EXPECT_EQ(filesIn(dir.filePath("named")), (QStringList{"s2 of 3 - TWO.pdf", "s3 of 3 - THREE.pdf"}));
}

TEST(PlotOutput, RastersAreAFileASheetAtTheChosenResolution)
{
    Fixture fixture;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    PlotRequest png = request(PlotFormat::Png, dir.filePath("png"), "2");
    png.dpi = 50.0;
    const auto report = fixture.plot(png);
    ASSERT_TRUE(report.ok()) << report.error().describe();
    ASSERT_EQ(report->files.size(), 1u);
    EXPECT_EQ(QFileInfo(report->files[0]).fileName(), "C02 TWO.png");
    const QImage image(report->files[0]);
    // A3 at 50 dpi: 420 and 297 mm are 826.8 and 584.6 pixels.
    EXPECT_EQ(image.size(), QSize(827, 585));
    EXPECT_EQ(image.dotsPerMeterX(), 1969); // 50 / 0.0254
    EXPECT_NE(image.format(), QImage::Format_Grayscale8);
    // The red line is in it, red: at 50 dpi a thin line is under a pixel
    // wide and antialiased to a pink, so red is what stands out.
    int red = 0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            const QRgb pixel = image.pixel(x, y);
            red += qRed(pixel) - qGreen(pixel) > 60 && qGreen(pixel) == qBlue(pixel) ? 1 : 0;
        }
    }
    EXPECT_GT(red, 50);

    // The TIFF of the same sheet holds the same pixels, at the same
    // resolution, deflated, and says what wrote it.
    PlotRequest tiff = png;
    tiff.format = PlotFormat::Tiff;
    tiff.destination = dir.filePath("tiff");
    const auto tiffReport = fixture.plot(tiff);
    ASSERT_TRUE(tiffReport.ok()) << tiffReport.error().describe();
    ASSERT_EQ(tiffReport->files.size(), 1u);
    EXPECT_EQ(QFileInfo(tiffReport->files[0]).fileName(), "C02 TWO.tif");
    const auto read = readTiff(contents(tiffReport->files[0]));
    ASSERT_TRUE(read.has_value());
    EXPECT_EQ(read->tag(256), 827u);
    EXPECT_EQ(read->tag(257), 585u);
    EXPECT_EQ(read->tags.at(258), (std::vector<std::uint32_t>{8, 8, 8}));
    EXPECT_EQ(read->tag(259), 8u); // Deflate
    EXPECT_EQ(read->tag(262), 2u); // RGB
    EXPECT_EQ(read->tags.at(282), (std::vector<std::uint32_t>{50, 1}));
    EXPECT_EQ(read->tag(296), 2u); // inches
    EXPECT_EQ(read->software, "Katana");
    EXPECT_TRUE(read->pixels == tiffPixelsOf(image));
}

TEST(PlotOutput, AGreyscaleOrMonochromeRasterIsOneGreyChannel)
{
    Fixture fixture;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    for (const PlotColourMode mode : {PlotColourMode::Greyscale, PlotColourMode::Monochrome}) {
        const QString folder = dir.filePath(QString::fromUtf8(katana::cad::toString(mode)));
        PlotRequest png = request(PlotFormat::Png, folder, "1");
        png.dpi = 50.0;
        png.colourMode = mode;
        const auto report = fixture.plot(png);
        ASSERT_TRUE(report.ok()) << report.error().describe();
        const QImage image(report->files.at(0));
        EXPECT_EQ(image.format(), QImage::Format_Grayscale8) << katana::cad::toString(mode);

        PlotRequest tiff = png;
        tiff.format = PlotFormat::Tiff;
        const auto tiffReport = fixture.plot(tiff);
        ASSERT_TRUE(tiffReport.ok()) << tiffReport.error().describe();
        const auto read = readTiff(contents(tiffReport->files.at(0)));
        ASSERT_TRUE(read.has_value());
        EXPECT_EQ(read->tags.at(258), (std::vector<std::uint32_t>{8}));
        EXPECT_EQ(read->tag(262), 1u); // BlackIsZero
        EXPECT_EQ(read->tag(277), 1u);
        EXPECT_TRUE(read->pixels == tiffPixelsOf(image));
    }
}

TEST(PlotOutput, TheSameRequestWritesTheSameBytes)
{
    Fixture fixture;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    for (const PlotFormat format : {PlotFormat::Png, PlotFormat::Tiff}) {
        PlotRequest first = request(format, dir.filePath("a"), "1");
        first.dpi = 60.0;
        first.colourMode = PlotColourMode::Greyscale;
        PlotRequest second = first;
        second.destination = dir.filePath("b");
        const auto a = fixture.plot(first);
        const auto b = fixture.plot(second);
        ASSERT_TRUE(a.ok() && b.ok());
        EXPECT_TRUE(contents(a->files.at(0)) == contents(b->files.at(0))) << katana::qt::toString(format);
    }
}

TEST(PlotOutput, ProgressIsToldOfEverySheetAndTheEnd)
{
    Fixture fixture;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    std::vector<std::tuple<std::size_t, std::size_t, std::string>> calls;
    const auto report = fixture.plot(request(PlotFormat::Pdf, dir.filePath("set.pdf"), "2-3"),
                                     [&calls](std::size_t done, std::size_t total, const std::string& name) {
                                         calls.emplace_back(done, total, name);
                                         return true;
                                     });
    ASSERT_TRUE(report.ok()) << report.error().describe();
    EXPECT_EQ(calls, (std::vector<std::tuple<std::size_t, std::size_t, std::string>>{
                         {0, 2, "TWO"}, {1, 2, "THREE"}, {2, 2, ""}}));
}

TEST(PlotOutput, TheSourceIsAskedForAgainBeforeEachSheet)
{
    Fixture fixture;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    int asked = 0;
    const auto report = katana::qt::plotSheets(
        fixture.set, request(PlotFormat::Pdf, dir.filePath("set.pdf")),
        [&] {
            ++asked;
            return fixture.source;
        },
        fixture.cache);
    ASSERT_TRUE(report.ok()) << report.error().describe();
    EXPECT_EQ(asked, 3);
}

TEST(PlotOutput, ACancelledPdfIsNotWrittenAndLeavesAnEarlierOneAsItWas)
{
    Fixture fixture;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    const QString path = dir.filePath("set.pdf");
    {
        QFile earlier(path);
        ASSERT_TRUE(earlier.open(QIODevice::WriteOnly));
        earlier.write("an earlier plot");
    }
    const PlotRequest asked = request(PlotFormat::Pdf, path);
    const auto report = fixture.plot(asked, [](std::size_t done, std::size_t, const std::string&) {
        return done < 1; // stop before the second sheet
    });
    ASSERT_TRUE(report.ok()) << report.error().describe();
    EXPECT_TRUE(report->cancelled);
    EXPECT_EQ(report->sheets, (std::vector<std::size_t>{0}));
    EXPECT_TRUE(report->files.empty());
    EXPECT_EQ(contents(path), QByteArray("an earlier plot"));
    EXPECT_EQ(filesIn(dir.path()), QStringList{"set.pdf"});
    EXPECT_EQ(report->summary(asked), "Cancelled after 1 of 3 sheets.");
}

TEST(PlotOutput, ACancelledFilePerSheetPlotKeepsTheFilesItFinished)
{
    Fixture fixture;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    for (const PlotFormat format : {PlotFormat::PdfPerSheet, PlotFormat::Png}) {
        const QString folder = dir.filePath(QString::fromUtf8(katana::qt::toString(format)));
        PlotRequest asked = request(format, folder);
        asked.dpi = 50.0;
        const auto report = fixture.plot(asked, [](std::size_t done, std::size_t, const std::string&) {
            return done < 2;
        });
        ASSERT_TRUE(report.ok()) << report.error().describe();
        EXPECT_TRUE(report->cancelled);
        EXPECT_EQ(report->sheets, (std::vector<std::size_t>{0, 1}));
        ASSERT_EQ(report->files.size(), 2u);
        const QString extension = katana::qt::plotFormatExtension(format);
        EXPECT_EQ(filesIn(folder), (QStringList{"C01 ONE" + extension, "C02 TWO" + extension}));
        EXPECT_EQ(report->summary(asked), QString("Cancelled after 2 of 3 sheets; 2 files kept in %1.")
                                              .arg(QDir::toNativeSeparators(folder)));
    }
}

TEST(PlotOutput, ARequestIsCheckedBeforeAnythingIsWritten)
{
    Fixture fixture;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    const auto refused = [&](const PlotRequest& asked) {
        const auto report = fixture.plot(asked);
        EXPECT_FALSE(report.ok());
        EXPECT_TRUE(filesIn(dir.path()).isEmpty());
        EXPECT_TRUE(QDir(dir.path()).entryList(QDir::Dirs | QDir::NoDotAndDotDot).isEmpty());
        return report.ok() ? std::string() : report.error().message;
    };
    EXPECT_EQ(refused(request(PlotFormat::Pdf, dir.filePath("set.pdf"), "4")),
              "there is no sheet 4: the set has 3 sheets");
    EXPECT_EQ(refused(request(PlotFormat::Pdf, "  ")), "choose the PDF file the plot goes in");
    EXPECT_EQ(refused(request(PlotFormat::Png, QString())), "choose the folder the plot's files go in");
    PlotRequest coarse = request(PlotFormat::Png, dir.filePath("out"));
    coarse.dpi = 20.0;
    EXPECT_EQ(refused(coarse), "the resolution must be 50 to 1200 dpi");
    PlotRequest thin = request(PlotFormat::Pdf, dir.filePath("set.pdf"));
    thin.lineWeightScale = 0.0;
    EXPECT_EQ(refused(thin), "the line weight scale must be 0.1 to 5");
    PlotRequest badPattern = request(PlotFormat::PdfPerSheet, dir.filePath("out"));
    badPattern.fileNamePattern = "{sheet}";
    EXPECT_NE(refused(badPattern).find("no token {sheet}"), std::string::npos);
    // One PDF names no file by the pattern, so its pattern is not asked about.
    PlotRequest single = request(PlotFormat::Pdf, dir.filePath("single.pdf"));
    single.fileNamePattern = "{sheet}";
    EXPECT_TRUE(katana::qt::validatePlotRequest(fixture.set, single).ok());

    // A raster too large to hold is refused with the resolution that fits.
    fixture.set.sheets[1].paper = katana::cad::PaperSize::A0;
    // Sheet 2 alone: the A3 sheets are too large at 1200 dpi as well.
    PlotRequest huge = request(PlotFormat::Tiff, dir.filePath("out"), "2");
    huge.dpi = 1200.0;
    EXPECT_EQ(refused(huge), "sheet 2 (TWO) at 1200 dpi is 2232 megapixels, more than the 200 a "
                             "raster may be: plot it at 359 dpi or less");
    // 359 dpi is 199.8 megapixels; 360 would be 200.9.
    huge.dpi = 359.0;
    EXPECT_TRUE(katana::qt::validatePlotRequest(fixture.set, huge).ok());
    huge.dpi = 360.0;
    EXPECT_FALSE(katana::qt::validatePlotRequest(fixture.set, huge).ok());
}

TEST(PlotOutput, PrintingPutsEachSheetOnAPageOfThePrinter)
{
    Fixture fixture;
    fixture.set.sheets[1].paper = katana::cad::PaperSize::A1;
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    // A printer that prints to a PDF: the pages it is given can be counted.
    QPrinter printer(QPrinter::HighResolution);
    printer.setOutputFormat(QPrinter::PdfFormat);
    const QString path = dir.filePath("printed.pdf");
    printer.setOutputFileName(path);
    PlotRequest asked;
    asked.sheets = "1-2";
    asked.colourMode = PlotColourMode::Monochrome;
    std::size_t told = 0;
    const auto report = katana::qt::printSheets(
        printer, fixture.set, asked, [&fixture] { return fixture.source; }, fixture.cache,
        [&told](std::size_t, std::size_t, const std::string&) {
            ++told;
            return true;
        });
    ASSERT_TRUE(report.ok()) << report.error().describe();
    EXPECT_EQ(report->sheets, (std::vector<std::size_t>{0, 1}));
    EXPECT_TRUE(report->files.empty());
    EXPECT_EQ(report->printer, QDir::toNativeSeparators(path));
    // A PDF printer takes any paper, so nothing was made smaller.
    EXPECT_TRUE(report->shrunk.empty());
    EXPECT_EQ(told, 3u);
    const QByteArray pdf = contents(path);
    EXPECT_EQ(pdfPages(pdf), 2);
    // The A1 page is 841 x 594 mm: 2384 x 1684 points.
    EXPECT_TRUE(pdf.contains("2383.9") || pdf.contains("2384"));
    EXPECT_EQ(report->summary(asked),
              QString("Printed 2 sheets on %1.").arg(QDir::toNativeSeparators(path)));

    // A printer the system does not have is named, not replaced by another.
    const auto missing = katana::qt::printSheets(QStringLiteral("No Printer By This Name"), fixture.set,
                                                 asked, [&fixture] { return fixture.source; },
                                                 fixture.cache);
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    EXPECT_EQ(missing.error().message, "there is no printer of that name");
}

TEST(TiffWriter, WritesGreyAndColourPixelsExactly)
{
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    // Tall enough for several strips: 300 x 1000 of RGB is 900 kB.
    QImage colour(300, 1000, QImage::Format_RGB32);
    for (int y = 0; y < colour.height(); ++y) {
        for (int x = 0; x < colour.width(); ++x) {
            colour.setPixel(x, y, qRgb(x % 256, y % 256, (x * y) % 256));
        }
    }
    const QString path = dir.filePath("colour.tif");
    ASSERT_TRUE(katana::qt::writeTiff(colour, path, 254.5).ok());
    const auto read = readTiff(contents(path));
    ASSERT_TRUE(read.has_value());
    EXPECT_GT(read->tags.at(273).size(), 1u);
    EXPECT_EQ(read->tags.at(282), (std::vector<std::uint32_t>{254500, 1000}));
    EXPECT_TRUE(read->pixels == tiffPixelsOf(colour));

    QImage grey(17, 5, QImage::Format_Grayscale8);
    for (int y = 0; y < grey.height(); ++y) {
        for (int x = 0; x < grey.width(); ++x) {
            grey.scanLine(y)[x] = static_cast<uchar>(x * 15 + y);
        }
    }
    ASSERT_TRUE(katana::qt::writeTiff(grey, dir.filePath("grey.tif"), 300.0).ok());
    const auto readGrey = readTiff(contents(dir.filePath("grey.tif")));
    ASSERT_TRUE(readGrey.has_value());
    EXPECT_EQ(readGrey->tag(262), 1u);
    EXPECT_TRUE(readGrey->pixels == tiffPixelsOf(grey));
}

TEST(TiffWriter, RefusesWhatItCannotWrite)
{
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    EXPECT_EQ(katana::qt::writeTiff(QImage(), dir.filePath("a.tif"), 300.0).error().code,
              ErrorCode::InvalidArgument);
    QImage image(4, 4, QImage::Format_RGB32);
    image.fill(Qt::white);
    EXPECT_EQ(katana::qt::writeTiff(image, dir.filePath("a.tif"), 0.0).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::qt::writeTiff(image, dir.filePath("no/such/folder/a.tif"), 300.0).error().code,
              ErrorCode::FileExportFailure);
}
