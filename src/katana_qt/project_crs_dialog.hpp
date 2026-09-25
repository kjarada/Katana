#pragma once

// File > Project Coordinate System: the project's coordinate system chosen
// from the common list, from what suits a place, or typed (an EPSG code, WKT
// or a PROJ string), each checked as it is typed (docs/cad.md, "The project's
// coordinate system"). The place is given by whoever opens it, or typed as a
// longitude and latitude (projectCrsPlace, then projectCrsSuggest) - filled
// in with the drawing's centre when the project has a system to say where
// that is.
//
// A thin front end over Document::setCoordinateSystem: OK is ONE undo step,
// and everything here can be done without it with the CRS verb (CRS SET,
// CRS FIND, CRS SUGGEST). The status bar's coordinate-system button and
// GIS > Online Data open it too.
//
// Non-modal, it follows the drawing while it is open (follow): a system set
// from elsewhere (a typed CRS SET, an undo) is shown in projectCrsCurrent, and
// in projectCrsText unless something else has been typed there; a new or
// opened drawing is shown afresh - its system, its place, no place the
// opener gave for the drawing before. Left open across an OPEN, it once
// showed the old project's system and Set put it on the new one.

#include <optional>
#include <utility>

#include <QDialog>

#include "katana/cad/document.hpp"

class QLabel;
class QLineEdit;
class QPushButton;
class QTreeWidget;

namespace katana::qt {

class ProjectCrsDialog final : public QDialog {
  public:
    // `place` is a longitude and latitude to suggest systems for (the area a
    // GIS import is about to fetch); without one, the list alone.
    ProjectCrsDialog(katana::cad::Document& document,
                     std::optional<std::pair<double, double>> place, QWidget* parent = nullptr);

    // What the text box holds, and what the check says of it.
    [[nodiscard]] QString text() const;
    [[nodiscard]] QString check() const;
    // Sets the project's coordinate system from the text box, as one step.
    // False, with the reason in the check line, when it names no system.
    bool apply();
    // Suggest: the systems for the place typed in projectCrsPlace listed
    // first, as CRS SUGGEST lists them. False, with the reason in the check
    // line, when it is not a longitude and a latitude.
    bool suggest();

  private:
    void fill(const QString& filter);
    void recheck();
    void follow(const katana::cad::DocumentChange& change);
    // The place box: the opener's place, else the drawing's centre, else
    // empty.
    void showPlace();

    katana::cad::Document& document_;
    katana::cad::Document::ListenerHandle listener_;
    std::optional<std::pair<double, double>> place_;
    // The project's system as projectCrsText was last filled with it: while
    // the box still says it, nothing has been typed there to keep.
    QString shownSystem_;
    QLabel* current_ = nullptr;
    QLineEdit* search_ = nullptr;
    QLineEdit* placeText_ = nullptr;
    QTreeWidget* list_ = nullptr;
    QLineEdit* text_ = nullptr;
    QLabel* check_ = nullptr;
};

// Opens the dialog and waits for it. True when the project's coordinate
// system changed.
bool chooseProjectCrs(QWidget* parent, katana::cad::Document& document,
                      std::optional<std::pair<double, double>> place = std::nullopt);

// "EPSG:7856  GDA2020 / MGA zone 56", or "no coordinate system" - what the
// status bar and the online data dialog show.
[[nodiscard]] QString projectCrsLabel(const katana::cad::Document& document);

} // namespace katana::qt
