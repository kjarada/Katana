#pragma once

// The drawing the vector verbs' tests (GIS BUFFER ... GIS SQL) act on: the
// executor's own context, as the window and the session hand it over, with
// helpers to draw what a test needs and to read back what a verb drew.

#include <filesystem>
#include <string>
#include <system_error>
#include <utility>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace katana::geo_test {

class VectorFixture : public ::testing::Test {
  protected:
    VectorFixture()
        : scratch_(std::filesystem::temp_directory_path() /
                   ("katana-geo-vector-" +
                    std::string(::testing::UnitTest::GetInstance()->current_test_info()->name())))
    {
        std::error_code error;
        std::filesystem::remove_all(scratch_, error);
        std::filesystem::create_directories(scratch_, error);
    }
    ~VectorFixture() override
    {
        std::error_code error;
        std::filesystem::remove_all(scratch_, error);
    }

    std::filesystem::path scratch_;
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    katana::app::geo::Context context{document, interpreter, reference, surfaces, scratch_, {}, {}};

    // The line through the one executor, all three phases.
    katana::core::Result<std::string> run(const std::string& line)
    {
        return katana::app::geo::runNow(context, line);
    }

    // `line` must run; its reply.
    std::string ok(const std::string& line)
    {
        auto reply = run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n" << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string();
    }

    // One entity drawn as one undo step; its id.
    katana::entity::EntityId add(katana::entity::Geometry geometry, const std::string& layer = "0",
                                 katana::entity::PropertyMap properties = {})
    {
        katana::entity::Entity entity;
        entity.geometry = std::move(geometry);
        entity.layer = layer;
        entity.properties = std::move(properties);
        if (!document.model().layers.contains(layer)) {
            katana::entity::Layer made;
            made.name = layer;
            EXPECT_TRUE(document.execute(katana::commands::createLayer(made)).ok());
        }
        EXPECT_TRUE(document.execute(katana::commands::createEntities({entity})).ok());
        const auto created = document.lastCreatedEntities();
        return created.empty() ? katana::entity::kInvalidEntityId : created.front();
    }

    // A closed polyline through `corners`.
    katana::entity::EntityId area(const std::vector<katana::geometry::Point2>& corners,
                                  const std::string& layer = "0",
                                  katana::entity::PropertyMap properties = {})
    {
        katana::geometry::Polyline2 ring;
        ring.vertices = corners;
        ring.closed = true;
        return add(ring, layer, std::move(properties));
    }

    // The x0,y0 - x1,y1 rectangle as a closed polyline, anticlockwise.
    katana::entity::EntityId rect(double x0, double y0, double x1, double y1,
                                  const std::string& layer = "0",
                                  katana::entity::PropertyMap properties = {})
    {
        return area({{x0, y0}, {x1, y0}, {x1, y1}, {x0, y1}}, layer, std::move(properties));
    }

    katana::entity::EntityId line(double x0, double y0, double x1, double y1,
                                  const std::string& layer = "0",
                                  katana::entity::PropertyMap properties = {})
    {
        return add(katana::geometry::Segment2{{x0, y0}, {x1, y1}}, layer, std::move(properties));
    }

    // Every entity on `layer`, in id order.
    std::vector<katana::entity::Entity> on(const std::string& layer) const
    {
        std::vector<katana::entity::Entity> found;
        document.model().entities.forEach([&](const katana::entity::Entity& entity) {
            if (entity.layer == layer) {
                found.push_back(entity);
            }
        });
        return found;
    }

    // The area the closed polylines and circles on `layer` enclose, holes
    // (tagged gis.ring=hole) taken away.
    double areaOn(const std::string& layer) const
    {
        double total = 0.0;
        for (const katana::entity::Entity& entity : on(layer)) {
            double one = 0.0;
            if (const auto* ring = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
                one = ring->area();
            } else if (const auto* circle = std::get_if<katana::geometry::Circle2>(&entity.geometry)) {
                one = circle->area();
            }
            const auto tag = entity.properties.find("gis.ring");
            const bool hole = tag != entity.properties.end() &&
                              katana::entity::toString(tag->second) == "hole";
            total += hole ? -one : one;
        }
        return total;
    }

    std::size_t entityCount() const { return document.model().entities.size(); }

    // The first record of `kind` in a reply.
    static std::optional<katana::app::geo::Record> record(const std::string& reply,
                                                          const std::string& kind)
    {
        for (katana::app::geo::Record& one : katana::app::geo::parseRecords(reply)) {
            if (one.kind == kind) {
                return one;
            }
        }
        return std::nullopt;
    }
};

} // namespace katana::geo_test
