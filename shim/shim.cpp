// Cubism Native Framework 的 Rust FFI 薄封装。
// 职责：加载 model3/moc3/纹理/动作，驱动每帧更新与 D3D11 绘制。
// 不含任何 Win32 窗口逻辑（窗口由 Rust 侧管理）。

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include <windows.h>
#include <CubismFramework.hpp>
#include <ICubismAllocator.hpp>
#include <CubismModelSettingJson.hpp>
#include <Id/CubismIdManager.hpp>
#include <Model/CubismUserModel.hpp>
#include <Motion/CubismMotion.hpp>
#include <Physics/CubismPhysics.hpp>
#include <Rendering/D3D11/CubismRenderer_D3D11.hpp>
#include <Rendering/D3D11/CubismDeviceInfo_D3D11.hpp>
#include <Utils/CubismString.hpp>

#define STB_IMAGE_IMPLEMENTATION
#include "stb_image.h"

namespace {

// ---------- 内存分配器 ----------
class ShimAllocator : public Csm::ICubismAllocator {
public:
    void* Allocate(const Csm::csmSizeType size) override { return std::malloc(size); }
    void Deallocate(void* memory) override { std::free(memory); }
    void* AllocateAligned(const Csm::csmSizeType size, const Csm::csmUint32 alignment) override {
        const std::size_t offset = alignment - 1 + sizeof(void*);
        void* allocation = std::malloc(size + offset);
        if (!allocation) return nullptr;
        char* aligned = static_cast<char*>(allocation) + sizeof(void*);
        const std::size_t shift = reinterpret_cast<std::size_t>(aligned) & (alignment - 1);
        if (shift != 0) aligned += (alignment - shift);
        reinterpret_cast<void**>(aligned)[-1] = allocation;
        return aligned;
    }
    void DeallocateAligned(void* alignedMemory) override {
        std::free(reinterpret_cast<void**>(alignedMemory)[-1]);
    }
};

ShimAllocator g_allocator;
// 注意：CubismFramework::StartUp 会保存 Option 指针，必须保证其生命周期贯穿整个运行期。
Csm::CubismFramework::Option g_option;
bool g_started = false;
ID3D11Device* g_device = nullptr;
ID3D11DeviceContext* g_context = nullptr;

// ---------- 文件 IO（供 Cubism Framework 加载 shader） ----------
Csm::csmByte* LoadFile(const std::string filePath, Csm::csmSizeInt* outSize) {
    FILE* fp = nullptr;
    if (fopen_s(&fp, filePath.c_str(), "rb") != 0 || !fp) {
        return nullptr;
    }
    fseek(fp, 0, SEEK_END);
    const long fileSize = ftell(fp);
    fseek(fp, 0, SEEK_SET);
    if (fileSize <= 0) {
        fclose(fp);
        return nullptr;
    }
    Csm::csmByte* data = static_cast<Csm::csmByte*>(std::malloc(static_cast<std::size_t>(fileSize)));
    const std::size_t read = fread(data, 1, static_cast<std::size_t>(fileSize), fp);
    fclose(fp);
    if (read != static_cast<std::size_t>(fileSize)) {
        std::free(data);
        return nullptr;
    }
    if (outSize) *outSize = static_cast<Csm::csmSizeInt>(fileSize);
    fprintf(stderr, "[shim] LoadFile ok: %s (%ld bytes)\n", filePath.c_str(), fileSize); fflush(stderr);
    return data;
}

void ReleaseBytes(Csm::csmByte* data) { std::free(data); }

// Cubism 内部日志回调（转发到 stderr，便于诊断）
void CubismLog(const char* message) {
    fprintf(stderr, "[cubism] %s", message ? message : "(null)");
    fflush(stderr);
}

// SEH 异常过滤器：定位访问违例等崩溃
LONG WINAPI ShimExceptionFilter(EXCEPTION_POINTERS* ep) {
    fprintf(stderr, "[shim] !!! EXCEPTION code=0x%08X addr=%p\n",
        (unsigned)ep->ExceptionRecord->ExceptionCode,
        ep->ExceptionRecord->ExceptionAddress);
    fflush(stderr);
    return EXCEPTION_EXECUTE_HANDLER;
}

std::string DirectoryOf(const std::string& path) {
    const std::size_t pos = path.find_last_of("/\\");
    return (pos == std::string::npos) ? std::string() : path.substr(0, pos + 1);
}

// ---------- 模型封装 ----------
class ModelWrapper : public Csm::CubismUserModel {
public:
    ModelWrapper() = default;
    ~ModelWrapper() override { Release(); }

