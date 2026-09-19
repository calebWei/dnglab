cmake_minimum_required(VERSION 3.15)
project(nikon_he_oracle CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Path to a local clone of the reference decoder (yogthos/LibRaw, branch
# nikon-he-decoder). The reference C++ is intentionally NOT vendored into dnglab;
# point this at your clone. Override with -DNIKON_HE_REF_DIR=...
set(NIKON_HE_REF_DIR "D:/_repos/_ref_libraw_he" CACHE PATH
    "Path to the yogthos/LibRaw clone (nikon-he-decoder branch)")

set(HE_SRC_DIR "${NIKON_HE_REF_DIR}/src/decoders/nikon_he")
if(NOT EXISTS "${HE_SRC_DIR}/nikon_he_decode.cpp")
  message(FATAL_ERROR "Reference sources not found at ${HE_SRC_DIR}. "
                      "Set -DNIKON_HE_REF_DIR to your yogthos/LibRaw clone.")
endif()

# The reference core is self-contained (only standard C++ headers). Exclude the
# LibRaw integration glue (nikon_he_decoder.cpp lives one dir up and pulls in
# dcraw_defs.h); we provide our own main().
file(GLOB HE_CORE "${HE_SRC_DIR}/*.cpp")

add_executable(nikon_he_oracle main.cpp ${HE_CORE})
target_include_directories(nikon_he_oracle PRIVATE "${HE_SRC_DIR}")

if(MSVC)
  target_compile_options(nikon_he_oracle PRIVATE /O2 /EHsc)
endif()
