// katana_12da_probe <file.12da> [--rewrite <out.12da>]
//
// Says what a 12d Archive holds and what Katana would make of it, without
// importing anything: the encoding, every kind of element with how many were
// read and how many would be imported, every block the reader has no member
// for, and every warning. It exists because "did the importer take all of
// it?" is a question about a specific FILE, and the honest answer is a list.
//
// --rewrite writes the archive back out and reads that again, reporting
// whether the second reading equals the first - the writer's promise, checked
// against a real file rather than a fixture.
//
// Plain .12da only. The zipped .12daz needs GDAL, which this module
// deliberately does not link; unzip it first, or use `katana_cli IMPORT`.

#include <chrono>
#include <cstdio>
#include <fstream>
#include <map>
#include <sstream>
#include <string>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/archive12d/text_encoding.hpp"
#include "katana/archive12d/writer.hpp"

namespace a12 = katana::archive12d;

namespace {

double secondsSince(std::chrono::steady_clock::time_point start)
{
    return std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
}

} // namespace

int main(int argc, char* argv[])
{
    if (argc < 2) {
        std::fprintf(stderr, "usage: katana_12da_probe <file.12da> [--rewrite <out.12da>]\n");
        return 2;
    }
    const std::string path = argv[1];
    std::string rewritePath;
    if (argc >= 4 && std::string(argv[2]) == "--rewrite") {
        rewritePath = argv[3];
    }

    std::ifstream file(path, std::ios::binary);
    if (!file) {
        std::fprintf(stderr, "cannot open %s\n", path.c_str());
        return 1;
    }
    std::ostringstream buffer;
    buffer << file.rdbuf();
    const std::string bytes = buffer.str();

    auto start = std::chrono::steady_clock::now();
    const auto decoded = a12::decodeText(bytes);
    if (!decoded) {
        std::fprintf(stderr, "%s\n", decoded.error().describe().c_str());
        return 1;
    }
    std::printf("%s\n  %zu bytes, %s%s, decoded in %.2f s\n", path.c_str(), bytes.size(),
                a12::toString(decoded->encoding), decoded->guessed ? " (inferred)" : "",
                secondsSince(start));

    start = std::chrono::steady_clock::now();
    const auto archive = a12::readArchive(decoded->text);
    if (!archive) {
        std::fprintf(stderr, "%s\n", archive.error().describe().c_str());
        return 1;
    }
    std::printf("  read in %.2f s: %zu elements in %zu models\n", secondsSince(start),
                archive->elements.size(), archive->modelNames.size());
    for (const a12::Field& setting : archive->headerSettings) {
        if (setting.key == "archive_version") {
            std::printf("  archive_version %s\n", setting.value.c_str());
        }
    }

    start = std::chrono::steady_clock::now();
    const auto domain = a12::toDomain(*archive);
    if (!domain) {
        std::fprintf(stderr, "%s\n", domain.error().describe().c_str());
        return 1;
    }
    std::printf("  mapped in %.2f s: %zu entities on %zu layers, %zu alignments, %zu surfaces, "
                "%zu point clouds\n",
                secondsSince(start), domain->entities.size(), domain->layersNeeded.size(),
                domain->alignments.size(), domain->surfaces.size(), domain->clouds.size());
    std::size_t symbolStyles = 0;
    for (const auto& style : domain->stylesNeeded) {
        symbolStyles += style.symbol.empty() ? 0 : 1;
    }
    std::printf("  %zu styles, %zu of them symbols\n", domain->stylesNeeded.size(), symbolStyles);
    std::printf("\n  %-28s %10s %10s\n", "element", "read", "imported");
    for (const a12::ElementTally& tally : domain->tally) {
        std::printf("  %-28s %10zu %10zu\n", tally.keyword.c_str(), tally.read, tally.imported);
    }
    for (const a12::ImportedSurface& surface : domain->surfaces) {
        std::printf("  surface \"%s\": %zu of %zu triangles\n", surface.name.c_str(),
                    surface.surface.triangleCount(), surface.trianglesInFile);
    }
    if (!domain->warnings.empty()) {
        std::printf("\n  warnings:\n");
        for (const std::string& warning : domain->warnings) {
            std::printf("    %s\n", warning.c_str());
        }
    }

    if (!rewritePath.empty()) {
        start = std::chrono::steady_clock::now();
        const std::string text = a12::writeArchive(*archive);
        std::ofstream out(rewritePath, std::ios::binary);
        out << text;
        out.close();
        if (!out) {
            std::fprintf(stderr, "cannot write %s\n", rewritePath.c_str());
            return 1;
        }
        const auto second = a12::readArchive(text);
        if (!second) {
            std::fprintf(stderr, "the rewritten file does not read: %s\n",
                         second.error().describe().c_str());
            return 1;
        }
        // The first writing may normalise (superseded strings become super
        // strings); a second must change nothing.
        const auto third = a12::readArchive(a12::writeArchive(*second));
        const bool stable = third.ok() && third->elements == second->elements;
        std::printf("\n  rewritten to %s (%zu bytes) in %.2f s; %zu elements read back; a second "
                    "rewrite is %s\n",
                    rewritePath.c_str(), text.size(), secondsSince(start), second->elements.size(),
                    stable ? "identical" : "DIFFERENT");
        if (!stable || second->elements.size() != archive->elements.size()) {
            return 1;
        }
    }
    return 0;
}