    bool Load(const std::string& model3Path, int width, int height,
              float scale, float offsetX, float offsetY) {
        _dir = DirectoryOf(model3Path);
        fprintf(stderr, "[shim]   dir=%s\n", _dir.c_str()); fflush(stderr);

        Csm::csmSizeInt size = 0;
        Csm::csmByte* buffer = LoadFile(model3Path, &size);
        if (!buffer) { fprintf(stderr, "[shim]   model3 read failed\n"); fflush(stderr); return false; }
        _modelJson = new Csm::CubismModelSettingJson(buffer, size);
        ReleaseBytes(buffer);
        fprintf(stderr, "[shim]   model3 parsed\n"); fflush(stderr);

        // moc3
        const std::string mocPath = _dir + _modelJson->GetModelFileName();
        fprintf(stderr, "[shim]   moc=%s\n", mocPath.c_str()); fflush(stderr);
        buffer = LoadFile(mocPath, &size);
        if (!buffer) { fprintf(stderr, "[shim]   moc read failed\n"); fflush(stderr); return false; }
        LoadModel(buffer, size);
        ReleaseBytes(buffer);
        if (!GetModel()) { fprintf(stderr, "[shim]   LoadModel failed\n"); fflush(stderr); return false; }
        fprintf(stderr, "[shim]   moc loaded\n"); fflush(stderr);

        // 渲染器
        fprintf(stderr, "[shim]   create renderer (%dx%d)\n", width, height); fflush(stderr);
        CreateRenderer(static_cast<Csm::csmUint32>(width), static_cast<Csm::csmUint32>(height));
        fprintf(stderr, "[shim]   renderer ok\n"); fflush(stderr);

        // 纹理
        fprintf(stderr, "[shim]   textures\n"); fflush(stderr);
        SetupTextures();
        fprintf(stderr, "[shim]   textures ok\n"); fflush(stderr);

        // 动作
        fprintf(stderr, "[shim]   motions\n"); fflush(stderr);
        for (Csm::csmInt32 i = 0; i < _modelJson->GetMotionGroupCount(); i++) {
            PreloadMotionGroup(_modelJson->GetMotionGroupName(i));
        }
        _motionManager->StopAllMotions();
        fprintf(stderr, "[shim]   motions ok\n"); fflush(stderr);

        // 物理
        const char* physicsName = _modelJson->GetPhysicsFileName();
        if (physicsName && physicsName[0]) {
            buffer = LoadFile(_dir + physicsName, &size);
            if (buffer) {
                LoadPhysics(buffer, size);
                ReleaseBytes(buffer);
            }
        }
        fprintf(stderr, "[shim]   physics ok\n"); fflush(stderr);


        // 应用角色包提供的 scale / offset 到模型矩阵
        if (_modelMatrix) {
            if (scale > 0.0f && scale != 1.0f) {
                _modelMatrix->ScaleRelative(scale, scale);
            }
            if (offsetX != 0.0f || offsetY != 0.0f) {
                _modelMatrix->TranslateRelative(offsetX, offsetY);
            }
        }
        _model->SaveParameters();
        return true;
    }

    /// 是否有非 Idle 动作正在播放（priority > 1）。
    bool IsBusy() const {
        return _motionManager && _motionManager->GetCurrentPriority() > 1;
    }

