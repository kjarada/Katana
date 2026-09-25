// The view widgets' tests need a QApplication, so they bring their own main
// rather than gtest_main.

#include <gtest/gtest.h>

#include <QApplication>

int main(int argc, char** argv)
{
    // Offscreen whatever the environment says: no window appears on a
    // developer's screen, and focus and activation are decided by Qt alone
    // rather than by a window manager that may refuse to raise a window.
    // KATANA_WIDGET_TEST_PLATFORM names another, for the cases that need a
    // GPU view shown (test_render_view_gpu.cpp): ctest sets xcb for them on
    // Linux, under Xvfb.
    const QByteArray platform = qEnvironmentVariableIsSet("KATANA_WIDGET_TEST_PLATFORM")
                                    ? qgetenv("KATANA_WIDGET_TEST_PLATFORM")
                                    : QByteArray("offscreen");
    qputenv("QT_QPA_PLATFORM", platform);
    QApplication application(argc, argv);
    testing::InitGoogleTest(&argc, argv);
    return RUN_ALL_TESTS();
}
