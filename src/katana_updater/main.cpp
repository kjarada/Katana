// katana_updater: the fourth program, the helper that swaps the install's files while Katana is
// not running, and puts them back if anything goes wrong (include/katana/update/apply.hpp has
// the algorithm; this file is the program around it: its command line, its environment, its
// exit codes).
//
// IT NEVER RUNS FROM bin/. The engine copies it to <U>/run-<tx>/ and runs that copy
// (stage.hpp, preflight), so at commit nothing under the install root is loaded; it is a static
// program that imports only the operating system's DLLs (src/katana_updater/CMakeLists.txt;
// the tests check the import table and run a copy of it with no toolchain on PATH).
//
// COMMAND LINES (the arguments arrive as UTF-8, read with update::utf8Arguments and never from
// argv, which on Windows is the ANSI code page's version of them: this is the program that is
// always started with paths under a user profile, whose name may hold a letter that code page
// cannot carry):
//
//   katana_updater --root R --stage S --state U --tx T --wait-pid P --relaunch <path|none>
//       the swap. The window has already exited when this starts. R, S, U and the relaunch path
//       must be absolute: the program that starts the helper knows them, and a relative one
//       would depend on a working directory nobody chose. T is 1 to 64 characters of 0-9 a-f.
//   katana_updater --recover R
//       puts back whatever a crash or a failed rollback left half swapped under R (the RunOnce
//       entry runs this at the next logon; recover() runs it at the next start of any program;
//       a person runs it by hand when the install no longer starts). R may be relative here
//       (a person typed it) and must be a folder.
//   katana_updater --selftest
//       proves this binary starts, links and can verify an Ed25519 signature, prints what it
//       found and exits 0 (below). The engine runs it on the copy before the window closes.
//   katana_updater --version
//       one line: the version and the protocol (apply.hpp, kApplierProtocol).
//   katana_updater --help
//       the usage text on standard output, exit 0.
//
// Anything else - an unknown or repeated option, a missing value or option, modes mixed together -
// is a usage error: the text on standard error and exit 20.
//
// EXIT CODES. They are the program's whole interface to a script and to a person at a prompt
// (the window reads last.json, not these):
//
//    0  ok: the update is installed and healthy; or --recover put everything right, or had
//       nothing to do; or --selftest and --version succeeded
//   10  blocked: a process under R did not exit (or the window did not); NOTHING was touched,
//       the old version was started again. apply.hpp, step 2
//   11  rolled back: the update failed after the plan and the old version is back (the commit
//       failed, or the new version did not start well enough: it exited non-zero or never
//       wrote its health marker). The sequence is added to `blocked` in state.json
//   12  rollback incomplete: putting the old version back failed too. The tree may be half
//       swapped, nothing is started, and the command to run is `katana_updater --recover R`.
//       Also what --recover itself returns when it could not finish
//   13  refused: nothing was touched, for a reason that is not about the release's signature:
//       another update holds the lock, the install cannot be written, a staged file is missing
//       or wrong, ... (the `reason=` line says which; apply.hpp lists them)
//   20  usage: the command line is wrong
//   21  verification failed: the staged release is not trusted by THIS program's keys (a
//       missing or unsigned manifest, a signature that does not match, another product or
//       target, a path rule, a downgrade, a release meant only to be announced, one that needs
//       a newer helper) - or --selftest found that this binary cannot verify a signature
//   30  internal error: an exception, or libcrypto could not be set up
//   99  (the test build only) a simulated crash, from --die-at
//
// WHAT IT PRINTS (standard output, key=value, one per line - it is a console program started
// without a window, so nobody sees it unless it is run from a prompt or by a test):
//   the swap:    outcome= reason= exit= detail= and a holder= line for each blocking process
//   --recover:   recover=ok|nothing_to_do|failed, exit=, detail=
//   --selftest:  selftest=ok|failed, version=, applier=, ed25519=, openssl_environment=, keys=
//
// THE ENVIRONMENT. main() reads OPENSSL_CONF, OPENSSL_MODULES and OPENSSL_ENGINES, then calls
// hardenOpenSslEnvironment(), which removes them and starts libcrypto without a configuration
// file: nothing in the environment can change what "verified" means (signature.hpp). The
// values are kept, because the program the helper starts afterwards - the new version for its
// health check, the old or new one at the end - is the person's program and must see the
// person's environment, so each launch lends the values back for its duration
// (HandOnLauncher). --selftest runs after the hardening and says whether the variables are
// really gone, so it fails on a binary whose hardening is missing.
// The helper also sets the process error mode (SEM_FAILCRITICALERRORS and its companions), which
// the programs it starts inherit: no "entry point not found", "not a valid Win32 application" or
// crash box can open and wait for a click on a desktop nobody is watching - measured: starting a
// file that is not a program with the default mode blocked the helper behind such a box until it
// was killed. A new version that cannot load then exits at once with a loader error and is rolled
// back, instead of waiting out the 60 s limit. The person's Katana, started at the end, gets back
// only SEM_FAILCRITICALERRORS (HandOnLauncher), the rest reset, so a crash of it reports as usual.
//
// WHICH PER-USER STATE IT WRITES.
//   the swap:    --state U is given, so last.json, state.json (`blocked`), the health marker's
//                removal and U/cleanup.list are written there - this is the window's own path.
//   --recover:   nothing in U, ever, and no last.json: recover() is called first by katana_cli
//                and katana_mcp too, and a headless program writes no per-user state
//                (docs/headless.md). It writes only R/.update/recovery.log, one line for each
//                run that found something to put right. The window learns from the install
//                itself (installed.json says which version is there), and from last.json when
//                the swap's own helper got to write one.
//
// TRUST. The keys come from trustForThisBuild() and from nowhere else:
//   - katana_updater (installed) passes productionTrust(), the keys pinned at build time.
//   - katana_updater_test (tests/update/CMakeLists.txt, never installed) is these same sources
//     compiled with KATANA_UPDATER_TEST_BUILD: a test key passed in place of the pinned ones,
//     and three more options that exist only there, so that the end-to-end tests can drive the
//     real program:
//       --die-at N            a FaultFs: the N-th change to the disk is not made and the
//                             process ends there (exit 99), as if it had been killed
//       --registry-base K     every registry key the helper uses is re-rooted under HKCU\K, so a
//                             test never touches the machine's RunOnce or Uninstall entries
//       --timings a,b,c,d,e   milliseconds: wait for the window, wait for the programs under R,
//                             poll interval, wait for the health check, and how long a rename
//                             that Windows refuses is retried - so a test need not wait a minute
//     A shipped helper rejects all three as unknown options (a test checks that).
//   So no test key exists in a shipped helper, and nothing in the environment moves the trust
//   root. tests/update/check_no_test_keys.cmake looks at the bytes.
//   The self-test's known-answer signature is RFC 8032's TEST 3, a published signature of a
//   2-byte message; it is not one of the test keys above, it is never given to the trust gate,
//   and verifying it proves the code path, not any release.