    /// 设置视线目标（-1..1 归一化，窗口坐标系；x 右为正，y 上为正）。
    void SetLook(float x, float y) {
        _lookTargetX = x < -1.0f ? -1.0f : (x > 1.0f ? 1.0f : x);
        _lookTargetY = y < -1.0f ? -1.0f : (y > 1.0f ? 1.0f : y);
        _lookActive = true;
    }

    void Update(float dt) {
        if (!_model) return;
        _model->LoadParameters();

        if (_motionManager->IsFinished()) {
            StartMotion("Idle", 0, 1);
        } else {
            _motionManager->UpdateMotion(_model, dt);
        }

        // 视线跟随：仅在 Idle（priority <= 1）时写入，避免干扰 Nod/Shake。
        if (_lookActive && _motionManager->GetCurrentPriority() <= 1) {
            UpdateLook(dt);
        }
        if (_physics) {
            _physics->Evaluate(_model, dt);
        }

        _model->SaveParameters();
        _model->Update();
    }

    void Draw(Csm::CubismMatrix44& projection) {
        auto* renderer = GetRenderer<Csm::Rendering::CubismRenderer_D3D11>();
        if (!_model || !renderer) return;
        projection.MultiplyByMatrix(_modelMatrix);
        // 缓存最近一帧的 MVP（供 HitTest 反算 canvas 坐标）
        _mvp.SetMatrix(projection.GetArray());
        renderer->SetMvpMatrix(&projection);
        renderer->DrawModel();
    }

    // 命中测试：窗口内坐标 (winX, winY)（窗口左上角原点，像素，Y 向下）
    // 是否落在任一可见且非透明的 drawable 几何区域上。
    bool HitTest(float winX, float winY, float winW, float winH) const {
        if (!_model || winW <= 0.0f || winH <= 0.0f) return false;

        // 窗口像素坐标 → NDC（Y 轴翻转；Cubism 画布 Y 向上）
        const float ndcX = 2.0f * winX / winW - 1.0f;
        const float ndcY = 1.0f - 2.0f * winY / winH;

        // MVP 逆变换 → canvas 坐标
        Csm::CubismMatrix44 inv = _mvp.GetInvert();
        const Csm::csmFloat32* m = inv.GetArray();
        // CubismMatrix44 为列主序（参见 D3D11 renderer 的转置代码）
        const float cx = m[0] * ndcX + m[4] * ndcY + m[12];
        const float cy = m[1] * ndcX + m[5] * ndcY + m[13];

        const Csm::csmInt32 drawableCount = _model->GetDrawableCount();
        for (Csm::csmInt32 i = 0; i < drawableCount; ++i) {
            if (!_model->GetDrawableDynamicFlagIsVisible(i)) continue;
            if (_model->GetDrawableOpacity(i) <= 0.001f) continue;

            const Csm::csmInt32 idxCount = _model->GetDrawableVertexIndexCount(i);
            const Csm::csmUint16* indices = _model->GetDrawableVertexIndices(i);
            const Live2D::Cubism::Core::csmVector2* verts = _model->GetDrawableVertexPositions(i);
            if (!indices || !verts) continue;

            for (Csm::csmInt32 k = 0; k + 2 < idxCount; k += 3) {
                const Live2D::Cubism::Core::csmVector2& p0 = verts[indices[k]];
                const Live2D::Cubism::Core::csmVector2& p1 = verts[indices[k + 1]];
                const Live2D::Cubism::Core::csmVector2& p2 = verts[indices[k + 2]];
                if (PointInTriangle(cx, cy, p0.X, p0.Y, p1.X, p1.Y, p2.X, p2.Y)) {
                    return true;
                }
            }
        }
        return false;
    }

    Csm::CubismMotionQueueEntryHandle StartMotion(const char* group, int no, int priority) {
        if (!_modelJson || _modelJson->GetMotionCount(group) == 0) {
            return Csm::InvalidMotionQueueEntryHandleValue;
        }
        if (priority == 3) {
            _motionManager->SetReservePriority(priority);
        } else if (!_motionManager->ReserveMotion(priority)) {
            return Csm::InvalidMotionQueueEntryHandleValue;
        }
        const Csm::csmString name = Csm::Utils::CubismString::GetFormatedString("%s_%d", group, no);
        Csm::CubismMotion* motion = static_cast<Csm::CubismMotion*>(_motions[name.GetRawString()]);
        if (!motion) return Csm::InvalidMotionQueueEntryHandleValue;
        return _motionManager->StartMotionPriority(motion, false, priority);
    }

