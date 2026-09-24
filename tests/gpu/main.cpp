// The GPU tests need a QApplication - QRhi's shaders and QRhiWidget live in
// QtGui and QtWidgets - so they bring their own main rather than gtest_main.

#include <gtest/gtest.h>

#include <QApplication>

int main(int argc, char** argv)
{
    // Offscreen whatever the environment says, as every Katana Qt test runs:
    // it is the platform the fallback rules are written for, and raw QRhi on
    // Direct3D 11 renders into textures under it all the same.
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication application(argc, argv);
    testing::InitGoogleTest(&argc, argv);
    return RUN_ALL_TESTS();
}
