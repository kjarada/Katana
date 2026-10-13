// The Windows Launcher of include/katana/update/process.hpp. Compiled on Windows only.
//
// CREATE_NO_WINDOW, in BOTH run() and startDetached(): katana_updater is a console program,
// and started from the window (a GUI program with no console of its own) Windows would open
// a black console for the length of the swap, and again for the preflight's --selftest. The
// flag gives a console program a console that has no window. It is not combined with
// DETACHED_PROCESS, which the documentation says makes it ignore CREATE_NO_WINDOW; a detached
// child outlives its parent anyway, and CREATE_BREAKAWAY_FROM_JOB (when the job allows it)
// is what lets the helper outlive a window that was started inside a job that kills its
// children when it closes.
//
// Nothing goes through a shell: the command line is made from the program and its arguments
// by the quoting rules CommandLineToArgvW reads back (command_line.hpp), and a program that
// is not found is an error, not a message from cmd.exe.
//
// No dialog, ever: CreateProcess on a file that is not a program Windows can run (a text
// file named .exe, a program for another architecture, a truncated download) does not just
// fail, it raises a system message box ("This app can't run on your PC", or on some builds
// "Unsupported 16-Bit Application") and waits for someone to click it - in a helper that runs
// unattended, or in a test, that is a hang. ErrorDialogsOff turns the box off for the length
// of the call (SEM_FAILCRITICALERRORS), and CREATE_DEFAULT_ERROR_MODE keeps the child from
// inheriting the setting, so the child behaves as it would if the user had started it.

#include "katana/update/process.hpp"

#ifndef _WIN32
#error "spawn_win.cpp is compiled on Windows only"
#endif

#include <algorithm>
#include <array>
#include <chrono>
#include <string>
#include <vector>

#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include "command_line.hpp"
#include "katana/core/path_text.hpp"

namespace katana::update {

namespace {

constexpr std::size_t kOutputLimit = 64 * 1024;

std::wstring wideFromUtf8(std::string_view text)
{
    if (text.empty()) {
        return {};
    }
    const int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text.data(),
                                           static_cast<int>(text.size()), nullptr, 0);
    if (length <= 0) {
        return {};
    }
    std::wstring wide(static_cast<std::size_t>(length), L'\0');
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text.data(), static_cast<int>(text.size()),
                        wide.data(), length);
    return wide;
}

// A path with the separators of the platform. cmd.exe reads its own command line for "/"
// switches and does not skip the quoted name of the program: a program path with a forward
// slash in it (C:/Windows/System32/cmd.exe) makes it find "/cmd.exe", read it as the switch /c
// and run ".exe /c ..." as a command. Callers join paths with "/" all the time, so the launcher
// settles it once.
std::filesystem::path preferred(std::filesystem::path path)
{
    path.make_preferred();
    return path;
}

core::Error startError(DWORD error, const std::filesystem::path& program)
{
    const bool missing = error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND;
    return core::makeError(missing ? core::ErrorCode::NotFound : core::ErrorCode::InvalidState,
                           "the program could not be started",
                           "os_error=" + std::to_string(error) + " " + core::pathToUtf8(program));
}

class Closer {
  public:
    explicit Closer(HANDLE handle = nullptr) : handle_(handle) {}
    Closer(const Closer&) = delete;
    Closer& operator=(const Closer&) = delete;
    ~Closer() { reset(); }
    void reset(HANDLE handle = nullptr)
    {
        if (handle_ != nullptr && handle_ != INVALID_HANDLE_VALUE) {
            CloseHandle(handle_);
        }
        handle_ = handle;
    }
    [[nodiscard]] HANDLE get() const { return handle_; }

  private:
    HANDLE handle_;
};

