// The start screen (src/katana_qt/start_dialog): driven by its controls'
// object names as a person uses it. Start hands the lines it shows to the
// runner, one by one, in order - the window's executor in the program, a
// recorder here - and a choice that is what the session already has makes
// no line. The built-ins are two invented ones, so that the test is the
// same whatever this build compiled in.

#include <gtest/gtest.h>

#include <memory>
#include <string>
#include <vector>

#include <QCheckBox>
#include <QComboBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>

#include "katana/cad/customisation_host.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/customisation.hpp"
#include "start_dialog.hpp"

using katana::cad::Document;
using katana::qt::StartChoices;
using katana::qt::StartDialog;
using katana::qt::VerbOutcome;

namespace {

template <typename T>
T* child(QWidget& parent, const char* name)
{
    auto* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

katana::cad::BuiltInEntry entryNamed(const std::string& name)
{
    const std::string text = R"({"format": "katana-customisation", "version": 1, "name": ")" +
                             name + R"(", "description": "An invented one."})";
    katana::cad::BuiltInEntry entry;
    auto read = katana::entity::customisationFromJson(text);
    EXPECT_TRUE(read.ok()) << (read.ok() ? std::string{} : read.error().describe());
    if (read.ok()) {
        entry.builtIn.customisation =
            std::make_shared<const katana::entity::Customisation>(std::move(*read));
        entry.builtIn.digest = katana::entity::customisationDigest(text);
        entry.name = name;
        entry.description = "An invented one.";
    }
    return entry;
}

// Two built-ins, the first the default; a name with a blank in it, so the
// line must quote it.
std::vector<katana::cad::BuiltInEntry> offered()
{
    return {entryNamed("Alpha"), entryNamed("Beta Works")};
}

// A session as a start leaves it: the first built-in installed, and kept.
void startOnAlpha(Document& document)
{
    ASSERT_TRUE(
        katana::cad::installBuiltInCustomisation(document, offered()[0].builtIn, true).ok());
    document.setBuiltInCustomisationName("Alpha");
}

// What the dialog handed over, in order; every line taken.
struct Recorder {
    std::vector<QString> lines;
    katana::qt::CommandRunner runner()
    {
        return [this](const QString& line) {
            lines.push_back(line);
            return VerbOutcome{true, QStringLiteral("done"), {}};
        };
    }
};

QStringList itemsOf(const QComboBox& box)
{
    QStringList items;
    for (int at = 0; at < box.count(); ++at) {
        items << box.itemText(at);
    }
    return items;
}

} // namespace

TEST(StartDialog, OpensOnTheSessionAsItIsAndThatMakesNoLine)
{
    Document document;
    startOnAlpha(document);
    Recorder recorder;
    StartDialog dialog(document, StartChoices{}, recorder.runner(), nullptr, offered());
    EXPECT_EQ(dialog.objectName(), QStringLiteral("startDialog"));
    for (const char* name :
         {"startLogo", "startTitle", "startCustomisation", "startCustomisationFile",
          "startCustomisationBrowse", "startCustomisationReplace", "startCustomisationKeep",
          "startCrs", "startCrsCheck", "startLine", "startStatus", "startShowAtStart", "startOk",
          "startSkip"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(QString::fromLatin1(name)), nullptr) << name;
    }
    auto* customisation = child<QComboBox>(dialog, "startCustomisation");
    ASSERT_NE(customisation, nullptr);
    EXPECT_EQ(itemsOf(*customisation),
              (QStringList{"Alpha (built in, the default)", "Beta Works (built in)",
                           "From a customisation file..."}));
    EXPECT_EQ(customisation->currentIndex(), 0);
    EXPECT_EQ(child<QComboBox>(dialog, "startCrs")->currentIndex(), 0); // local coordinates

    const auto lines = dialog.lines();
    ASSERT_TRUE(lines.ok());
    EXPECT_TRUE(lines->isEmpty());
    EXPECT_EQ(child<QLabel>(dialog, "startLine")->text(),
              QStringLiteral("Nothing to change: the session starts as it is."));
    // No kept file: nothing to keep, whatever is chosen.
    customisation->setCurrentIndex(1);
    EXPECT_FALSE(child<QCheckBox>(dialog, "startCustomisationKeep")->isEnabled());
    EXPECT_TRUE(dialog.start());
    EXPECT_EQ(recorder.lines, std::vector<QString>{"CUSTOMISE BUILTIN \"Beta Works\""});
    EXPECT_EQ(dialog.result(), QDialog::Accepted);
}

