#include <gtest/gtest.h>

#include <QComboBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>

#include "annotation/leader_manager.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "widget_harness.hpp"

namespace {

struct Drawing {
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    QStringList lines;

    katana::qt::LeaderManagerContext context()
    {
        katana::qt::LeaderManagerContext context;
        context.run = [this](const QString& line) {
            lines.push_back(line);
            const auto reply = interpreter.run(line.toStdString());
            return reply ? katana::qt::VerbOutcome{true, QString::fromStdString(*reply), {}}
                         : katana::qt::VerbOutcome{false, {}, QString::fromStdString(reply.error().describe())};
        };
        return context;
    }
};

} // namespace

TEST(LeaderManagerPositions, TheNotePositionAndBoxUseOneSharedCommandWithoutRounding)
{
    Drawing drawing;
    ASSERT_TRUE(drawing.interpreter.run("LEADER 0,0 5,5 10,5 text=Note").ok());
    katana::qt::LeaderManagerDialog dialog(drawing.document, drawing.context());
    ASSERT_TRUE(dialog.showLeader(1));
    auto* point = dialog.findChild<QComboBox*>("leaderPositionPoint");
    auto* east = dialog.findChild<QLineEdit*>("leaderPositionX");
    auto* north = dialog.findChild<QLineEdit*>("leaderPositionY");
    auto* frame = dialog.findChild<QComboBox*>("leaderCallout");
    auto* angle = dialog.findChild<QComboBox*>("leaderLandingAngle");
    ASSERT_NE(point, nullptr);
    ASSERT_NE(east, nullptr);
    ASSERT_NE(north, nullptr);
    ASSERT_NE(frame, nullptr);
    ASSERT_NE(angle, nullptr);
    EXPECT_EQ(point->currentIndex(), 2);
    EXPECT_EQ(east->text(), "10");
    EXPECT_EQ(north->text(), "5");
    east->setText("306005.123456789");
    north->setText("6270008.987654321");
    frame->setCurrentIndex(frame->findData(static_cast<int>(katana::entity::CalloutShape::Box)));
    angle->setCurrentText("180");
    const auto before = drawing.document.history().undoCount();
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    ASSERT_EQ(drawing.lines.size(), 1);
    EXPECT_TRUE(drawing.lines[0].contains("vertex=2"));
    EXPECT_TRUE(drawing.lines[0].contains("callout=box"));
    const auto& leader = std::get<katana::entity::LeaderGeometry>(
        drawing.document.model().entities.find(1)->geometry);
    EXPECT_EQ(leader.vertices.back(), katana::geometry::Point2(306005.123456789, 6270008.987654321));
    EXPECT_EQ(leader.vertices.front(), katana::geometry::Point2(0, 0));
    EXPECT_EQ(leader.callout, katana::entity::CalloutShape::Box);
    EXPECT_EQ(drawing.document.history().undoCount(), before + 1);
    ASSERT_TRUE(drawing.document.undo().ok());
    katana::qt::test::processEvents();
    EXPECT_EQ(east->text(), "10");
    EXPECT_EQ(north->text(), "5");
}

TEST(LeaderManagerPositions, ChoosingAPointIsNotAnEditAndInvalidInputRunsNoLine)
{
    Drawing drawing;
    ASSERT_TRUE(drawing.interpreter.run("LEADER 0,0 5,5 10,5 text=Note").ok());
    katana::qt::LeaderManagerDialog dialog(drawing.document, drawing.context());
    ASSERT_TRUE(dialog.showLeader(1));
    auto* point = dialog.findChild<QComboBox*>("leaderPositionPoint");
    auto* east = dialog.findChild<QLineEdit*>("leaderPositionX");
    auto* north = dialog.findChild<QLineEdit*>("leaderPositionY");
    ASSERT_NE(point, nullptr);
    ASSERT_NE(east, nullptr);
    ASSERT_NE(north, nullptr);
    point->setCurrentIndex(1);
    EXPECT_EQ(east->text(), "5");
    ASSERT_TRUE(dialog.apply());
    EXPECT_TRUE(drawing.lines.empty());
    east->setText("nan");
    EXPECT_FALSE(dialog.apply());
    EXPECT_FALSE(dialog.problem().isEmpty());
    EXPECT_TRUE(drawing.lines.empty());
    east->setText("7.125");
    north->setText("9.5");
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(std::get<katana::entity::LeaderGeometry>(
                  drawing.document.model().entities.find(1)->geometry).vertices[1],
              katana::geometry::Point2(7.125, 9.5));
}

TEST(LeaderManagerPositions, AnAttachedTipIsShownButCannotBeAccidentallyDetachedByCoordinates)
{
    Drawing drawing;
    ASSERT_TRUE(drawing.interpreter.run("POINT 0,0").ok());
    ASSERT_TRUE(drawing.interpreter.run("LEADER #1 10,5 text=Note").ok());
    katana::qt::LeaderManagerDialog dialog(drawing.document, drawing.context());
    ASSERT_TRUE(dialog.showLeader(2));
    auto* point = dialog.findChild<QComboBox*>("leaderPositionPoint");
    auto* east = dialog.findChild<QLineEdit*>("leaderPositionX");
    auto* north = dialog.findChild<QLineEdit*>("leaderPositionY");
    ASSERT_NE(point, nullptr);
    ASSERT_NE(east, nullptr);
    ASSERT_NE(north, nullptr);
    point->setCurrentIndex(0);
    EXPECT_FALSE(east->isEnabled());
    EXPECT_FALSE(north->isEnabled());
    point->setCurrentIndex(1);
    EXPECT_TRUE(east->isEnabled());
    EXPECT_TRUE(north->isEnabled());
}
