#include <gtest/gtest.h>

#include <cmath>
#include <numbers>
#include <string>
#include <vector>

#include "katana/commands/command_stack.hpp"
#include "katana/entity/model.hpp"
#include "katana/ifc/export.hpp"
#include "katana/ifc/import.hpp"

namespace ifc = katana::ifc;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

constexpr double kPi = std::numbers::pi;

// An IFC2X3 file in millimetres, as most building models are: a water main
// placed at (5000, 2000, 1000) mm with its x axis turned to north, its axis
// 3000 mm along that and 500 mm down. In metres, from (5, 2, 1) to (5, 5,
// 0.5). Its diameter, 150 mm, is a length and comes in as 0.15.
constexpr std::string_view kMillimetres = R"(ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [CoordinationView_V2.0]'),'2;1');
FILE_NAME('mm.ifc','2020-01-01T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('IFC2X3'));
ENDSEC;
DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);
#3=IFCUNITASSIGNMENT((#1,#2));
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#5,$);
#7=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'mm',$,$,$,$,(#6),#3);
#8=IFCLOCALPLACEMENT($,#5);
#9=IFCSITE('1YvctVUKr0kugbFTf53O9L',$,'Site',$,$,#8,$,$,.ELEMENT.,$,$,$,$,$);
#10=IFCCARTESIANPOINT((5000.,2000.,1000.));
#11=IFCDIRECTION((0.,1.,0.));
#12=IFCDIRECTION((0.,0.,1.));
#13=IFCAXIS2PLACEMENT3D(#10,#12,#11);
#14=IFCLOCALPLACEMENT(#8,#13);
#15=IFCCARTESIANPOINT((0.,0.,0.));
#16=IFCCARTESIANPOINT((3000.,0.,-500.));
#17=IFCPOLYLINE((#15,#16));
#18=IFCSHAPEREPRESENTATION(#6,'Axis','Curve3D',(#17));
#19=IFCPRODUCTDEFINITIONSHAPE($,$,(#18));
#20=IFCFLOWSEGMENT('2YvctVUKr0kugbFTf53O9L',$,'Main 1','A water main',$,#14,#19,'WM-1');
#21=IFCPROPERTYSINGLEVALUE('NominalDiameter',$,IFCPOSITIVELENGTHMEASURE(150.),$);
#22=IFCPROPERTYSINGLEVALUE('Material',$,IFCLABEL('DICL'),$);
#23=IFCPROPERTYSINGLEVALUE('InService',$,IFCBOOLEAN(.T.),$);
#24=IFCPROPERTYSET('3YvctVUKr0kugbFTf53O9L',$,'Pset_Main',$,(#21,#22,#23));
#25=IFCRELDEFINESBYPROPERTIES('0ZvctVUKr0kugbFTf53O9L',$,$,$,(#20),#24);
#26=IFCRELCONTAINEDINSPATIALSTRUCTURE('1ZvctVUKr0kugbFTf53O9L',$,$,$,(#20),#9);
ENDSEC;
END-ISO-10303-21;
)";