TEST(StartDialog, ABuiltInKeptAndACoordinateSystemAreTheLinesInOrder)
{
    Document document;
    startOnAlpha(document);
    Recorder recorder;
    StartChoices last;
    last.keptFile = QStringLiteral("C:/Users/someone/AppData/Katana/customisation.json");
    StartDialog dialog(document, last, recorder.runner(), nullptr, offered());
    auto* keep = child<QCheckBox>(dialog, "startCustomisationKeep");
    // The session was the one the next start gives, so a new choice is too
    // unless that is cleared - as Settings' Use keeps what was kept.
    EXPECT_FALSE(keep->isEnabled()) << "the session as it is makes nothing to keep";
    child<QComboBox>(dialog, "startCustomisation")->setCurrentIndex(1);
    EXPECT_TRUE(keep->isEnabled());
    EXPECT_TRUE(keep->isChecked());
    child<QComboBox>(dialog, "startCrs")->setEditText(QStringLiteral("epsg:7856"));
    EXPECT_TRUE(child<QLabel>(dialog, "startCrsCheck")->text().startsWith("EPSG:7856  GDA2020 / MGA zone 56"))
        << child<QLabel>(dialog, "startCrsCheck")->text().toStdString();
    const QStringList expected{"CUSTOMISE BUILTIN \"Beta Works\"", "CUSTOMISE KEEP",
                               "CRS SET epsg:7856"};
    EXPECT_EQ(child<QLabel>(dialog, "startLine")->text(), expected.join('\n'));
    ASSERT_TRUE(dialog.start());
    EXPECT_EQ(recorder.lines, (std::vector<QString>(expected.begin(), expected.end())));
    // What the window keeps for the next start: the system as the verb takes it.
    EXPECT_EQ(dialog.coordinateSystem(), QStringLiteral("epsg:7856"));

    // Cleared, the choice is this session's alone.
    Recorder second;
    StartDialog again(document, last, second.runner(), nullptr, offered());
    child<QComboBox>(again, "startCustomisation")->setCurrentIndex(1);
    child<QCheckBox>(again, "startCustomisationKeep")->setChecked(false);
    ASSERT_TRUE(again.start());
    EXPECT_EQ(second.lines, std::vector<QString>{"CUSTOMISE BUILTIN \"Beta Works\""});
}

TEST(StartDialog, AFileIsLoadedMergedOrInPlaceAndNoFileIsRefused)
{
    Document document;
    startOnAlpha(document);
    Recorder recorder;
    StartDialog dialog(document, StartChoices{}, recorder.runner(), nullptr, offered());
    auto* customisation = child<QComboBox>(dialog, "startCustomisation");
    customisation->setCurrentIndex(customisation->count() - 1);
    auto* file = child<QLineEdit>(dialog, "startCustomisationFile");
    EXPECT_TRUE(file->isEnabled());
    const auto none = dialog.lines();
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().message, "no file is named: type its path, or choose it with Browse");
    EXPECT_FALSE(child<QPushButton>(dialog, "startOk")->isEnabled());
    EXPECT_FALSE(dialog.start());
    EXPECT_TRUE(child<QLabel>(dialog, "startStatus")->text().startsWith("Not started: no file is named"));
    EXPECT_TRUE(recorder.lines.empty());

    // As a file manager hands a path over, quotes and back slashes and all.
    file->setText(QStringLiteral("\"C:\\site data\\codes.customisation.json\""));
    EXPECT_EQ(dialog.lines().value(),
              QStringList{"CUSTOMISE \"C:/site data/codes.customisation.json\""});
    child<QCheckBox>(dialog, "startCustomisationReplace")->setChecked(true);
    EXPECT_EQ(dialog.lines().value(),
              QStringList{"CUSTOMISE REPLACE \"C:/site data/codes.customisation.json\""});
}