#include <algorithm>
#include <array>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <exception>
#include <filesystem>
#include <format>
#include <iostream>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#else
#include <unistd.h>
#endif

#include "katana/core/error.hpp"
#include "katana/core/path_text.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/fault_fs.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/install.hpp"
#include "katana/update/process.hpp"
#include "katana/update/recover.hpp"
#include "katana/update/registry.hpp"
#include "katana/update/signature.hpp"
#include "katana/update/trust.hpp"
#include "katana/update/version.hpp"

#ifdef KATANA_UPDATER_TEST_BUILD
namespace katana::update {
// Defined by tests/update/test_key.cpp, which only the test executables link.
const Trust& testTrust();
#ifdef _WIN32
// Defined by src/katana_updater/registry_win.cpp: the registry under HKCU\<base>.
std::unique_ptr<RegistryOps> makeRegistryOpsUnder(std::string_view base);
#endif
} // namespace katana::update
#endif

namespace {

namespace upd = katana::update;
namespace core = katana::core;
using Path = std::filesystem::path;

// ---- exit codes (the table at the top) ----------------------------------------------------------

constexpr int kExitOk = 0;
constexpr int kExitBlocked = 10;
constexpr int kExitRolledBack = 11;
constexpr int kExitRollbackIncomplete = 12;
constexpr int kExitRefused = 13;
constexpr int kExitUsage = 20;
constexpr int kExitVerification = 21;
constexpr int kExitInternal = 30;
[[maybe_unused]] constexpr int kExitSimulatedCrash = 99;

// The ApplyResult reasons that mean "this program does not trust what was staged" (apply.hpp
// lists every reason; the rest are refusals about the machine or the files).
bool isVerificationReason(std::string_view reason)
{
    return reason == "staged_manifest" || reason == "notify_only" || reason == "applier_too_new" ||
           reason == "downgrade";
}

int exitCodeFor(const upd::ApplyResult& result)
{
    switch (result.outcome) {
    case upd::ApplyOutcome::Committed:
        return kExitOk;
    case upd::ApplyOutcome::Blocked:
        return kExitBlocked;
    case upd::ApplyOutcome::RolledBack:
        return kExitRolledBack;
    case upd::ApplyOutcome::RollbackIncomplete:
        return kExitRollbackIncomplete;
    case upd::ApplyOutcome::Refused:
        return isVerificationReason(result.reason) ? kExitVerification : kExitRefused;
    }
    return kExitInternal;
}

// ---- text ---------------------------------------------------------------------------------------

constexpr std::string_view kUsage =
    "usage: katana_updater --root R --stage S --state U --tx T --wait-pid P "
    "--relaunch <path|none>\n"
    "       katana_updater --recover R\n"
    "       katana_updater --selftest | --version | --help\n";

// `text` on one line: a control character becomes %XX, so that a value read from a file or a
// process cannot make one record look like two.
std::string oneLine(std::string_view text)
{
    constexpr std::string_view hex = "0123456789ABCDEF";
    std::string out;
    out.reserve(text.size());
    for (const char c : text) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte < 0x20 || byte == 0x7F) {
            out += '%';
            out += hex[byte >> 4];
            out += hex[byte & 0x0F];
        } else {
            out += c;
        }
    }
    return out;
}

