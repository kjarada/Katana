#pragma once

// Where an import lands, in the window (docs/interop.md, "Placing an
// import"): the Placement group the import dialogs share, File > Import's
// small step for a DXF or a .12da archive, and the one place the window
// decides - or asks - where data it has read goes.
//
// What a placement MEANS is cad::resolveImportShift
// (include/katana/cad/import_placement.hpp), the same for a typed IMPORT
// line, katana_cli and katana_mcp; this file is only its Qt face.
//
// Object names:
//   importPlacement            the group
//   importPlacementKeep        its own coordinates (the default)
//   importPlacementLocal       lower-left corner to 0,0 (LOCAL)
//   importPlacementAlongside   lower-left corner onto the drawing's (ALONGSIDE)
//   importPlacementOffset      moved by importOffsetE, importOffsetN (OFFSET=dE,dN)
//   importPlacementShift       what the choice will do, and the line that does the same
//   importPlacementDialog      File > Import's step, with importPlacementImport and
//                              importPlacementCancel

#include <functional>
#include <optional>

#include <QDialog>
#include <QGroupBox>
#include <QString>

#include "katana/cad/import_placement.hpp"
#include "katana/geometry/primitives2d.hpp"

class QDoubleSpinBox;
class QLabel;
class QRadioButton;
class QWidget;

namespace katana::qt {

// The IMPORT line that imports `path` with `placement`: the path quoted, then
// the placement's word when it has one - what a person would type, and what
// File > Import runs through the window's one executor.
[[nodiscard]] QString importLine(const QString& path,
                                 const katana::cad::ImportPlacement& placement);

class ImportPlacementBox final : public QGroupBox {
  public:
    // `drawing` is the drawing's extent now, for what Alongside will do;
    // `path` the file, for the line shown. Starts at the choice last made
    // (remember), Keep the first time.
    ImportPlacementBox(const katana::geometry::Box2& drawing, const QString& path,
                       QWidget* parent = nullptr);

    [[nodiscard]] katana::cad::ImportPlacement placement() const;
    void setPlacement(const katana::cad::ImportPlacement& placement);
    // What importPlacementShift says now.
    [[nodiscard]] QString said() const;
    // Keeps the choice for the next import (QSettings, import/placement):
    // called when an import is accepted, not on every click.
    void remember() const;

  private:
    void refresh();

    katana::geometry::Box2 drawing_;
    QString path_;
    QRadioButton* keep_ = nullptr;
    QRadioButton* local_ = nullptr;
    QRadioButton* alongside_ = nullptr;
    QRadioButton* offset_ = nullptr;
    QDoubleSpinBox* east_ = nullptr;
    QDoubleSpinBox* north_ = nullptr;
    QLabel* shift_ = nullptr;
};

// File > Import's step for a file whose only choice is where it lands: a
// DXF or a .12da archive (a GIS vector file's is in Import Vector Data's own
// dialog, which has more to ask).
class ImportPlacementDialog final : public QDialog {
  public:
    ImportPlacementDialog(const QString& path, const katana::geometry::Box2& drawing,
                          QWidget* parent = nullptr);
    [[nodiscard]] ImportPlacementBox& box() const { return *box_; }
    // The IMPORT line the choice makes (importLine).
    [[nodiscard]] QString line() const;

  private:
    QString path_;
    ImportPlacementBox* box_ = nullptr;
};

// The window's question when data that keeps its own coordinates lands far
// from the drawing (interop::advisePlacement's `advice`): Shift Alongside,
// Keep Survey Coordinates, or Cancel. What an IMPORT line asks through the
// executor's farApart (src/katana_app/geo/geo_verbs.hpp), and what
// decideImportPlacement asks for the import dialogs.
enum class FarApartAnswer { ShiftAlongside, Keep, Cancel };
[[nodiscard]] FarApartAnswer askFarApart(QWidget* parent, const QString& advice,
                                         const katana::geometry::Box2& incoming);

// What the window does about where data it has read lands, the one place
// its three importers decide it. Any placement but Keep is
// cad::resolveImportShift, whose sentence is logged. Keep asks, when the
// data lands far from the drawing (interop::advisePlacement): Shift
// Alongside, Keep or Cancel - and in a headless session, where nobody can
// answer, logs the advice and keeps the coordinates.
struct PlacementDecision {
    bool cancelled = false;
    // The origin shift to read the data again with; nullopt keeps it where
    // it is.
    std::optional<katana::geometry::Vec2> shift;
};
[[nodiscard]] PlacementDecision
decideImportPlacement(QWidget* parent, bool headless, const katana::cad::ImportPlacement& placement,
                      const katana::geometry::Box2& drawing, const katana::geometry::Box2& incoming,
                      const std::function<void(const QString& text, bool isError)>& log);

} // namespace katana::qt
