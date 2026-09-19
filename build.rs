//! 构建脚本：
//! 1. 编译 C++ Cubism shim 静态库
//! 2. 编译并链接 Windows 资源（图标）
//! 3. 部署 shader / characters 到输出目录

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shim_build = manifest.join("shim").join("build");

    // 编译 Windows 资源（应用图标），链接进 exe
    build_resource(&manifest);

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

/// 用 rc.exe 编译 assets/desktop-pet.rc 为 .res，并把 .res 链接进最终 exe。
/// 需要 Windows SDK 的 rc.exe（已在 VS BuildTools 环境）。
fn build_resource(manifest: &PathBuf) {
    let rc = manifest.join("assets").join("desktop-pet.rc");
    if !rc.is_file() {
        println!("cargo:warning=assets/desktop-pet.rc not found, skipping icon resource");
        return;
    }
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let res = out_dir.join("desktop-pet.res");

    // 查找 rc.exe（优先 PATH，其次 Windows SDK 最新版本）
    let rc_exe = find_rc_exe();
    let rc_exe = match rc_exe {
        Some(p) => p,
        None => {
            println!("cargo:warning=rc.exe not found, skipping icon resource");
            return;
        }
    };

    // 资源文件在 assets/ 下，rc.exe 用相对路径查找 icon.ico，设置 cwd 为 assets
    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg("desktop-pet.rc")
        .current_dir(manifest.join("assets"))
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            println!("cargo:warning=rc.exe exited with {s}");
            return;
        }
        Err(e) => {
            println!("cargo:warning=failed to run rc.exe: {e}");
            return;
        }
    }

    // 链接 .res 到最终二进制
    println!("cargo:rustc-link-arg=/NOLOGO");
    println!("cargo:rustc-link-arg={}", res.display());
    println!("cargo:rerun-if-changed=assets/desktop-pet.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon_tray.ico");
}

/// 查找 rc.exe：优先 PATH，其次 Windows SDK bin/10.x/x64/rc.exe（选版本号最高的）。
fn find_rc_exe() -> Option<PathBuf> {
    // 1. PATH
    if let Ok(out) = Command::new("where").arg("rc.exe").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(first) = s.lines().next() {
                let p = PathBuf::from(first.trim());
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    // 2. Windows SDK 常见路径
    let sdk_root = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if sdk_root.is_dir() {
        let mut versions: Vec<PathBuf> = std::fs::read_dir(&sdk_root)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        // 过滤掉 arm64/x64/x86 等架构目录名（只要版本号目录）
        versions.retain(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("10."))
                .unwrap_or(false)
        });
        // 按版本号字符串降序（对形如 10.0.26100.0 有效）
        versions.sort();
        for v in versions.iter().rev() {
            let rc = v.join("x64").join("rc.exe");
            if rc.is_file() {
                return Some(rc);
            }
        }
    }
    None
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
