#include "shader_compiler.hpp"

#include <array>
#include <cstring>
#include <map>
#include <mutex>
#include <tuple>

#include <d3dcompiler.h>

namespace katana::qt::gpu {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;

[[nodiscard]] const char* targetFor(QShader::Stage stage)
{
    switch (stage) {
    case QShader::VertexStage:
        return "vs_5_0";
    case QShader::GeometryStage:
        return "gs_5_0";
    case QShader::FragmentStage:
        return "ps_5_0";
    default:
        return nullptr;
    }
}

// Releases a COM blob on every path out.
struct BlobHolder {
    ID3DBlob* blob = nullptr;
    ~BlobHolder()
    {
        if (blob != nullptr) {
            blob->Release();
        }
    }
};

// The process's compiled stages. Keyed by the source text itself, not by
// (program, expansion, stage): two libraries asking for the same text share
// the bytecode, and a changed source can never be answered from a stale entry.
struct Cache {
    std::mutex mutex;
    std::map<std::tuple<std::string, int>, QByteArray> bytecode;
};

Cache& cache()
{
    static Cache instance;
    return instance;
}

[[nodiscard]] katana::core::Result<QShader> compiledShader(QShader::Stage stage, const char* source)
{
    const std::tuple<std::string, int> key{source, static_cast<int>(stage)};
    QByteArray bytecode;
    {
        std::lock_guard lock(cache().mutex);
        if (auto found = cache().bytecode.find(key); found != cache().bytecode.end()) {
            bytecode = found->second;
        }
    }
    if (bytecode.isEmpty()) {
        // Compiled outside the lock, so two threads warming different stages
        // do not wait for each other; if both compile the same stage, the
        // results are identical and either may win.
        auto compiled = compileHlsl(source, stage);
        if (!compiled) {
            return compiled.error();
        }
        bytecode = *compiled;
        std::lock_guard lock(cache().mutex);
        cache().bytecode.emplace(key, bytecode);
    }
    QShader shader;
    shader.setStage(stage);
    // Bytecode first - QRhi's Direct3D 11 backend takes DXBC when there is
    // some and compiles the source only when there is not - and the source
    // beside it, so a reader of the QShader sees what the bytecode came from.
    shader.setShader(QShaderKey(QShader::DxbcShader, QShaderVersion(50)), QShaderCode(bytecode));
    shader.setShader(QShaderKey(QShader::HlslShader, QShaderVersion(50)),
                     QShaderCode(QByteArray(source), QByteArrayLiteral("main")));
    return shader;
}

class CompiledHlslLibrary final : public ShaderLibrary {
  public:
    [[nodiscard]] katana::core::Result<ShaderStages> program(Program program,
                                                             Expansion expansion) const override
    {
        ShaderStages stages;
        auto vertex = compiledShader(QShader::VertexStage,
                                     hlslSource(program, expansion, QShader::VertexStage));
        if (!vertex) {
            return vertex.error();
        }
        stages.vertex = *vertex;
        if (const char* geometry = hlslSource(program, expansion, QShader::GeometryStage);
            *geometry != '\0') {
            auto compiled = compiledShader(QShader::GeometryStage, geometry);
            if (!compiled) {
                return compiled.error();
            }
            stages.geometry = *compiled;
        }
        auto fragment = compiledShader(QShader::FragmentStage,
                                       hlslSource(program, expansion, QShader::FragmentStage));
        if (!fragment) {
            return fragment.error();
        }
        stages.fragment = *fragment;
        return stages;
    }
    [[nodiscard]] std::string describe() const override
    {
        return "HLSL compiled once per process to Direct3D bytecode";
    }
};

} // namespace

katana::core::Result<QByteArray> compileHlsl(const char* source, QShader::Stage stage)
{
    const char* target = targetFor(stage);
    if (source == nullptr || *source == '\0' || target == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "no HLSL source, or a stage it cannot compile");
    }
    BlobHolder code;
    BlobHolder errors;
    const HRESULT result =
        D3DCompile(source, std::strlen(source), nullptr, nullptr, nullptr, "main", target,
                   D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, &code.blob, &errors.blob);
    if (FAILED(result) || code.blob == nullptr) {
        std::string message = "the ";
        message += target;
        message += " shader did not compile";
        if (errors.blob != nullptr) {
            message += ": ";
            message.append(static_cast<const char*>(errors.blob->GetBufferPointer()),
                           errors.blob->GetBufferSize());
            while (!message.empty() && (message.back() == '\0' || message.back() == '\n')) {
                message.pop_back();
            }
        }
        return makeError(ErrorCode::RenderingFailure, message);
    }
    return QByteArray(static_cast<const char*>(code.blob->GetBufferPointer()),
                      static_cast<qsizetype>(code.blob->GetBufferSize()));
}

const ShaderLibrary& compiledHlslShaders()
{
    static const CompiledHlslLibrary library;
    return library;
}

const ShaderLibrary& defaultShaders() { return compiledHlslShaders(); }

katana::core::Status precompileHlslShaders(Expansion expansion)
{
    for (const Program program : kAllPrograms) {
        if (auto stages = compiledHlslShaders().program(program, expansion); !stages) {
            return stages.error();
        }
    }
    return {};
}

} // namespace katana::qt::gpu
