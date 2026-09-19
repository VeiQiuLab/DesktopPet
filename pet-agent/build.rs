//! 链接 LiquidGlass 静态库（D3D11 液态玻璃渲染）。

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lg_dir = manifest.join("thirdparty").join("LiquidGlass");
    let lib_dir = lg_dir.join("build").join("Release");
    let lib = lib_dir.join("liquid_glass_shim.lib");

    // 首次构建时自动编译 LiquidGlass
    if !lib.exists() {
        let configure = Command::new("cmake")
            .args([
                "-S",
                ".",
                "-B",
                "build",
                "-G",
                "Visual Studio 17 2022",
                "-A",
                "x64",
            ])
            .current_dir(&lg_dir)
            .status()
            .expect("cmake configure failed");
        assert!(configure.success());

        let build = Command::new("cmake")
            .args(["--build", "build", "--config", "Release"])
            .current_dir(&lg_dir)
            .status()
            .expect("cmake build failed");
        assert!(build.success());
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=liquid_glass_shim");
    // LiquidGlass.cpp 的 #pragma comment 在 Rust 链接阶段不生效，需显式链接
    println!("cargo:rustc-link-lib=dylib=d3d11");
    println!("cargo:rustc-link-lib=dylib=d3dcompiler");
    println!("cargo:rustc-link-lib=dylib=dxgi");
    println!("cargo:rustc-link-lib=dylib=dxguid");
    println!("cargo:rustc-link-lib=dylib=ole32");
    println!("cargo:rustc-link-lib=dylib=windowscodecs");

    println!("cargo:rerun-if-changed=thirdparty/LiquidGlass/liquid_glass_shim.cpp");
    println!("cargo:rerun-if-changed=thirdparty/LiquidGlass/LiquidGlass.cpp");
    println!("cargo:rerun-if-changed=thirdparty/LiquidGlass/LiquidGlass.h");
}
