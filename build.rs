//! 构建脚本：链接 C++ Cubism shim 静态库，并部署运行时资源（shader）。

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shim_build = manifest.join("shim").join("build");

    // 确保 C++ shim 已构建（Release）。若已存在则跳过，避免重复编译。
    let shim_lib = shim_build.join("Release").join("cubism_shim.lib");
    if !shim_lib.exists() {
        let configure = Command::new("cmake")
            .args([
                "-S",
                "shim",
                "-B",
                "shim/build",
                "-G",
                "Visual Studio 17 2022",
                "-A",
                "x64",
            ])
            .current_dir(&manifest)
            .status()
            .expect("failed to run cmake configure");
        assert!(configure.success(), "cmake configure failed");

        let build = Command::new("cmake")
            .args(["--build", "shim/build", "--config", "Release"])
            .current_dir(&manifest)
            .status()
            .expect("failed to run cmake build");
        assert!(build.success(), "cmake build failed");
    }

    // 库搜索路径
    println!(
        "cargo:rustc-link-search=native={}",
        shim_build.join("Release").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        shim_build.join("Framework").join("Release").display()
    );
    let core_lib = manifest
        .join("vendor")
        .join("CubismSdkForNative-5-r.5")
        .join("Core")
        .join("lib")
        .join("windows")
        .join("x86_64")
        .join("143");
    println!("cargo:rustc-link-search=native={}", core_lib.display());

    // 链接静态库与系统库
    println!("cargo:rustc-link-lib=static=cubism_shim");
    println!("cargo:rustc-link-lib=static=Framework");
    println!("cargo:rustc-link-lib=static=Live2DCubismCore_MD");
    println!("cargo:rustc-link-lib=dylib=d3d11");
    println!("cargo:rustc-link-lib=dylib=d3dcompiler");
    println!("cargo:rustc-link-lib=dylib=dxguid");

    // 部署 shader 到输出目录（Framework 通过相对路径 FrameworkShaders/ 加载）
    deploy_shaders(&manifest);

    // 部署 characters/ 到输出目录（连同 exe 一起可移动）
    deploy_characters(&manifest);

    println!("cargo:rerun-if-changed=shim/shim.cpp");
    println!("cargo:rerun-if-changed=shim/CMakeLists.txt");
    println!("cargo:rerun-if-changed=characters");
}

fn deploy_characters(manifest: &PathBuf) {
    let src = manifest.join("characters");
    if !src.is_dir() {
        return;
    }
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let profile_dir = out_dir.ancestors().nth(3).unwrap().to_path_buf();
    let dest = profile_dir.join("characters");
    let _ = std::fs::create_dir_all(&dest);
    copy_dir_recursive(&src, &dest);
}

fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) {
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap();
            let target = dst.join(name);
            if path.is_dir() {
                let _ = std::fs::create_dir_all(&target);
                copy_dir_recursive(&path, &target);
            } else {
                let _ = std::fs::copy(&path, &target);
            }
        }
    }
}

fn deploy_shaders(manifest: &PathBuf) {
    let src = manifest
        .join("vendor")
        .join("CubismSdkForNative-5-r.5")
        .join("Framework")
        .join("src")
        .join("Rendering")
        .join("D3D11")
        .join("Shaders");

    // 输出目录：target/<profile>/  (通过 OUT_DIR 推断)
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // OUT_DIR = target/<profile>/build/<pkg>-<hash>/out，向上 3 级到 target/<profile>
    let profile_dir = out_dir.ancestors().nth(3).unwrap().to_path_buf();
    let dest = profile_dir.join("FrameworkShaders");

    let _ = std::fs::create_dir_all(&dest);
    if let Ok(entries) = std::fs::read_dir(&src) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "fx").unwrap_or(false) {
                let target = dest.join(path.file_name().unwrap());
                let _ = std::fs::copy(&path, &target);
            }
        }
    }
}
