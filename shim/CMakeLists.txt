cmake_minimum_required(VERSION 3.16)
project(CubismShim CXX)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
# 与 Rust (windows-msvc) 默认一致：动态 CRT
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreadedDLL")

# Cubism SDK 源码含 UTF-8 中文注释，必须显式告知 MSVC 源文件编码，
# 否则以本地代码页(936/GBK)误读会导致行连接吞掉代码行。
if(MSVC)
  add_compile_options(/utf-8)
endif()

set(SDK_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../vendor/CubismSdkForNative-5-r.5")
set(CORE_PATH "${SDK_ROOT}/Core")
set(FRAMEWORK_PATH "${SDK_ROOT}/Framework")
set(DX_MATH_INC "C:/AI/QwenGame/dxmath_extract/include")

set(ARCH x86_64)
set(COMPILER 143)
set(CRT MD)

# Cubism Core（预编译静态库）
add_library(Live2DCubismCore STATIC IMPORTED)
set_target_properties(Live2DCubismCore PROPERTIES
  IMPORTED_LOCATION_RELEASE "${CORE_PATH}/lib/windows/${ARCH}/${COMPILER}/Live2DCubismCore_${CRT}.lib"
  IMPORTED_LOCATION_DEBUG   "${CORE_PATH}/lib/windows/${ARCH}/${COMPILER}/Live2DCubismCore_${CRT}d.lib"
  INTERFACE_INCLUDE_DIRECTORIES "${CORE_PATH}/include"
)

# Cubism Native Framework（源码静态库，D3D11 后端）
set(FRAMEWORK_SOURCE D3D11)
add_subdirectory("${FRAMEWORK_PATH}" "${CMAKE_CURRENT_BINARY_DIR}/Framework")
target_include_directories(Framework PUBLIC "${DX_MATH_INC}")
target_link_libraries(Framework Live2DCubismCore d3d11 d3dcompiler)

# Shim 静态库（暴露给 Rust 的 C ABI）
add_library(cubism_shim STATIC shim.cpp)
target_include_directories(cubism_shim PUBLIC "${CORE_PATH}/include" "${DX_MATH_INC}")
target_include_directories(cubism_shim PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/thirdparty")
target_link_libraries(cubism_shim Framework)
