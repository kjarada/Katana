#pragma once

// The sheet editor: a project's sheets on paper, laid out by hand or
// generated, and plotted (docs/plotting.md, "The sheet editor").
//
// A window of its own beside the drawing, as the style manager is, rather
// than a view kind: sheets are paper, not a way of looking at the model, and a
// sheet set is edited while the drawing views go on showing the drawing.
//
//   the sheets    listed on the left: add, duplicate, remove, reorder, rename
//   the canvas    the current sheet painted by the sheet painter - exactly
//                 what plots - with its viewports picked, dragged and resized
//                 on handles, snapped to the drawing area and each other;
//                 Shift-drag pans the drawing inside a plan viewport
//   properties    on the right: the selected viewport's, else the sheet's,
//                 with the title-block values this sheet overrides
//   the toolbar   generate sheets, add a view, tile with a preset, the
//                 title block and logo, plot the sheet or the whole set
//
// Every change is ONE undoable step through sheet_commands.hpp, a drag
// committed on release, so the command stack never fills with the positions a
// drag passed through. The editor listens to the document and redraws on
// every change, undo and redo included.

#include <cstddef>
#include <functional>
#include <optional>
#include <string>

#include <QImage>
#include <QMainWindow>
#include <QPointF>
#include <QWidget>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"
#include "sheet_painter.hpp"

class QListWidget;
class QScrollArea;
class QLabel;

namespace katana::qt {

class SheetEditor;

// The paper: one sheet, painted and edited.
class SheetCanvas final : public QWidget {
  public:
    SheetCanvas(katana::cad::Document& document, SheetEditor& editor, QWidget* parent = nullptr);

    void setSheet(std::size_t index);
    [[nodiscard]] std::size_t sheet() const { return sheet_; }
    // The selected viewport's id; empty for none (the sheet itself).
    [[nodiscard]] const std::string& selected() const { return selected_; }
    void select(std::string viewportId);

    // The whole paper in the window.
    void fitPage();
    // Paint again from the document: after any change to it.
    void invalidate();

    // Logical pixels per paper millimetre, and where the paper's top-left is.
    [[nodiscard]] double zoom() const { return zoom_; }
    [[nodiscard]] QPointF paperToWidget(const katana::geometry::Point2& paper) const;
    [[nodiscard]] katana::geometry::Point2 widgetToPaper(const QPointF& widget) const;

    // What the last paint reported: viewports it could not draw.
    [[nodiscard]] const SheetPaintStats& lastStats() const { return stats_; }
    // How many times the sheet has been painted, for tests of the cache.
    [[nodiscard]] int renders() const { return renders_; }

    std::function<void(const std::string&)> onSelectionChanged;
    std::function<void(const QPointF& global, const std::string& viewportId)> onContextMenu;

  protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;
    void contextMenuEvent(QContextMenuEvent* event) override;

  private:
    enum class Drag { None, Move, Resize, Pan, PanView };

    [[nodiscard]] const katana::cad::plotting::Sheet* currentSheet() const;
    [[nodiscard]] double paperHeight() const;
    [[nodiscard]] const katana::cad::plotting::Viewport* viewport(const std::string& id) const;
    [[nodiscard]] std::string viewportAt(const QPointF& widget) const;
    // 0..7 clockwise from the top-left corner; nothing when not on a handle
    // of the selected viewport.
    [[nodiscard]] std::optional<int> handleAt(const QPointF& widget) const;
    [[nodiscard]] QRectF widgetRect(const katana::geometry::Box2& paper) const;
    void render();
    void updateDrag(const QPointF& widget, Qt::KeyboardModifiers modifiers);
    void commitDrag();

    katana::cad::Document& document_;
    SheetEditor& editor_;
    SheetPaintCache cache_;
    SheetPaintStats stats_;
    std::size_t sheet_ = 0;
    std::string selected_;

    double zoom_ = 2.0;
    QPointF origin_{20.0, 20.0};
    bool fitted_ = false;

    // The painted sheet, and what it was painted at.
    QImage image_;
    QPointF imageOrigin_;
    double imageZoom_ = 0.0;
    std::uint64_t version_ = 1;
    std::uint64_t imageVersion_ = 0;
    int renders_ = 0;

    Drag drag_ = Drag::None;
    int handle_ = 0;
    QPointF pressWidget_;
    katana::geometry::Point2 pressPaper_{};
    katana::geometry::Box2 startRect_;
    katana::geometry::Box2 liveRect_;
    katana::geometry::Point2 startCentre_{};
    katana::geometry::Point2 viewShift_{}; // paper mm the drawing was dragged by
    std::optional<double> guideX_;
    std::optional<double> guideY_;
};

class SheetEditor final : public QMainWindow {
  public:
    // What the sheets are drawn from, asked for at every paint: the drawing,
    // the window's surfaces and reference data, the logo, the fields.
    using SourceProvider = std::function<SheetSource()>;

    SheetEditor(katana::cad::Document& document, SourceProvider source, QWidget* parent = nullptr);
    ~SheetEditor() override;

    SheetEditor(const SheetEditor&) = delete;
    SheetEditor& operator=(const SheetEditor&) = delete;

    [[nodiscard]] SheetSource source() const { return source_(); }
    [[nodiscard]] SheetCanvas* canvas() const { return canvas_; }
    [[nodiscard]] std::size_t currentSheet() const;
    void setCurrentSheet(std::size_t index);

    // Everything read again from the document: the list, the canvas, the
    // properties.
    void refresh();

    // ---- the edits the toolbar makes; each one undoable step ----------------------
    // A viewport of `kind` on the current sheet, placed in the free drawing
    // area, with sensible defaults (a plan fitted to the drawing, a section
    // along the first alignment); selected. A new sheet first when there is none.
    katana::core::Status addViewport(katana::cad::plotting::ViewportKind kind);
    katana::core::Status tile(katana::cad::plotting::TilingPreset preset);
    katana::core::Status addBlankSheet();
    katana::core::Status removeSelectedViewport();
    // Plots the current sheet, or every sheet, to `path`. Problems are
    // reported through onMessage.
    katana::core::Status plotToPdf(const QString& path, bool allSheets);

    // The dialogs; each applies what it is given as one step.
    void generateSheets();
    void editTitleBlock();

    // Messages for the main window's log: the text, and whether it is an error.
    std::function<void(const QString&, bool)> onMessage;

  private:
    void buildActions();
    void rebuildList();
    void rebuildProperties();
    void showContextMenu(const QPointF& global, const std::string& viewportId);
    void report(const QString& text, bool error = false);
    void plotInteractive(bool allSheets);

    katana::cad::Document& document_;
    SourceProvider source_;
    SheetCanvas* canvas_ = nullptr;
    QListWidget* list_ = nullptr;
    QScrollArea* properties_ = nullptr;
    QLabel* status_ = nullptr;
    bool rebuilding_ = false;
    katana::cad::Document::ListenerHandle listener_;
};

} // namespace katana::qt
