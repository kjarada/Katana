// The GPU tests need a QApplication - QRhi's shaders and QRhiWidget live in
// QtGui and QtWidgets - so they bring their own main rather than gtest_main.

#include <gtest/gtest.h>

#include <QApplication>

int main(int argc, char** argv)
{
    // Offscreen, as every Katana Qt test runs: it is the platform the
    // fallback rules are written for, and raw QRhi on Direct3D 11 renders
    // into textures under it all the same. KATANA_GPU_TEST_PLATFORM=windows
    // runs them on the desktop platform instead, where the cases that need a
    // real QRhiWidget (test_gpu_scene_view.cpp) stop skipping; ctest never
    // sets it, because a headless runner has no desktop.
    const QByteArray platform = qEnvironmentVariableIsSet("KATANA_GPU_TEST_PLATFORM")
                                    ? qgetenv("KATANA_GPU_TEST_PLATFORM")
                                    : QByteArray("offscreen");
    qputenv("QT_QPA_PLATFORM", platform);
    QApplication application(argc, argv);
    testing::InitGoogleTest(&argc, argv);
    return RUN_ALL_TESTS();
}
