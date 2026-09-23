// Survey field data into the drawing (PLAN.MD 45).
//
// The test this file exists for is ImportedPointsAreNotTransposed. Every other
// failure mode here announces itself; a transposed survey does not - it is
// internally consistent, it plots, and it is in the wrong place.

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <limits>
#include <string>
#include <variant>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/cad/survey_import.hpp"
#include "katana/entity/entity.hpp"
#include "katana/survey/data_model.hpp"

namespace {

using katana::cad::Document;
using katana::cad::SurveyImportOptions;
using katana::cad::SurveyImportReport;
using katana::cad::importSurveyProject;
using katana::entity::Entity;
using katana::survey::CoordinateSource;
using katana::survey::SourceRecord;
using katana::survey::SurveyPoint;
using katana::survey::SurveyProject;

// Northing and easting of unmistakably different magnitudes, so that a
// transposition cannot pass any assertion here by coincidence. These are the
// shape of a real projected survey coordinate: northing in the millions,
// easting in the hundreds of thousands.
constexpr double kNorthing = 5'432'109.25;
constexpr double kEasting = 456'789.5;

SurveyPoint pointAt(std::string id, double northing, double easting, double elevation = 0.0)
{
    SurveyPoint point;
    point.id = std::move(id);
    point.northing = northing;
    point.easting = easting;
    point.elevation = elevation;
    return point;
}

SurveyProject oneLeicaPoint()
{
    SurveyProject project;
    SurveyPoint point = pointAt("101", kNorthing, kEasting, 42.125);
    point.code = "BM";
    point.description = "brass plug in kerb";
    point.coordinateSource = CoordinateSource::FieldObserved;
    point.source = SourceRecord{"Leica", "GSI-16", "16 byte words", "FIELD_2026.GSI", 183};
    project.points.push_back(std::move(point));
    return project;
}

// Executes the command the bridge produced into a document the caller owns:
// Document is move-only, and a test reads better owning the thing it asserts on.
void importInto(Document& document, const SurveyProject& project,
                const SurveyImportOptions& options, SurveyImportReport* report = nullptr)
{
    auto command = importSurveyProject(document, project, options, report);
    ASSERT_TRUE(command.ok()) << (command.ok() ? "" : command.error().describe());
    if (*command != nullptr) {
        const auto status = document.execute(std::move(*command));
        ASSERT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    }
}

[[nodiscard]] const Entity* onlyEntity(const Document& document)
{
    const auto ids = document.model().entities.ids();
    if (ids.size() != 1) {
        return nullptr;
    }
    return document.model().entities.find(ids.front());
}

[[nodiscard]] double realProperty(const Entity& entity, std::string_view key)
{
    const auto found = entity.properties.find(key);
    if (found == entity.properties.end()) {
        return std::numeric_limits<double>::quiet_NaN();
    }
    const double* value = std::get_if<double>(&found->second);
    return value != nullptr ? *value : std::numeric_limits<double>::quiet_NaN();
}

[[nodiscard]] std::string textProperty(const katana::entity::PropertyMap& map, std::string_view key)
{
    const auto found = map.find(key);
    if (found == map.end()) {
        return {};
    }
    const std::string* value = std::get_if<std::string>(&found->second);
    return value != nullptr ? *value : std::string{};
}

} // namespace

// THE test. A survey point is (northing, easting) - north first, as a field book
// and LandXML write it. A CAD Point2 is (x, y), which is EASTING first. The two
// magnitudes here differ by a factor of ten, so a swap cannot pass.
TEST(SurveyImport, ImportedPointsAreNotTransposed)
{
    Document document;
    ASSERT_NO_FATAL_FAILURE(importInto(document, oneLeicaPoint(), {}));
    const Entity* entity = onlyEntity(document);
    ASSERT_NE(entity, nullptr);

    const auto* point = std::get_if<katana::entity::PointGeometry>(&entity->geometry);
    ASSERT_NE(point, nullptr) << "a survey point should import as a point entity";
    EXPECT_DOUBLE_EQ(point->position.x, kEasting) << "x is EASTING";
    EXPECT_DOUBLE_EQ(point->position.y, kNorthing) << "y is NORTHING";
}

TEST(SurveyImport, ThePointKeepsItsNumberCodeDescriptionAndHeight)
{
    Document document;
    ASSERT_NO_FATAL_FAILURE(importInto(document, oneLeicaPoint(), {}));
    const Entity* entity = onlyEntity(document);
    ASSERT_NE(entity, nullptr);

    EXPECT_EQ(textProperty(entity->properties, "point"), "101");
    EXPECT_EQ(textProperty(entity->properties, "code"), "BM");
    EXPECT_EQ(textProperty(entity->properties, "description"), "brass plug in kerb");
    EXPECT_DOUBLE_EQ(realProperty(*entity, katana::entity::kElevationProperty), 42.125);
}