std::string utcNow()
{
    return std::format("{:%Y-%m-%dT%H:%M:%SZ}",
                       std::chrono::floor<std::chrono::seconds>(std::chrono::system_clock::now()));
}

// The last component of a path as UTF-8, or "?" for a name that is not text (an unpaired
// surrogate, which Windows file names allow, makes the conversion throw).
std::string nameOf(const Path& path)
{
    try {
        return core::pathToUtf8(path.filename());
    } catch (const std::exception&) {
        return "?";
    }
}

// ---- the environment ----------------------------------------------------------------------------

#ifdef _WIN32
using NativeText = std::wstring;
#else
using NativeText = std::string;
#endif

struct UserVariable {
    NativeText name;
    NativeText value;
};

constexpr std::array<std::string_view, 3> kOpenSslVariables = {"OPENSSL_CONF", "OPENSSL_MODULES",
                                                               "OPENSSL_ENGINES"};

NativeText nativeFromAscii(std::string_view text)
{
    return NativeText(text.begin(), text.end());
}

std::optional<NativeText> readVariable(const NativeText& name)
{
#ifdef _WIN32
    NativeText value(256, L'\0');
    for (int attempt = 0; attempt < 4; ++attempt) {
        const DWORD size =
            GetEnvironmentVariableW(name.c_str(), value.data(), static_cast<DWORD>(value.size()));
        if (size == 0) {
            return std::nullopt; // not set, or set to nothing, which Windows reads as not set
        }
        if (size < value.size()) {
            value.resize(size);
            return value;
        }
        value.assign(size + 1, L'\0'); // too small: the size needed, then try again
    }
    return std::nullopt;
#else
    const char* value = std::getenv(name.c_str());
    if (value == nullptr || *value == '\0') {
        return std::nullopt;
    }
    return NativeText(value);
#endif
}

void writeVariable(const NativeText& name, const std::optional<NativeText>& value)
{
#ifdef _WIN32
    SetEnvironmentVariableW(name.c_str(), value ? value->c_str() : nullptr);
#else
    if (value) {
        setenv(name.c_str(), value->c_str(), 1);
    } else {
        unsetenv(name.c_str());
    }
#endif
}

// The person's OPENSSL_* values, read BEFORE hardenOpenSslEnvironment() removes them.
std::vector<UserVariable> captureOpenSslVariables()
{
    std::vector<UserVariable> kept;
    for (const std::string_view name : kOpenSslVariables) {
        const NativeText native = nativeFromAscii(name);
        if (const std::optional<NativeText> value = readVariable(native)) {
            kept.push_back({native, *value});
        }
    }
    return kept;
}

bool openSslVariableIsSet(std::string_view name)
{
    return readVariable(nativeFromAscii(name)).has_value();
}


#ifdef _WIN32
// SetErrorMode for the length of a scope.
class ErrorModeScope {
  public:
    explicit ErrorModeScope(UINT mode) : previous_(SetErrorMode(mode)) {}
    ErrorModeScope(const ErrorModeScope&) = delete;
    ErrorModeScope& operator=(const ErrorModeScope&) = delete;
    ~ErrorModeScope() { SetErrorMode(previous_); }

  private:
    UINT previous_;
};
#endif