    Csm::Rendering::CubismRenderer_D3D11* Renderer() {
        return GetRenderer<Csm::Rendering::CubismRenderer_D3D11>();
    }

private:
    void SetupTextures() {
        auto* renderer = GetRenderer<Csm::Rendering::CubismRenderer_D3D11>();
        if (!renderer || !g_device) return;

        const Csm::csmInt32 count = _modelJson->GetTextureCount();
        for (Csm::csmInt32 i = 0; i < count; i++) {
            const char* texName = _modelJson->GetTextureFileName(i);
            if (!texName || !texName[0]) continue;
            const std::string texPath = _dir + texName;

            int w = 0, h = 0, channels = 0;
            unsigned char* pixels = stbi_load(texPath.c_str(), &w, &h, &channels, 4);
            if (!pixels) continue;

            // PSD2Live 导出的 atlas 是 straight alpha；Cubism premultiplied shader
            // 期望预乘贴图。此处把 RGB 预乘 alpha，避免发丝边缘暗/白边。
            {
                const size_t count = static_cast<size_t>(w) * static_cast<size_t>(h);
                for (size_t p = 0; p < count; ++p) {
                    unsigned char* px = pixels + p * 4;
                    const unsigned int a = px[3];
                    px[0] = static_cast<unsigned char>(px[0] * a / 255);
                    px[1] = static_cast<unsigned char>(px[1] * a / 255);
                    px[2] = static_cast<unsigned char>(px[2] * a / 255);
                }
            }

            D3D11_TEXTURE2D_DESC desc = {};
            desc.Width = static_cast<UINT>(w);
            desc.Height = static_cast<UINT>(h);
            desc.MipLevels = 1;
            desc.ArraySize = 1;
            desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
            desc.SampleDesc.Count = 1;
            desc.Usage = D3D11_USAGE_DEFAULT;
            desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;

            D3D11_SUBRESOURCE_DATA initData = {};
            initData.pSysMem = pixels;
            initData.SysMemPitch = static_cast<UINT>(w * 4);

            ID3D11Texture2D* tex = nullptr;
            HRESULT hr = g_device->CreateTexture2D(&desc, &initData, &tex);
            stbi_image_free(pixels);
            if (FAILED(hr) || !tex) continue;

            D3D11_SHADER_RESOURCE_VIEW_DESC srvDesc = {};
            srvDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
            srvDesc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
            srvDesc.Texture2D.MipLevels = 1;

            ID3D11ShaderResourceView* srv = nullptr;
            g_device->CreateShaderResourceView(tex, &srvDesc, &srv);
            tex->Release();

            if (srv) {
                renderer->BindTexture(static_cast<Csm::csmUint32>(i), srv);
                _textureViews.push_back(srv);
            }
        }

        renderer->IsPremultipliedAlpha(true);
    }

    void PreloadMotionGroup(const char* group) {
        const Csm::csmInt32 count = _modelJson->GetMotionCount(group);
        for (Csm::csmInt32 i = 0; i < count; i++) {
            const std::string path = _dir + _modelJson->GetMotionFileName(group, i);
            Csm::csmSizeInt size = 0;
            Csm::csmByte* buffer = LoadFile(path, &size);
            if (!buffer) continue;

            const Csm::csmString name = Csm::Utils::CubismString::GetFormatedString("%s_%d", group, i);
            Csm::ACubismMotion* motion = LoadMotion(buffer, size, name.GetRawString(), nullptr, nullptr, _modelJson, group, i);
            ReleaseBytes(buffer);

            if (motion) {
                if (_motions[name]) {
                    Csm::ACubismMotion::Delete(_motions[name]);
                }
                _motions[name] = motion;
            }
        }
    }

