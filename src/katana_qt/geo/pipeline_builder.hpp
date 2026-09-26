#pragma once

// The GDAL Toolbox's Pipeline tab (docs/geoprocessing.md, "Pipelines"):
// several of GDAL's algorithms chained in one run - read, then steps, then
// write - without a file between them, raster to vector where a step turns
// one into the other ("read ! contour --interval=0.5 ! buffer --distance=0.1 !
// write").
//
// The steps offered are GDAL's own pipeline steps (the pipeline usage's
// pipeline_algorithms), each the catalogue algorithm under raster or vector
// whose arguments its form shows, restricted to those a step takes - and only
// the steps that read what the pipeline makes at that point: after a raster,
// raster steps; after contour or polygonize, vector steps. `external` (it
// runs a program) and `tee` (a pipeline of its own) are never offered, and a
// text naming external is refused as the verb refuses it.
//
// The tab writes the GDAL line the steps describe:
//   GDAL pipeline "read ! <step> ... ! write" FROM input <source> TO <target>
// and Run hands it to the window's one executor, as every geoprocessing
// dialog does. The pipeline's text is shown in gdalPipelineText and may be
// edited there; an edit is read back into steps where it can be, and a text
// that cannot be (an argument GDAL does not declare) still runs as typed,
// the status saying why the steps did not follow. A recipe is kept by saving
// the line into a script (.kcs, File > Run Script replays it), not as a
// .gdalg.json, which cannot hold a dataset bound from the drawing.
//
// Object names (tests/qt_widgets/geo/test_pipeline_builder.cpp):
//   gdalPipelineSource ...   the dataset read (a picker: gdalPipelineKind,
//                            gdalPipelineRaster, gdalPipelineFile,
//                            gdalPipelineScope ...)
//   gdalPipelineSteps        read, the steps, write
//   gdalPipelineAdd          a step to add after the one selected (before write)
//   gdalPipelineUp, gdalPipelineDown, gdalPipelineRemove   the step selected
//   gdalStep.<arg>           the selected step's arguments (ArgumentForm; the
//                            folded ones under gdalPipelineAdvanced)
//   gdalPipelineText         the pipeline's text
//   gdalPipelineOutputKind, gdalPipelineOutputName   where the result goes
//   gdalPipelineScript, gdalPipelineSave   the script the line is saved into
//   gdalPipelineCommand, gdalPipelinePreview, gdalPipelineRun,
//   gdalPipelineStatus, gdalPipelineReply   (GeoRunPanel)

#include <QString>
#include <QStringList>
#include <QWidget>

#include <string>
#include <vector>

#include "geo/binding_picker.hpp"
#include "geo/geo_dialog_support.hpp"
#include "katana/core/error.hpp"

class QComboBox;
class QLineEdit;
class QListWidget;
class QPushButton;

namespace katana::qt {

class ArgumentForm;

// A step a pipeline can take.
struct PipelineStepKind {
    std::string name;              // "hillshade"
    std::vector<std::string> path; // the catalogue algorithm: {"raster", "hillshade"}
    unsigned reads = 0;            // processing::DatasetKind bits
    unsigned makes = 0;
    QString description;
};

// Every step offered, in GDAL's order: in its pipeline usage's
// pipeline_algorithms and a leaf of the catalogue under raster or vector.
// Read and write are the pipeline's own ends; external and tee are not
// offered. Read once.
[[nodiscard]] const std::vector<PipelineStepKind>& pipelineSteps();

// The names GDAL's pipeline usage lists as steps, as it lists them.
[[nodiscard]] std::vector<std::string> pipelineStepNames();

// One step of a pipeline: the algorithm and its words (--name=value).
struct PipelineStep {
    std::vector<std::string> path;
    QStringList words;
};

// "read ! hillshade --zfactor=2 ! write". InvalidArgument for a word with a
// blank or a double quote, which a pipeline's text on one line cannot carry.
[[nodiscard]] katana::core::Result<QString> pipelineText(const std::vector<PipelineStep>& steps);

// A pipeline's text read into steps, each value checked by the step's own
// form: read first and write last, bare (the source and the target name
// their datasets); `--name=value`, `--name value`, `-n value`, a flag, and
// values given by position in GDAL's order. `reads` is what the source is
// (a DatasetKind bit, or 0 for either), which chooses between a raster and a
// vector step of one name. InvalidArgument naming what it cannot read.
[[nodiscard]] katana::core::Result<std::vector<PipelineStep>> parsePipeline(const QString& text,
                                                                           unsigned reads);

// What the Pipeline tab's fields hold.
struct PipelineForm {
    QString text;
    QString source;
    QString sourceError;
    QString outputKind; // LAYER, REFERENCE, FILE
    QString outputName;
};

// GDAL pipeline "<text>" FROM input <source> TO <kind> <name>. InvalidArgument
// for no source, a text the executor refuses (a step called external), an
// empty text, and a layer or file path left empty.
[[nodiscard]] katana::core::Result<QString> pipelineLine(const PipelineForm& form);

class PipelineBuilder final : public QWidget {
  public:
    explicit PipelineBuilder(GeoDialogContext context, QWidget* parent = nullptr);

    [[nodiscard]] const std::vector<PipelineStep>& steps() const { return steps_; }
    [[nodiscard]] PipelineForm form() const;
    [[nodiscard]] GeoRunPanel& runPanel() const { return *panel_; }
    [[nodiscard]] ArgumentForm& stepForm() const { return *stepForm_; }
    // Adds the step `name` after the one selected (before write), and
    // selects it. False when no step of that name reads what the pipeline
    // makes there.
    bool addStep(const QString& name);
    // Selects the step at `index` (0 is read).
    void select(int index);
    // Writes the line into the script `path` (.kcs), after what is there.
    [[nodiscard]] katana::core::Status saveTo(const QString& path);
    void reload();

  private:
    [[nodiscard]] unsigned sourceKind() const;
    [[nodiscard]] unsigned kindAfter(int steps) const;
    void rebuild(bool text);
    void stepEdited();
    void textEdited(const QString& text);
    void showStep();
    void offerSteps();
    void move(int by);
    void save();

    GeoDialogContext context_;
    std::vector<PipelineStep> steps_;
    BindingPicker* source_ = nullptr;
    QListWidget* list_ = nullptr;
    QComboBox* add_ = nullptr;
    QPushButton* up_ = nullptr;
    QPushButton* down_ = nullptr;
    QPushButton* remove_ = nullptr;
    ArgumentForm* stepForm_ = nullptr;
    QLineEdit* text_ = nullptr;
    QComboBox* outputKind_ = nullptr;
    QLineEdit* outputName_ = nullptr;
    QLineEdit* script_ = nullptr;
    GeoRunPanel* panel_ = nullptr;
    // The text as typed when it could not be read into steps: the line runs
    // it as it is.
    QString typed_;
    bool updating_ = false;
    // The builder is writing the text itself: no edit to read back.
    bool writingText_ = false;
};

} // namespace katana::qt