// The Launcher the swap uses to start the person's Katana (the health check, the relaunch):
// the system one, with the person's OPENSSL_* values lent back to the process for the length of
// the call - the child inherits them - and, for the detached relaunch, the error mode with only
// SEM_FAILCRITICALERRORS (see the top). The helper does one thing at a time, so there is no other
// thread to see the variables while they are back.
class HandOnLauncher final : public upd::Launcher {
  public:
    HandOnLauncher(std::unique_ptr<upd::Launcher> inner, std::vector<UserVariable> variables)
        : inner_(std::move(inner)), variables_(std::move(variables))
    {
    }

    core::Result<upd::SpawnResult> run(const upd::SpawnRequest& request) override
    {
        const Lent lent(variables_);
        return inner_->run(request);
    }

    core::Status startDetached(const upd::SpawnRequest& request) override
    {
        const Lent lent(variables_);
#ifdef _WIN32
        const ErrorModeScope mode(SEM_FAILCRITICALERRORS);
#endif
        return inner_->startDetached(request);
    }

  private:
    class Lent {
      public:
        explicit Lent(const std::vector<UserVariable>& variables) : variables_(variables)
        {
            for (const UserVariable& variable : variables_) {
                writeVariable(variable.name, variable.value);
            }
        }
        Lent(const Lent&) = delete;
        Lent& operator=(const Lent&) = delete;
        ~Lent()
        {
            for (const UserVariable& variable : variables_) {
                writeVariable(variable.name, std::nullopt);
            }
        }

      private:
        const std::vector<UserVariable>& variables_;
    };

    std::unique_ptr<upd::Launcher> inner_;
    std::vector<UserVariable> variables_;
};

// ---- where this program is ----------------------------------------------------------------------

