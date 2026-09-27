#pragma once

// Shared between the katana_io sources, never installed: GDAL's errors and
// warnings, collected for one call on the thread it runs on.
//
// CPLPushErrorHandlerEx is a per-thread stack, so a collector sees exactly the
// messages of the call on its own thread, however many run at once. Messages
// raised on threads GDAL starts itself skip it and reach the process-wide
// CPLQuietErrorHandler gdal_adapter.cpp installs, which stays: a global
// handler per call would take other threads' messages too. The bridge
// (geo/processing.cpp) has had the same collector since F0, in its own file;
// this is the adapter's, for reads and writes of files.

#include <cstddef>
#include <map>
#include <string>
#include <vector>

#include <cpl_error.h>

namespace katana::gis::detail {

class CplErrorCollector {
  public:
    struct Message {
        bool failure = false; // CE_Failure or worse; otherwise a warning
        std::string text;
    };

    CplErrorCollector() { CPLPushErrorHandlerEx(&CplErrorCollector::handler, this); }
    ~CplErrorCollector() { CPLPopErrorHandler(); }
    CplErrorCollector(const CplErrorCollector&) = delete;
    CplErrorCollector& operator=(const CplErrorCollector&) = delete;

    [[nodiscard]] std::size_t size() const { return messages_.size(); }

    // The first failure raised since message `from`, or nullptr.
    [[nodiscard]] const Message* firstFailure(std::size_t from = 0) const
    {
        for (std::size_t i = from; i < messages_.size(); ++i) {
            if (messages_[i].failure) {
                return &messages_[i];
            }
        }
        return nullptr;
    }

    // The warnings, each once, with how many times GDAL said it when it said
    // it more than once: GDAL warns per feature, and a thousand copies of one
    // sentence tell a person less than the sentence and the count.
    [[nodiscard]] std::vector<std::string> warnings() const
    {
        std::vector<std::string> order;
        std::map<std::string, std::size_t> counts;
        for (const Message& message : messages_) {
            if (message.failure) {
                continue;
            }
            if (counts[message.text]++ == 0) {
                order.push_back(message.text);
            }
        }
        std::vector<std::string> out;
        out.reserve(order.size());
        for (const std::string& text : order) {
            const std::size_t count = counts[text];
            out.push_back(count > 1 ? text + " (" + std::to_string(count) + " times)" : text);
        }
        return out;
    }

  private:
    static void CPL_STDCALL handler(CPLErr kind, CPLErrorNum, const char* message)
    {
        if (kind == CE_Debug || kind == CE_None) {
            return;
        }
        auto* self = static_cast<CplErrorCollector*>(CPLGetErrorHandlerUserData());
        self->messages_.push_back(Message{kind >= CE_Failure, message != nullptr ? message : ""});
    }

    std::vector<Message> messages_;
};

} // namespace katana::gis::detail
