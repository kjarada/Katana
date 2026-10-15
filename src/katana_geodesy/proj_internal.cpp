#include "proj_internal.hpp"

#include <cstddef>
#include <cstdlib>
#include <string>
#include <utility>

#include "katana/core/library_data.hpp"

namespace katana::geodesy::detail {

namespace {

// Upper bound of the captured log. A batch of millions of out-of-domain points
// makes PROJ log one line per point; only the first lines are diagnostic.
constexpr std::size_t kMaxLogBytes = 2048;

} // namespace

bool ProjContext::open(const std::optional<std::filesystem::path>& data)
{
    if (context_ != nullptr) {
        proj_context_destroy(context_);
    }
    log_.clear();
    context_ = proj_context_create();
    if (context_ == nullptr) {
        return false;
    }
    proj_log_func(context_, this, &ProjContext::logCallback);
    proj_log_level(context_, PJ_LOG_ERROR);
    proj_context_set_enable_network(context_, 0);
    if (data) {
        // Before proj.db is first opened below. UTF-8, as PROJ takes a path.
        const std::u8string utf8 = data->generic_u8string();
        const std::string path(utf8.begin(), utf8.end());
        const char* const paths[] = {path.c_str()};
        proj_context_set_search_paths(context_, 1, paths);
    }
    // Forces PROJ to locate and open proj.db now, so that a broken installation
    // is reported as such instead of as a confusing "crs not found" later.
    return proj_context_get_database_path(context_) != nullptr;
}

core::Result<std::unique_ptr<ProjContext>> ProjContext::create()
{
    std::unique_ptr<ProjContext> context(new ProjContext());
    // In a relocated Linux prefix PROJ's own search path is wrong
    // (core/library_data.hpp), and Katana points it at the data beside libproj.
    const auto& pointed = core::projDataDirectory();
    if (context->open(pointed)) {
        context->clearDiagnostics();
        return core::Result<std::unique_ptr<ProjContext>>(std::move(context));
    }
    if (context->context_ == nullptr) {
        return core::makeError(core::ErrorCode::Internal, "PROJ could not allocate a context");
    }

    // PROJ could not open proj.db where it looked. That is not always a
    // missing installation: PROJ_DATA or PROJ_LIB replaces the search beside the
    // DLL, and another GIS package (PostGIS, QGIS, OSGeo4W) leaves one
    // machine-wide naming data of its own, usually a database of an older PROJ's
    // layout, which this PROJ rejects. The package carries its own, so fall
    // back on it - a fresh context, so that nothing of the failed attempt
    // lingers in PROJ's error state. (An explicit choice that WORKS is kept:
    // this is reached only when it does not.)
    std::string detail = "search paths: " + toStdString(proj_info().searchpath);
    if (const std::string diagnostics = context->consumeDiagnostics(); !diagnostics.empty()) {
        detail += "; " + diagnostics;
    }
    if (const auto& bundled = core::bundledProjDataDirectory(); bundled && bundled != pointed) {
        if (context->open(bundled)) {
            context->clearDiagnostics();
            return core::Result<std::unique_ptr<ProjContext>>(std::move(context));
        }
        if (context->context_ == nullptr) {
            return core::makeError(core::ErrorCode::Internal, "PROJ could not allocate a context");
        }
        detail += "; the data beside libproj, " + bundled->string() + ", did not open either";
        if (const std::string diagnostics = context->consumeDiagnostics(); !diagnostics.empty()) {
            detail += ": " + diagnostics;
        }
    }

    // Say what is wrong in the words that fit: an environment that names data
    // which does not open is not a missing installation, and telling a person
    // whose package carries PROJ's data to install it was the wrong advice.
    const char* variable = std::getenv("PROJ_DATA") != nullptr ? "PROJ_DATA"
                           : std::getenv("PROJ_LIB") != nullptr ? "PROJ_LIB"
                                                                : nullptr;
    if (variable != nullptr) {
        return core::makeError(core::ErrorCode::NotFound,
                               std::string("PROJ resource database (proj.db) not usable: ") +
                                   variable + " names " + std::getenv(variable) +
                                   ", which holds no proj.db this PROJ can open; unset it, or "
                                   "point it at the data of this PROJ",
                               std::move(detail));
    }
    return core::makeError(core::ErrorCode::NotFound,
                           "PROJ resource database (proj.db) not found; install PROJ's data "
                           "files or set the PROJ_DATA environment variable",
                           std::move(detail));
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
