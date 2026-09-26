// The Properties panel's tree (src/katana_qt/property_panel): its groups, a
// vertex's place with its attributes beneath it, a long level made a page at
// a time, several entities read as one, the filter, and rows kept open across
// a refresh. Every expectation is worked out by hand from the drawing each
// test types.

#include <gtest/gtest.h>

#include <QAction>
#include <QApplication>
#include <QClipboard>
#include <QCoreApplication>
#include <QElapsedTimer>
#include <QLineEdit>
#include <QTreeView>

#include <string>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "property_panel.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::qt::kPropertyPage;
using katana::qt::PropertyTreePanel;

namespace {

struct Panel {
    Document document;
    CommandInterpreter interpreter{document};
    PropertyTreePanel panel;

    void run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        ASSERT_TRUE(reply.ok()) << line << ": " << reply.error().describe();
    }

    void show() { panel.showSelection(document); }

    [[nodiscard]] QTreeView& tree() const { return *panel.tree(); }

    // The row whose first column says `label`, beneath `parent`.
    [[nodiscard]] QModelIndex row(const QString& label, const QModelIndex& parent = {}) const
    {
        const QAbstractItemModel& model = *tree().model();
        for (int at = 0; at < model.rowCount(parent); ++at) {
            const QModelIndex index = model.index(at, 0, parent);
            if (index.data().toString() == label) {
                return index;
            }
        }
        return {};
    }

    [[nodiscard]] QString value(const QModelIndex& index) const
    {
        return index.siblingAtColumn(1).data().toString();
    }

    // A level's rows as "label | value", joined by " ; ".
    [[nodiscard]] QString rows(const QModelIndex& parent = {}) const
    {
        const QAbstractItemModel& model = *tree().model();
        QStringList out;
        for (int at = 0; at < model.rowCount(parent); ++at) {
            const QModelIndex index = model.index(at, 0, parent);
            out << index.data().toString() + " | " + value(index);
        }
        return out.join(" ; ");
    }

    // Opens a row as a click on its expander does.
    void open(const QModelIndex& index)
    {
        QAbstractItemModel& model = *tree().model();
        if (model.canFetchMore(index) && model.rowCount(index) == 0) {
            model.fetchMore(index);
        }
        tree().expand(index);
    }
};

// A string of five vertices as an archive import leaves it: attributes on
// vertices 1 and 2, a group, a code, and heights on all but the fourth.
void typeTheString(Panel& p)
{
    p.run("PLINE 0,0 10,0 20,5 30,5 40,0");
    p.run("SELECT 1");
    p.run("PROP SET vertex/1/QL B");
    p.run("PROP SET vertex/1/Depth 0.6");
    p.run("PROP SET vertex/2/QL A");
    p.run("PROP SET Asset/Dimensions/Size 300");
    p.run("PROP SET Asset/Owner Council");
    p.run("PROP SET code KERB");
    p.run("PROP SET elevations \"1 2 3 null 5\"");
}

// Runs the event loop until `done` or two seconds: the filter waits for a
// pause in the typing.
template <typename Done> bool waitFor(Done done)
{
    QElapsedTimer clock;
    clock.start();
    while (!done() && clock.elapsed() < 2000) {
        QCoreApplication::processEvents(QEventLoop::AllEvents, 20);
    }
    return done();
}

} // namespace

TEST(PropertyPanel, NothingSelectedSaysSoAndHowManyEntitiesThereAre)
{
    Panel p;
    p.run("LINE 0,0 1,1");
    p.run("LINE 0,1 1,2");
    p.run("SELECT NONE");
    p.show();
    EXPECT_EQ(p.rows(), "Selection | none ; Entities | 2");
}

TEST(PropertyPanel, OneStringIsItsGroupsWithTheVerticesLeftClosedUntilOpened)
{
    Panel p;
    typeTheString(p);
    p.show();
    // No Source group: the string was typed, not imported.
    EXPECT_EQ(p.rows(), "General |  ; Geometry |  ; Vertices | 5 vertices ; Attributes | 7 values");
    for (const char* open : {"General", "Geometry", "Attributes"}) {
        EXPECT_TRUE(p.tree().isExpanded(p.row(open))) << open;
    }
    const QModelIndex vertices = p.row("Vertices");
    EXPECT_FALSE(p.tree().isExpanded(vertices));
    // An expander, and nothing made beneath it until it is opened.
    EXPECT_TRUE(p.tree().model()->hasChildren(vertices));
    EXPECT_EQ(p.tree().model()->rowCount(vertices), 0);

    EXPECT_EQ(p.rows(p.row("General")), "Id | 1 ; Type | Polyline ; Layer | 0 ; Colour | ByLayer");
    // The count is the Vertices group's; 10 + sqrt(125) + 10 + sqrt(125).
    EXPECT_EQ(p.rows(p.row("Geometry")), "Closed | no ; Length | 42.3607");
    // The vertices' attributes are under Vertices, and the heights too.
    EXPECT_EQ(p.rows(p.row("Attributes")),
              "Asset | 2 values ; code | KERB ; elevations | see Vertices");
}