// An IFC4 file georeferenced to MGA zone 56 with its x axis turned
// atan2(0.8, 0.6) from east, and its angles in degrees. A point at (10, 0,
// 2) is at E 300000 + 0.6 x 10, N 6200000 + 0.8 x 10, height 10 + 2; a
// quarter circle of radius 5 about the origin, trimmed 0 to 90 degrees,
// starts at the rotation's angle; the next quarter, trimmed by the points
// (0, 5) and (-5, 0), 90 degrees after it.
constexpr std::string_view kRotated = R"(ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [ReferenceView_V1.2]'),'2;1');
FILE_NAME('rotated.ifc','2020-01-01T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#2=IFCDIMENSIONALEXPONENTS(0,0,0,0,0,0,0);
#3=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);
#4=IFCMEASUREWITHUNIT(IFCPLANEANGLEMEASURE(0.017453292519943295),#3);
#5=IFCCONVERSIONBASEDUNIT(#2,.PLANEANGLEUNIT.,'DEGREE',#4);
#6=IFCUNITASSIGNMENT((#1,#5));
#7=IFCCARTESIANPOINT((0.,0.,0.));
#8=IFCAXIS2PLACEMENT3D(#7,$,$);
#9=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#8,$);
#10=IFCPROJECTEDCRS('EPSG:7856','GDA2020 / MGA zone 56',$,$,$,$,$);
#11=IFCMAPCONVERSION(#9,#10,300000.,6200000.,10.,0.6,0.8,$);
#12=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'rotated',$,$,$,$,(#9),#6);
#13=IFCLOCALPLACEMENT($,#8);
#14=IFCCARTESIANPOINT((10.,0.,2.));
#15=IFCSHAPEREPRESENTATION(#9,'Annotation','Point',(#14));
#16=IFCPRODUCTDEFINITIONSHAPE($,$,(#15));
#17=IFCANNOTATION('1YvctVUKr0kugbFTf53O9L',$,'Peg',$,$,#13,#16);
#18=IFCCARTESIANPOINT((0.,0.));
#19=IFCAXIS2PLACEMENT2D(#18,$);
#20=IFCCIRCLE(#19,5.);
#21=IFCTRIMMEDCURVE(#20,(IFCPARAMETERVALUE(0.)),(IFCPARAMETERVALUE(90.)),.T.,.PARAMETER.);
#22=IFCSHAPEREPRESENTATION(#9,'Annotation','Curve2D',(#21));
#23=IFCPRODUCTDEFINITIONSHAPE($,$,(#22));
#24=IFCANNOTATION('2YvctVUKr0kugbFTf53O9L',$,'Kerb return',$,$,#13,#23);
#25=IFCCARTESIANPOINT((0.,5.));
#26=IFCCARTESIANPOINT((-5.,0.));
#27=IFCTRIMMEDCURVE(#20,(#25),(#26),.T.,.CARTESIAN.);
#28=IFCSHAPEREPRESENTATION(#9,'Annotation','Curve2D',(#27));
#29=IFCPRODUCTDEFINITIONSHAPE($,$,(#28));
#30=IFCANNOTATION('3YvctVUKr0kugbFTf53O9L',$,'Trimmed by points',$,$,#13,#29);
ENDSEC;
END-ISO-10303-21;
)";

// An IFC 4.3 alignment that starts on a curve - a circular arc of radius
// 100 turning left for 50 m, then 50 m of tangent - which has no PI form:
// it comes in as its exact geometry. The arc ends 0.5 rad round, at
// (100 sin 0.5, 100 (1 - cos 0.5)).
constexpr std::string_view kStartsOnACurve = R"(ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [Alignment-basedView]'),'2;1');
FILE_NAME('curve.ifc','2020-01-01T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('IFC4X3_ADD2'));
ENDSEC;
DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#2=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);
#3=IFCUNITASSIGNMENT((#1,#2));
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#5,$);
#7=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'curve',$,$,$,$,(#6),#3);
#8=IFCLOCALPLACEMENT($,#5);
#9=IFCALIGNMENT('1YvctVUKr0kugbFTf53O9L',$,'Ramp A',$,$,#8,$,$);
#10=IFCALIGNMENTHORIZONTAL('2YvctVUKr0kugbFTf53O9L',$,$,$,$,#8,$);
#11=IFCRELNESTS('3YvctVUKr0kugbFTf53O9L',$,$,$,#9,(#10));
#12=IFCCARTESIANPOINT((0.,0.));
#13=IFCALIGNMENTHORIZONTALSEGMENT($,$,#12,0.,100.,100.,50.,$,.CIRCULARARC.);
#14=IFCALIGNMENTSEGMENT('0ZvctVUKr0kugbFTf53O9L',$,$,$,$,#8,$,#13);
#15=IFCCARTESIANPOINT((47.942553860420304,12.241743810962724));
#16=IFCALIGNMENTHORIZONTALSEGMENT($,$,#15,0.5,0.,0.,50.,$,.LINE.);
#17=IFCALIGNMENTSEGMENT('1ZvctVUKr0kugbFTf53O9L',$,$,$,$,#8,$,#16);
#18=IFCCARTESIANPOINT((91.82168195493894,36.213020741172876));
#19=IFCALIGNMENTHORIZONTALSEGMENT($,$,#18,0.5,0.,0.,0.,$,.LINE.);
#20=IFCALIGNMENTSEGMENT('2ZvctVUKr0kugbFTf53O9L',$,$,$,$,#8,$,#19);
#21=IFCRELNESTS('3ZvctVUKr0kugbFTf53O9L',$,$,$,#10,(#14,#17,#20));
ENDSEC;
END-ISO-10303-21;
)";

