// katana_make_icons <output-directory>
//
// A DEVELOPER tool, not part of the product and not built by default. It runs
// the same painting code the application uses and writes:
//
//   katana.ico        the application icon, 16 to 256 px, for the Windows
//                     resource in katana.rc and for an installer
//   katana.png        the 256 px icon, for a README or a store listing
//   icon_sheet.png    every toolbar icon on the theme's own ground, large
//                     enough to judge - the way to review the set without
//                     launching the application
//
// The .ico is COMMITTED (resources/katana.ico) rather than generated during
// the build. Generating it would make an ordinary build run a Qt program
// before it can compile a resource file, which fails wherever that program
// cannot start - a cross build, a CI image with no display plugin - and buys
// nothing, since the icon changes about once a year. Regenerate it with:
//
//   cmake --build build/release --target katana_make_icons
//   ./build/release/bin/katana_make_icons.exe resources

#include <QBuffer>
#include <QByteArray>
#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QImage>
#include <QPainter>

#include <cstdio>
#include <vector>

#include "icons.hpp"
#include "theme.hpp"

namespace {

void appendLittleEndian(QByteArray& bytes, quint32 value, int size)
{
    for (int i = 0; i < size; ++i) {
        bytes.append(static_cast<char>((value >> (8 * i)) & 0xffu));
    }
}

// An .ico holding one PNG per size. PNG-compressed entries have been valid
// since Windows Vista and are a fraction of the size of the BMP form; the
// header is six bytes and each directory entry sixteen (Microsoft, "Icons",
// ICONDIR and ICONDIRENTRY).
bool writeIco(const QString& path, const std::vector<int>& sizes)
{
    std::vector<QByteArray> images;
    for (const int size : sizes) {
        QByteArray png;
        QBuffer buffer(&png);
        buffer.open(QIODevice::WriteOnly);
        if (!katana::qt::applicationIconImage(size).save(&buffer, "PNG")) {
            return false;
        }
        images.push_back(png);
    }
    QByteArray file;
    appendLittleEndian(file, 0, 2);                                  // reserved
    appendLittleEndian(file, 1, 2);                                  // 1 = icon
    appendLittleEndian(file, static_cast<quint32>(sizes.size()), 2); // image count
    quint32 offset = 6u + 16u * static_cast<quint32>(sizes.size());
    for (std::size_t i = 0; i < sizes.size(); ++i) {
        // A width or height of 256 is written as 0: the field is one byte.
        const quint32 dimension = sizes[i] >= 256 ? 0u : static_cast<quint32>(sizes[i]);
        appendLittleEndian(file, dimension, 1);
        appendLittleEndian(file, dimension, 1);
        appendLittleEndian(file, 0, 1);  // palette size: none
        appendLittleEndian(file, 0, 1);  // reserved
        appendLittleEndian(file, 1, 2);  // colour planes
        appendLittleEndian(file, 32, 2); // bits per pixel
        appendLittleEndian(file, static_cast<quint32>(images[i].size()), 4);
        appendLittleEndian(file, offset, 4);
        offset += static_cast<quint32>(images[i].size());
    }
    for (const QByteArray& image : images) {
        file.append(image);
    }
    QFile out(path);
    return out.open(QIODevice::WriteOnly) && out.write(file) == file.size();
}

// Every icon at 96 px, eight to a row, on the window colour, with the
// application icon at the end. Large, because the point is to look at them.
bool writeSheet(const QString& path)
{
    const auto& icons = katana::qt::allIcons();
    constexpr int kCell = 120;
    constexpr int kIcon = 96;
    constexpr int kColumns = 8;
    const int rows = (static_cast<int>(icons.size()) + kColumns - 1) / kColumns + 3;
    QImage sheet(kColumns * kCell, rows * kCell, QImage::Format_ARGB32_Premultiplied);
    sheet.fill(katana::qt::theme::window());
    QPainter painter(&sheet);
    for (std::size_t i = 0; i < icons.size(); ++i) {
        const int column = static_cast<int>(i) % kColumns;
        const int row = static_cast<int>(i) / kColumns;
        const QRectF cell(column * kCell + (kCell - kIcon) / 2.0, row * kCell + (kCell - kIcon) / 2.0,
                          kIcon, kIcon);
        katana::qt::paintIcon(painter, icons[i], cell, katana::qt::theme::text(),
                              katana::qt::theme::accent());
    }
    // The same icons at the size they are actually used, 20 px, in one row:
    // what reads at 96 does not always read at 20, and 20 is what ships.
    const int smallRow = (rows - 3) * kCell + 30;
    for (std::size_t i = 0; i < icons.size(); ++i) {
        const QRectF cell(16.0 + static_cast<double>(i) * 27.0, smallRow, 20, 20);
        katana::qt::paintIcon(painter, icons[i], cell, katana::qt::theme::text(),
                              katana::qt::theme::accent());
    }
    // The application icon, large and at the sizes a taskbar and a title bar use.
    const int appRow = (rows - 2) * kCell;
    int x = 16;
    for (const int size : {192, 96, 48, 32, 24, 16}) {
        painter.drawImage(QPointF(x, appRow + (2 * kCell - size) / 2.0),
                          katana::qt::applicationIconImage(size));
        x += size + 28;
    }
    painter.end();
    return sheet.save(path, "PNG");
}

} // namespace

int main(int argc, char* argv[])
{
    // No window is ever shown, so the offscreen platform is enough - and it
    // is what lets this run on a machine with no display.
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QGuiApplication application(argc, argv);
    if (argc < 2) {
        std::fprintf(stderr, "usage: katana_make_icons <output-directory>\n");
        return 2;
    }
    const QDir directory(QString::fromLocal8Bit(argv[1]));
    if (!directory.exists() && !QDir().mkpath(directory.absolutePath())) {
        std::fprintf(stderr, "cannot create %s\n", argv[1]);
        return 1;
    }
    const bool ok = writeIco(directory.filePath("katana.ico"), {16, 20, 24, 32, 40, 48, 64, 128, 256}) &&
                    katana::qt::applicationIconImage(256).save(directory.filePath("katana.png"), "PNG") &&
                    writeSheet(directory.filePath("icon_sheet.png"));
    if (!ok) {
        std::fprintf(stderr, "failed to write the icons into %s\n", argv[1]);
        return 1;
    }
    std::printf("wrote katana.ico, katana.png and icon_sheet.png into %s\n", argv[1]);
    return 0;
}