TEST(PropertyPanel, AVertexIsItsPlaceAndHeightWithItsAttributesBeneathIt)
{
    Panel p;
    typeTheString(p);
    p.show();
    const QModelIndex vertices = p.row("Vertices");
    p.open(vertices);
    // The fourth has no height ("null"): none shown, not a zero.
    EXPECT_EQ(p.rows(vertices), "Vertex 1 | 0.0000, 0.0000, 1.0000 ; "
                                "Vertex 2 | 10.0000, 0.0000, 2.0000 ; "
                                "Vertex 3 | 20.0000, 5.0000, 3.0000 ; "
                                "Vertex 4 | 30.0000, 5.0000 ; "
                                "Vertex 5 | 40.0000, 0.0000, 5.0000");
    const QModelIndex first = p.row("Vertex 1", vertices);
    EXPECT_TRUE(p.tree().model()->hasChildren(first));
    EXPECT_FALSE(p.tree().model()->hasChildren(p.row("Vertex 3", vertices)));
    p.open(first);
    EXPECT_EQ(p.rows(first), "Depth | 0.6 ; QL | B");

    const QModelIndex asset = p.row("Asset", p.row("Attributes"));
    p.open(asset);
    EXPECT_EQ(p.rows(asset), "Dimensions | 1 value ; Owner | Council");
}

TEST(PropertyPanel, ALongStringIsMadeAPageAtATimeWithAShowMoreRow)
{
    Panel p;
    // 450 vertices along the x axis, one attribute on each.
    std::string line = "PLINE";
    for (int i = 0; i < 450; ++i) {
        line += " " + std::to_string(i) + ",0";
    }
    p.run(line);
    p.run("SELECT 1");
    p.run("PROP SET vertex/450/QL D");
    p.show();
    const QModelIndex vertices = p.row("Vertices");
    EXPECT_EQ(p.value(vertices), "450 vertices");
    p.open(vertices);
    // One page and the Show more row: 450 = 200 + 200 + 50.
    ASSERT_EQ(kPropertyPage, 200u);
    QAbstractItemModel& model = *p.tree().model();
    ASSERT_EQ(model.rowCount(vertices), 201);
    const QModelIndex more = model.index(200, 0, vertices);
    EXPECT_EQ(more.data().toString(), "Show 200 more");
    EXPECT_EQ(p.value(more), "200 of 450 shown");

    // A click on it makes the next page where it is, once the click is over.
    emit p.tree().clicked(more);
    EXPECT_EQ(model.rowCount(vertices), 201);
    QCoreApplication::processEvents();
    ASSERT_EQ(model.rowCount(vertices), 401);
    const QModelIndex again = model.index(400, 0, vertices);
    EXPECT_EQ(again.data().toString(), "Show 50 more");
    EXPECT_EQ(p.value(again), "400 of 450 shown");
    emit p.tree().activated(again); // Enter on it, as well as a click
    QCoreApplication::processEvents();
    ASSERT_EQ(model.rowCount(vertices), 450);
    const QModelIndex last = model.index(449, 0, vertices);
    EXPECT_EQ(last.data().toString(), "Vertex 450");
    EXPECT_EQ(p.value(last), "449.0000, 0.0000");
    EXPECT_TRUE(model.hasChildren(last));
    EXPECT_FALSE(model.canFetchMore(vertices));
}

TEST(PropertyPanel, VertexAttributesOfAnEntityWithNoVertexListAreInNumberOrder)
{
    Panel p;
    p.run("POINT 0,0");
    p.run("SELECT 1");
    p.run("PROP SET vertex/10/QL C");
    p.run("PROP SET vertex/2/QL A");
    p.show();
    const QModelIndex vertex = p.row("vertex", p.row("Attributes"));
    ASSERT_TRUE(vertex.isValid()) << p.rows(p.row("Attributes")).toStdString();
    p.open(vertex);
    EXPECT_EQ(p.rows(vertex), "Vertex 2 | 1 value ; Vertex 10 | 1 value");
}

TEST(PropertyPanel, AttributesOfAVertexTheStringNoLongerHasStayInSight)
{
    Panel p;
    p.run("PLINE 0,0 10,0 20,0");
    p.run("SELECT 1");
    p.run("PROP SET vertex/2/QL A");
    p.run("PROP SET vertex/7/QL Z"); // three vertices: there is no seventh
    p.show();
    const QModelIndex vertex = p.row("vertex", p.row("Attributes"));
    ASSERT_TRUE(vertex.isValid()) << p.rows(p.row("Attributes")).toStdString();
    p.open(vertex);
    EXPECT_EQ(p.rows(vertex), "Vertex 2 | 1 value ; Vertex 7 | 1 value");
}

