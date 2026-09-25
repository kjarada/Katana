#include "proj_internal.hpp"

#include <cstddef>
#include <string>
#include <utility>

#include "katana/core/library_data.hpp"

namespace katana::geodesy::detail {

namespace {

// Upper bound of the captured log. A batch of millions of out-of-domain points
// makes PROJ log one line per point; only the first lines are diagnostic.
constexpr std::size_t kMaxLogBytes = 2048;

} // namespace

core::Result<std::unique_ptr<ProjContext>> ProjContext::create()
{
    std::unique_ptr<ProjContext> context(new ProjContext());
    context->context_ = proj_context_create();
    if (context->context_ == nullptr) {
        return core::makeError(core::ErrorCode::Internal, "PROJ could not allocate a context");
    }
    proj_log_func(context->context_, context.get(), &ProjContext::logCallback);
    proj_log_level(context->context_, PJ_LOG_ERROR);
    proj_context_set_enable_network(context->context_, 0);
    // Before proj.db is first opened below: in a relocated Linux prefix
    // PROJ's own search path is wrong (core/library_data.hpp).
    if (const auto& data = core::projDataDirectory()) {
        const std::string path = data->string();
        const char* const paths[] = {path.c_str()};
        proj_context_set_search_paths(context->context_, 1, paths);
    }

    // Forces PROJ to locate and open proj.db now, so that a broken installation
    // is reported as such instead of as a confusing "crs not found" later.
    if (proj_context_get_database_path(context->context_) == nullptr) {
        std::string detail = "search paths: " + toStdString(proj_info().searchpath);
        const std::string diagnostics = context->consumeDiagnostics();
        if (!diagnostics.empty()) {
            detail += "; " + diagnostics;
        }
        return core::makeError(core::ErrorCode::NotFound,
                               "PROJ resource database (proj.db) not found; install PROJ's data "
                               "files or set the PROJ_DATA environment variable",
                               std::move(detail));
    }
    context->clearDiagnostics();
    return core::Result<std::unique_ptr<ProjContext>>(std::move(context));
}

ProjContext::~ProjContext()
{
    if (context_ != nullptr) {
        proj_context_destroy(context_);
    }
}

void ProjContext::logCallback(void* appData, int /*level*/, const char* message)
{
    auto* self = static_cast<ProjContext*>(appData);
    if (self == nullptr || message == nullptr || self->log_.size() >= kMaxLogBytes) {
        return;
    }
    if (!self->log_.empty()) {
        self->log_ += " | ";
    }
    self->log_ += message;
}

std::string ProjContext::consumeDiagnostics()
{
    std::string text;
    const int code = proj_context_errno(context_);
    if (code != 0) {
        text = "PROJ error " + std::to_string(code) + ": " +
               toStdString(proj_context_errno_string(context_, code));
    }
    if (!log_.empty()) {
        if (!text.empty()) {
            text += "; ";
        }
        text += "PROJ log: " + log_;
    }
    clearDiagnostics();
    return text;
}

void ProjContext::clearDiagnostics()
{
    log_.clear();
}

std::string abbreviate(const std::string& text)
{
    constexpr std::size_t kMaxContextBytes = 240;
    return text.size() <= kMaxContextBytes ? text : text.substr(0, kMaxContextBytes) + "...";
}

core::Result<PjPtr> createCrs(ProjContext& context, const std::string& definition)
{
    context.clearDiagnostics();
    PjPtr object(proj_create(context.get(), definition.c_str()));
    if (!object) {
        return core::makeError(core::ErrorCode::InvalidCRS,
                               "PROJ does not recognise the CRS definition",
                               "definition='" + abbreviate(definition) + "'; " +
                                   context.consumeDiagnostics());
    }
    if (proj_is_crs(object.get()) == 0) {
        return core::makeError(core::ErrorCode::InvalidCRS,
                               "the definition describes a PROJ object that is not a CRS",
                               "definition='" + abbreviate(definition) + "'; object='" +
                                   toStdString(proj_get_name(object.get())) + "'");
    }
    context.clearDiagnostics();
    return core::Result<PjPtr>(std::move(object));
}

std::string describeErrno(ProjContext& context, int projErrno)
{
    return "PROJ error " + std::to_string(projErrno) + ": " +
           toStdString(proj_context_errno_string(context.get(), projErrno));
}

} // namespace katana::geodesy::detail