std::string metadata(const Entity& entity, const std::string& key)
{
    const auto found = entity.metadata.find(key);
    return found == entity.metadata.end() ? std::string("<absent>")
                                          : katana::entity::toString(found->second);
}

std::string property(const Entity& entity, const std::string& name)
{
    const auto found = entity.properties.find(name);
    return found == entity.properties.end() ? std::string("<absent>")
                                            : katana::entity::toString(found->second);
}

const Entity* withName(const ifc::IfcImport& imported, const std::string& name)
{
    for (const Entity& entity : imported.entities) {
        if (metadata(entity, "ifc.name") == name) {
            return &entity;
        }
    }
    return nullptr;
}

// The vertices of a line or polyline.
std::vector<Point2> verticesOf(const Entity& entity)
{
    if (const auto* line = std::get_if<katana::geometry::Segment2>(&entity.geometry)) {
        return {line->start, line->end};
    }
    if (const auto* polyline = std::get_if<Polyline2>(&entity.geometry)) {
        return polyline->vertices;
    }
    return {};
}

ifc::IfcImport read(std::string_view text, const ifc::ImportOptions& options = {})
{
    auto result = ifc::readIfc(text, options);
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(*result) : ifc::IfcImport{};
}

// The drawing of test_export.cpp's scenario, smaller: MC01 and a kerb.
Model scenario()
{
    Model model;
    katana::entity::Alignment alignment;
    alignment.name = "MC01";
    alignment.horizontal.startStation = 1000.0;
    alignment.horizontal.pis = {{Point2{333900.0, 6249950.0}, 0.0, 0.0, 0.0},
                                {Point2{334000.0, 6250020.0}, 80.0, 20.0, 20.0},
                                {Point2{334100.0, 6249990.0}, 60.0, 0.0, 0.0},
                                {Point2{334200.0, 6250060.0}, 0.0, 0.0, 0.0}};
    alignment.vertical = katana::geometry::VerticalAlignment{
        {{1000.0, 21.5, 0.0}, {1080.0, 23.0, 60.0}, {1180.0, 20.0, 80.0}, {1330.5, 21.0, 0.0}}};
    EXPECT_TRUE(model.alignments.add(alignment).ok());
    Entity kerb;
    kerb.geometry =
        Polyline2{{{333990.0, 6250000.0}, {334010.0, 6250004.0}, {334030.0, 6250008.0}}, false};
    kerb.layer = "Survey/Kerb";
    katana::entity::setHeights(kerb.properties, {20.10, std::nullopt, 20.00});
    kerb.properties["code"] = std::string("KB");
    EXPECT_TRUE(model.entities.add(kerb).ok());
    Entity text;
    text.geometry = katana::entity::TextGeometry{{334000.0, 6250020.0}, "LOT 42"};
    text.layer = "Survey/Text";
    EXPECT_TRUE(model.entities.add(text).ok());
    return model;
}

std::string exportedScenario(const Model& model)
{
    ifc::ExportOptions options;
    options.georeference.name = "EPSG:7856";
    auto out = ifc::writeIfc({&model, {}, {}}, options);
    EXPECT_TRUE(out.ok()) << (out.ok() ? "" : out.error().describe());
    return out.ok() ? out->text : std::string();
}

} // namespace

// ---- Katana's own files -------------------------------------------------------------