// The height goes in the property the 12d import writes and the surface builder
// reads. A second name for it would build every drawing's surface flat.
TEST(SurveyImport, TheHeightUsesTheSamePropertyTheSurfaceBuilderReads)
{
    EXPECT_EQ(katana::entity::kElevationProperty, "elevation");
    Document document;
    ASSERT_NO_FATAL_FAILURE(importInto(document, oneLeicaPoint(), {}));
    const Entity* entity = onlyEntity(document);
    ASSERT_NE(entity, nullptr);
    EXPECT_TRUE(entity->properties.contains(std::string(katana::entity::kElevationProperty)));
}

// The brief's section 18: the drawing alone has to answer "where did this come
// from", down to the record.
TEST(SurveyImport, EveryPointRecordsTheFileAndRecordItCameFrom)
{
    Document document;
    ASSERT_NO_FATAL_FAILURE(importInto(document, oneLeicaPoint(), {}));
    const Entity* entity = onlyEntity(document);
    ASSERT_NE(entity, nullptr);

    EXPECT_EQ(textProperty(entity->metadata, std::string(katana::cad::kSourceManufacturerMeta)),
              "Leica");
    EXPECT_EQ(textProperty(entity->metadata, std::string(katana::cad::kSourceFormatMeta)),
              "GSI-16");
    EXPECT_EQ(textProperty(entity->metadata, std::string(katana::cad::kSourceFormatVersionMeta)),
              "16 byte words");
    EXPECT_EQ(textProperty(entity->metadata, std::string(katana::cad::kSourceFileMeta)),
              "FIELD_2026.GSI");

    const auto record = entity->metadata.find(std::string(katana::cad::kSourceRecordMeta));
    ASSERT_NE(record, entity->metadata.end());
    const std::int64_t* number = std::get_if<std::int64_t>(&record->second);
    ASSERT_NE(number, nullptr) << "a record number is a number, so a report can sort on it";
    EXPECT_EQ(*number, 183);
}

// Calculated must be distinguishable from observed, because a report may not
// present a derived coordinate as evidence.
TEST(SurveyImport, AnObservedCoordinateIsDistinguishableFromACalculatedOne)
{
    SurveyProject project;
    SurveyPoint observed = pointAt("1", kNorthing, kEasting);
    observed.coordinateSource = CoordinateSource::FieldObserved;
    SurveyPoint calculated = pointAt("2", kNorthing + 10.0, kEasting + 10.0);
    calculated.coordinateSource = CoordinateSource::Calculated;
    project.points = {observed, calculated};

    Document document;
    ASSERT_NO_FATAL_FAILURE(importInto(document, project, {}));
    const auto ids = document.model().entities.ids();
    ASSERT_EQ(ids.size(), 2U);

    const std::string key(katana::cad::kCoordinateSourceProperty);
    EXPECT_EQ(textProperty(document.model().entities.find(ids[0])->properties, key),
              "field observed");
    EXPECT_EQ(textProperty(document.model().entities.find(ids[1])->properties, key), "calculated");
}

// A whole import is one undo step. This is the brief's section 19 and it is the
// reason the bridge returns one command rather than a list of them.
TEST(SurveyImport, OneUndoRemovesTheWholeImportAndOneRedoBringsItBack)
{
    SurveyProject project;
    for (int i = 1; i <= 25; ++i) {
        project.points.push_back(
            pointAt(std::to_string(i), kNorthing + i, kEasting + i, 10.0 + i));
    }

    Document document;
    auto command = importSurveyProject(document, project, {});
    ASSERT_TRUE(command.ok()) << command.error().describe();
    ASSERT_NE(*command, nullptr);
    ASSERT_TRUE(document.execute(std::move(*command)).ok());
    ASSERT_EQ(document.model().entities.size(), 25U);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().entities.size(), 0U) << "one undo, not twenty-five";

    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(document.model().entities.size(), 25U);
}

// The layer the points need is created inside the same transaction, so the undo
// above takes it away again too.
TEST(SurveyImport, TheLayerIsCreatedInTheSameTransactionAndUndoneWithIt)
{
    Document document;
    SurveyImportOptions options;
    options.layer = "survey/field/2026";

    SurveyImportReport report;
    auto command = importSurveyProject(document, oneLeicaPoint(), options, &report);
    ASSERT_TRUE(command.ok()) << command.error().describe();
    ASSERT_EQ(report.layersCreated, std::vector<std::string>{"survey/field/2026"});

    ASSERT_TRUE(document.execute(std::move(*command)).ok());
    ASSERT_NE(document.model().layers.find("survey/field/2026"), nullptr);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().layers.find("survey/field/2026"), nullptr)
        << "the layer came in with the import, so it goes out with it";
}

