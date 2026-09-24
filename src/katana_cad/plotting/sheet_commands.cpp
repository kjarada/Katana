#include "katana/cad/plotting/sheet_commands.hpp"

#include <array>
#include <format>
#include <fstream>
#include <iterator>
#include <system_error>
#include <utility>

#include "katana/cad/plotting/generators.hpp"

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

Status outOfRange(std::size_t index, std::size_t count)
{
    return makeError(ErrorCode::NotFound, "no sheet at that position",
                     std::to_string(index) + " of " + std::to_string(count));
}

// The image formats a logo may be, told by their first bytes rather than by
// the file's extension, and the extension it is stored under.
std::optional<std::string_view> imageExtension(std::string_view bytes)
{
    if (bytes.starts_with("\x89PNG\r\n\x1a\n")) {
        return ".png";
    }
    if (bytes.starts_with("\xff\xd8\xff")) {
        return ".jpg";
    }
    if (bytes.starts_with("GIF87a") || bytes.starts_with("GIF89a")) {
        return ".gif";
    }
    if (bytes.starts_with("BM")) {
        return ".bmp";
    }
    return std::nullopt;
}

// The source file's name reduced to letters, digits, '-' and '_', so the
// stored name is safe on every file system a project travels to.
std::string safeStem(const std::filesystem::path& image)
{
    std::string stem;
    for (const char c : image.stem().string()) {
        const bool plain = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
                           (c >= '0' && c <= '9') || c == '-' || c == '_';
        stem += plain ? c : '_';
    }
    return stem.empty() ? std::string("logo") : stem;
}

Result<std::string> readAll(const std::filesystem::path& path)
{
    std::ifstream in(path, std::ios::binary);
    if (!in) {
        return makeError(ErrorCode::NotFound, "the image cannot be opened", path.string());
    }
    std::string bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    if (in.bad()) {
        return makeError(ErrorCode::FileImportFailure, "the image cannot be read", path.string());
    }
    return bytes;
}

} // namespace

Status addSheets(Document& document, std::vector<Sheet> sheets, std::string stepName)
{
    SheetSet set = document.sheetSet();
    prepareForAppend(set, sheets);
    for (Sheet& sheet : sheets) {
        set.sheets.push_back(std::move(sheet));
    }
    return document.setSheetSet(set, std::move(stepName));
}

Status addSheet(Document& document, Sheet sheet, std::optional<std::size_t> at)
{
    SheetSet set = document.sheetSet();
    const std::size_t position = at.value_or(set.sheets.size());
    if (position > set.sheets.size()) {
        return outOfRange(position, set.sheets.size());
    }
    std::vector<Sheet> one{std::move(sheet)};
    prepareForAppend(set, one);
    set.sheets.insert(set.sheets.begin() + static_cast<std::ptrdiff_t>(position),
                      std::move(one.front()));
    return document.setSheetSet(set, "ADD_SHEET");
}

Status removeSheet(Document& document, std::size_t index)
{
    SheetSet set = document.sheetSet();
    if (index >= set.sheets.size()) {
        return outOfRange(index, set.sheets.size());
    }
    set.sheets.erase(set.sheets.begin() + static_cast<std::ptrdiff_t>(index));
    return document.setSheetSet(set, "REMOVE_SHEET");
}

Status moveSheet(Document& document, std::size_t from, std::size_t to)
{
    SheetSet set = document.sheetSet();
    if (from >= set.sheets.size()) {
        return outOfRange(from, set.sheets.size());
    }
    if (to >= set.sheets.size()) {
        return outOfRange(to, set.sheets.size());
    }
    Sheet moving = std::move(set.sheets[from]);
    set.sheets.erase(set.sheets.begin() + static_cast<std::ptrdiff_t>(from));
    set.sheets.insert(set.sheets.begin() + static_cast<std::ptrdiff_t>(to), std::move(moving));
    return document.setSheetSet(set, "MOVE_SHEET");
}

Status duplicateSheet(Document& document, std::size_t index)
{
    SheetSet set = document.sheetSet();
    if (index >= set.sheets.size()) {
        return outOfRange(index, set.sheets.size());
    }
    Sheet copy = set.sheets[index];
    copy.name += " (copy)";
    // A typed sheet number belongs to the sheet it was typed on; two sheets
    // saying the same number is exactly what automatic numbering prevents.
    copy.fields.erase("sheet_number");
    copy.id.clear();
    for (Viewport& viewport : copy.viewports) {
        viewport.id.clear();
    }
    std::vector<Sheet> one{std::move(copy)};
    prepareForAppend(set, one);
    set.sheets.insert(set.sheets.begin() + static_cast<std::ptrdiff_t>(index + 1),
                      std::move(one.front()));
    return document.setSheetSet(set, "DUPLICATE_SHEET");
}