// What Katana writes comes back as what it was: the alignment as its PIs
// and PVIs (reconstructed from the business logic, not stored anywhere in
// the file), the entities where they were in MGA, on their layers, with
// their heights - an absent one still absent - and their properties.
TEST(IfcImport, AnExportReadBackIsTheDrawingItWas)
{
    const Model model = scenario();
    const auto imported = read(exportedScenario(model));
    EXPECT_EQ(imported.schema, "IFC4X3_ADD2");
    EXPECT_EQ(imported.coordinateSystem, "EPSG:7856");
    EXPECT_EQ(imported.alignmentsAsPolylines, 0u);
    EXPECT_TRUE(imported.warnings.empty()) << imported.warnings.front();

    ASSERT_EQ(imported.alignments.size(), 1u);
    const auto& original = *model.alignments.find("MC01");
    const auto& back = imported.alignments[0];
    EXPECT_EQ(back.name, "MC01");
    EXPECT_NEAR(back.horizontal.startStation, 1000.0, 1e-9);
    ASSERT_EQ(back.horizontal.pis.size(), original.horizontal.pis.size());
    for (std::size_t i = 0; i < back.horizontal.pis.size(); ++i) {
        const auto& a = back.horizontal.pis[i];
        const auto& b = original.horizontal.pis[i];
        EXPECT_NEAR(a.point.x, b.point.x, 1e-6) << i;
        EXPECT_NEAR(a.point.y, b.point.y, 1e-6) << i;
        EXPECT_NEAR(a.radius, b.radius, 1e-6) << i;
        EXPECT_NEAR(a.spiralIn, b.spiralIn, 1e-6) << i;
        EXPECT_NEAR(a.spiralOut, b.spiralOut, 1e-6) << i;
    }
    ASSERT_TRUE(back.vertical);
    ASSERT_EQ(back.vertical->pvis.size(), original.vertical->pvis.size());
    for (std::size_t i = 0; i < back.vertical->pvis.size(); ++i) {
        const auto& a = back.vertical->pvis[i];
        const auto& b = original.vertical->pvis[i];
        EXPECT_NEAR(a.station, b.station, 1e-6) << i;
        EXPECT_NEAR(a.elevation, b.elevation, 1e-6) << i;
        EXPECT_NEAR(a.curveLength, b.curveLength, 1e-6) << i;
    }

    const Entity* kerb = nullptr;
    const Entity* text = nullptr;
    for (const Entity& entity : imported.entities) {
        if (metadata(entity, "ifc.class") == "IfcKerb") {
            kerb = &entity;
        } else if (std::holds_alternative<katana::entity::TextGeometry>(entity.geometry)) {
            text = &entity;
        }
    }
    ASSERT_NE(kerb, nullptr);
    EXPECT_EQ(kerb->layer, "Survey/Kerb");
    const auto vertices = verticesOf(*kerb);
    ASSERT_EQ(vertices.size(), 3u);
    EXPECT_NEAR(vertices[0].x, 333990.0, 1e-6);
    EXPECT_NEAR(vertices[2].y, 6250008.0, 1e-6);
    const auto heights = katana::entity::heightsOf(kerb->properties, 3);
    ASSERT_TRUE(heights[0] && heights[2]);
    EXPECT_NEAR(*heights[0], 20.10, 1e-9);
    EXPECT_FALSE(heights[1]); // absent is not zero
    EXPECT_EQ(property(*kerb, "code"), "KB");
    EXPECT_EQ(metadata(*kerb, "ifc.container"), "Site");

    ASSERT_NE(text, nullptr);
    const auto& literal = std::get<katana::entity::TextGeometry>(text->geometry);
    EXPECT_EQ(literal.text, "LOT 42");
    EXPECT_NEAR(literal.position.x, 334000.0, 1e-6);
    EXPECT_EQ(text->layer, "Survey/Text");
}

