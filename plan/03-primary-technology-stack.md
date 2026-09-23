<!-- Katana plan, section 3 of 47. Index: ../PLAN.MD. Previous: 02-core-architecture.md. Next: 04-absolute-architectural-rules.md -->

# 3. Primary Technology Stack

## Core


* C++26 (the build variable `KATANA_CXX_STANDARD`; see "The language standard is a build variable" in section 24)
* CMake
* Clang
* GCC
* MSVC

## GUI

Use:

* Qt 6
* C++

## Rendering

Use:

* Vulkan

Create a rendering abstraction so the domain model does not directly depend on Vulkan.

## Numerical Mathematics

Use:

* Eigen

## Computational Geometry

Use:

* CGAL where appropriate

## Solid CAD Geometry

Use:

* Open CASCADE Technology

## Coordinate Reference Systems

Use:

* PROJ

## GIS

Use:

* GDAL

## Point Clouds

Use:

* PDAL

## Database

Use:

* SQLite

## Compression

Consider:

* Zstandard
* LZ4
* zlib

## Testing

Use:

* GoogleTest
* Property-based testing where appropriate

## Profiling

Use:

* Tracy
* Native platform profilers
* GPU profiling tools

## AI

Use:

* Python
* LLM APIs
* Python AI orchestration

Python must communicate with the C++ application through a defined API or IPC mechanism.

---