TEST(PropertyPanel, SeveralEntitiesShowWhatTheyShareAndWhereTheyVary)
{
    Panel p;
    p.run("POINT 0,0");
    p.run("POINT 5,0");
    p.run("LINE 0,0 5,5");
    p.run("SELECT 1 2");
    p.run("PROP SET code TREE");
    p.run("SELECT 1");
    p.run("PROP SET height 1");
    p.run("SELECT 2");
    p.run("PROP SET height 2");
    p.run("SELECT 1 2 3");
    p.show();
    EXPECT_EQ(p.rows(p.row("General")), "Selection | 3 entities ; Line | 1 ; Point | 2");
    // The line holds neither, so both vary.
    EXPECT_EQ(p.rows(p.row("Attributes")), "code | <varies> ; height | <varies>");
    p.run("SELECT 1 2");
    p.show();
    EXPECT_EQ(p.rows(p.row("Attributes")), "code | TREE ; height | <varies>");
}

TEST(PropertyPanel, TheFilterListsEveryValueHoldingTheWordsHoweverDeep)
{
    Panel p;
    typeTheString(p);
    p.show();
    p.panel.filter()->setText("ql");
    // Seven values searched: the string's seven properties, and no source
    // record, since it was typed.
    ASSERT_TRUE(waitFor([&] { return p.row("Matches").isValid(); }));
    EXPECT_EQ(p.rows(), "Matches | 2 of 7 values hold \"ql\" ; "
                        "vertex/1/QL | B ; vertex/2/QL | A");
    // A value matches as well as a name: "Council".
    p.panel.filter()->setText("COUN");
    ASSERT_TRUE(waitFor([&] { return p.rows().contains("Asset/Owner"); }));
    EXPECT_EQ(p.rows(), "Matches | 1 of 7 values hold \"COUN\" ; Asset/Owner | Council");
    // Cleared: the tree again, at once.
    p.panel.filter()->clear();
    EXPECT_TRUE(p.row("Vertices").isValid());
    EXPECT_TRUE(p.tree().isExpanded(p.row("Attributes")));
}

TEST(PropertyPanel, OpenRowsStayOpenAcrossARefreshAndClosedGroupsStayClosed)
{
    Panel p;
    typeTheString(p);
    p.show();
    p.open(p.row("Vertices"));
    p.open(p.row("Vertex 1", p.row("Vertices")));
    p.tree().collapse(p.row("General"));
    // An edit elsewhere refreshes the panel, as the window does after every
    // change; the same string is shown.
    p.run("LAYER NEW other");
    p.show();
    ASSERT_TRUE(p.tree().isExpanded(p.row("Vertices")));
    EXPECT_TRUE(p.tree().isExpanded(p.row("Vertex 1", p.row("Vertices"))));
    EXPECT_FALSE(p.tree().isExpanded(p.row("General")));

    // Another entity: its groups as they were left - General closed - and
    // nothing deeper.
    p.run("POINT 1,1");
    p.run("SELECT 2");
    p.run("PROP SET code X");
    p.show();
    EXPECT_FALSE(p.tree().isExpanded(p.row("General")));
    EXPECT_TRUE(p.tree().isExpanded(p.row("Attributes")));
}

TEST(PropertyPanel, CopyPutsTheSelectedRowsOnTheClipboardNameTabValue)
{
    Panel p;
    typeTheString(p);
    p.show();
    const QModelIndex code = p.row("code", p.row("Attributes"));
    ASSERT_TRUE(code.isValid());
    p.tree().selectionModel()->select(code, QItemSelectionModel::ClearAndSelect |
                                                QItemSelectionModel::Rows);
    auto* copy = p.tree().findChild<QAction*>("propertyCopy");
    ASSERT_NE(copy, nullptr);
    copy->trigger();
    EXPECT_EQ(QApplication::clipboard()->text(), "code\tKERB");
}

TEST(PropertyPanel, OpeningALongLevelAsAPersonDoesReadsItsFirstPageOnly)
{
    // The view's own expand, which fetches from inside its layout: the path
    // that once wrote past the end of the view's rows (property_panel.cpp,
    // canFetchMore). Shown, so the view lays out as on screen.
    Panel p;
    std::string line = "PLINE";
    for (int i = 0; i < 450; ++i) {
        line += " " + std::to_string(i) + ",0";
    }
    p.run(line);
    p.run("SELECT 1");
    p.panel.resize(320, 600);
    p.panel.show();
    p.show();
    QCoreApplication::processEvents();
    const QModelIndex vertices = p.row("Vertices");
    p.tree().expand(vertices);
    QCoreApplication::processEvents();
    EXPECT_EQ(p.tree().model()->rowCount(vertices), 201);
    // Collapsed and opened again: nothing is read twice, no page is added.
    p.tree().collapse(vertices);
    p.tree().expand(vertices);
    QCoreApplication::processEvents();
    EXPECT_EQ(p.tree().model()->rowCount(vertices), 201);
    (void)p.panel.grab();
}
