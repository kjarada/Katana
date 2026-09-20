#pragma once

// Structured error handling (PLAN.MD section 36).
//
// Fallible operations return Result<T>. There are no silent failures: a Result
// either holds a value or an Error carrying a category, a human readable
// message and optional diagnostic context. Exceptions are reserved for
// programming errors (violated preconditions such as reading the value of a
// failed Result).

#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <variant>

namespace katana::core {

enum class ErrorCode {
    InvalidArgument,
    InvalidState,
    NotFound,
    AlreadyExists,
    ParseFailure,
    Unsupported,
    InvalidGeometry,
    InvalidCRS,
    InvalidSurveyObservation,
    AdjustmentFailure,
    TriangulationFailure,
    FileImportFailure,
    FileExportFailure,
    DatabaseFailure,
    RenderingFailure,
    CommandRejected,
    Internal,
};

[[nodiscard]] constexpr std::string_view toString(ErrorCode code)
{
    switch (code) {
    case ErrorCode::InvalidArgument:
        return "InvalidArgument";
    case ErrorCode::InvalidState:
        return "InvalidState";
    case ErrorCode::NotFound:
        return "NotFound";
    case ErrorCode::AlreadyExists:
        return "AlreadyExists";
    case ErrorCode::ParseFailure:
        return "ParseFailure";
    case ErrorCode::Unsupported:
        return "Unsupported";
    case ErrorCode::InvalidGeometry:
        return "InvalidGeometry";
    case ErrorCode::InvalidCRS:
        return "InvalidCRS";
    case ErrorCode::InvalidSurveyObservation:
        return "InvalidSurveyObservation";
    case ErrorCode::AdjustmentFailure:
        return "AdjustmentFailure";
    case ErrorCode::TriangulationFailure:
        return "TriangulationFailure";
    case ErrorCode::FileImportFailure:
        return "FileImportFailure";
    case ErrorCode::FileExportFailure:
        return "FileExportFailure";
    case ErrorCode::DatabaseFailure:
        return "DatabaseFailure";
    case ErrorCode::RenderingFailure:
        return "RenderingFailure";
    case ErrorCode::CommandRejected:
        return "CommandRejected";
    case ErrorCode::Internal:
        return "Internal";
    }
    return "Unknown";
}

struct Error {
    ErrorCode code = ErrorCode::Internal;
    std::string message;
    std::string context; // where / with what input; empty when not applicable

    [[nodiscard]] std::string describe() const
    {
        std::string text(toString(code));
        text += ": ";
        text += message;
        if (!context.empty()) {
            text += " [";
            text += context;
            text += "]";
        }
        return text;
    }
};

[[nodiscard]] inline Error makeError(ErrorCode code, std::string message, std::string context = {})
{
    return Error{code, std::move(message), std::move(context)};
}

// Thrown only when a Result is misused (value() on failure, error() on success).
class BadResultAccess : public std::logic_error {
  public:
    using std::logic_error::logic_error;
};

template <typename T> class [[nodiscard]] Result {
  public:
    Result(T value) : state_(std::in_place_index<0>, std::move(value)) {}
    Result(Error error) : state_(std::in_place_index<1>, std::move(error)) {}

    [[nodiscard]] bool ok() const { return state_.index() == 0; }
    explicit operator bool() const { return ok(); }

    [[nodiscard]] T& value() &
    {
        requireValue();
        return std::get<0>(state_);
    }
    [[nodiscard]] const T& value() const&
    {
        requireValue();
        return std::get<0>(state_);
    }
    [[nodiscard]] T&& value() &&
    {
        requireValue();
        return std::get<0>(std::move(state_));
    }

    [[nodiscard]] const Error& error() const
    {
        if (ok()) {
            throw BadResultAccess("Result::error() called on a successful Result");
        }
        return std::get<1>(state_);
    }

    [[nodiscard]] T valueOr(T fallback) const& { return ok() ? std::get<0>(state_) : fallback; }

    T* operator->() { return &value(); }
    const T* operator->() const { return &value(); }
    T& operator*() & { return value(); }
    const T& operator*() const& { return value(); }

  private:
    void requireValue() const
    {
        if (!ok()) {
            throw BadResultAccess("Result::value() called on a failed Result: " +
                                  std::get<1>(state_).describe());
        }
    }

    std::variant<T, Error> state_;
};

template <> class [[nodiscard]] Result<void> {
  public:
    Result() = default;
    Result(Error error) : error_(std::move(error)), failed_(true) {}

    [[nodiscard]] bool ok() const { return !failed_; }
    explicit operator bool() const { return ok(); }

    [[nodiscard]] const Error& error() const
    {
        if (ok()) {
            throw BadResultAccess("Result::error() called on a successful Result");
        }
        return error_;
    }

  private:
    Error error_;
    bool failed_ = false;
};

using Status = Result<void>;

} // namespace katana::core
