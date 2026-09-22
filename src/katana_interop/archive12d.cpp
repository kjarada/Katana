#include "katana/interop/archive12d.hpp"

#include <algorithm>
#include <cctype>
#include <fstream>
#include <sstream>
#include <system_error>

#include "katana/archive12d/reader.hpp"
#include "katana/archive12d/text_encoding.hpp"
#include "katana/archive12d/writer.hpp"
#include "katana/gis/zip_container.hpp"
#include "katana/interop/import.hpp"

namespace katana::interop {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

std::string lowerExtension(const std::filesystem::path& path)
{
    std::string extension = path.extension().string();
    if (!extension.empty() && extension.front() == '.') {
        extension.erase(0, 1);
    }
    std::transform(extension.begin(), extension.end(), extension.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return extension;
}

// A path as UTF-8. path::string() is the ANSI code page on Windows, and what
// comes out of here goes into entity metadata - where bytes that are not UTF-8
// are refused for the whole import - and to GDAL, which expects UTF-8 too.
std::string utf8(const std::filesystem::path& path)
{
    const std::u8string text = path.u8string();
    return std::string(text.begin(), text.end());
}

Result<std::string> readWholeFile(const std::filesystem::path& path, std::uint64_t maxBytes)
{
    std::error_code error;
    const auto size = std::filesystem::file_size(path, error);
    if (error) {
        return makeError(ErrorCode::FileImportFailure, "the file cannot be read", path.string());
    }
    if (size > maxBytes) {
        return makeError(ErrorCode::FileImportFailure,
                         "the file is larger than the limit of " + std::to_string(maxBytes) +
                             " bytes",
                         path.string());
    }
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return makeError(ErrorCode::FileImportFailure, "the file cannot be opened", path.string());
    }
    std::string bytes(static_cast<std::size_t>(size), '\0');
    file.read(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    if (static_cast<std::uint64_t>(file.gcount()) != size) {
        return makeError(ErrorCode::FileImportFailure, "the file could not be read to its end",
                         path.string());
    }
    return bytes;
}

// The .12da inside a .12daz. 12d Model names it after the project, NOT after
// the zip - "UT5527 Appin Rd V5.12daz" holds "Appin Rd V5.12da" - so the name
// cannot be predicted: it is the one member that is a .12da.
Result<std::string> readZippedArchive(const std::filesystem::path& path, std::uint64_t maxBytes,
                                      std::string& memberName)
{
    auto members = katana::gis::listZip(path);
    if (!members) {
        return members.error();
    }
    std::vector<std::string> candidates;
    for (const katana::gis::ZipMember& member : *members) {
        if (lowerExtension(std::filesystem::path(member.name)) == "12da") {
            candidates.push_back(member.name);
        }
    }
    if (candidates.empty()) {
        return makeError(ErrorCode::FileImportFailure, "the archive holds no .12da file",
                         path.string());
    }
    if (candidates.size() > 1) {
        // Which one was meant is not ours to guess; importing the first and
        // saying nothing would silently drop the rest.
        return makeError(ErrorCode::Unsupported,
                         "the archive holds " + std::to_string(candidates.size()) +
                             " .12da files; unzip it and import them one at a time",
                         path.string());
    }
    memberName = candidates.front();
    return katana::gis::readZipMember(path, memberName, maxBytes);
}

PointCloudLayer toLayer(const katana::archive12d::ImportedCloud& cloud,
                        const std::filesystem::path& source)
{
    PointCloudLayer layer;
    layer.name = cloud.name;
    layer.source = source;
    layer.points.reserve(cloud.points.size());
    for (const katana::archive12d::ImportedCloudPoint& point : cloud.points) {
        katana::pointcloud::PointCloudPoint out;
        out.x = point.x;
        out.y = point.y;
        out.z = point.z;
        out.intensity = point.intensity;
        out.classification = point.classification;
        layer.points.push_back(out);
        layer.bounds.minX = std::min(layer.bounds.minX, point.x);
        layer.bounds.minY = std::min(layer.bounds.minY, point.y);
        layer.bounds.minZ = std::min(layer.bounds.minZ, point.z);
        layer.bounds.maxX = std::max(layer.bounds.maxX, point.x);
        layer.bounds.maxY = std::max(layer.bounds.maxY, point.y);
        layer.bounds.maxZ = std::max(layer.bounds.maxZ, point.z);
    }
    layer.sourcePointCount = cloud.points.size();
    return layer;
}

// The LAS file a ref_data cloud names, if it sits beside the archive.
// Only the file NAME is taken from the reference: a 12da records something
// like "..\..\scans\site.las", and following that out of the directory the
// archive was found in would let a file choose what gets read.
std::optional<PointCloudLayer> referencedCloud(const std::filesystem::path& archive,
                                               const std::string& reference)
{
    if (reference.empty()) {
        return std::nullopt;
    }
    std::error_code ignored;
    const std::filesystem::path beside =
        archive.parent_path() / std::filesystem::path(reference).filename();
    if (!std::filesystem::is_regular_file(beside, ignored)) {
        return std::nullopt;
    }
    auto cloud = importPointCloud(beside);
    if (!cloud) {
        return std::nullopt;
    }
    return std::move(*cloud);
}

} // namespace

std::vector<std::string> archive12dExtensions()
{
    return {"12da", "12daz"};
}

bool isZippedArchive12d(const std::filesystem::path& path)
{
    const std::string extension = lowerExtension(path);
    return extension == "12daz";
}

Result<Archive12dImportResult> importArchive12d(const std::filesystem::path& path,
                                                const Archive12dImportOptions& options)
{
    Archive12dImportResult result;
    auto bytes = isZippedArchive12d(path)
                     ? readZippedArchive(path, options.maxBytes, result.memberName)
                     : readWholeFile(path, options.maxBytes);
    if (!bytes) {
        return bytes.error();
    }
    auto decoded = katana::archive12d::decodeText(*bytes);
    if (!decoded) {
        return makeError(decoded.error().code, decoded.error().message, path.string());
    }
    bytes->clear();
    bytes->shrink_to_fit(); // a 58 MB UTF-16 file is not needed beside its decoded text
    result.encoding = katana::archive12d::toString(decoded->encoding);

    auto archive = katana::archive12d::readArchive(decoded->text);
    if (!archive) {
        const std::string where =
            archive.error().context.empty() ? std::string() : " [" + archive.error().context + "]";
        return makeError(archive.error().code, archive.error().message,
                         path.filename().string() + where);
    }
    for (const katana::archive12d::Field& setting : archive->headerSettings) {
        if (setting.key == "archive_version") {
            result.archiveVersion = setting.value;
        }
    }

    katana::archive12d::ImportOptions mapping;
    mapping.originShift = options.originShift;
    mapping.layerPrefix = options.layerPrefix;
    mapping.curveTolerance = options.curveTolerance;
    mapping.sourceName = utf8(path.filename());
    auto domain = katana::archive12d::toDomain(*archive, mapping);
    if (!domain) {
        return domain.error();
    }

    result.entities = std::move(domain->entities);
    result.layersNeeded = std::move(domain->layersNeeded);
    result.stylesNeeded = std::move(domain->stylesNeeded);
    result.alignments = std::move(domain->alignments);
    result.surfaces = std::move(domain->surfaces);
    result.meshes = std::move(domain->meshes);
    result.tally = std::move(domain->tally);
    result.bounds = domain->bounds;
    result.warnings = std::move(domain->warnings);
    if (decoded->guessed) {
        result.warnings.insert(result.warnings.begin(),
                               "the file has no byte order mark; it was read as " +
                                   result.encoding);
    }
    for (const katana::archive12d::ImportedCloud& cloud : domain->clouds) {
        if (cloud.points.empty()) {
            // A ref_data cloud names a LAS file rather than holding points,
            // by a path relative to a 12d project this archive has left
            // behind. The file is LOOKED FOR beside the archive - where a
            // 12da and its scans travel together when they are sent on -
            // and imported when it is there. It is not chased any further
            // than that: a relative path out of a project directory that no
            // longer exists would have this reading arbitrary files from
            // wherever the archive happens to sit.
            if (auto found = referencedCloud(path, cloud.referenceFile)) {
                found->name = cloud.name.empty() ? found->name : cloud.name;
                result.warnings.push_back("point cloud \"" + cloud.name + "\" was read from '" +
                                          cloud.referenceFile + "', found beside the archive");
                result.clouds.push_back(std::move(*found));
                continue;
            }
            result.warnings.push_back("point cloud \"" + cloud.name + "\" refers to the file '" +
                                      cloud.referenceFile + "', which is not beside the archive; "
                                      "import it separately");
            continue;
        }
        result.clouds.push_back(toLayer(cloud, path));
    }
    for (const katana::archive12d::SuperTin& superTin : domain->superTins) {
        std::string members;
        for (const std::string& name : superTin.tins) {
            members += (members.empty() ? "" : ", ") + name;
        }
        result.warnings.push_back("super tin \"" + superTin.name + "\" ranks the tins " + members +
                                  "; they are imported as separate surfaces");
    }
    return result;
}

Result<Archive12dExportResult>
exportArchive12d(const katana::entity::Model& model,
                 const std::vector<katana::archive12d::ExportSurface>& surfaces,
                 const std::filesystem::path& path, const Archive12dExportOptions& options,
                 const std::vector<katana::archive12d::ExportMesh>& meshes)
{
    const std::string extension = lowerExtension(path);
    const auto known = archive12dExtensions();
    if (std::find(known.begin(), known.end(), extension) == known.end()) {
        return makeError(ErrorCode::InvalidArgument,
                         "a 12d archive must be named .12da or .12daz", path.string());
    }

    katana::archive12d::ExportOptions mapping;
    mapping.entities = options.entities;
    mapping.originShift = options.originShift;
    auto domain = katana::archive12d::fromDomain(model, surfaces, mapping, meshes);
    if (!domain) {
        return domain.error();
    }

    katana::archive12d::WriteOptions writing;
    writing.decimalPlaces = options.decimalPlaces;
    writing.banner = "Written by Katana\nexport_file_name : " + utf8(path.filename());
    std::string text = katana::archive12d::writeArchive(domain->archive, writing);
    if (options.utf16) {
        auto encoded = katana::archive12d::encodeUtf16LittleEndian(text);
        if (!encoded) {
            return makeError(ErrorCode::FileExportFailure, encoded.error().message, path.string());
        }
        text = std::move(*encoded);
    }

    if (isZippedArchive12d(path)) {
        std::filesystem::path member = path.filename();
        member.replace_extension(".12da");
        if (auto status = katana::gis::writeZip(path, utf8(member), text); !status) {
            return status.error();
        }
    } else {
        std::ofstream file(path, std::ios::binary | std::ios::trunc);
        file.write(text.data(), static_cast<std::streamsize>(text.size()));
        file.close();
        if (!file) {
            return makeError(ErrorCode::FileExportFailure, "the file could not be written",
                             path.string());
        }
    }

    Archive12dExportResult result;
    result.entitiesWritten = domain->entitiesWritten;
    result.entitiesSkipped = domain->entitiesSkipped;
    result.alignmentsWritten = domain->alignmentsWritten;
    result.surfacesWritten = domain->surfacesWritten;
    result.bytesWritten = text.size();
    result.warnings = std::move(domain->warnings);
    return result;
}

} // namespace katana::interop
