//! Direct3D11 设备 + DirectComposition 交换链。
//!
//! 采用 DXGI flip-model + DXGI_ALPHA_MODE_PREMULTIPLIED 的合成交换链，
//! 配合 WS_EX_NOREDIRECTIONBITMAP 窗口，实现逐像素透明的桌宠窗口。

use windows::core::Interface;
use windows::Win32::Foundation::{FALSE, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

use crate::config::log_line;

pub struct Gfx {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub swap_chain: IDXGISwapChain1,
    pub rtv: Option<ID3D11RenderTargetView>,
    /// 以下三项必须持有以维持 DirectComposition 合成链存活（drop 即解绑）。
    #[allow(dead_code)]
    pub comp_device: IDCompositionDevice,
    #[allow(dead_code)]
    pub comp_target: IDCompositionTarget,
    #[allow(dead_code)]
    pub comp_visual: IDCompositionVisual,
}

impl Gfx {
    pub unsafe fn new(hwnd: HWND, width: i32, height: i32) -> windows::core::Result<Self> {
        // 1. 创建 D3D11 设备
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let mut feature_level = D3D_FEATURE_LEVEL_11_0;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[
                D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_10_0,
            ]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut feature_level),
            Some(&mut context),
        )?;
        let device = device.unwrap();
        let context = context.unwrap();

        // 2. 获取 DXGI 工厂
        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter: IDXGIAdapter = dxgi_device.GetAdapter()?;
        let factory: IDXGIFactory2 = adapter.GetParent()?;

        // 3. 创建合成交换链（premultiplied alpha）
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width as u32,
            Height: height as u32,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: FALSE,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: 0,
        };
        let swap_chain: IDXGISwapChain1 =
            factory.CreateSwapChainForComposition(&device, &desc, None)?;

        // 4. DirectComposition
        let comp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
        let comp_target = comp_device.CreateTargetForHwnd(hwnd, true)?;
        let comp_visual = comp_device.CreateVisual()?;
        comp_visual.SetContent(&swap_chain)?;
        comp_target.SetRoot(&comp_visual)?;
        comp_device.Commit()?;

        let mut gfx = Gfx {
            device,
            context,
            swap_chain,
            rtv: None,
            comp_device,
            comp_target,
            comp_visual,
        };
        gfx.create_rtv()?;
        log_line("gfx initialized (D3D11 + DirectComposition)");
        Ok(gfx)
    }

    unsafe fn create_rtv(&mut self) -> windows::core::Result<()> {
        let back_buffer: ID3D11Texture2D = self.swap_chain.GetBuffer(0)?;
        let mut rtv: Option<ID3D11RenderTargetView> = None;
        self.device
            .CreateRenderTargetView(&back_buffer, None, Some(&mut rtv))?;
        self.rtv = rtv;
        Ok(())
    }

    /// 暴露 D3D11 设备与上下文原始指针（供 C++ shim 使用）。
    pub fn device_ptr(&self) -> *mut std::ffi::c_void {
        self.device.as_raw()
    }

    pub fn context_ptr(&self) -> *mut std::ffi::c_void {
        self.context.as_raw()
    }

    /// 清屏为完全透明（桌宠默认状态）。
    pub unsafe fn clear_transparent(&self) {
        if let Some(rtv) = &self.rtv {
            let color = [0.0f32, 0.0, 0.0, 0.0];
            self.context.ClearRenderTargetView(rtv, &color);
        }
    }

    /// 绑定交换链后备缓冲为渲染目标，并设置全屏视口。
    pub unsafe fn bind_render_target(&self, width: i32, height: i32) {
        if let Some(rtv) = &self.rtv {
            self.context
                .OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            let viewport = D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: width as f32,
                Height: height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            self.context.RSSetViewports(Some(&[viewport]));
        }
    }

    pub unsafe fn present(&self) -> windows::core::Result<()> {
        self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
        Ok(())
    }

    #[allow(dead_code)]
    pub unsafe fn resize(&mut self, width: i32, height: i32) -> windows::core::Result<()> {
        self.rtv = None;
        self.context.OMSetRenderTargets(None, None);
        self.swap_chain.ResizeBuffers(
            0,
            width as u32,
            height as u32,
            DXGI_FORMAT_UNKNOWN,
            DXGI_SWAP_CHAIN_FLAG(0),
        )?;
        self.create_rtv()?;
        Ok(())
    }
}
