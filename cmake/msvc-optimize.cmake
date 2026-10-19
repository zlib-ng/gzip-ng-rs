# Under the Visual Studio generator cmake-rs replaces CMAKE_<LANG>_FLAGS_RELEASE with flags it has
# stripped of optimization, so zlib-ng would build without /O2, rust-lang/cmake-rs#240. This puts
# back what CMake sets for Release on its own.
foreach(lang C CXX)
  if(NOT CMAKE_${lang}_FLAGS_RELEASE MATCHES "/O2")
    set(CMAKE_${lang}_FLAGS_RELEASE "${CMAKE_${lang}_FLAGS_RELEASE} /O2 /Ob2 /DNDEBUG"
        CACHE STRING "" FORCE)
  endif()
endforeach()
