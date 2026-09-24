// The Qt benchmarks' own main: Google Benchmark's benchmark_main cannot make
// the QApplication a widget needs. The offscreen platform, as the headless
// tests use, so a run shows nothing and needs no display.

#include <benchmark/benchmark.h>

#include <QApplication>
#include <QByteArray>

#include <filesystem>
#include <string>

#include <QCoreApplication>

int main(int argc, char** argv)
{
    if (qEnvironmentVariableIsEmpty("QT_QPA_PLATFORM")) {
        qputenv("QT_QPA_PLATFORM", QByteArray("offscreen"));
    }
    QApplication application(argc, argv);
    benchmark::Initialize(&argc, argv);
    if (benchmark::ReportUnrecognizedArguments(argc, argv)) {
        return 1;
    }
    benchmark::RunSpecifiedBenchmarks();
    benchmark::Shutdown();
    // The fixture's project, saved once so its spatial index is an opened
    // project's (bench_plan_paint.cpp).
    std::error_code ignored;
    std::filesystem::remove_all(std::filesystem::temp_directory_path() /
                                    ("katana_plan_bench_" +
                                     std::to_string(QCoreApplication::applicationPid())),
                                ignored);
    return 0;
}