// The drawing system's curves (docs/drawing.md) written by the export come
// back as linework on the same curves: an elliptical arc as the arc, not the
// whole ellipse it is trimmed from; a rational spline, not a point at 0,0.
TEST(IfcImport, AnExportedEllipticalArcAndSplineReadBackOnTheirCurves)
{
    Model model = scenario();
    // Semi-axes 4 east and 2, from eccentric anomaly 5.5 for 2 radians -
    // through the parameter 0, so the file's trim end is below its start.
    const katana::geometry::Ellipse2 arc{Point2{334020.0, 6250020.0},
                                         katana::geometry::Vec2(4.0, 0.0), 0.5, 5.5, 2.0};
    Entity ellipse;
    ellipse.geometry = arc;
    ellipse.layer = "Survey/Detail";
    ASSERT_TRUE(model.entities.add(ellipse).ok());
    katana::geometry::Spline2 curve;
    curve.degree = 2;
    curve.controlPoints = {Point2{334020.0, 6250040.0}, Point2{334025.0, 6250045.0},
                           Point2{334030.0, 6250040.0}};
    curve.knots = {0.0, 0.0, 0.0, 1.0, 1.0, 1.0};
    curve.weights = {1.0, 0.5, 1.0};
    Entity spline;
    spline.geometry = curve;
    spline.layer = "Survey/Curve";
    ASSERT_TRUE(model.entities.add(spline).ok());

    const auto imported = read(exportedScenario(model));
    const Entity* ellipseBack = nullptr;
    const Entity* splineBack = nullptr;
    for (const Entity& entity : imported.entities) {
        if (entity.layer == "Survey/Detail") {
            ellipseBack = &entity;
        } else if (entity.layer == "Survey/Curve") {
            splineBack = &entity;
        }
    }
    ASSERT_NE(ellipseBack, nullptr);
    ASSERT_NE(splineBack, nullptr);

    const auto* arcBack = std::get_if<Polyline2>(&ellipseBack->geometry);
    ASSERT_NE(arcBack, nullptr);
    EXPECT_FALSE(arcBack->closed) << "the arc came back as the whole ellipse";
    ASSERT_GE(arcBack->vertices.size(), 3u);
    EXPECT_NEAR(arcBack->vertices.front().distanceTo(arc.startPoint()), 0.0, 1e-6);
    EXPECT_NEAR(arcBack->vertices.back().distanceTo(arc.endPoint()), 0.0, 1e-6);
    for (const Point2& p : arcBack->vertices) {
        EXPECT_NEAR(arc.distanceTo(p), 0.0, 1e-6) << "a vertex off the ellipse";
    }

    const auto* splineLine = std::get_if<Polyline2>(&splineBack->geometry);
    ASSERT_NE(splineLine, nullptr) << "the spline did not come back as linework";
    ASSERT_GE(splineLine->vertices.size(), 3u);
    // Clamped: it starts and ends on its end control points.
    EXPECT_NEAR(splineLine->vertices.front().distanceTo(curve.controlPoints.front()), 0.0, 1e-6);
    EXPECT_NEAR(splineLine->vertices.back().distanceTo(curve.controlPoints.back()), 0.0, 1e-6);
    for (const Point2& p : splineLine->vertices) {
        // Spline2::distanceTo measures to its own chords, to a tenth of
        // kCurveChordTolerance (curves2d.hpp): the bound on a vertex ON it.
        EXPECT_LE(curve.distanceTo(p), 0.1 * katana::geometry::kCurveChordTolerance + 1e-9)
            << "a vertex off the spline";
    }
}

// ---- other writers' files -----------------------------------------------------------

TEST(IfcImport, LengthsComeInAsMetresWhateverTheFilesUnitAndPlacementsAreFollowed)
{
    ifc::ImportOptions options;
    options.sourceName = "mm.ifc";
    const auto imported = read(kMillimetres, options);
    EXPECT_EQ(imported.schema, "IFC2X3");
    EXPECT_EQ(imported.products, 1u);
    EXPECT_EQ(imported.productsImported, 1u);
    EXPECT_EQ(imported.classes.at("IfcFlowSegment"), 1u);
    const Entity* main = withName(imported, "Main 1");
    ASSERT_NE(main, nullptr);
    const auto vertices = verticesOf(*main);
    ASSERT_EQ(vertices.size(), 2u);
    EXPECT_NEAR(vertices[0].x, 5.0, 1e-12);
    EXPECT_NEAR(vertices[0].y, 2.0, 1e-12);
    EXPECT_NEAR(vertices[1].x, 5.0, 1e-12);
    EXPECT_NEAR(vertices[1].y, 5.0, 1e-12);
    const auto heights = katana::entity::heightsOf(main->properties, 2);
    ASSERT_TRUE(heights[0] && heights[1]);
    EXPECT_NEAR(*heights[0], 1.0, 1e-12);
    EXPECT_NEAR(*heights[1], 0.5, 1e-12);

    EXPECT_EQ(property(*main, "Pset_Main/NominalDiameter"), "0.15");
    EXPECT_EQ(property(*main, "Pset_Main/Material"), "DICL");
    EXPECT_EQ(property(*main, "Pset_Main/InService"), "true");
    EXPECT_EQ(metadata(*main, "ifc.class"), "IfcFlowSegment");
    EXPECT_EQ(metadata(*main, "ifc.globalId"), "2YvctVUKr0kugbFTf53O9L");
    EXPECT_EQ(metadata(*main, "ifc.description"), "A water main");
    EXPECT_EQ(metadata(*main, "ifc.tag"), "WM-1");
    EXPECT_EQ(metadata(*main, "ifc.container"), "Site");
    EXPECT_EQ(metadata(*main, "source"), "mm.ifc");
    // With no presentation layer, a layer by where it is and what it is.
    EXPECT_EQ(main->layer, "IFC/Site/FlowSegment");
    EXPECT_TRUE(imported.coordinateSystem.empty());
}