TEST(SurveyImport, ALayerPerCodePutsEachCodeOnItsOwnLayerBeneathTheTarget)
{
    SurveyProject project;
    SurveyPoint kerb = pointAt("1", kNorthing, kEasting);
    kerb.code = "EP";
    SurveyPoint mark = pointAt("2", kNorthing + 5.0, kEasting + 5.0);
    mark.code = "BM";
    SurveyPoint uncoded = pointAt("3", kNorthing + 9.0, kEasting + 9.0);
    project.points = {kerb, mark, uncoded};

    SurveyImportOptions options;
    options.layerPerCode = true;
    SurveyImportReport report;
    Document document;
    ASSERT_NO_FATAL_FAILURE(importInto(document, project, options, &report));

    // Name order, because the layers are created from an ordered set.
    EXPECT_EQ(report.layersCreated,
              (std::vector<std::string>{"survey/points", "survey/points/BM",
                                        "survey/points/EP"}));
    ASSERT_NE(document.model().layers.find("survey/points/EP"), nullptr);
    ASSERT_NE(document.model().layers.find("survey/points/BM"), nullptr);
    // An uncoded point has nowhere else to go than the target layer itself.
    ASSERT_NE(document.model().layers.find("survey/points"), nullptr);
}

// A layer name IS a path, so a '/' in a field code would silently build a
// subtree. It is replaced instead.
TEST(SurveyImport, ASeparatorInAFieldCodeDoesNotSilentlyCreateALayerSubtree)
{
    SurveyProject project;
    SurveyPoint point = pointAt("1", kNorthing, kEasting);
    point.code = "KERB/TOP";
    project.points.push_back(point);

    SurveyImportOptions options;
    options.layerPerCode = true;
    Document document;
    ASSERT_NO_FATAL_FAILURE(importInto(document, project, options));

    EXPECT_NE(document.model().layers.find("survey/points/KERB_TOP"), nullptr);
    EXPECT_EQ(document.model().layers.find("survey/points/KERB"), nullptr)
        << "a code is user data and must not be able to nest a layer";
}

TEST(SurveyImport, AProjectWithNoPointsIsNothingToDoRatherThanAFailure)
{
    Document document;
    auto command = importSurveyProject(document, SurveyProject{}, {});
    ASSERT_TRUE(command.ok()) << "an empty project is not an error";
    EXPECT_EQ(*command, nullptr) << "and it is not a command either";
}

// The bridge asks the survey layer rather than checking again itself, so an
// inconsistent project is refused with that layer's own error.
TEST(SurveyImport, AnInconsistentProjectIsRefusedBeforeTheDrawingIsTouched)
{
    SurveyProject project;
    project.points = {pointAt("1", kNorthing, kEasting), pointAt("1", 0.0, 0.0)};

    Document document;
    const auto command = importSurveyProject(document, project, {});
    EXPECT_FALSE(command.ok()) << "two points cannot share an id";
    EXPECT_EQ(document.model().entities.size(), 0U);
}

TEST(SurveyImport, RefusingToCreateLayersRefusesTheImportRatherThanMovingThePoints)
{
    SurveyImportOptions options;
    options.createLayers = false;

    Document document;
    const auto command = importSurveyProject(document, oneLeicaPoint(), options);
    ASSERT_FALSE(command.ok());
    EXPECT_EQ(command.error().code, katana::core::ErrorCode::NotFound);
    EXPECT_EQ(document.model().entities.size(), 0U)
        << "a point on the wrong layer is worse than an error";
}

// The file's own extra fields survive the import, but never on top of a field
// the import itself writes.
TEST(SurveyImport, ExtraFieldsFromTheFileAreKeptButDoNotOverwriteTheImportsOwn)
{
    SurveyProject project;
    SurveyPoint point = pointAt("101", kNorthing, kEasting);
    point.code = "BM";
    point.metadata = {{"prism", "0.030"}, {"code", "SHOULD NOT WIN"}};
    project.points.push_back(point);

    SurveyImportReport report;
    Document document;
    ASSERT_NO_FATAL_FAILURE(importInto(document, project, {}, &report));
    const Entity* entity = onlyEntity(document);
    ASSERT_NE(entity, nullptr);

    EXPECT_EQ(textProperty(entity->properties, "prism"), "0.030");
    EXPECT_EQ(textProperty(entity->properties, "code"), "BM") << "the code column wins";
    ASSERT_EQ(report.warnings.size(), 1U) << "and the clash is reported, not hidden";
    EXPECT_NE(report.warnings.front().find("code"), std::string::npos);
}

// The default property names are the ones applySurveyCodes() looks for, so
// importing field data and then applying a mapfile is one workflow.
TEST(SurveyImport, TheDefaultCodePropertyIsOneTheSurveyCodingLooksFor)
{
    const SurveyImportOptions options;
    const auto& candidates = katana::cad::codePropertyCandidates();
    EXPECT_NE(std::find(candidates.begin(), candidates.end(), options.codeProperty),
              candidates.end())
        << "an import whose code property the coding cannot find is two workflows, not one";
}