Status editSheet(Document& document, std::size_t index,
                 const std::function<Status(Sheet&)>& edit, std::string stepName)
{
    SheetSet set = document.sheetSet();
    if (index >= set.sheets.size()) {
        return outOfRange(index, set.sheets.size());
    }
    if (Status status = edit(set.sheets[index]); !status) {
        return status;
    }
    return document.setSheetSet(set, std::move(stepName));
}

Status editViewport(Document& document, std::string_view viewportId,
                    const std::function<Status(Viewport&)>& edit, std::string stepName)
{
    SheetSet set = document.sheetSet();
    for (Sheet& sheet : set.sheets) {
        for (Viewport& viewport : sheet.viewports) {
            if (viewport.id == viewportId) {
                if (Status status = edit(viewport); !status) {
                    return status;
                }
                return document.setSheetSet(set, std::move(stepName));
            }
        }
    }
    return makeError(ErrorCode::NotFound, "no viewport with that id", std::string(viewportId));
}

Status editSheetSet(Document& document, const std::function<Status(SheetSet&)>& edit,
                    std::string stepName)
{
    SheetSet set = document.sheetSet();
    if (Status status = edit(set); !status) {
        return status;
    }
    return document.setSheetSet(set, std::move(stepName));
}

Result<std::string> importLogo(Document& document, const std::filesystem::path& image)
{
    const auto project = document.projectDirectory();
    if (!project) {
        return makeError(ErrorCode::InvalidState,
                         "save the drawing as a project first: the logo is kept in the project's "
                         "assets folder");
    }
    std::error_code error;
    const auto size = std::filesystem::file_size(image, error);
    if (error) {
        return makeError(ErrorCode::NotFound, "the image cannot be found", image.string());
    }
    if (size > kMaximumLogoBytes) {
        return makeError(ErrorCode::InvalidArgument,
                         "the image is too large for a logo (at most 4 MB)",
                         image.string() + ": " + std::to_string(size) + " bytes");
    }
    auto bytes = readAll(image);
    if (!bytes) {
        return bytes.error();
    }
    const auto extension = imageExtension(*bytes);
    if (!extension) {
        return makeError(ErrorCode::Unsupported, "a logo must be a PNG, JPEG, GIF or BMP image",
                         image.string());
    }
    const std::filesystem::path assets = *project / "assets";
    std::filesystem::create_directories(assets, error);
    if (error) {
        return makeError(ErrorCode::FileExportFailure, "the project's assets folder cannot be made",
                         assets.string() + ": " + error.message());
    }
    // A name of its own, never overwriting another file: an undo may bring
    // back the logo a later import would otherwise have replaced. The same
    // image imported twice is found and reused.
    const std::string stem = safeStem(image);
    std::string name;
    for (int attempt = 1;; ++attempt) {
        name = attempt == 1 ? stem + std::string(*extension)
                            : std::format("{}-{}{}", stem, attempt, *extension);
        const std::filesystem::path target = assets / name;
        if (!std::filesystem::exists(target)) {
            std::ofstream out(target, std::ios::binary);
            out.write(bytes->data(), static_cast<std::streamsize>(bytes->size()));
            out.close();
            if (!out) {
                return makeError(ErrorCode::FileExportFailure, "the logo cannot be written",
                                 target.string());
            }
            break;
        }
        auto existing = readAll(target);
        if (existing && *existing == *bytes) {
            break;
        }
    }
    const Status status = editSheetSet(
        document,
        [&name](SheetSet& set) -> Status {
            set.defaults.logoAsset = name;
            return {};
        },
        "SET_LOGO");
    if (!status) {
        return status.error();
    }
    return name;
}

std::optional<std::filesystem::path> logoPath(const Document& document)
{
    const std::string& name = document.sheetSet().defaults.logoAsset;
    const auto project = document.projectDirectory();
    if (name.empty() || !project) {
        return std::nullopt;
    }
    return *project / "assets" / name;
}

FieldContext fieldContextFor(const Document& document, std::string plotDate)
{
    FieldContext context;
    const auto& metadata = document.metadata();
    context.projectName = metadata.name;
    context.projectDescription = metadata.description;
    context.coordinateSystem = metadata.coordinateSystem;
    const auto project = document.projectDirectory();
    context.fileName = project ? project->filename().string() : metadata.name;
    context.plotDate = std::move(plotDate);
    return context;
}

std::string frameDate(std::chrono::year_month_day date)
{
    return std::format("{:02}/{:02}/{:02}", static_cast<unsigned>(date.day()),
                       static_cast<unsigned>(date.month()),
                       static_cast<int>(date.year()) % 100);
}

} // namespace katana::cad::plotting
