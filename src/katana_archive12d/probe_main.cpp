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
// --roundtrip goes further: it takes what the DOMAIN made of the archive,
// puts it in a model as the application would, writes that, and reads it
// back - the trip a user's drawing makes. The first pass normalises (an arc
// is written as a two-vertex string, a drainage string as its line plus its
// pits), so what it reports is whether the SECOND and THIRD passes agree,
// which is the property that must hold. tests/interop/test_round_trip.cpp
// asserts the same thing on the fixtures; this is for the large archives,
// which are real project data and are not in the repository.
//
// Plain .12da only. The zipped .12daz needs GDAL, which this module
// deliberately does not link; unzip it first, or use `katana_cli IMPORT`.

#include <chrono>
#include <cstdio>
#include <fstream>
#include <map>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

#include "katana/entity/model.hpp"

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/core/text_encoding.hpp"
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
        std::fprintf(stderr,
                     "usage: katana_12da_probe <file.12da> [--rewrite <out.12da>] "
                     "[--roundtrip]\n");
        return 2;
    }
    const std::string path = argv[1];
    std::string rewritePath;
    bool roundTrip = false;
    for (int i = 2; i < argc; ++i) {
        const std::string argument = argv[i];
        if (argument == "--rewrite" && i + 1 < argc) {
            rewritePath = argv[++i];
        } else if (argument == "--roundtrip") {
            roundTrip = true;
        }
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
    const auto decoded = katana::core::decodeText(bytes);
    if (!decoded) {
        std::fprintf(stderr, "%s\n", decoded.error().describe().c_str());
        return 1;
    }
    std::printf("%s\n  %zu bytes, %s%s, decoded in %.2f s\n", path.c_str(), bytes.size(),
                katana::core::toString(decoded->encoding), decoded->guessed ? " (inferred)" : "",
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
    if (!domain->meshes.empty()) {
        std::size_t faces = 0;
        std::size_t coloured = 0;
        for (const a12::ImportedMesh& mesh : domain->meshes) {
            faces += mesh.mesh.triangleCount();
            coloured += mesh.faceColourNames.empty() ? 0 : 1;
        }
        std::printf("  %zu meshes, %zu triangles, %zu with per-face colours\n",
                    domain->meshes.size(), faces, coloured);
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

    if (roundTrip) {
        // Model -> archive -> text -> archive -> domain, twice: the first
        // pass puts the drawing into the form Katana writes, and every pass
        // after it must change nothing.
        const auto pass = [](const a12::DomainImport& from) -> std::optional<a12::DomainImport> {
            katana::entity::Model model;
            for (const auto& layer : from.layersNeeded) {
                if (!model.layers.add(layer)) {
                    return std::nullopt;
                }
            }
            for (const auto& style : from.stylesNeeded) {
                if (!model.styles.add(style)) {
                    return std::nullopt;
                }
            }
            for (const auto& entity : from.entities) {
                if (!model.entities.add(entity)) {
                    return std::nullopt;
                }
            }
            for (const auto& alignment : from.alignments) {
                if (!model.alignments.add(alignment)) {
                    return std::nullopt;
                }
            }
            std::vector<a12::ExportSurface> surfaces;
            for (const auto& surface : from.surfaces) {
                surfaces.push_back(a12::ExportSurface{surface.name, &surface.surface});
            }
            std::vector<a12::ExportMesh> meshes;
            for (const auto& mesh : from.meshes) {
                meshes.push_back(a12::ExportMesh{mesh.name, mesh.layer, mesh.colourName,
                                                 &mesh.mesh, mesh.faceColourNames});
            }
            auto written = a12::fromDomain(model, surfaces, {}, meshes);
            if (!written) {
                return std::nullopt;
            }
            auto read = a12::readArchive(a12::writeArchive(written->archive));
            if (!read) {
                return std::nullopt;
            }
            auto back = a12::toDomain(*read);
            if (!back) {
                return std::nullopt;
            }
            return std::move(*back);
        };

        start = std::chrono::steady_clock::now();
        const auto second = pass(*domain);
        if (!second) {
            std::fprintf(stderr, "the round trip failed on the first pass\n");
            return 1;
        }
        const auto third = pass(*second);
        if (!third) {
            std::fprintf(stderr, "the round trip failed on the second pass\n");
            return 1;
        }
        const auto describe = [](const a12::DomainImport& d) {
            return std::to_string(d.entities.size()) + " entities, " +
                   std::to_string(d.layersNeeded.size()) + " layers, " +
                   std::to_string(d.stylesNeeded.size()) + " styles, " +
                   std::to_string(d.surfaces.size()) + " surfaces, " +
                   std::to_string(d.meshes.size()) + " meshes, " +
                   std::to_string(d.alignments.size()) + " alignments";
        };
        std::printf("\n  round trip in %.2f s\n    read:   %s\n    pass 1: %s\n    pass 2: %s\n",
                    secondsSince(start), describe(*domain).c_str(), describe(*second).c_str(),
                    describe(*third).c_str());
        // Anything the first pass REFUSED on the way back in: a surface that
        // will not read is the loss this tool exists to show, and it does not
        // change the counts below because it is gone from both passes.
        for (const std::string& warning : second->warnings) {
            if (warning.find("skipped") != std::string::npos) {
                std::printf("    refused on re-import: %s\n", warning.c_str());
            }
        }
        const bool stable = second->entities.size() == third->entities.size() &&
                            second->layersNeeded.size() == third->layersNeeded.size() &&
                            second->stylesNeeded.size() == third->stylesNeeded.size() &&
                            second->surfaces.size() == third->surfaces.size() &&
                            second->meshes.size() == third->meshes.size() &&
                            second->tally == third->tally;
        std::printf("    the form Katana writes is %s\n", stable ? "STABLE" : "NOT STABLE");
        if (!stable) {
            return 1;
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