TEST(StartDialog, AnythingButACoordinateSystemIsRefusedAndStartsNothing)
{
    Document document;
    startOnAlpha(document);
    Recorder recorder;
    StartDialog dialog(document, StartChoices{}, recorder.runner(), nullptr, offered());
    child<QComboBox>(dialog, "startCrs")->setEditText(QStringLiteral("no such system"));
    EXPECT_TRUE(child<QLabel>(dialog, "startCrsCheck")->text().startsWith("Not a coordinate system"));
    const auto lines = dialog.lines();
    ASSERT_FALSE(lines.ok());
    EXPECT_EQ(lines.error().code, katana::core::ErrorCode::InvalidCRS);
    EXPECT_FALSE(child<QPushButton>(dialog, "startOk")->isEnabled());
    EXPECT_FALSE(dialog.start());
    EXPECT_TRUE(recorder.lines.empty());
    EXPECT_NE(dialog.result(), QDialog::Accepted);
}

TEST(StartDialog, TheSystemChosenLastIsOfferedAndTheDrawingsOwnComesFirst)
{
    Document document;
    startOnAlpha(document);
    StartChoices last;
    last.coordinateSystem = QStringLiteral("EPSG:7856");
    {
        StartDialog dialog(document, last, {}, nullptr, offered());
        EXPECT_TRUE(child<QComboBox>(dialog, "startCrs")->currentText().startsWith("EPSG:7856  "));
        EXPECT_EQ(dialog.coordinateSystem(), QStringLiteral("EPSG:7856"));
        // Without a runner the line goes through the CRS verb on the
        // Document: one undo step.
        const std::size_t before = document.history().undoCount();
        ASSERT_TRUE(dialog.start());
        EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:7856");
        EXPECT_EQ(document.history().undoCount(), before + 1);
    }
    // The drawing has a system now; the one chosen last does not replace it,
    // and leaving it is no line.
    last.coordinateSystem = QStringLiteral("EPSG:28356");
    StartDialog dialog(document, last, {}, nullptr, offered());
    EXPECT_EQ(dialog.coordinateSystem(), QStringLiteral("EPSG:7856"));
    EXPECT_TRUE(dialog.lines().value().isEmpty());
    // Back to local coordinates clears it.
    child<QComboBox>(dialog, "startCrs")->setCurrentIndex(0);
    EXPECT_EQ(dialog.lines().value(), QStringList{"CRS CLEAR"});
}

TEST(StartDialog, ShowAtStartIsWhatItWasGivenUntilChanged)
{
    Document document;
    StartChoices last;
    last.showAtStart = false;
    StartDialog dialog(document, last, {}, nullptr, offered());
    auto* show = child<QCheckBox>(dialog, "startShowAtStart");
    EXPECT_FALSE(show->isChecked());
    EXPECT_FALSE(dialog.showAtStart());
    show->setChecked(true);
    EXPECT_TRUE(dialog.showAtStart());
}

// A run whose seam says none starts with nothing installed (the program
// tests all do, tests/CMakeLists.txt): the screen opens on that, as on any
// session it was given, and Start then changes nothing; a built-in is one
// choice away.
TEST(StartDialog, ASessionWithNothingInstalledIsOfferedAsItIsAndStartChangesNothing)
{
    Document document;
    Recorder recorder;
    StartDialog dialog(document, StartChoices{}, recorder.runner(), nullptr, offered());
    auto* customisation = child<QComboBox>(dialog, "startCustomisation");
    ASSERT_NE(customisation, nullptr);
    EXPECT_EQ(customisation->itemText(0),
              QStringLiteral("None - as it is, no customisation installed"));
    EXPECT_EQ(customisation->currentIndex(), 0);
    EXPECT_TRUE(dialog.lines().value().isEmpty());
    customisation->setCurrentIndex(1);
    EXPECT_EQ(dialog.lines().value(), QStringList{"CUSTOMISE BUILTIN Alpha"});
    customisation->setCurrentIndex(0);
    ASSERT_TRUE(dialog.start());
    EXPECT_TRUE(recorder.lines.empty());
}