    void Release() {
        for (auto it = _motions.Begin(); it != _motions.End(); ++it) {
            Csm::ACubismMotion::Delete(it->Second);
        }
        _motions.Clear();

        for (auto* srv : _textureViews) {
            if (srv) srv->Release();
        }
        _textureViews.clear();

        delete _modelJson;
        _modelJson = nullptr;
    }

    void UpdateLook(float dt) {
        // 指数平滑（与帧率无关）
        const float rate = 10.0f;
        float a = 1.0f - std::exp(-rate * dt);
        if (a < 0.0f) a = 0.0f;
        if (a > 1.0f) a = 1.0f;
        _lookCurrentX += (_lookTargetX - _lookCurrentX) * a;
        _lookCurrentY += (_lookTargetY - _lookCurrentY) * a;

        Csm::CubismIdManager* idm = Csm::CubismFramework::GetIdManager();
        if (!idm) return;

        Csm::CubismIdHandle idAngleX = idm->GetId("ParamAngleX");
        Csm::CubismIdHandle idAngleY = idm->GetId("ParamAngleY");
        Csm::CubismIdHandle idEyeX   = idm->GetId("ParamEyeBallX");
        Csm::CubismIdHandle idEyeY   = idm->GetId("ParamEyeBallY");

        // 标准范围：角度 [-30,30]，眼球 [-1,1]；不存在的参数为 no-op
        _model->SetParameterValue(idAngleX, _lookCurrentX * 30.0f, 1.0f);
        _model->SetParameterValue(idAngleY, _lookCurrentY * 30.0f, 1.0f);
        _model->SetParameterValue(idEyeX,   _lookCurrentX, 1.0f);
        _model->SetParameterValue(idEyeY,   _lookCurrentY, 1.0f);
    }

    static bool PointInTriangle(float px, float py,
                                float ax, float ay, float bx, float by, float cx, float cy) {
        const float v0x = cx - ax, v0y = cy - ay;
        const float v1x = bx - ax, v1y = by - ay;
        const float v2x = px - ax, v2y = py - ay;
        const float dot00 = v0x * v0x + v0y * v0y;
        const float dot01 = v0x * v1x + v0y * v1y;
        const float dot02 = v0x * v2x + v0y * v2y;
        const float dot11 = v1x * v1x + v1y * v1y;
        const float dot12 = v1x * v2x + v1y * v2y;
        const float denom = dot00 * dot11 - dot01 * dot01;
        if (denom == 0.0f) return false;
        const float invDenom = 1.0f / denom;
        const float u = (dot11 * dot02 - dot01 * dot12) * invDenom;
        const float v = (dot00 * dot12 - dot01 * dot02) * invDenom;
        return u >= 0.0f && v >= 0.0f && u + v <= 1.0f;
    }

    std::string _dir;
    Csm::CubismModelSettingJson* _modelJson = nullptr;
    Csm::csmMap<Csm::csmString, Csm::ACubismMotion*> _motions;
    std::vector<ID3D11ShaderResourceView*> _textureViews;
    Csm::CubismMatrix44 _mvp;


    // 视线跟随状态
    bool _lookActive = false;
    float _lookTargetX = 0.0f;
    float _lookTargetY = 0.0f;
    float _lookCurrentX = 0.0f;
    float _lookCurrentY = 0.0f;

};

} // namespace

