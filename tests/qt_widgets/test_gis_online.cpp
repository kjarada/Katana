// GIS > Online Data's dialog (src/katana_qt/gis_online_dialog.hpp), driven by
// its object names as a person drives it by clicking: the provider tree and
// its search, the details a layer shows, and the command the dialog hands on
// - which must be exactly what the ONLINE IMPORT verb would have parsed,
// because the dialog has no other way to import. The context is the test's
// own, so nothing here reaches the network or a window.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTreeWidget>
#include <QTreeWidgetItemIterator>

#include <map>
#include <optional>
#include <string>
#include <vector>

#include "gis_online_dialog.hpp"
#include "katana/interop/online_catalogue.hpp"

namespace {

using katana::core::ErrorCode;
using katana::interop::OnlineAreaKind;
using katana::interop::OnlineCatalogue;
using katana::interop::OnlineCommand;
using katana::qt::OnlineDataDialog;
using katana::qt::OnlineDialogContext;

// What the dialog did to its context.
struct Recorder {
    OnlineCatalogue catalogue;
    std::string crs = "EPSG:7856";
    std::vector<OnlineCommand> run;
    std::vector<std::string> custom;
    std::map<std::string, std::string> keys;
};

OnlineDialogContext contextFor(Recorder& recorder)
{
    OnlineDialogContext context;
    context.catalogue = [&recorder]() -> const OnlineCatalogue& { return recorder.catalogue; };
    context.projectCrs = [&recorder] { return recorder.crs; };
    context.hasKey = [&recorder](const std::string& name) { return recorder.keys.contains(name); };
    context.saveKey = [&recorder](const std::string& name, const std::string& value) {
        recorder.keys[name] = value;
    };
    context.run = [&recorder](const OnlineCommand& command) -> katana::core::Status {
        recorder.run.push_back(command);
        return {};
    };
    context.addCustom = [&recorder](const std::string& url) -> katana::core::Status {
        recorder.custom.push_back(url);
        return {};
    };
    return context;
}

Recorder withBuiltIn()
{
    Recorder recorder;
    auto catalogue = katana::interop::builtInCatalogue();
    EXPECT_TRUE(catalogue.ok());
    recorder.catalogue = *catalogue;
    return recorder;
}

template <typename T>
T* child(QWidget& dialog, const char* name)
{
    auto* found = dialog.findChild<T*>(name);
    EXPECT_NE(found, nullptr) << name;
    return found;
}

// The texts of the visible leaves: the layers a person can see.
std::vector<std::string> visibleLayers(QTreeWidget& tree)
{
    std::vector<std::string> out;
    for (QTreeWidgetItemIterator it(&tree); *it != nullptr; ++it) {
        bool shown = !(*it)->isHidden();
        for (QTreeWidgetItem* parent = (*it)->parent(); shown && parent != nullptr; parent = parent->parent()) {
            shown = !parent->isHidden();
        }
        if (shown && !(*it)->data(0, Qt::UserRole + 1).toString().isEmpty()) {
            out.push_back((*it)->data(0, Qt::UserRole).toString().toStdString() + " " +
                          (*it)->data(0, Qt::UserRole + 1).toString().toStdString());
        }
    }
    return out;
}

} // namespace