TEST(IfcImport, AMapConversionPutsTheFileWhereItIsAndAnglesAreInTheFilesUnit)
{
    const auto imported = read(kRotated);
    EXPECT_EQ(imported.coordinateSystem, "EPSG:7856");
    const Entity* peg = withName(imported, "Peg");
    ASSERT_NE(peg, nullptr);
    const auto& point = std::get<katana::entity::PointGeometry>(peg->geometry);
    EXPECT_NEAR(point.position.x, 300006.0, 1e-9);
    EXPECT_NEAR(point.position.y, 6200008.0, 1e-9);
    ASSERT_TRUE(katana::entity::heightsOf(peg->properties, 1)[0]);
    EXPECT_NEAR(*katana::entity::heightsOf(peg->properties, 1)[0], 12.0, 1e-9);

    const Entity* kerbReturn = withName(imported, "Kerb return");
    ASSERT_NE(kerbReturn, nullptr);
    const auto* arc = std::get_if<katana::geometry::Arc2>(&kerbReturn->geometry);
    ASSERT_NE(arc, nullptr);
    EXPECT_NEAR(arc->center.x, 300000.0, 1e-9);
    EXPECT_NEAR(arc->center.y, 6200000.0, 1e-9);
    EXPECT_NEAR(arc->radius, 5.0, 1e-12);
    EXPECT_NEAR(arc->startAngle, std::atan2(0.8, 0.6), 1e-12);
    EXPECT_NEAR(arc->sweep, kPi / 2.0, 1e-12);

    const Entity* byPoints = withName(imported, "Trimmed by points");
    ASSERT_NE(byPoints, nullptr);
    const auto* next = std::get_if<katana::geometry::Arc2>(&byPoints->geometry);
    ASSERT_NE(next, nullptr);
    EXPECT_NEAR(next->startAngle, std::atan2(0.8, 0.6) + kPi / 2.0, 1e-12);
    EXPECT_NEAR(next->sweep, kPi / 2.0, 1e-12);
}

TEST(IfcImport, AnAlignmentWithNoPiFormComesInAsItsExactGeometryAndSaysWhy)
{
    ifc::ImportOptions options;
    options.curveTolerance = 0.001;
    const auto imported = read(kStartsOnACurve, options);
    EXPECT_TRUE(imported.alignments.empty());
    EXPECT_EQ(imported.alignmentsAsPolylines, 1u);
    ASSERT_FALSE(imported.warnings.empty());
    EXPECT_NE(imported.warnings[0].find("Ramp A"), std::string::npos) << imported.warnings[0];
    const Entity* ramp = nullptr;
    for (const Entity& entity : imported.entities) {
        if (metadata(entity, "ifc.alignment") == "Ramp A") {
            ramp = &entity;
        }
    }
    ASSERT_NE(ramp, nullptr);
    EXPECT_EQ(ramp->layer, "IFC/Alignments");
    const auto vertices = verticesOf(*ramp);
    ASSERT_GE(vertices.size(), 4u);
    EXPECT_NEAR(vertices.front().x, 0.0, 1e-9);
    EXPECT_NEAR(vertices.front().y, 0.0, 1e-9);
    EXPECT_NEAR(vertices.back().x, 100.0 * std::sin(0.5) + 50.0 * std::cos(0.5), 1e-6);
    EXPECT_NEAR(vertices.back().y, 100.0 * (1.0 - std::cos(0.5)) + 50.0 * std::sin(0.5), 1e-6);
    // Every vertex on the arc is on the circle about (0, 100), and the chords
    // stand no further than the tolerance from it.
    for (const Point2& vertex : vertices) {
        if (vertex.x < 100.0 * std::sin(0.5) - 1e-9) {
            EXPECT_NEAR(std::hypot(vertex.x, vertex.y - 100.0), 100.0, 1e-9);
        }
    }
    for (std::size_t i = 1; i < vertices.size(); ++i) {
        const Point2 middle{(vertices[i - 1].x + vertices[i].x) / 2.0,
                            (vertices[i - 1].y + vertices[i].y) / 2.0};
        if (vertices[i].x < 100.0 * std::sin(0.5) + 1e-9) {
            EXPECT_LE(100.0 - std::hypot(middle.x, middle.y - 100.0), 0.001 + 1e-12);
        }
    }
}

