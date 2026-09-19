cmake_minimum_required(VERSION 3.16)
project(LiquidGlassShim CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreadedDLL")

if(MSVC)
  add_compile_options(/utf-8)
  add_compile_options(/EHsc)
endif()

add_library(liquid_glass_shim STATIC
  liquid_glass_shim.cpp
  LiquidGlass.cpp
)
target_include_directories(liquid_glass_shim PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
