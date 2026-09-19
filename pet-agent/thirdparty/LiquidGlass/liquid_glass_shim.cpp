// LiquidGlass 的 C ABI 薄封装（供 Rust FFI 使用）。
#include "LiquidGlass.h"
#include <cstring>
#include <cstdio>
#include <vector>
#include <dwmapi.h>
#pragma comment(lib, "dwmapi.lib")

using namespace LiquidGlass;

namespace {
Renderer* g_glass = nullptr;
}

extern "C" {

// 初始化：创建 D3D11 设备/交换链并绑定到 hwnd。返回 1 成功。
int lg_init(void* hwnd, int width, int height) {
    if (g_glass) return 1;
    g_glass = new Renderer();
    if (!g_glass->Init((HWND)hwnd, width, height)) {
        delete g_glass;
        g_glass = nullptr;
        return 0;
    }
    return 1;
}

void lg_shutdown() {
    if (g_glass) {
        g_glass->Shutdown();
        delete g_glass;
        g_glass = nullptr;
    }
}

// 设置玻璃参数（一次性配置）
void lg_config(float blur, float radius, float saturation, float refr,
               float refr_neg, float refr_h, float dispersion, float darkening,
               float tint_r, float tint_g, float tint_b, float tint_a) {
    if (!g_glass) return;
    g_glass->Blur(blur).Radius(radius).Saturation(saturation)
        .RefrAmountCorrect(refr).RefrAmountNegative(refr_neg)
        .RefractionHeight(refr_h).Dispersion(dispersion).Darkening(darkening)
        .GlassTint(tint_r, tint_g, tint_b, tint_a)
        .Depth(true)
        .ShadowAlpha(0.0f)       // 无阴影
        .HighlightAlpha(0.0f);   // 无高光
}

void lg_set_background(float r, float g, float b) {
    if (g_glass) g_glass->SetBackgroundColor(r, g, b);
}

// 捕获屏幕指定区域并保存为 BMP（供玻璃折射真实桌面）
static bool SaveRegionBmp(int x, int y, int w, int h, const wchar_t* path) {
    HDC screen = GetDC(NULL);
    if (!screen) return false;
    HDC mem = CreateCompatibleDC(screen);
    HBITMAP bmp = CreateCompatibleBitmap(screen, w, h);
    HGDIOBJ old = SelectObject(mem, bmp);
    BOOL ok = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY);
    SelectObject(mem, old);
    DeleteDC(mem);
    ReleaseDC(NULL, screen);
    if (!ok) { DeleteObject(bmp); return false; }

    // 组装 BMP 文件
    BITMAPINFOHEADER bi{};
    bi.biSize = sizeof(BITMAPINFOHEADER);
    bi.biWidth = w;
    bi.biHeight = h; // bottom-up
    bi.biPlanes = 1;
    bi.biBitCount = 32;
    bi.biCompression = BI_RGB;
    BITMAPINFO bmi{};
    bmi.bmiHeader = bi;
    DWORD imgSize = (DWORD)w * h * 4;
    std::vector<unsigned char> pixels(imgSize);
    HDC screen2 = GetDC(NULL);
    GetDIBits(screen2, bmp, 0, h, pixels.data(), &bmi, DIB_RGB_COLORS);
    ReleaseDC(NULL, screen2);
    DeleteObject(bmp);

    FILE* f = nullptr;
    if (_wfopen_s(&f, path, L"wb") != 0 || !f) return false;
    BITMAPFILEHEADER fh{};
    fh.bfType = 0x4D42;
    fh.bfOffBits = sizeof(BITMAPFILEHEADER) + sizeof(BITMAPINFOHEADER);
    fh.bfSize = fh.bfOffBits + imgSize;
    fwrite(&fh, sizeof(fh), 1, f);
    fwrite(&bi, sizeof(bi), 1, f);
    fwrite(pixels.data(), 1, imgSize, f);
    fclose(f);
    return true;
}

// 捕获 hwnd 所在屏幕区域（含边距），设为玻璃背景。
// 为避免截到自己：临时隐藏窗口，等 DWM 合成一帧后再截，然后显示回来。
void lg_capture_behind(void* hwnd, int pad) {
    if (!g_glass || !hwnd) return;
    HWND h = (HWND)hwnd;
    RECT r;
    if (!GetWindowRect(h, &r)) return;
    const int w = (r.right - r.left) + pad * 2;
    const int h2 = (r.bottom - r.top) + pad * 2;

    // 隐藏自己（不移动、不改位置），flush 一帧让屏幕出现真实背景
    ShowWindow(h, SW_HIDE);
    DwmFlush();
    Sleep(50);

    wchar_t tmp[MAX_PATH];
    GetTempPathW(MAX_PATH, tmp);
    wcscat_s(tmp, L"DesktopPet_lg_bg.bmp");
    const int x = r.left - pad;
    const int y = r.top - pad;
    bool ok = SaveRegionBmp(x, y, w, h2, tmp);

    // 恢复显示，位置不动
    ShowWindow(h, SW_SHOWNOACTIVATE);
    DwmFlush();

    if (ok) g_glass->LoadBackgroundImage(tmp);
}

// 渲染一帧：绘制整个窗口（含指定区域的玻璃）
void lg_render_frame(int win_w, int win_h, float gx, float gy, float gw, float gh) {
    if (!g_glass) return;
    g_glass->BeginFrame();
    g_glass->RenderGlass(gx, gy, gw, gh);
    g_glass->EndFrame();
    (void)win_w; (void)win_h;
}

void lg_resize(int width, int height) {
    if (g_glass) g_glass->Resize(width, height);
}

int lg_ok() { return g_glass ? 1 : 0; }

} // extern "C"
