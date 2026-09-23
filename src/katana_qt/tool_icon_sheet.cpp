// katana_tool_icon_sheet <output.png>
//
// A DEVELOPER tool, as katana_make_icons is: every tool in the catalogue
// (include/katana/cad/interactive_tool.hpp) drawn at the two sizes the
// application uses - 20 px on a toolbar, 16 px in a menu - and large, with its
// menu, name, id and aliases, on the theme's own ground. It is how a tool's
// author reviews an icon without launching the application, and it prints how
// many tools still have only the placeholder glyph.
//
//   cmake --build build/debug --target katana_tool_icon_sheet
//   QT_QPA_PLATFORM=offscreen ./build/debug/bin/katana_tool_icon_sheet.exe sheet.png

#include <QGuiApplication>
#include <QImage>
#include <QPainter>

#include <cstdio>

#include "katana/cad/interactive_tool.hpp"
#include "theme.hpp"
#include "tools/tool_icons.hpp"

int main(int argc, char** argv)
{
    QGuiApplication application(argc, argv);
    if (argc != 2) {
        std::fprintf(stderr, "usage: katana_tool_icon_sheet <output.png>\n");
        return 2;
    }
    const auto tools = katana::cad::toolCatalog().all();
    for (const std::string& problem : katana::cad::toolCatalogProblems()) {
        std::fprintf(stderr, "catalogue refused a tool: %s\n", problem.c_str());
    }

    constexpr int kRow = 64;
    constexpr int kWidth = 760;
    const int height = std::max(1, static_cast<int>(tools.size())) * kRow + 16;
    QImage image(kWidth, height, QImage::Format_ARGB32_Premultiplied);
    image.fill(katana::qt::theme::window());
    QPainter painter(&image);
    painter.setRenderHint(QPainter::Antialiasing, true);

    int placeholders = 0;
    int row = 0;
    for (const katana::cad::ToolInfo* tool : tools) {
        const int y = 8 + row * kRow;
        const QIcon icon = katana::qt::tools::toolIcon(tool->id);
        painter.drawPixmap(12, y + 22, icon.pixmap(16, 16));
        painter.drawPixmap(40, y + 20, icon.pixmap(20, 20));
        painter.drawPixmap(72, y + 4, icon.pixmap(48, 48));
        if (!katana::qt::tools::hasToolIcon(tool->id)) {
            ++placeholders;
        }
        painter.setPen(katana::qt::theme::text());
        std::string aliases;
        for (const std::string& alias : tool->aliases) {
            aliases += (aliases.empty() ? "" : " ") + alias;
        }
        painter.drawText(136, y + 24,
                         QString::fromStdString(tool->category + " > " + tool->group + " > " +
                                                tool->name));
        painter.setPen(katana::qt::theme::textMuted());
        painter.drawText(136, y + 44, QString::fromStdString(tool->id + "   " + aliases));
        ++row;
    }
    painter.end();
    if (!image.save(QString::fromLocal8Bit(argv[1]))) {
        std::fprintf(stderr, "could not write %s\n", argv[1]);
        return 1;
    }
    std::printf("%zu tools, %d without an icon of their own\n", tools.size(), placeholders);
    return 0;
}