// The absolute path of the running program, which becomes ApplyRequest.helper (the RunOnce
// command names it, and the clean-up list names its folder). The operating system's answer is
// preferred to argv[0]: that is what the caller typed, a bare name found through PATH, in any
// case; the file is what is running. argv[0], made absolute, is the fallback.
Path ownExecutable(const std::string& argv0)
{
#ifdef _WIN32
    std::wstring buffer(260, L'\0');
    while (buffer.size() <= 32768) { // the longest path Windows has
        const DWORD length =
            GetModuleFileNameW(nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
        if (length == 0) {
            break;
        }
        if (length < buffer.size()) {
            buffer.resize(length);
            return Path(buffer);
        }
        buffer.resize(buffer.size() * 2);
    }
#else
    std::string buffer(4096, '\0');
    const ssize_t length = readlink("/proc/self/exe", buffer.data(), buffer.size());
    if (length > 0 && static_cast<std::size_t>(length) < buffer.size()) {
        buffer.resize(static_cast<std::size_t>(length));
        return Path(buffer);
    }
#endif
    std::error_code ec;
    const Path absolute = std::filesystem::absolute(core::pathFromUtf8(argv0), ec);
    return ec ? core::pathFromUtf8(argv0) : absolute;
}

// ---- the command line ---------------------------------------------------------------------------

enum class Mode { Version, SelfTest, Help, Recover, Apply };

// Only the test build fills these in (see the top); a shipped helper never sees them.
struct TestOptions {
    std::uint64_t dieAt = 0;     // 0: never
    std::string registryBase;    // empty: the machine's keys
    std::optional<upd::ApplyTimings> timings;
    std::optional<upd::RetryPolicy> retry;
};

struct Command {
    Mode mode = Mode::Help;
    Path recoverRoot;
    upd::ApplyRequest request;
    TestOptions test;
};

core::Error usageError(std::string message, std::string context = {})
{
    return core::makeError(core::ErrorCode::InvalidArgument, std::move(message),
                           std::move(context));
}

#ifdef KATANA_UPDATER_TEST_BUILD
bool parseCount(std::string_view text, std::uint64_t& value)
{
    if (text.empty() || text.size() > 12) {
        return false;
    }
    value = 0;
    for (const char c : text) {
        if (c < '0' || c > '9') {
            return false;
        }
        value = value * 10 + static_cast<std::uint64_t>(c - '0');
    }
    return true;
}

// Takes --die-at, --registry-base and --timings out of `arguments`, wherever they stand.
core::Status extractTestOptions(std::vector<std::string>& arguments, TestOptions& options)
{
    std::vector<std::string> rest;
    bool sawDieAt = false;
    bool sawBase = false;
    bool sawTimings = false;
    for (std::size_t i = 0; i < arguments.size(); ++i) {
        const std::string& argument = arguments[i];
        const bool isTestOption =
            argument == "--die-at" || argument == "--registry-base" || argument == "--timings";
        if (!isTestOption) {
            rest.push_back(argument);
            continue;
        }
        if (i + 1 >= arguments.size()) {
            return usageError("option without a value", argument);
        }
        const std::string& value = arguments[++i];
        if (argument == "--die-at") {
            if (sawDieAt) {
                return usageError("option given twice", argument);
            }
            sawDieAt = true;
            if (!parseCount(value, options.dieAt) || options.dieAt == 0) {
                return usageError("--die-at is the number of a change to the disk, 1 or more",
                                  value);
            }
        } else if (argument == "--registry-base") {
            if (sawBase) {
                return usageError("option given twice", argument);
            }
            sawBase = true;
#ifdef _WIN32
            if (value.empty() || value.front() == '\\' || value.back() == '\\') {
                return usageError("--registry-base is an HKCU subkey with no leading or trailing "
                                  "backslash",
                                  value);
            }
            options.registryBase = value;
#else
            return usageError("--registry-base exists on Windows only", value);
#endif
        } else {
            if (sawTimings) {
                return usageError("option given twice", argument);
            }
            sawTimings = true;
            std::array<std::uint64_t, 5> milliseconds{};
            std::size_t begin = 0;
            for (std::size_t index = 0; index < milliseconds.size(); ++index) {
                const std::size_t comma = value.find(',', begin);
                const bool last = index + 1 == milliseconds.size();
                if ((comma == std::string::npos) != last) {
                    return usageError(
                        "--timings is five numbers of milliseconds, comma separated", value);
                }
                const std::string_view part =
                    std::string_view(value).substr(begin, last ? std::string_view::npos
                                                                : comma - begin);
                if (!parseCount(part, milliseconds[index])) {
                    return usageError(
                        "--timings is five numbers of milliseconds, comma separated", value);
                }
                begin = comma + 1;
            }
            using ms = std::chrono::milliseconds;
            upd::ApplyTimings timings;
            timings.waitForParent = ms(static_cast<std::int64_t>(milliseconds[0]));
            timings.waitForHolders = ms(static_cast<std::int64_t>(milliseconds[1]));
            timings.pollInterval = ms(static_cast<std::int64_t>(milliseconds[2]));
            timings.healthWait = ms(static_cast<std::int64_t>(milliseconds[3]));
            options.timings = timings;
            upd::RetryPolicy retry;
            retry.total = ms(static_cast<std::int64_t>(milliseconds[4]));
            retry.interval = std::min(retry.interval, std::max(retry.total, ms(1)));
            options.retry = retry;
        }
    }
    arguments = std::move(rest);
    return {};
}
#endif

bool isAbsolute(const Path& path)
{
    return path.is_absolute();
}

// `arguments` is the command line without argv[0].
core::Result<Command> parseCommand(std::vector<std::string> arguments)
{
    Command command;
#ifdef KATANA_UPDATER_TEST_BUILD
    if (const core::Status taken = extractTestOptions(arguments, command.test); !taken.ok()) {
        return taken.error();
    }
#endif
    if (arguments.empty()) {
        return usageError("no command: say what to do");
    }
    const std::string& first = arguments.front();
    if (first == "--version" || first == "--selftest" || first == "--help") {
        if (arguments.size() != 1) {
            return usageError("this option takes no arguments and no other option", first);
        }
        command.mode = first == "--version"    ? Mode::Version
                       : first == "--selftest" ? Mode::SelfTest
                                               : Mode::Help;
        return command;
    }
    if (first == "--recover") {
        if (arguments.size() != 2 || arguments[1].empty()) {
            return usageError("--recover takes one folder: the install folder", "");
        }
        command.mode = Mode::Recover;
        std::error_code ec;
        const Path given = core::pathFromUtf8(arguments[1]);
        command.recoverRoot = std::filesystem::absolute(given, ec);
        if (ec) {
            return usageError("the folder cannot be made absolute", arguments[1]);
        }
        if (!std::filesystem::is_directory(command.recoverRoot, ec)) {
            return usageError("--recover needs an existing folder", arguments[1]);
        }
        return command;
    }

    core::Result<upd::ApplyRequest> request =
        upd::parseHelperArguments(std::span<const std::string>(arguments));
    if (!request.ok()) {
        return request.error();
    }
    command.mode = Mode::Apply;
    command.request = std::move(request).value();
    const upd::ApplyRequest& values = command.request;
    const std::array<std::pair<std::string_view, const Path*>, 3> folders = {
        {{"--root", &values.root},
         {"--stage", &values.stageDirectory},
         {"--state", &values.stateDirectory}}};
    for (const auto& [name, path] : folders) {
        if (!isAbsolute(*path)) {
            return usageError(std::string(name) + " must be an absolute path",
                              core::pathToUtf8(*path));
        }
    }
    if (!values.relaunch.empty() && !isAbsolute(values.relaunch)) {
        return usageError("--relaunch must be an absolute path or none",
                          core::pathToUtf8(values.relaunch));
    }
    return command;
}

// ---- the trust of this build --------------------------------------------------------------------

const upd::Trust& trustForThisBuild()
{
#ifdef KATANA_UPDATER_TEST_BUILD
    return upd::testTrust();
#else
    return upd::productionTrust();
#endif
}

constexpr std::string_view trustKind()
{
#ifdef KATANA_UPDATER_TEST_BUILD
    return "test";
#else
    return "pinned";
#endif
}

// ---- --selftest ---------------------------------------------------------------------------------

// RFC 8032, section 7.1, TEST 3: the public key, the 2-byte message and the signature, as the
// RFC prints them. Checked against the RFC's own text and against an independent derivation
// (tests/update/tool_tests/rfc8032_oracle.py).
constexpr std::string_view kKnownPublicKey =
    "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025";
constexpr std::string_view kKnownMessage = "af82";
constexpr std::string_view kKnownSignature =
    "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac"
    "18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a";

std::optional<std::vector<std::uint8_t>> bytesFromHex(std::string_view hex)
{
    if (hex.size() % 2 != 0) {
        return std::nullopt;
    }
    const auto nibble = [](char c) -> int {
        if (c >= '0' && c <= '9') {
            return c - '0';
        }
        if (c >= 'a' && c <= 'f') {
            return c - 'a' + 10;
        }
        return -1;
    };
    std::vector<std::uint8_t> bytes;
    bytes.reserve(hex.size() / 2);
    for (std::size_t i = 0; i < hex.size(); i += 2) {
        const int high = nibble(hex[i]);
        const int low = nibble(hex[i + 1]);
        if (high < 0 || low < 0) {
            return std::nullopt;
        }
        bytes.push_back(static_cast<std::uint8_t>(high * 16 + low));
    }
    return bytes;
}

int runSelfTest()
{
    std::vector<std::string> problems;
    const auto problem = [&problems](std::string text) { problems.push_back(std::move(text)); };

    // The variables are gone from this process, not merely cleared in a copy of it.
    bool environmentCleared = true;
    for (const std::string_view name : kOpenSslVariables) {
        if (openSslVariableIsSet(name)) {
            environmentCleared = false;
            problem(std::string(name) + " is still set: the environment was not hardened");
        }
    }

    // The signature code, on a published vector: a good signature verifies, and the same
    // signature over a message that differs in one bit is refused for being wrong - not for
    // libcrypto failing to run, which would also "refuse" it.
    bool signatureOk = false;
    const core::Result<upd::PublicKey> key = upd::parsePublicKeyHex(kKnownPublicKey);
    const std::optional<std::vector<std::uint8_t>> signatureBytes = bytesFromHex(kKnownSignature);
    const std::optional<std::vector<std::uint8_t>> messageBytes = bytesFromHex(kKnownMessage);
    if (!key.ok() || !signatureBytes || signatureBytes->size() != 64 || !messageBytes) {
        problem("the built-in test vector is malformed");
    } else {
        upd::SignatureBytes signature{};
        std::copy(signatureBytes->begin(), signatureBytes->end(), signature.begin());
        std::string message(messageBytes->begin(), messageBytes->end());
        const core::Status good = upd::verifyEd25519(key.value(), message, signature);
        message.back() = static_cast<char>(message.back() ^ 0x01);
        const core::Status altered = upd::verifyEd25519(key.value(), message, signature);
        if (!good.ok()) {
            problem("a known good Ed25519 signature did not verify: " + good.error().describe());
        } else if (altered.ok()) {
            problem("an Ed25519 signature over an altered message was accepted");
        } else if (altered.error().code != core::ErrorCode::InvalidArgument) {
            problem("an altered message failed for the wrong reason: " +
                    altered.error().describe());
        } else {
            signatureOk = true;
        }
    }

    // The keys this build pins can be used: a mistyped pin is found here, not at the first update.
    // The placeholders are not keys (trust.hpp) and are reported as what they are.
    const upd::Trust& trust = trustForThisBuild();
    std::string keys(trustKind());
    if (trust.placeholder) {
        keys = "placeholder";
    } else {
        for (const upd::TrustedKey& pinned : trust.keys) {
            if (const core::Status usable = upd::checkPublicKey(pinned.key); !usable.ok()) {
                problem("the pinned key " + pinned.id +
                        " is not usable: " + usable.error().describe());
            }
        }
        if (trust.keys.empty()) {
            problem("no key is pinned");
        }
    }

    const core::Result<upd::Version> version = upd::programVersion();
    if (!version.ok()) {
        problem("the program's version is not a version: " + version.error().describe());
    }

    std::cout << "selftest=" << (problems.empty() ? "ok" : "failed") << '\n'
              << "version=" << upd::programVersionText() << '\n'
              << "applier=" << upd::kApplierProtocol << '\n'
              << "ed25519=" << (signatureOk ? "ok" : "failed") << '\n'
              << "openssl_environment=" << (environmentCleared ? "cleared" : "set") << '\n'
              << "keys=" << keys << '\n';
    for (const std::string& text : problems) {
        std::cout << "problem=" << oneLine(text) << '\n';
    }
    std::cout << std::flush;
    return problems.empty() ? kExitOk : kExitVerification;
}

// ---- the services of a run ----------------------------------------------------------------------

// The real file system, process scan, launcher and registry; in the test build optionally
// wrapped or re-rooted (see the top). Lives for one run.
class Rig {
  public:
    Rig(const TestOptions& test, std::vector<UserVariable> userVariables)
    {
        system_ = upd::makeSystemFsOps(test.retry.value_or(upd::RetryPolicy{}));
        fs_ = system_.get();
#ifdef KATANA_UPDATER_TEST_BUILD
        if (test.dieAt != 0) {
            faulty_.emplace(*system_);
            faulty_->crashBefore(test.dieAt);
            fs_ = &*faulty_;
        }
        if (test.timings) {
            timings_ = *test.timings;
        }
#ifdef _WIN32
        if (!test.registryBase.empty()) {
            registry_ = upd::makeRegistryOpsUnder(test.registryBase);
        }
#endif
#endif
        if (!registry_) {
            registry_ = upd::makeSystemRegistryOps();
        }
        processes_ = upd::makeSystemProcessOps();
        launcher_ = std::make_unique<HandOnLauncher>(upd::makeSystemLauncher(),
                                                     std::move(userVariables));
    }

    [[nodiscard]] upd::FsOps& fs() { return *fs_; }
    [[nodiscard]] upd::ApplyServices services()
    {
        return upd::ApplyServices{*fs_,       *processes_,         *launcher_,
                                  *registry_, trustForThisBuild(), timings_};
    }

  private:
    std::unique_ptr<upd::FsOps> system_;
#ifdef KATANA_UPDATER_TEST_BUILD
    std::optional<upd::FaultFs> faulty_;
#endif
    upd::FsOps* fs_ = nullptr;
    std::unique_ptr<upd::ProcessOps> processes_;
    std::unique_ptr<upd::Launcher> launcher_;
    std::unique_ptr<upd::RegistryOps> registry_;
    upd::ApplyTimings timings_{};
};

// ---- the swap -----------------------------------------------------------------------------------

int runApply(upd::ApplyRequest request, const std::string& argv0, Rig& rig)
{
    request.helper = ownExecutable(argv0);
    const upd::ApplyResult result = upd::apply(request, rig.services(), std::stop_token{});
    const int code = exitCodeFor(result);

    std::cout << "outcome=" << upd::toString(result.outcome) << '\n'
              << "reason=" << result.reason << '\n'
              << "exit=" << code << '\n';
    for (const upd::RunningProgram& holder : result.holders) {
        const std::string name = nameOf(holder.image);
        std::cout << "holder=" << oneLine(name == "?" || name.empty() ? "Katana" : name) << " ("
                  << holder.pid << ")\n";
    }
    std::cout << "detail=" << oneLine(result.detail) << std::endl;
    return code;
}

// ---- --recover ----------------------------------------------------------------------------------

// The names of the transaction folders under R/.update: what an interrupted update leaves, and
// what recovery clears. A folder is looked at by name only; recoverInProcess reads the journals.
std::vector<std::string> transactionFolders(const upd::FsOps& fs, const Path& root)
{
    std::vector<std::string> names;
    const core::Result<std::vector<Path>> entries =
        fs.listDirectory(upd::updateWorkDirectory(root));
    if (!entries.ok()) {
        return names;
    }
    for (const Path& entry : entries.value()) {
        const std::string name = nameOf(entry);
        if (upd::isValidTransactionId(name)) {
            names.push_back(name);
        }
    }
    std::sort(names.begin(), names.end());
    return names;
}

std::string joined(const std::vector<std::string>& names)
{
    std::string text;
    for (const std::string& name : names) {
        text += text.empty() ? name : "," + name;
    }
    return text;
}

int runRecover(const Path& root, Rig& rig)
{
    const std::vector<std::string> found = transactionFolders(rig.fs(), root);
    const core::Status status = upd::recoverInProcess(root, rig.services());
    const std::vector<std::string> left = transactionFolders(rig.fs(), root);

    // One line for a run that had something to put right, in the install's own folder and
    // nowhere else (see the top). A run that found nothing writes nothing: it is what the
    // RunOnce entry does at every logon after a clean update, and a log is not a clock.
    if (!found.empty() || !status.ok()) {
        std::string line = utcNow() + " recover found=" + joined(found) +
                           " result=" + (status.ok() ? "ok" : "failed") + " left=" + joined(left);
        if (!status.ok()) {
            line += " detail=" + oneLine(status.error().describe());
        }
        line += '\n';
        (void)rig.fs().appendDurable(upd::updateWorkDirectory(root) / "recovery.log", line);
    }

    if (!status.ok()) {
        std::cout << "recover=failed\nexit=" << kExitRollbackIncomplete
                  << "\ndetail=" << oneLine(status.error().describe()) << std::endl;
        return kExitRollbackIncomplete;
    }
    // OK is also what a live update holding the lock gets (recover.hpp: not ours to touch), and a
    // finished transaction's folder is kept for its health check: say what stays.
    std::string detail = "nothing was interrupted";
    if (!found.empty()) {
        detail = left.empty() ? "the install is whole"
                              : "left alone (another update holds the lock, or its health check "
                                "is not over): " +
                                    joined(left);
    }
    std::cout << "recover=" << (found.empty() ? "nothing_to_do" : "ok") << "\nexit=" << kExitOk
              << "\ndetail=" << detail << std::endl;
    return kExitOk;
}

// ---- main ---------------------------------------------------------------------------------------

int run(int argc, char** argv)
{
    // 1. The person's OPENSSL_* values, read before they are taken away.
    std::vector<UserVariable> userVariables = captureOpenSslVariables();

    // 2. FIRST, before anything uses libcrypto: nothing in the environment moves the trust root.
    if (const core::Status hardened = upd::hardenOpenSslEnvironment(); !hardened.ok()) {
        std::cerr << "katana_updater: " << hardened.error().describe() << '\n';
        return kExitInternal;
    }

#ifdef _WIN32
    // No "entry point not found" or crash box, here or in the health-check start that inherits
    // this: a new version that cannot load must fail at once, not wait behind a dialog.
    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
#endif

    // 3. The command line, as UTF-8 and never from argv.
    std::vector<std::string> arguments = upd::utf8Arguments(argc, argv);
    const std::string argv0 = arguments.empty() ? std::string() : arguments.front();
    if (!arguments.empty()) {
        arguments.erase(arguments.begin());
    }
    core::Result<Command> parsed = parseCommand(std::move(arguments));
    if (!parsed.ok()) {
        std::cerr << "katana_updater: " << parsed.error().describe() << '\n' << kUsage;
        return kExitUsage;
    }
    const Command& command = parsed.value();

    switch (command.mode) {
    case Mode::Version:
        std::cout << "katana_updater " << upd::programVersionText()
                  << " applier=" << upd::kApplierProtocol << std::endl;
        return kExitOk;
    case Mode::Help:
        std::cout << kUsage << std::flush;
        return kExitOk;
    case Mode::SelfTest:
        return runSelfTest();
    case Mode::Recover:
    case Mode::Apply:
        break;
    }

    Rig rig(command.test, std::move(userVariables));
    return command.mode == Mode::Recover ? runRecover(command.recoverRoot, rig)
                                         : runApply(command.request, argv0, rig);
}

} // namespace

int main(int argc, char** argv)
{
    try {
        return run(argc, argv);
    }
#ifdef KATANA_UPDATER_TEST_BUILD
    catch (const katana::update::SimulatedCrash& crash) {
        // A process that was killed leaves what it had written and nothing else: no destructor
        // runs, no buffer is flushed (a held lock is released by the process ending).
        std::cerr << "katana_updater: simulated crash before change " << crash.operation << ": "
                  << crash.description << '\n';
        std::cerr.flush();
        std::_Exit(kExitSimulatedCrash);
    }
#endif
    catch (const std::exception& error) {
        std::cerr << "katana_updater: unexpected error: " << error.what() << '\n';
        return kExitInternal;
    }
}