// Takes what the child has written so far; keeps the first kOutputLimit bytes and reads
// (and drops) the rest, so a chatty child never blocks on a full pipe.
void drain(HANDLE pipe, std::string& output)
{
    std::array<char, 4096> buffer{};
    while (true) {
        DWORD available = 0;
        if (!PeekNamedPipe(pipe, nullptr, 0, nullptr, &available, nullptr) || available == 0) {
            return;
        }
        DWORD got = 0;
        const DWORD want = std::min<DWORD>(available, static_cast<DWORD>(buffer.size()));
        if (!ReadFile(pipe, buffer.data(), want, &got, nullptr) || got == 0) {
            return;
        }
        if (output.size() < kOutputLimit) {
            output.append(buffer.data(), std::min<std::size_t>(got, kOutputLimit - output.size()));
        }
    }
}

// Turns the system's "cannot run this file" message box off while it lives, and puts the
// error mode back. The process error mode is shared by every thread, so two launches at once
// can restore the wrong value; the flags this adds are the ones this library wants always,
// and the helper sets the same flags for the whole process in main(), so the worst case of a
// race is a restore to a value that already had them.
class ErrorDialogsOff {
  public:
    ErrorDialogsOff() : previous_(SetErrorMode(0))
    {
        // SetErrorMode returns the old mode: keep what the process already asked for.
        SetErrorMode(previous_ | SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    }
    ~ErrorDialogsOff() { SetErrorMode(previous_); }
    ErrorDialogsOff(const ErrorDialogsOff&) = delete;
    ErrorDialogsOff& operator=(const ErrorDialogsOff&) = delete;

  private:
    UINT previous_;
};

class WindowsLauncher final : public Launcher {
  public:
    core::Result<SpawnResult> run(const SpawnRequest& request) override
    {
        std::wstring command =
            wideFromUtf8(detail::commandLine(core::pathToUtf8(preferred(request.executable)), request.arguments));
        if (command.empty()) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the command line is not valid UTF-8",
                                   core::pathToUtf8(request.executable));
        }
        command.push_back(L'\0');

        SECURITY_ATTRIBUTES inheritable{sizeof(SECURITY_ATTRIBUTES), nullptr, TRUE};
        HANDLE readEnd = nullptr;
        HANDLE writeEnd = nullptr;
        if (!CreatePipe(&readEnd, &writeEnd, &inheritable, 0)) {
            return startError(GetLastError(), request.executable);
        }
        Closer reader(readEnd);
        Closer writer(writeEnd);
        SetHandleInformation(readEnd, HANDLE_FLAG_INHERIT, 0);
        // The child reads nothing: its standard input is the null device.
        Closer nothing(CreateFileW(L"NUL", GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
                                   &inheritable, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr));

        // Only these two handles are inherited, not every inheritable one this process has.
        HANDLE inherited[2] = {nothing.get(), writer.get()};
        SIZE_T attributeBytes = 0;
        InitializeProcThreadAttributeList(nullptr, 1, 0, &attributeBytes);
        std::vector<char> attributeStorage(attributeBytes);
        auto* attributes = reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(attributeStorage.data());
        if (!InitializeProcThreadAttributeList(attributes, 1, 0, &attributeBytes) ||
            !UpdateProcThreadAttribute(attributes, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, inherited,
                                       sizeof(inherited), nullptr, nullptr)) {
            return startError(GetLastError(), request.executable);
        }

        STARTUPINFOEXW startup{};
        startup.StartupInfo.cb = sizeof(startup);
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = nothing.get();
        startup.StartupInfo.hStdOutput = writer.get();
        startup.StartupInfo.hStdError = writer.get();
        startup.lpAttributeList = attributes;

