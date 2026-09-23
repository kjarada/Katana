// The view widgets' tests need a QApplication, so they bring their own main
// rather than gtest_main.

#include <gtest/gtest.h>

#include <QApplication>

int main(int argc, char** argv)
{
    // Offscreen whatever the environment says: no window appears on a
    // developer's screen, and focus and activation are decided by Qt alone
    // rather than by a window manager that may refuse to raise a window.
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication application(argc, argv);
    testing::InitGoogleTest(&argc, argv);
    return RUN_ALL_TESTS();
}