extern "C" {

int cubism_shim_version(void) { return 1; }

int cubism_shim_startup(void) {
    if (g_started) return 0;
    fprintf(stderr, "[shim] startup begin\n"); fflush(stderr);
    SetUnhandledExceptionFilter(ShimExceptionFilter);
    g_option.LogFunction = CubismLog;
    g_option.LoggingLevel = Csm::CubismFramework::Option::LogLevel_Verbose;
    g_option.LoadFileFunction = LoadFile;
    g_option.ReleaseBytesFunction = ReleaseBytes;
    Csm::CubismFramework::StartUp(&g_allocator, &g_option);
    fprintf(stderr, "[shim] StartUp done\n"); fflush(stderr);
    Csm::CubismFramework::Initialize();
    fprintf(stderr, "[shim] Initialize done\n"); fflush(stderr);
    g_started = true;
    return 0;
}

void cubism_shim_shutdown(void) {
    if (g_started) {
        Csm::CubismFramework::Dispose();
        g_started = false;
    }
}

void cubism_shim_set_device(void* device, void* context) {
    g_device = static_cast<ID3D11Device*>(device);
    g_context = static_cast<ID3D11DeviceContext*>(context);
    fprintf(stderr, "[shim] set_device dev=%p ctx=%p\n", device, context); fflush(stderr);
    if (g_device) {
        Csm::Rendering::CubismRenderer_D3D11::SetConstantSettings(1, g_device);
        fprintf(stderr, "[shim] SetConstantSettings done\n"); fflush(stderr);
    }
}

void* cubism_shim_model_load(const char* model3Path, int width, int height,
                             float scale, float offsetX, float offsetY) {
    fprintf(stderr, "[shim] model_load: %s (%dx%d) scale=%.3f off=(%.3f,%.3f) dev=%p\n",
            model3Path ? model3Path : "(null)", width, height, scale, offsetX, offsetY, (void*)g_device);
    fflush(stderr);
    if (!model3Path || !g_device) return nullptr;
    auto* model = new ModelWrapper();
    if (!model->Load(model3Path, width, height, scale, offsetX, offsetY)) {
        fprintf(stderr, "[shim] model_load FAILED\n"); fflush(stderr);
        delete model;
        return nullptr;
    }
    fprintf(stderr, "[shim] model_load OK\n"); fflush(stderr);
    return model;
}

void cubism_shim_model_set_look(void* handle, float x, float y) {
    if (!handle) return;
    static_cast<ModelWrapper*>(handle)->SetLook(x, y);
}

int cubism_shim_model_is_busy(void* handle) {
    if (!handle) return 0;
    return static_cast<ModelWrapper*>(handle)->IsBusy() ? 1 : 0;
}

void cubism_shim_model_free(void* handle) {
    delete static_cast<ModelWrapper*>(handle);
}

void cubism_shim_model_update(void* handle, float dt) {
    if (!handle) return;
    static_cast<ModelWrapper*>(handle)->Update(dt);
}

void cubism_shim_model_draw(void* handle, float width, float height) {
    if (!handle) return;
    auto* model = static_cast<ModelWrapper*>(handle);
    auto* renderer = model->Renderer();
    if (!renderer || !g_context) return;

    renderer->StartFrame(g_context);

    Csm::CubismMatrix44 projection;
    projection.LoadIdentity();

    const Csm::CubismModel* m = model->GetModel();
    const float canvasW = m->GetCanvasWidth();
    const float canvasH = m->GetCanvasHeight();
    const float aspectCanvas = (canvasH > 0.0f) ? canvasW / canvasH : 1.0f;
    const float aspectWindow = (height > 0.0f) ? width / height : 1.0f;

    if (aspectCanvas > aspectWindow) {
        projection.Scale(1.0f, aspectWindow / aspectCanvas);
    } else {
        projection.Scale(aspectCanvas / aspectWindow, 1.0f);
    }

    model->Draw(projection);
    renderer->EndFrame();
}

int cubism_shim_model_start_motion(void* handle, const char* group, int no, int priority) {
    if (!handle || !group) return -1;
    auto r = static_cast<ModelWrapper*>(handle)->StartMotion(group, no, priority);
    return (r == Csm::InvalidMotionQueueEntryHandleValue) ? -1 : 0;
}

// 命中测试：窗口内坐标 (winX, winY)，窗口尺寸 (winW, winH)，单位像素。
// 返回 1 表示命中模型可见几何区域，0 表示未命中。
int cubism_shim_model_hit_test(void* handle, float winX, float winY, float winW, float winH) {
    if (!handle) return 0;
    return static_cast<ModelWrapper*>(handle)->HitTest(winX, winY, winW, winH) ? 1 : 0;
}
} // extern "C"