        const std::wstring program = preferred(request.executable).native();
        const std::wstring directory = preferred(request.workingDirectory).native();
        PROCESS_INFORMATION process{};
        BOOL started = FALSE;
        DWORD startedError = 0;
        {
            const ErrorDialogsOff noDialogs;
            started = CreateProcessW(
                program.c_str(), command.data(), nullptr, nullptr, TRUE,
                CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT |
                    CREATE_DEFAULT_ERROR_MODE,
                nullptr, directory.empty() ? nullptr : directory.c_str(), &startup.StartupInfo,
                &process);
            startedError = GetLastError();
        }
        DeleteProcThreadAttributeList(attributes);
        if (!started) {
            return startError(startedError, request.executable);
        }
        Closer processHandle(process.hProcess);
        Closer threadHandle(process.hThread);
        // Our copy of the write end must go, or the pipe never reports the end of the output.
        writer.reset();
        nothing.reset();

        SpawnResult result;
        const auto deadline = std::chrono::steady_clock::now() + request.timeout;
        while (true) {
            drain(reader.get(), result.output);
            if (WaitForSingleObject(process.hProcess, 10) == WAIT_OBJECT_0) {
                break;
            }
            if (std::chrono::steady_clock::now() >= deadline) {
                TerminateProcess(process.hProcess, 1);
                WaitForSingleObject(process.hProcess, 5000);
                result.timedOut = true;
                break;
            }
        }
        drain(reader.get(), result.output);
        DWORD code = 0;
        if (GetExitCodeProcess(process.hProcess, &code)) {
            result.exitCode = static_cast<int>(code);
        }
        return result;
    }

    core::Status startDetached(const SpawnRequest& request) override
    {
        std::wstring command =
            wideFromUtf8(detail::commandLine(core::pathToUtf8(preferred(request.executable)), request.arguments));
        if (command.empty()) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the command line is not valid UTF-8",
                                   core::pathToUtf8(request.executable));
        }
        command.push_back(L'\0');

        const std::wstring program = preferred(request.executable).native();
        const std::wstring directory = preferred(request.workingDirectory).native();
        STARTUPINFOW startup{};
        startup.cb = sizeof(startup);
        PROCESS_INFORMATION process{};
        const DWORD common = CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | CREATE_NEW_PROCESS_GROUP |
                             CREATE_DEFAULT_ERROR_MODE;
        BOOL started = FALSE;
        DWORD startedError = 0;
        {
            const ErrorDialogsOff noDialogs;
            // Break away from a job that would kill the child with its parent, when the job
            // allows it; when it does not, CreateProcess says access denied and the child just
            // starts.
            started = CreateProcessW(program.c_str(), command.data(), nullptr, nullptr, FALSE,
                                     common | CREATE_BREAKAWAY_FROM_JOB, nullptr,
                                     directory.empty() ? nullptr : directory.c_str(), &startup,
                                     &process);
            startedError = GetLastError();
            if (!started && startedError == ERROR_ACCESS_DENIED) {
                started = CreateProcessW(program.c_str(), command.data(), nullptr, nullptr, FALSE,
                                         common, nullptr,
                                         directory.empty() ? nullptr : directory.c_str(), &startup,
                                         &process);
                startedError = GetLastError();
            }
        }
        if (!started) {
            return startError(startedError, request.executable);
        }
        CloseHandle(process.hProcess);
        CloseHandle(process.hThread);
        return {};
    }
};

} // namespace

std::unique_ptr<Launcher> makeSystemLauncher()
{
    return std::make_unique<WindowsLauncher>();
}

// process.hpp says what is asked and why. TokenElevationTypeFull is the elevated half of a
// split token; TokenElevationTypeLimited is the other half, and TokenElevationTypeDefault a
// token that was never split (User Account Control off, or the built-in administrator).
// Every failure to ask is "not elevated".
bool processIsElevated()
{
    HANDLE opened = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &opened)) {
        return false;
    }
    const Closer token(opened);
    TOKEN_ELEVATION_TYPE kind = TokenElevationTypeDefault;
    DWORD written = 0;
    if (!GetTokenInformation(token.get(), TokenElevationType, &kind, sizeof(kind), &written) ||
        written != sizeof(kind)) {
        return false;
    }
    return kind == TokenElevationTypeFull;
}

} // namespace katana::update
