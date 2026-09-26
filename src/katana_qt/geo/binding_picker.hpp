#pragma once

// Where a geoprocessing dataset comes from, as a dialog picks it
// (docs/geoprocessing.md, "Bindings"): the window's side of the ONE source
// clause every geoprocessing verb reads (src/katana_app/geo/bindings.hpp),
//
//   <source> := <scope> | RASTER <id|name> | SURFACE <name> [CELL <m>] | FILE <path>
//
// A picker offers only the kinds its dataset can be: a raster-only input
// offers no drawing, a vector-only one no raster or surface. It never reads a
// dataset itself: words() writes the words of the clause, which the dialog
// puts on its line and the executor reads with parseSource - so the picker
// and a typed line cannot mean different things.
//
//   Drawing           Global Modify's own scope and filter controls
//                     (ScopeFilterWidget), whose words are the shared grammar's
//   Reference raster  one of the window's reference rasters, by id
//   Surface           one of the window's surfaces, by name, and the cell it is
//                     sampled at (blank: suggested for its extent)
//   File              a path typed or browsed to; a /vsi path or a URL is
//                     typed and passes through as it is
//
// Object names, the prefix given and then:
//   <prefix>Source    the picker
//   <prefix>Kind      which kind (Drawing, Reference raster, Surface, File)
//   <prefix>Raster    the reference raster ("<id>  <name>")
//   <prefix>Surface   the surface; <prefix>Cell its cell, metres
//   <prefix>File      the path; <prefix>Browse chooses one (a headless session
//                     says to fill <prefix>File instead)
//   <prefix>Scope ... the drawing's scope and filter (ScopeFilterWidget's own
//                     names under the same prefix: <prefix>ScopeDrawing,
//                     <prefix>Layers, <prefix>TypePoint ...)

#include <QString>
#include <QStringList>
#include <QWidget>

#include <functional>

#include "geo/geo_dialog_support.hpp"
#include "katana/core/error.hpp"

class QComboBox;
class QLineEdit;
class QListWidget;
class QPushButton;
class QStackedWidget;

namespace katana::qt {

// The kinds a picker offers, as bits; BindNone is an optional dataset left
// out.
enum BindingKind : unsigned {
    BindNone = 0,
    BindDrawing = 1,
    BindRaster = 2,
    BindSurface = 4,
    BindFile = 8,
};

// The kinds that can give a dataset of GDAL's kinds (processing::DatasetKind
// bits): a vector from the drawing or a file, a raster from a reference
// raster, a surface or a file. What the executor's sourcesFor says in words.
[[nodiscard]] unsigned bindingKindsFor(unsigned datasetKinds);

class BindingPicker final : public QWidget {
  public:
    BindingPicker(const QString& prefix, unsigned kinds, GeoDialogContext context,
                  QWidget* parent = nullptr);

    // Called after any control is edited.
    std::function<void()> onChanged{};
    // What the picker has to say that is no field's value: that a headless
    // session opens no file dialog. The dialog shows it in its status.
    std::function<void(const QString& text)> say{};

    [[nodiscard]] BindingKind kind() const;
    // Chooses `kind` when the picker offers it; false when it does not.
    bool setKind(BindingKind kind);
    // For an optional dataset: offers "Not given" first, and chooses it.
    void setOptional();
    // The words of the source clause, or why the controls say none, naming
    // the field; empty for an optional dataset not given.
    [[nodiscard]] katana::core::Result<QString> words() const;
    // Refills the reference rasters, the surfaces and the scope's layers and
    // views, keeping what is chosen.
    void reload();
    // The drawing's controls; null when the picker offers no drawing.
    [[nodiscard]] ScopeFilterWidget* scope() const { return scope_; }
    [[nodiscard]] QLineEdit* file() const { return file_; }

  private:
    void changed();
    void browse();

    QString prefix_;
    GeoDialogContext context_;
    QComboBox* kind_ = nullptr;
    QStackedWidget* pages_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QComboBox* raster_ = nullptr;
    QComboBox* surface_ = nullptr;
    QLineEdit* cell_ = nullptr;
    QLineEdit* file_ = nullptr;
    QPushButton* browse_ = nullptr;
};

// Several datasets for one argument - a mosaic's tiles, raster calc's
// inputs: a picker, and the list its Add builds. With nothing listed, the
// picker's own dataset is the one. Object names: the picker's, and
// <prefix><listName> (the list), <prefix>Add, <prefix>Remove.
class BindingList final : public QWidget {
  public:
    BindingList(const QString& prefix, unsigned kinds, GeoDialogContext context,
                const QString& listName = "List", QWidget* parent = nullptr);

    std::function<void()> onChanged{};

    [[nodiscard]] BindingPicker& picker() const { return *picker_; }
    // The source clauses, in order: the list's, or the picker's alone.
    [[nodiscard]] katana::core::Result<QStringList> words() const;
    // The picker's dataset added to the list; false, `why` set, when the
    // picker says none.
    bool add(QString* why = nullptr);
    void reload() { picker_->reload(); }

  private:
    BindingPicker* picker_ = nullptr;
    QListWidget* list_ = nullptr;
};

} // namespace katana::qt
