#pragma once

// A form for one GDAL algorithm's arguments, generated from what GDAL
// declares at run time (processing::describe, docs/geoprocessing.md
// "Arguments as data"): no argument is written into Katana by hand, so a GDAL
// upgrade that adds an argument or moves a bound changes the form with it.
//
//   Boolean                a check box
//   String                 a line, or a choice of GDAL's own choices; the
//                          output format offers the formats GDAL suggests
//   Integer, Real          a spin box carrying GDAL's minimum and maximum; an
//                          exclusive bound is refused at its end value when
//                          the line is written
//   lists                  a comma-separated line, each item checked
//
// A spin box starts "not given" (its special value) unless GDAL declares a
// default, and a choice starts on GDAL's default. Only what differs from GDAL's
// default is written, as --name=value, in GDAL's order: the line says what the
// person chose and nothing GDAL would do anyway. Base arguments show; the
// Advanced, Esoteric and Common ones are in a group folded away. Of an
// exclusion group only one may be given - the others are disabled once one
// is - and an argument that depends on another is enabled once that one is.
//
// Datasets are not the form's: the dialog binds them (binding_picker.hpp).
// Nor are the words the executor refuses (--quiet: processing::checkTokens),
// nor those a dialog says elsewhere (the output's format and overwriting,
// which its target says).
//
// Object names: <prefix><argument> for each control ("gdalArg.zfactor"), and
// the folded group as the name given (gdalToolboxAdvanced).

#include <QString>
#include <QStringList>
#include <QWidget>

#include <functional>
#include <map>
#include <set>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/gis/processing.hpp"

class QFormLayout;
class QGroupBox;

namespace katana::qt {

class ArgumentForm final : public QWidget {
  public:
    ArgumentForm(QString prefix, QString advancedName, QWidget* parent = nullptr);

    // Called after any control is edited.
    std::function<void()> onChanged{};

    // Builds the controls for `spec`'s arguments: every one that is no
    // dataset, is not refused by the executor and is not in `leaveOut`; with
    // `pipelineStep`, only those a pipeline's step takes. Replaces what was.
    void setAlgorithm(const katana::gis::processing::AlgorithmSpec& spec,
                      const std::set<std::string>& leaveOut = {}, bool pipelineStep = false);

    // The words of the arguments given, --name=value in GDAL's order (a word
    // with a blank quoted, as the command line reads it), or why a value is
    // refused, naming the argument: not a number, below an exclusive minimum
    // or above a maximum, a list with too few or too many items, a double
    // quote, a required argument not given.
    [[nodiscard]] katana::core::Result<QStringList> words() const;

    // The control of an argument; null when the form has none of that name.
    [[nodiscard]] QWidget* control(const std::string& name) const;
    // Whether an argument is given: set to something other than GDAL's default.
    [[nodiscard]] bool given(const std::string& name) const;
    // The arguments the form shows, in GDAL's order.
    [[nodiscard]] std::vector<std::string> arguments() const;

    // Sets an argument from a value as a line writes it ("2", "flat",
    // "a,b", "true"): what re-reading a pipeline's text does. False when the
    // form has no such argument or the value does not fit its control.
    bool setValue(const std::string& name, const QString& value);
    // Every control back to GDAL's default.
    void clear();

  private:
    struct Entry {
        katana::gis::processing::ArgSpec spec;
        QWidget* control = nullptr;
    };

    void changed();
    void updateEnabled();
    [[nodiscard]] QString text(const Entry& entry) const;
    [[nodiscard]] QString defaultText(const Entry& entry) const;

    QString prefix_;
    QString advancedName_;
    std::vector<std::string> path_;
    std::vector<Entry> entries_;
    QFormLayout* base_ = nullptr;
    QGroupBox* advanced_ = nullptr;
    QWidget* advancedBody_ = nullptr;
    QFormLayout* advancedForm_ = nullptr;
    bool building_ = false;
};

} // namespace katana::qt
