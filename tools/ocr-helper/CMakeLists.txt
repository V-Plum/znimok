# znimok-ocr: the on-device OCR helper for Windows (ZK-120). Built by build.py against static
# Leptonica and Tesseract built from pinned sources; static CRT, so the exe has no DLLs.
cmake_minimum_required(VERSION 3.21)
project(znimok_ocr CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")

find_package(Leptonica CONFIG REQUIRED)
find_package(Tesseract CONFIG REQUIRED)
# The installed Tesseract target carries no include directory.
find_path(ZNIMOK_TESS_INCLUDE tesseract/baseapi.h REQUIRED)

add_executable(znimok-ocr main.cpp)
target_include_directories(znimok-ocr PRIVATE ${ZNIMOK_TESS_INCLUDE})
target_link_libraries(znimok-ocr PRIVATE Tesseract::libtesseract)
if(MSVC)
  target_compile_options(znimok-ocr PRIVATE /W4 /utf-8)
  target_sources(znimok-ocr PRIVATE znimok-ocr.rc)
endif()
install(TARGETS znimok-ocr RUNTIME DESTINATION .)

# The model converter (float → integer) against Tesseract's internal headers: a build-time
# tool, not shipped.
if(ZNIMOK_TESS_SRC)
  file(GLOB ZNIMOK_TESS_SRC_DIRS LIST_DIRECTORIES true "${ZNIMOK_TESS_SRC}/src/*")
  add_executable(znimok-ocr-convert convert.cpp)
  target_include_directories(znimok-ocr-convert PRIVATE ${ZNIMOK_TESS_INCLUDE} ${ZNIMOK_TESS_SRC_DIRS}
                             ${ZNIMOK_TESS_SRC}/include ${ZNIMOK_TESS_BUILD}/include
                             ${ZNIMOK_TESS_INCLUDE}/leptonica)
  # The library's own configuration (FAST_FLOAT etc.): without it the classes' layout differs
  # from the compiled library and the converted model is broken.
  target_include_directories(znimok-ocr-convert PRIVATE ${ZNIMOK_TESS_BUILD})
  target_compile_definitions(znimok-ocr-convert PRIVATE HAVE_CONFIG_H)
  target_link_libraries(znimok-ocr-convert PRIVATE Tesseract::libtesseract)
endif()
