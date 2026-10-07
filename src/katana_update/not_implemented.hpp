#pragma once

// What the skeleton's stubs say. INTERNAL to the update library and never
// installed. Every source file of the library that a later task owns starts as
// a stub built from this header; the task that fills the file deletes its
// stub, and the last task to need nothing here deletes this header.
//
// Two ways to say "not yet", by what the function returns:
//   - Result<T> / Status: an Unsupported error, "not implemented: <what>", with
//     the owning task in the context. A caller sees a refusal with a reason,
//     never a value that looks real.
//   - anything else (a string, a path, a vector): there is no way to carry an
//     error, so it throws std::logic_error - the project's rule for misuse
//     (core/error.hpp) - rather than return something plausible and wrong.
//
// The Unimplemented* classes are the system seams (fsops.hpp, process.hpp,
// registry.hpp, credential.hpp) with every operation refused, so that
// makeSystemFsOps() and its siblings link and run in the skeleton and say what
// they are.

#include <filesystem>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/update/credential.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/process.hpp"
#include "katana/update/registry.hpp"

namespace katana::update::detail {

using Path = std::filesystem::path;

[[nodiscard]] inline core::Error notImplemented(std::string_view what, std::string_view task)
{
    return core::makeError(core::ErrorCode::Unsupported, "not implemented: " + std::string(what),
                           "update skeleton; task " + std::string(task));
}

[[noreturn]] inline void notYet(std::string_view what, std::string_view task)
{
    throw std::logic_error("not implemented: " + std::string(what) + " (update skeleton; task " +
                           std::string(task) + ")");
}

class UnimplementedFsOps final : public FsOps {
  public:
    explicit UnimplementedFsOps(std::string_view task) : task_(task) {}

    [[nodiscard]] core::Result<EntryInfo> stat(const Path&) const override
    {
        return no("FsOps::stat");
    }
    [[nodiscard]] core::Result<std::vector<Path>> listDirectory(const Path&) const override
    {
        return no("FsOps::listDirectory");
    }
    [[nodiscard]] core::Result<std::string> readFile(const Path&) const override
    {
        return no("FsOps::readFile");
    }
    [[nodiscard]] core::Result<Path> resolve(const Path&) const override
    {
        return no("FsOps::resolve");
    }
    [[nodiscard]] core::Result<std::uint64_t> freeSpace(const Path&) const override
    {
        return no("FsOps::freeSpace");
    }
    [[nodiscard]] core::Status makeDirectories(const Path&) override
    {
        return no("FsOps::makeDirectories");
    }
    [[nodiscard]] core::Status copyFile(const Path&, const Path&) override
    {
        return no("FsOps::copyFile");
    }
    [[nodiscard]] core::Status renameFile(const Path&, const Path&) override
    {
        return no("FsOps::renameFile");
    }
    [[nodiscard]] core::Status removeFile(const Path&) override
    {
        return no("FsOps::removeFile");
    }
    [[nodiscard]] core::Status removeEmptyDirectory(const Path&) override
    {
        return no("FsOps::removeEmptyDirectory");
    }
    [[nodiscard]] core::Status appendDurable(const Path&, std::string_view) override
    {
        return no("FsOps::appendDurable");
    }
    [[nodiscard]] core::Status writeDurable(const Path&, std::string_view) override
    {
        return no("FsOps::writeDurable");
    }
    [[nodiscard]] core::Status setExecutable(const Path&) override
    {
        return no("FsOps::setExecutable");
    }
    [[nodiscard]] core::Status syncDirectory(const Path&) override
    {
        return no("FsOps::syncDirectory");
    }
    [[nodiscard]] core::Result<std::unique_ptr<FileLock>> tryLock(const Path&) override
    {
        return no("FsOps::tryLock");
    }

  private:
    [[nodiscard]] core::Error no(std::string_view what) const
    {
        return notImplemented(what, task_);
    }
    std::string task_;
};

class UnimplementedProcessOps final : public ProcessOps {
  public:
    explicit UnimplementedProcessOps(std::string_view task) : task_(task) {}

    [[nodiscard]] std::uint64_t currentPid() const override
    {
        notYet("ProcessOps::currentPid", task_);
    }
    [[nodiscard]] bool isAlive(std::uint64_t) const override
    {
        notYet("ProcessOps::isAlive", task_);
    }
    [[nodiscard]] core::Result<std::vector<RunningProgram>>
    holdersUnder(const Path&) const override
    {
        return notImplemented("ProcessOps::holdersUnder", task_);
    }

  private:
    std::string task_;
};

class UnimplementedLauncher final : public Launcher {
  public:
    explicit UnimplementedLauncher(std::string_view task) : task_(task) {}

    [[nodiscard]] core::Result<SpawnResult> run(const SpawnRequest&) override
    {
        return notImplemented("Launcher::run", task_);
    }
    [[nodiscard]] core::Status startDetached(const SpawnRequest&) override
    {
        return notImplemented("Launcher::startDetached", task_);
    }

  private:
    std::string task_;
};

class UnimplementedRegistryOps final : public RegistryOps {
  public:
    explicit UnimplementedRegistryOps(std::string_view task) : task_(task) {}

    [[nodiscard]] core::Result<std::optional<Path>> installerLocation() const override
    {
        return notImplemented("RegistryOps::installerLocation", task_);
    }
    [[nodiscard]] core::Status setRunOnce(std::string_view, std::string_view) override
    {
        return notImplemented("RegistryOps::setRunOnce", task_);
    }
    [[nodiscard]] core::Status clearRunOnce(std::string_view) override
    {
        return notImplemented("RegistryOps::clearRunOnce", task_);
    }
    [[nodiscard]] core::Status setInstalledVersion(std::string_view, std::string_view) override
    {
        return notImplemented("RegistryOps::setInstalledVersion", task_);
    }

  private:
    std::string task_;
};

class UnimplementedCredentialStore final : public CredentialStore {
  public:
    UnimplementedCredentialStore(std::string_view where, std::string_view task)
        : where_(where), task_(task)
    {
    }

    [[nodiscard]] core::Result<std::string> read() const override
    {
        return notImplemented("CredentialStore::read", task_);
    }
    [[nodiscard]] core::Status write(std::string_view) override
    {
        return notImplemented("CredentialStore::write", task_);
    }
    [[nodiscard]] core::Status clear() override
    {
        return notImplemented("CredentialStore::clear", task_);
    }
    [[nodiscard]] std::string_view where() const override { return where_; }

  private:
    std::string where_;
    std::string task_;
};

} // namespace katana::update::detail