TEST(IfcImport, LocalMovesEverythingAlignmentsIncluded)
{
    const Model model = scenario();
    const std::string text = exportedScenario(model);
    ifc::ImportOptions options;
    options.originShift = katana::geometry::Vec2(333900.0, 6249900.0);
    const auto shifted = read(text, options);
    ASSERT_EQ(shifted.alignments.size(), 1u);
    EXPECT_NEAR(shifted.alignments[0].horizontal.pis[0].point.x, 0.0, 1e-6);
    EXPECT_NEAR(shifted.alignments[0].horizontal.pis[0].point.y, 50.0, 1e-6);
    // The extent is everything's: MC01's first PI is the furthest west.
    EXPECT_NEAR(shifted.bounds.min.x, 0.0, 1e-6);
}

// ---- what is not an IFC file, or is a hostile one -----------------------------------

TEST(IfcImport, RefusesWhatIsNotAnIfcFileNamingTheLine)
{
    const auto code = [](std::string_view text) {
        const auto result = ifc::readIfc(text);
        return result.ok() ? std::string("ok") : result.error().describe();
    };
    EXPECT_NE(code("").find("does not begin ISO-10303-21"), std::string::npos);
    EXPECT_NE(code("0\nSECTION\n").find("does not begin ISO-10303-21"), std::string::npos);
    const std::string header = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\n"
                               "FILE_NAME('','',(''),(''),'','','');\n";
    const std::string ifc4 = header + "FILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n";
    // Truncated.
    EXPECT_NE(code(ifc4 + "#1=IFCCARTESIANPOINT((0.,0.").find("ParseFailure"), std::string::npos);
    EXPECT_NE(code(ifc4 + "#1=IFCLABEL('never closed);\nENDSEC;\nEND-ISO-10303-21;\n")
                  .find("never closed"),
              std::string::npos);
    EXPECT_NE(code(ifc4 + "#1=IFCCARTESIANPOINT((0.,0.));\n#1=IFCCARTESIANPOINT((1.,0.));\n"
                          "ENDSEC;\nEND-ISO-10303-21;\n")
                  .find("defined twice"),
              std::string::npos);
    EXPECT_NE(code(ifc4 + "#99999999999=IFCCARTESIANPOINT((0.,0.));\nENDSEC;\nEND-ISO-10303-21;\n")
                  .find("out of range"),
              std::string::npos);
    EXPECT_NE(code(ifc4 + "#1=IFCCARTESIANPOINT((1.E999,0.));\nENDSEC;\nEND-ISO-10303-21;\n")
                  .find("is not a number"),
              std::string::npos);
    // Lists nested past any schema's need are refused, not recursed into.
    EXPECT_NE(code(ifc4 + "#1=X(" + std::string(10000, '(') + std::string(10000, ')') +
                   ");\nENDSEC;\nEND-ISO-10303-21;\n")
                  .find("nest more than"),
              std::string::npos);
    // A STEP file of another schema is not an IFC file.
    const auto other = ifc::readIfc(header + "FILE_SCHEMA(('AP214'));\nENDSEC;\nDATA;\nENDSEC;\n"
                                             "END-ISO-10303-21;\n");
    ASSERT_FALSE(other.ok());
    EXPECT_EQ(other.error().code, ErrorCode::Unsupported);
    // The error names the line.
    const auto bad = ifc::readIfc(ifc4 + "#1=IFCCARTESIANPOINT((0.,0.));\n#2=?;\n");
    ASSERT_FALSE(bad.ok());
    EXPECT_EQ(bad.error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(bad.error().context, "line 9");

    EXPECT_EQ(ifc::readIfcFile("/no/such/file.ifc").error().code, ErrorCode::NotFound);
}

// A placement relative to itself, a reference to nothing, a representation
// that is not one: read, the bad parts counted, never a hang or a crash.
TEST(IfcImport, AFileThatBreaksTheSchemaIsReadAsFarAsItMakesSense)
{
    const std::string text = R"(ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('','',(''),(''),'','','');
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
#1=IFCCARTESIANPOINT((1.,2.,3.));
#2=IFCAXIS2PLACEMENT3D(#1,$,$);
#3=IFCLOCALPLACEMENT(#3,#2);
#4=IFCSHAPEREPRESENTATION($,'Axis','Curve3D',(#77,#1));
#5=IFCPRODUCTDEFINITIONSHAPE($,$,(#4,#88));
#6=IFCANNOTATION('1YvctVUKr0kugbFTf53O9L',$,'Loop',$,$,#3,#5);
#7=IFCWALL('2YvctVUKr0kugbFTf53O9L',$,'Nowhere',$,$,$,#99,$,$);
#8=IFCPIPESEGMENT('3YvctVUKr0kugbFTf53O9L',$,'Odd',$,$,#1,'text',$,$);
#9=IFCALIGNMENT('0ZvctVUKr0kugbFTf53O9L',$,'Empty',$,$,$,$,$);
#10=IFCRELNESTS('1ZvctVUKr0kugbFTf53O9L',$,$,$,#9,(#9,#10,#123));
#11=IFCMAPCONVERSION($,$,'east',$,$,$,$,$);
#12=IFCSIUNIT(*,.LENGTHUNIT.,.GIGA.,.PARSEC.);
#13=IFCUNITASSIGNMENT((#12,#12));
ENDSEC;
END-ISO-10303-21;
)";
    auto imported = read(text);
    EXPECT_EQ(imported.products, 3u);
    EXPECT_LE(imported.entities.size(), 4u);
    EXPECT_FALSE(imported.warnings.empty());
    // What did come in is fit for the drawing, which checks every entity.
    Model model;
    katana::commands::CommandStack stack(model);
    if (auto command = ifc::importCommand(imported, model)) {
        const auto status = stack.execute(std::move(command));
        EXPECT_TRUE(status.ok()) << status.error().describe();
    }
}

