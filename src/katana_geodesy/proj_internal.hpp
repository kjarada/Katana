#pragma once

// INTERNAL to src/katana_geodesy: RAII ownership of PROJ handles and capture of
// PROJ diagnostics. This header is the only one that includes <proj.h>; it must
// never be included from include/katana/ (PLAN.MD Rule 4, enforced by
// tools/check_layering.cmake). PROJ types stay behind the pimpl of the public
// classes.

#include <memory>
#include <string>

#include <proj.h>

#include "katana/core/error.hpp"

namespace katana::geodesy::detail {

struct PjDeleter {
    void operator()(PJ* object) const noexcept { proj_destroy(object); }
};
using PjPtr = std::unique_ptr<PJ, PjDeleter>;

struct PjListDeleter {
    void operator()(PJ_OBJ_LIST* list) const noexcept { proj_list_destroy(list); }
};
using PjListPtr = std::unique_ptr<PJ_OBJ_LIST, PjListDeleter>;

struct PjFactoryContextDeleter {
    void operator()(PJ_OPERATION_FACTORY_CONTEXT* context) const noexcept
    {
        proj_operation_factory_context_destroy(context);
    }
};
using PjFactoryContextPtr = std::unique_ptr<PJ_OPERATION_FACTORY_CONTEXT, PjFactoryContextDeleter>;

// Owns one PJ_CONTEXT. A PROJ context (and every PJ created in it) may be used
// by one thread at a time, so each public object owns its own ProjContext and
// nothing is shared or global.
//
// PROJ's log output is routed into the context's diagnostic buffer instead of
// stderr, and is attached to the Error returned to the caller.
//
// The log callback holds a pointer to this object: it is neither copyable nor
// movable and is always heap allocated (see create()).
class ProjContext {
  public:
    // Creates a context with the network disabled (results must not depend on
    // what happens to be downloadable, PLAN.MD Rule 7) and verifies that
    // proj.db can be opened. Fails with NotFound otherwise.
    [[nodiscard]] static core::Result<std::unique_ptr<ProjContext>> create();

    ~ProjContext();
    ProjContext(const ProjContext&) = delete;
    ProjContext& operator=(const ProjContext&) = delete;

    [[nodiscard]] PJ_CONTEXT* get() const { return context_; }

    // PROJ's description of the latest failure: the context errno text followed
    // by the log lines captured since the last call. Clears the log buffer.
    // (PROJ offers no public way to reset a context errno, so call this only
    // directly after a PROJ call that reported failure.)
    [[nodiscard]] std::string consumeDiagnostics();
    void clearDiagnostics();

  private:
    ProjContext() = default;
    static void logCallback(void* appData, int level, const char* message);

    PJ_CONTEXT* context_ = nullptr;
    std::string log_;
};

// Null-safe conversion of the C strings PROJ returns.
[[nodiscard]] inline std::string toStdString(const char* text)
{
    return text == nullptr ? std::string{} : std::string(text);
}

// Shortens long definitions (WKT) for use inside Error::context.
[[nodiscard]] std::string abbreviate(const std::string& text);

// Instantiates `definition` and checks that it is a CRS. InvalidCRS on failure,
// with PROJ's message in Error::context.
[[nodiscard]] core::Result<PjPtr> createCrs(ProjContext& context, const std::string& definition);

// Text of a PROJ errno for a PJ that failed during proj_trans().
[[nodiscard]] std::string describeErrno(ProjContext& context, int projErrno);

} // namespace katana::geodesy::detail