TEST(GisOnlineDialog, EveryControlHasItsObjectName)
{
    Recorder recorder = withBuiltIn();
    OnlineDataDialog dialog(contextFor(recorder));
    EXPECT_EQ(dialog.objectName(), "onlineDataDialog");
    EXPECT_FALSE(dialog.isModal());
    for (const char* name :
         {"onlineSearch", "onlineProviders", "onlineDetails", "onlineArea", "onlineBox", "onlineBoxCrs",
          "onlineResolutionAuto", "onlineResolution", "onlineTargetLayer", "onlineTag", "onlineUseDates",
          "onlineFrom", "onlineTo", "onlineCloud", "onlineProjectCrs", "onlineCrs", "onlineKey",
          "onlineSaveKey", "onlineCustomUrl", "onlineAddCustom", "onlineImport", "onlineStatus",
          "onlineClose"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
}

TEST(GisOnlineDialog, TheTreeIsAustraliaByStateThenGlobal)
{
    Recorder recorder = withBuiltIn();
    OnlineDataDialog dialog(contextFor(recorder));
    auto* tree = child<QTreeWidget>(dialog, "onlineProviders");
    ASSERT_GE(tree->topLevelItemCount(), 2);
    EXPECT_EQ(tree->topLevelItem(0)->text(0), "Australia");
    EXPECT_EQ(tree->topLevelItem(1)->text(0), "Global");
    QStringList states;
    for (int i = 0; i < tree->topLevelItem(0)->childCount(); ++i) {
        states << tree->topLevelItem(0)->child(i)->text(0);
    }
    EXPECT_TRUE(states.contains("NSW"));
    EXPECT_TRUE(states.contains("National"));
    // No custom services yet: the Custom branch is not shown empty.
    EXPECT_TRUE(tree->topLevelItem(2)->isHidden());
    EXPECT_EQ(child<QLabel>(dialog, "onlineProjectCrs")->text(), "EPSG:7856");
}

TEST(GisOnlineDialog, SearchNarrowsTheTreeAndChoosesWhatIsLeft)
{
    Recorder recorder = withBuiltIn();
    OnlineDataDialog dialog(contextFor(recorder));
    auto* tree = child<QTreeWidget>(dialog, "onlineProviders");
    child<QLineEdit>(dialog, "onlineSearch")->setText("sentinel");
    const auto layers = visibleLayers(*tree);
    EXPECT_EQ(layers, (std::vector<std::string>{"sentinel2 truecolour", "sentinel2 composite"}));
    // The details describe the first of them, licence and attribution included.
    const QString details = child<QLabel>(dialog, "onlineDetails")->text();
    EXPECT_TRUE(details.contains("Contains modified Copernicus Sentinel data")) << details.toStdString();
    EXPECT_TRUE(details.contains("Copernicus Sentinel data terms"));
    EXPECT_TRUE(child<QPushButton>(dialog, "onlineImport")->isEnabled());
    // The Sentinel-2 options are live; the OpenStreetMap tag is not.
    EXPECT_TRUE(child<QDoubleSpinBox>(dialog, "onlineCloud")->isEnabled());
    EXPECT_FALSE(child<QLineEdit>(dialog, "onlineTag")->isEnabled());
    child<QLineEdit>(dialog, "onlineSearch")->setText("");
    EXPECT_GT(visibleLayers(*tree).size(), 30u);
}

TEST(GisOnlineDialog, ImportHandsOnExactlyTheCommandTheVerbWouldParse)
{
    Recorder recorder = withBuiltIn();
    OnlineDataDialog dialog(contextFor(recorder));
    ASSERT_TRUE(dialog.selectLayer("nsw-spatial", "lots"));
    auto* area = child<QComboBox>(dialog, "onlineArea");
    area->setCurrentIndex(area->findText("Typed box"));
    child<QLineEdit>(dialog, "onlineBox")->setText("151.215, -33.865, 151.2, -33.875");
    auto* boxCrs = child<QComboBox>(dialog, "onlineBoxCrs");
    boxCrs->setCurrentIndex(boxCrs->findText("WGS 84 longitude, latitude"));
    child<QLineEdit>(dialog, "onlineTargetLayer")->setText("cadastre/lots from nsw");
    child<QPushButton>(dialog, "onlineImport")->click();

    ASSERT_EQ(recorder.run.size(), 1u);
    const OnlineCommand& command = recorder.run.front();
    EXPECT_EQ(command.verb, katana::interop::OnlineVerb::Import);
    EXPECT_EQ(command.provider, "nsw-spatial");
    EXPECT_EQ(command.layer, "lots");
    EXPECT_EQ(command.area, OnlineAreaKind::Box);
    EXPECT_TRUE(command.boxIsLonLat);
    // Corners in either order make the same box.
    EXPECT_DOUBLE_EQ(command.box.minX, 151.2);
    EXPECT_DOUBLE_EQ(command.box.minY, -33.875);
    EXPECT_DOUBLE_EQ(command.box.maxX, 151.215);
    EXPECT_DOUBLE_EQ(command.box.maxY, -33.865);
    EXPECT_EQ(command.targetLayer, "cadastre/lots from nsw");
    // A vector layer has no resolution to give.
    EXPECT_FALSE(command.resolution.has_value());
    EXPECT_TRUE(child<QLabel>(dialog, "onlineStatus")->text().contains("in the background"));
}

TEST(GisOnlineDialog, ARasterCarriesItsResolutionWhenNotAutomatic)
{
    Recorder recorder = withBuiltIn();
    OnlineDataDialog dialog(contextFor(recorder));
    ASSERT_TRUE(dialog.selectLayer("copernicus", "dem"));
    auto command = dialog.command();
    ASSERT_TRUE(command.ok()) << command.error().describe();
    EXPECT_EQ(command->area, OnlineAreaKind::View);
    EXPECT_FALSE(command->resolution.has_value());
    child<QCheckBox>(dialog, "onlineResolutionAuto")->setChecked(false);
    child<QDoubleSpinBox>(dialog, "onlineResolution")->setValue(12.5);
    command = dialog.command();
    ASSERT_TRUE(command.ok());
    ASSERT_TRUE(command->resolution.has_value());
    EXPECT_DOUBLE_EQ(*command->resolution, 12.5);
}

TEST(GisOnlineDialog, AMalformedBoxIsSaidInTheStatusAndNothingRuns)
{
    Recorder recorder = withBuiltIn();
    OnlineDataDialog dialog(contextFor(recorder));
    ASSERT_TRUE(dialog.selectLayer("osm", "buildings"));
    auto* area = child<QComboBox>(dialog, "onlineArea");
    area->setCurrentIndex(area->findText("Typed box"));
    child<QLineEdit>(dialog, "onlineBox")->setText("1,2,3");
    child<QPushButton>(dialog, "onlineImport")->click();
    EXPECT_TRUE(recorder.run.empty());
    EXPECT_TRUE(child<QLabel>(dialog, "onlineStatus")->text().contains("four numbers"));
}

TEST(GisOnlineDialog, NoLayerChosenMeansNoImport)
{
    Recorder recorder = withBuiltIn();
    OnlineDataDialog dialog(contextFor(recorder));
    EXPECT_FALSE(child<QPushButton>(dialog, "onlineImport")->isEnabled());
    EXPECT_EQ(dialog.command().error().code, ErrorCode::InvalidArgument);
    // A catalogue search is not importable either.
    ASSERT_TRUE(dialog.selectLayer("data-gov-au", "search"));
    EXPECT_FALSE(child<QPushButton>(dialog, "onlineImport")->isEnabled());
    EXPECT_TRUE(child<QLabel>(dialog, "onlineDetails")->text().contains("ONLINE LAYERS data-gov-au"));
}

TEST(GisOnlineDialog, AKeyIsSavedUnderTheLayersKeyNameAndNeverShownBack)
{
    Recorder recorder = withBuiltIn();
    OnlineDataDialog dialog(contextFor(recorder));
    ASSERT_TRUE(dialog.selectLayer("nasa-gibs", "modis-terra"));
    auto* key = child<QLineEdit>(dialog, "onlineKey");
    EXPECT_EQ(key->echoMode(), QLineEdit::Password);
    key->setText("abc123");
    child<QPushButton>(dialog, "onlineSaveKey")->click();
    EXPECT_EQ(recorder.keys.at("nasa-gibs"), "abc123");
    EXPECT_TRUE(key->text().isEmpty());
    EXPECT_FALSE(child<QLabel>(dialog, "onlineStatus")->text().contains("abc123"));
    EXPECT_FALSE(child<QLabel>(dialog, "onlineDetails")->text().contains("abc123"));
}

TEST(GisOnlineDialog, AddCustomServiceNeedsAnAddress)
{
    Recorder recorder = withBuiltIn();
    OnlineDataDialog dialog(contextFor(recorder));
    child<QPushButton>(dialog, "onlineAddCustom")->click();
    EXPECT_TRUE(recorder.custom.empty());
    EXPECT_TRUE(child<QLabel>(dialog, "onlineStatus")->text().contains("address"));
    child<QLineEdit>(dialog, "onlineCustomUrl")->setText("https://example.org/wms?key=SECRET");
    child<QPushButton>(dialog, "onlineAddCustom")->click();
    ASSERT_EQ(recorder.custom.size(), 1u);
    EXPECT_EQ(recorder.custom.front(), "https://example.org/wms?key=SECRET");
    // What the status repeats has the key taken out.
    const QString status = child<QLabel>(dialog, "onlineStatus")->text();
    EXPECT_FALSE(status.contains("SECRET")) << status.toStdString();
}

TEST(GisOnlineDialog, ACustomProviderAppearsUnderCustomAfterARefresh)
{
    Recorder recorder = withBuiltIn();
    OnlineDataDialog dialog(contextFor(recorder));
    auto custom = katana::interop::parseCatalogue(R"({"providers": [{"id": "custom-example-org",
        "title": "Example WMS", "group": "Custom", "licence": "L", "attribution": "A",
        "services": [{"id": "wms", "title": "W", "type": "wms", "endpoint": "https://example.org/wms",
          "layers": [{"id": "roads", "title": "Roads", "kind": "imagery", "layer": "roads"}]}]}]})",
                                                  true);
    ASSERT_TRUE(custom.ok());
    recorder.catalogue = katana::interop::mergeCatalogues(recorder.catalogue, *custom);
    dialog.refreshCatalogue();
    auto* tree = child<QTreeWidget>(dialog, "onlineProviders");
    EXPECT_FALSE(tree->topLevelItem(2)->isHidden());
    EXPECT_EQ(tree->topLevelItem(2)->text(0), "Custom");
    EXPECT_TRUE(dialog.selectLayer("custom-example-org", "roads"));
}
