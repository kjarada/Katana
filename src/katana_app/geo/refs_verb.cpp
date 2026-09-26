// REFS (docs/interop.md, "Reference layers"): the reference layers - rasters
// and point clouds drawn under the drawing - as records, one per layer, then
// a count. It reads only what the session holds, so it answers at once.
//
//   REFS [LIST]

#include <string>

#include "gis_records.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage = "usage: REFS [LIST]";

std::string listed(const katana::interop::ReferenceData& reference)
{
    std::string text;
    for (const katana::interop::RasterOverlay& raster : reference.rasters()) {
        text += referenceRecord(raster) + "\n";
    }
    for (const katana::interop::PointCloudLayer& cloud : reference.pointClouds()) {
        text += referenceRecord(cloud) + "\n";
    }
    // Said when there are none too: an empty reply reads as a line that did
    // nothing.
    return text + "references rasters=" + std::to_string(reference.rasters().size()) +
           " clouds=" + std::to_string(reference.pointClouds().size());
}

} // namespace

Result<Prepared> prepareRefs(Context& context, const Tokens& tokens, std::string_view)
{
    if (tokens.size() > 2 || (tokens.size() == 2 && !tokens.is(1, "LIST"))) {
        return makeError(ErrorCode::InvalidArgument, kUsage, tokens.size() > 1 ? tokens[1] : "");
    }
    Prepared prepared;
    prepared.title = "REFS";
    prepared.reply = listed(context.reference);
    return prepared;
}

} // namespace katana::app::geo
