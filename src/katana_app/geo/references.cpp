// The reference layers a project records (references.hpp).

#include "references.hpp"

#include <utility>

namespace katana::app::geo {

void recordReferences(Context& context)
{
    katana::storage::ProjectMetadata metadata = context.document.metadata();
    metadata.referenceLayers = katana::interop::referenceRecords(context.reference);
    // setMetadata changes, and marks, nothing when the records are the same.
    context.document.setMetadata(std::move(metadata));
}

bool recordsReferences(const Context& context)
{
    return !context.document.metadata().referenceLayers.empty();
}

} // namespace katana::app::geo
