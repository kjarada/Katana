// Where File > Open and Save As start (src/katana_qt/project_folder.hpp,
// docs/desktop.md, "The File menu"): the folder of the open project, else the
// last one used, else Documents - and never the working folder, which is the
// program's own bin\ for a zip copy and its install folder for the installed
// one, so that a first project was proposed there and went with the program
// when its folder was deleted.

#include <gtest/gtest.h>

#include <QDir>
#include <QTemporaryDir>

#include "project_folder.hpp"

using katana::qt::projectDialogFolder;

TEST(ProjectFolder, TheOpenProjectsFolderComesFirstThenTheLastOneThenDocuments)
{
    QTemporaryDir parent;
    QTemporaryDir last;
    QTemporaryDir documents;
    ASSERT_TRUE(parent.isValid());
    ASSERT_TRUE(last.isValid());
    ASSERT_TRUE(documents.isValid());

    EXPECT_EQ(projectDialogFolder(parent.path(), last.path(), documents.path()), parent.path());
    EXPECT_EQ(projectDialogFolder({}, last.path(), documents.path()), last.path());
    EXPECT_EQ(projectDialogFolder({}, {}, documents.path()), documents.path());
}

TEST(ProjectFolder, AFolderThatHasGoneIsSkipped)
{
    QTemporaryDir real;
    ASSERT_TRUE(real.isValid());
    const QString gone = QDir(real.path()).filePath("deleted since");
    ASSERT_FALSE(QDir(gone).exists());
    EXPECT_EQ(projectDialogFolder(gone, gone, real.path()), real.path());
    EXPECT_EQ(projectDialogFolder(gone, real.path(), gone), real.path());
}

TEST(ProjectFolder, WithNoFolderToOfferItIsTheHomeFolderAndNeverTheWorkingOne)
{
    QTemporaryDir working;
    ASSERT_TRUE(working.isValid());
    const QString before = QDir::currentPath();
    ASSERT_TRUE(QDir::setCurrent(working.path()));
    const QString gone = QDir(working.path()).filePath("nothing here");
    const QString none = projectDialogFolder({}, {}, {});
    const QString allGone = projectDialogFolder(gone, gone, gone);
    ASSERT_TRUE(QDir::setCurrent(before));

    EXPECT_EQ(none, QDir::homePath());
    EXPECT_EQ(allGone, QDir::homePath());
    EXPECT_NE(QDir::cleanPath(none), QDir::cleanPath(working.path()))
        << "the dialog would open in the working folder";
}
