#pragma once

// GIS > Dataset Information (docs/interop.md, "Dataset information"): what a
// file holds, as the INFO line reads it. The dialog builds INFO lines and runs
// them through the window's one executor (MainWindow::runVerbLine), as a
// person or an agent would type them; it never reads the file itself.
// Opened on a file it runs INFO <file> and INFO <file> JSON; its buttons run
// INFO <file> STATS and INFO <file> CHECK.
//
// Object names:
//   datasetInfoDialog     the dialog
//   datasetInfoTabs       Summary, Fields, Bands, JSON
//   datasetInfoSummary    the records that are not a field or a band: the
//                         dataset, raster, layers, subdatasets, point cloud
//   datasetInfoFields     a table: Layer, Field, Type, Width
//   datasetInfoBands      a table: Band, Type, No data, Min, Max, Mean,
//                         Std dev, Overviews
//   datasetInfoJson       GDAL's own JSON (INFO ... JSON), indented
//   datasetInfoCopyJson   copies it to the clipboard
//   datasetInfoStats      INFO ... STATS: every band's statistics, computed
//                         from every pixel, no sidecar written beside the file
//   datasetInfoCheck      INFO ... CHECK: every value read, and what failed
//   datasetInfoCommand    the line last run, exactly as it would be typed
//   datasetInfoReply      what the last line said: the check, or why it failed
//   datasetInfoClose      closes it

#include <QDialog>
#include <QString>

#include <functional>

#include "command_runner.hpp"

class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QTabWidget;
class QTableWidget;

namespace katana::qt {

// How the dialog runs a line and learns what it did. `run` is the window's
// one executor. A line of the geoprocessing executor may start a background
// job, whose reply comes when the job ends: `await` is handed what `run`
// returned and, when that started a job still running, calls `done` with the
// job's outcome once it ends and returns true - false when what `run`
// returned is the answer. Empty, every answer is taken as it comes.
struct DatasetInfoRunner {
    CommandRunner run;
    std::function<bool(const VerbOutcome& started, std::function<void(const VerbOutcome&)> done)>
        await;
};

class DatasetInfoDialog final : public QDialog {
  public:
    DatasetInfoDialog(const QString& path, DatasetInfoRunner runner, QWidget* parent = nullptr);

    // The INFO line of this file with `words` after the path ("STATS"): the
    // path quoted, as a person would type it.
    [[nodiscard]] QString line(const QString& words = {}) const;

    // Whether INFO <file> has answered, and said what the file holds.
    [[nodiscard]] bool described() const { return described_; }

  private:
    // `words`' line run, and `use` given its outcome - now, or when its job
    // ends. Never after the dialog has gone.
    void runLine(const QString& words, std::function<void(const VerbOutcome&)> use);
    void showRecords(const VerbOutcome& outcome);
    void showJson(const VerbOutcome& outcome);

    QString path_;
    DatasetInfoRunner runner_;
    bool described_ = false;
    QTabWidget* tabs_ = nullptr;
    QPlainTextEdit* summary_ = nullptr;
    QTableWidget* fields_ = nullptr;
    QTableWidget* bands_ = nullptr;
    QPlainTextEdit* json_ = nullptr;
    QPushButton* copyJson_ = nullptr;
    QPushButton* stats_ = nullptr;
    QPushButton* check_ = nullptr;
    QLineEdit* command_ = nullptr;
    QLabel* reply_ = nullptr;
};

} // namespace katana::qt