// ---- into the drawing ---------------------------------------------------------------

TEST(IfcImportCommand, TheImportIsOneUndoStepAndASecondImportRenamesItsAlignment)
{
    Model model;
    katana::commands::CommandStack stack(model);
    const Model source = scenario();
    const std::string text = exportedScenario(source);

    auto first = read(text);
    const std::size_t entities = first.entities.size();
    auto command = ifc::importCommand(first, model);
    ASSERT_NE(command, nullptr);
    ASSERT_TRUE(stack.execute(std::move(command)).ok());
    EXPECT_EQ(model.entities.size(), entities);
    EXPECT_TRUE(model.alignments.contains("MC01"));
    EXPECT_TRUE(model.layers.contains("Survey/Kerb"));
    EXPECT_TRUE(model.layers.contains("Survey"));
    EXPECT_EQ(stack.undoCount(), 1u);

    auto second = read(text);
    auto again = ifc::importCommand(second, model);
    ASSERT_NE(again, nullptr);
    ASSERT_TRUE(stack.execute(std::move(again)).ok());
    EXPECT_TRUE(model.alignments.contains("MC01 (2)"));
    ASSERT_FALSE(second.warnings.empty());
    EXPECT_NE(second.warnings.back().find("MC01 (2)"), std::string::npos);
    EXPECT_EQ(model.entities.size(), 2 * entities);

    ASSERT_TRUE(stack.undo().ok());
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_EQ(model.entities.size(), 0u);
    EXPECT_FALSE(model.alignments.contains("MC01"));
    EXPECT_FALSE(model.layers.contains("Survey/Kerb"));

    ifc::IfcImport nothing;
    EXPECT_EQ(ifc::importCommand(nothing, model), nullptr);
}
