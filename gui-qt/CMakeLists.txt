cmake_minimum_required(VERSION 3.21)

if((CMAKE_HOST_APPLE AND NOT DEFINED CMAKE_SYSTEM_NAME) OR CMAKE_SYSTEM_NAME STREQUAL "Darwin")
    if(NOT DEFINED CMAKE_OSX_DEPLOYMENT_TARGET OR CMAKE_OSX_DEPLOYMENT_TARGET STREQUAL "")
        set(BITFOX_DEFAULT_OSX_DEPLOYMENT_TARGET "$ENV{MACOSX_DEPLOYMENT_TARGET}")
        if(BITFOX_DEFAULT_OSX_DEPLOYMENT_TARGET STREQUAL "")
            set(BITFOX_DEFAULT_OSX_DEPLOYMENT_TARGET "14.0")
        endif()
        set(CMAKE_OSX_DEPLOYMENT_TARGET "${BITFOX_DEFAULT_OSX_DEPLOYMENT_TARGET}" CACHE STRING
            "Minimum macOS deployment target" FORCE)
    endif()
endif()

file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/../engine/Cargo.toml" BITFOX_ENGINE_VERSION_LINE
    REGEX "^version[ ]*=[ ]*\"[^\"]+\"")
if(NOT BITFOX_ENGINE_VERSION_LINE)
    message(FATAL_ERROR "Could not read Bitfox version from engine/Cargo.toml")
endif()
list(GET BITFOX_ENGINE_VERSION_LINE 0 BITFOX_ENGINE_VERSION_LINE)
string(REGEX REPLACE "^version[ ]*=[ ]*\"([^\"]+)\".*" "\\1" BITFOX_ENGINE_VERSION
    "${BITFOX_ENGINE_VERSION_LINE}")

project(BitfoxBoard VERSION "${BITFOX_ENGINE_VERSION}" LANGUAGES CXX)

include(CTest)
include(GNUInstallDirs)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)

find_package(Qt6 REQUIRED COMPONENTS Widgets Multimedia Network)
find_program(CARGO_EXECUTABLE cargo REQUIRED)
find_program(RUSTC_EXECUTABLE rustc REQUIRED)
if(COMMAND qt_standard_project_setup)
    qt_standard_project_setup()
endif()

set(BITFOX_ENGINE_SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../engine")
set(BITFOX_DEFAULT_RUST_TARGET "")
if(DEFINED ENV{CARGO_BUILD_TARGET})
    set(BITFOX_DEFAULT_RUST_TARGET "$ENV{CARGO_BUILD_TARGET}")
endif()
set(BITFOX_RUST_TARGET "${BITFOX_DEFAULT_RUST_TARGET}" CACHE STRING
    "Rust target triple for the engine core")
string(STRIP "${BITFOX_RUST_TARGET}" BITFOX_RUST_TARGET)
if(BITFOX_RUST_TARGET MATCHES "[/\\\\]")
    message(FATAL_ERROR "BITFOX_RUST_TARGET must be a Rust target triple, not a target JSON path")
endif()

execute_process(
    COMMAND "${RUSTC_EXECUTABLE}" -vV
    RESULT_VARIABLE BITFOX_RUSTC_RESULT
    OUTPUT_VARIABLE BITFOX_RUSTC_VERSION
    OUTPUT_STRIP_TRAILING_WHITESPACE)
if(NOT BITFOX_RUSTC_RESULT EQUAL 0 OR
   NOT BITFOX_RUSTC_VERSION MATCHES "host: ([A-Za-z0-9_.-]+)")
    message(FATAL_ERROR "Could not determine the Rust host target from rustc -vV")
endif()
set(BITFOX_RUST_HOST "${CMAKE_MATCH_1}")
set(BITFOX_EFFECTIVE_RUST_TARGET "${BITFOX_RUST_TARGET}")

if(APPLE)
    set(BITFOX_OSX_ARCHITECTURES ${CMAKE_OSX_ARCHITECTURES})
    list(LENGTH BITFOX_OSX_ARCHITECTURES BITFOX_OSX_ARCHITECTURE_COUNT)
    if(BITFOX_OSX_ARCHITECTURE_COUNT GREATER 1)
        message(FATAL_ERROR
            "A universal Qt build needs separately built Rust libraries; "
            "configure one of arm64 or x86_64 per build directory")
    endif()

    if(BITFOX_OSX_ARCHITECTURE_COUNT EQUAL 1)
        list(GET BITFOX_OSX_ARCHITECTURES 0 BITFOX_OSX_ARCHITECTURE)
        if(BITFOX_OSX_ARCHITECTURE STREQUAL "arm64")
            set(BITFOX_EXPECTED_RUST_TARGET "aarch64-apple-darwin")
        elseif(BITFOX_OSX_ARCHITECTURE STREQUAL "x86_64")
            set(BITFOX_EXPECTED_RUST_TARGET "x86_64-apple-darwin")
        else()
            message(FATAL_ERROR
                "Unsupported CMAKE_OSX_ARCHITECTURES value: ${BITFOX_OSX_ARCHITECTURE}")
        endif()

        if(BITFOX_RUST_TARGET AND
           NOT BITFOX_RUST_TARGET STREQUAL BITFOX_EXPECTED_RUST_TARGET)
            message(FATAL_ERROR
                "CMAKE_OSX_ARCHITECTURES=${BITFOX_OSX_ARCHITECTURE} requires "
                "BITFOX_RUST_TARGET=${BITFOX_EXPECTED_RUST_TARGET}")
        endif()
        set(BITFOX_EFFECTIVE_RUST_TARGET "${BITFOX_EXPECTED_RUST_TARGET}")
    else()
        if(BITFOX_EFFECTIVE_RUST_TARGET)
            set(BITFOX_ARCH_CHECK_TARGET "${BITFOX_EFFECTIVE_RUST_TARGET}")
        else()
            set(BITFOX_ARCH_CHECK_TARGET "${BITFOX_RUST_HOST}")
        endif()

        if(BITFOX_ARCH_CHECK_TARGET STREQUAL "aarch64-apple-darwin")
            set(BITFOX_RUST_ARCHITECTURE "arm64")
        elseif(BITFOX_ARCH_CHECK_TARGET STREQUAL "x86_64-apple-darwin")
            set(BITFOX_RUST_ARCHITECTURE "x86_64")
        else()
            message(FATAL_ERROR
                "The Qt board requires a macOS Rust target, got ${BITFOX_ARCH_CHECK_TARGET}")
        endif()

        if(CMAKE_SYSTEM_PROCESSOR AND
           NOT CMAKE_SYSTEM_PROCESSOR STREQUAL BITFOX_RUST_ARCHITECTURE)
            message(FATAL_ERROR
                "Rust would build ${BITFOX_RUST_ARCHITECTURE}, while CMake defaults to "
                "${CMAKE_SYSTEM_PROCESSOR}; set CMAKE_OSX_ARCHITECTURES explicitly")
        endif()
    endif()
elseif(CMAKE_CROSSCOMPILING AND NOT BITFOX_RUST_TARGET)
    message(FATAL_ERROR
        "Cross-compiling the Qt board requires an explicit BITFOX_RUST_TARGET")
endif()

set(BITFOX_ENGINE_LIB_INPUT "")
if(DEFINED ENGINE_LIB_DIR)
    if(NOT "${ENGINE_LIB_DIR}" STREQUAL "")
        set(BITFOX_ENGINE_LIB_INPUT "${ENGINE_LIB_DIR}")
        cmake_path(ABSOLUTE_PATH BITFOX_ENGINE_LIB_INPUT
            BASE_DIRECTORY "${CMAKE_BINARY_DIR}" NORMALIZE
            OUTPUT_VARIABLE BITFOX_ENGINE_LIB_INPUT)
    endif()
endif()

set(BITFOX_CARGO_TARGET_INPUT "")
if(DEFINED CARGO_TARGET_DIR)
    if(NOT "${CARGO_TARGET_DIR}" STREQUAL "")
        set(BITFOX_CARGO_TARGET_INPUT "${CARGO_TARGET_DIR}")
    endif()
endif()
if(NOT BITFOX_CARGO_TARGET_INPUT AND DEFINED ENV{CARGO_TARGET_DIR})
    if(NOT "$ENV{CARGO_TARGET_DIR}" STREQUAL "")
        set(BITFOX_CARGO_TARGET_INPUT "$ENV{CARGO_TARGET_DIR}")
    endif()
endif()
if(BITFOX_CARGO_TARGET_INPUT)
    cmake_path(ABSOLUTE_PATH BITFOX_CARGO_TARGET_INPUT
        BASE_DIRECTORY "${BITFOX_ENGINE_SOURCE_DIR}" NORMALIZE
        OUTPUT_VARIABLE BITFOX_CARGO_TARGET_INPUT)
endif()

if(BITFOX_EFFECTIVE_RUST_TARGET)
    set(BITFOX_RELEASE_SUBDIR "${BITFOX_EFFECTIVE_RUST_TARGET}/release")
else()
    set(BITFOX_RELEASE_SUBDIR "release")
endif()

if(BITFOX_ENGINE_LIB_INPUT AND NOT BITFOX_CARGO_TARGET_INPUT)
    cmake_path(GET BITFOX_ENGINE_LIB_INPUT FILENAME BITFOX_PROFILE_NAME)
    cmake_path(GET BITFOX_ENGINE_LIB_INPUT PARENT_PATH BITFOX_INFERRED_TARGET_DIR)
    if(NOT BITFOX_PROFILE_NAME STREQUAL "release")
        message(FATAL_ERROR "ENGINE_LIB_DIR must end in the Cargo release profile directory")
    endif()
    if(BITFOX_EFFECTIVE_RUST_TARGET)
        cmake_path(GET BITFOX_INFERRED_TARGET_DIR FILENAME BITFOX_INFERRED_RUST_TARGET)
        if(NOT BITFOX_INFERRED_RUST_TARGET STREQUAL BITFOX_EFFECTIVE_RUST_TARGET)
            message(FATAL_ERROR
                "ENGINE_LIB_DIR must end in ${BITFOX_EFFECTIVE_RUST_TARGET}/release")
        endif()
        cmake_path(GET BITFOX_INFERRED_TARGET_DIR PARENT_PATH BITFOX_INFERRED_TARGET_DIR)
    endif()
    set(BITFOX_CARGO_TARGET_INPUT "${BITFOX_INFERRED_TARGET_DIR}")
endif()

if(NOT BITFOX_CARGO_TARGET_INPUT)
    set(BITFOX_CARGO_TARGET_INPUT "${BITFOX_ENGINE_SOURCE_DIR}/target")
    cmake_path(NORMAL_PATH BITFOX_CARGO_TARGET_INPUT)
endif()

set(BITFOX_EXPECTED_ENGINE_LIB_DIR
    "${BITFOX_CARGO_TARGET_INPUT}/${BITFOX_RELEASE_SUBDIR}")
cmake_path(NORMAL_PATH BITFOX_EXPECTED_ENGINE_LIB_DIR)
if(BITFOX_ENGINE_LIB_INPUT AND
   NOT BITFOX_ENGINE_LIB_INPUT STREQUAL BITFOX_EXPECTED_ENGINE_LIB_DIR)
    message(FATAL_ERROR
        "ENGINE_LIB_DIR and CARGO_TARGET_DIR disagree. Expected "
        "ENGINE_LIB_DIR=${BITFOX_EXPECTED_ENGINE_LIB_DIR}")
endif()

set(CARGO_TARGET_DIR "${BITFOX_CARGO_TARGET_INPUT}" CACHE PATH
    "Cargo target directory used to build the engine core" FORCE)
set(ENGINE_LIB_DIR "${BITFOX_EXPECTED_ENGINE_LIB_DIR}" CACHE PATH
    "Directory containing the engine core library (libbitfox)" FORCE)
set(ENGINE_LIB_DIR_REAL "${ENGINE_LIB_DIR}")
set(BITFOX_CORE "${ENGINE_LIB_DIR_REAL}/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}")
if(WIN32)
    if(MINGW)
        set(BITFOX_CORE_IMPLIB "${ENGINE_LIB_DIR_REAL}/libbitfox.dll.a")
    else()
        set(BITFOX_CORE_IMPLIB "${ENGINE_LIB_DIR_REAL}/bitfox.dll.lib")
    endif()
endif()
set(BITFOX_CORE_BYPRODUCTS "${BITFOX_CORE}")
if(WIN32)
    list(APPEND BITFOX_CORE_BYPRODUCTS "${BITFOX_CORE_IMPLIB}")
endif()

file(GLOB_RECURSE BITFOX_CORE_RUST_SOURCES CONFIGURE_DEPENDS
    "${BITFOX_ENGINE_SOURCE_DIR}/src/*.rs")
set(BITFOX_CORE_INPUTS
    "${BITFOX_ENGINE_SOURCE_DIR}/Cargo.toml"
    "${BITFOX_ENGINE_SOURCE_DIR}/Cargo.lock"
    "${BITFOX_ENGINE_SOURCE_DIR}/.cargo/config.toml"
    "${BITFOX_ENGINE_SOURCE_DIR}/networks/bitfox.nnue"
    "${CMAKE_CURRENT_SOURCE_DIR}/../rust-toolchain.toml"
    ${BITFOX_CORE_RUST_SOURCES})

set(BITFOX_CARGO_COMMAND
    "${CMAKE_COMMAND}" -E env
    --unset=CARGO_BUILD_TARGET
    "CARGO_TARGET_DIR=${CARGO_TARGET_DIR}")

cmake_path(GET CMAKE_CURRENT_SOURCE_DIR PARENT_PATH BITFOX_REMAP_REPO_ROOT)
if(DEFINED ENV{CARGO_HOME} AND NOT "$ENV{CARGO_HOME}" STREQUAL "")
    set(BITFOX_REMAP_CARGO_HOME "$ENV{CARGO_HOME}")
elseif(WIN32 AND DEFINED ENV{USERPROFILE})
    set(BITFOX_REMAP_CARGO_HOME "$ENV{USERPROFILE}/.cargo")
elseif(DEFINED ENV{HOME})
    set(BITFOX_REMAP_CARGO_HOME "$ENV{HOME}/.cargo")
else()
    message(FATAL_ERROR "Could not determine the Cargo home for path remapping")
endif()
cmake_path(SET BITFOX_REMAP_CARGO_HOME NORMALIZE "${BITFOX_REMAP_CARGO_HOME}")
cmake_path(ABSOLUTE_PATH BITFOX_REMAP_CARGO_HOME
    BASE_DIRECTORY "${BITFOX_ENGINE_SOURCE_DIR}" NORMALIZE)
set(BITFOX_REMAP_FLAGS
    "--remap-path-prefix=${BITFOX_REMAP_CARGO_HOME}=cargo"
    "--remap-path-prefix=${BITFOX_REMAP_REPO_ROOT}=bitfox")
foreach(BITFOX_REMAP_NAME IN ITEMS CARGO_HOME REPO_ROOT)
    file(REAL_PATH "${BITFOX_REMAP_${BITFOX_REMAP_NAME}}" BITFOX_REMAP_REAL)
    if(NOT BITFOX_REMAP_REAL STREQUAL BITFOX_REMAP_${BITFOX_REMAP_NAME})
        if(BITFOX_REMAP_NAME STREQUAL "CARGO_HOME")
            set(BITFOX_REMAP_DESTINATION "cargo")
        else()
            set(BITFOX_REMAP_DESTINATION "bitfox")
        endif()
        list(APPEND BITFOX_REMAP_FLAGS
            "--remap-path-prefix=${BITFOX_REMAP_REAL}=${BITFOX_REMAP_DESTINATION}")
    endif()
endforeach()

set(BITFOX_HAS_ENV_RUSTFLAGS FALSE)
string(ASCII 31 BITFOX_RUST_FLAG_SEPARATOR)
if(DEFINED ENV{CARGO_ENCODED_RUSTFLAGS})
    set(BITFOX_ENCODED_RUSTFLAGS "$ENV{CARGO_ENCODED_RUSTFLAGS}")
    set(BITFOX_HAS_ENV_RUSTFLAGS TRUE)
elseif(DEFINED ENV{RUSTFLAGS})
    string(STRIP "$ENV{RUSTFLAGS}" BITFOX_ENCODED_RUSTFLAGS)
    string(REGEX REPLACE "[ \t\r\n]+" "${BITFOX_RUST_FLAG_SEPARATOR}"
        BITFOX_ENCODED_RUSTFLAGS "${BITFOX_ENCODED_RUSTFLAGS}")
    set(BITFOX_HAS_ENV_RUSTFLAGS TRUE)
endif()
if(BITFOX_HAS_ENV_RUSTFLAGS)
    foreach(BITFOX_REMAP_FLAG IN LISTS BITFOX_REMAP_FLAGS)
        if(NOT BITFOX_ENCODED_RUSTFLAGS STREQUAL "")
            string(APPEND BITFOX_ENCODED_RUSTFLAGS "${BITFOX_RUST_FLAG_SEPARATOR}")
        endif()
        string(APPEND BITFOX_ENCODED_RUSTFLAGS "${BITFOX_REMAP_FLAG}")
    endforeach()
    string(REPLACE ";" "\\;" BITFOX_ENCODED_RUSTFLAGS "${BITFOX_ENCODED_RUSTFLAGS}")
    list(APPEND BITFOX_CARGO_COMMAND
        "CARGO_ENCODED_RUSTFLAGS=${BITFOX_ENCODED_RUSTFLAGS}")
endif()

if(APPLE AND CMAKE_OSX_DEPLOYMENT_TARGET)
    list(APPEND BITFOX_CARGO_COMMAND
        "MACOSX_DEPLOYMENT_TARGET=${CMAKE_OSX_DEPLOYMENT_TARGET}")
endif()
list(APPEND BITFOX_CARGO_COMMAND
    -- "${CARGO_EXECUTABLE}" build --release --locked)
if(BITFOX_EFFECTIVE_RUST_TARGET)
    list(APPEND BITFOX_CARGO_COMMAND --target "${BITFOX_EFFECTIVE_RUST_TARGET}")
endif()
if(NOT BITFOX_HAS_ENV_RUSTFLAGS)
    set(BITFOX_REMAP_ARRAY "")
    foreach(BITFOX_REMAP_FLAG IN LISTS BITFOX_REMAP_FLAGS)
        string(REPLACE "\\" "\\\\" BITFOX_REMAP_FLAG "${BITFOX_REMAP_FLAG}")
        string(REPLACE "\"" "\\\"" BITFOX_REMAP_FLAG "${BITFOX_REMAP_FLAG}")
        string(REPLACE "\t" "\\t" BITFOX_REMAP_FLAG "${BITFOX_REMAP_FLAG}")
        string(REPLACE "\r" "\\r" BITFOX_REMAP_FLAG "${BITFOX_REMAP_FLAG}")
        string(REPLACE "\n" "\\n" BITFOX_REMAP_FLAG "${BITFOX_REMAP_FLAG}")
        if(NOT BITFOX_REMAP_ARRAY STREQUAL "")
            string(APPEND BITFOX_REMAP_ARRAY ",")
        endif()
        string(APPEND BITFOX_REMAP_ARRAY "\"${BITFOX_REMAP_FLAG}\"")
    endforeach()
    if(BITFOX_EFFECTIVE_RUST_TARGET)
        set(BITFOX_REMAP_TARGET "${BITFOX_EFFECTIVE_RUST_TARGET}")
    else()
        set(BITFOX_REMAP_TARGET "${BITFOX_RUST_HOST}")
    endif()
    set(BITFOX_REMAP_CONFIG
        "target.\"${BITFOX_REMAP_TARGET}\".rustflags=[${BITFOX_REMAP_ARRAY}]")
    string(REPLACE ";" "\\;" BITFOX_REMAP_CONFIG "${BITFOX_REMAP_CONFIG}")
    list(APPEND BITFOX_CARGO_COMMAND --config "${BITFOX_REMAP_CONFIG}")
endif()

add_custom_target(bitfox-core
    COMMAND ${BITFOX_CARGO_COMMAND}
    BYPRODUCTS ${BITFOX_CORE_BYPRODUCTS}
    DEPENDS ${BITFOX_CORE_INPUTS}
    WORKING_DIRECTORY "${BITFOX_ENGINE_SOURCE_DIR}"
    COMMENT "Building Bitfox engine core"
    VERBATIM
    USES_TERMINAL)

add_library(bitfox_core SHARED IMPORTED GLOBAL)
set_target_properties(bitfox_core PROPERTIES
    IMPORTED_LOCATION "${BITFOX_CORE}"
    INTERFACE_INCLUDE_DIRECTORIES "${BITFOX_ENGINE_SOURCE_DIR}/include"
)
if(WIN32)
    set_target_properties(bitfox_core PROPERTIES
        IMPORTED_IMPLIB "${BITFOX_CORE_IMPLIB}"
    )
elseif(UNIX AND NOT APPLE)
    set_target_properties(bitfox_core PROPERTIES
        IMPORTED_NO_SONAME TRUE
    )
endif()

qt_add_executable(bitfox-board
    src/main.cpp
    src/ui/MainWindow.cpp
    src/ui/SidePanel.cpp
    src/ui/Icons.cpp
    src/ui/ReleasesDialog.cpp
    src/ui/AppUpdateDialog.cpp
    src/update/AppUpdater.cpp
    src/board/BoardView.cpp
    src/board/CapturedView.cpp
    src/core/CoreBoard.cpp
    src/audio/SoundBank.cpp
    src/engine/UciEngine.cpp
    src/engine/EngineCatalog.cpp
    src/engine/ReleaseManager.cpp
    src/game/GameController.cpp
    resources/resources.qrc
)

target_include_directories(bitfox-board PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/src"
)

target_compile_definitions(bitfox-board PRIVATE BITFOX_APP_VERSION="${PROJECT_VERSION}")

target_link_libraries(bitfox-board PRIVATE
    Qt6::Widgets
    Qt6::Multimedia
    Qt6::Network
    bitfox_core
)

add_dependencies(bitfox-board bitfox-core)

option(BITFOX_STRICT_WARNINGS "Treat C++ compiler warnings as errors" OFF)
function(bitfox_enable_strict_warnings target)
    if(BITFOX_STRICT_WARNINGS)
        if(MSVC)
            target_compile_options(${target} PRIVATE /W4 /WX)
        else()
            target_compile_options(${target} PRIVATE -Wall -Wextra -Wpedantic -Werror)
        endif()
    endif()
endfunction()

bitfox_enable_strict_warnings(bitfox-board)

if(APPLE)
    set_target_properties(bitfox-board PROPERTIES
        BUILD_RPATH "@loader_path;${ENGINE_LIB_DIR}"
        INSTALL_RPATH "@loader_path")
elseif(UNIX)
    set_target_properties(bitfox-board PROPERTIES
        BUILD_RPATH "$ORIGIN;${ENGINE_LIB_DIR}"
        INSTALL_RPATH "$ORIGIN")
endif()

add_custom_command(TARGET bitfox-board POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
            "${BITFOX_CORE}" "$<TARGET_FILE_DIR:bitfox-board>"
    VERBATIM)

if(APPLE)
    add_custom_command(TARGET bitfox-board POST_BUILD
        COMMAND ${CMAKE_INSTALL_NAME_TOOL}
                -id "@rpath/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}"
                "$<TARGET_FILE_DIR:bitfox-board>/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}"
        COMMAND ${CMAKE_INSTALL_NAME_TOOL}
                -change "${ENGINE_LIB_DIR_REAL}/deps/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}"
                "@rpath/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}"
                "$<TARGET_FILE:bitfox-board>"
        VERBATIM)
endif()

set(BITFOX_RUNTIME_CORE
    "$<TARGET_FILE_DIR:bitfox-board>/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}")

install(TARGETS bitfox-board
    BUNDLE DESTINATION .
    RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}")
install(FILES "${BITFOX_RUNTIME_CORE}"
    DESTINATION "${CMAKE_INSTALL_BINDIR}")

if(BUILD_TESTING)
    find_package(Qt6 REQUIRED COMPONENTS Test)

    add_executable(UciEngineTest
        tests/UciEngineTest.cpp
        src/engine/UciEngine.cpp
        src/game/GameController.cpp
        src/core/CoreBoard.cpp
        src/board/BoardView.cpp
        resources/resources.qrc)
    target_include_directories(UciEngineTest PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}/src")
    target_link_libraries(UciEngineTest PRIVATE
        Qt6::Test
        Qt6::Widgets
        bitfox_core)
    add_dependencies(UciEngineTest bitfox-core)
    bitfox_enable_strict_warnings(UciEngineTest)

    add_executable(MainWindowTest
        tests/MainWindowTest.cpp
        src/ui/MainWindow.cpp
        src/ui/SidePanel.cpp
        src/ui/Icons.cpp
        src/ui/ReleasesDialog.cpp
        src/ui/AppUpdateDialog.cpp
        src/update/AppUpdater.cpp
        src/board/BoardView.cpp
        src/board/CapturedView.cpp
        src/core/CoreBoard.cpp
        src/audio/SoundBank.cpp
        src/engine/UciEngine.cpp
        src/engine/EngineCatalog.cpp
        src/engine/ReleaseManager.cpp
        src/game/GameController.cpp
        resources/resources.qrc)
    target_include_directories(MainWindowTest PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}/src")
    target_compile_definitions(MainWindowTest PRIVATE BITFOX_APP_VERSION="${PROJECT_VERSION}")
    target_link_libraries(MainWindowTest PRIVATE
        Qt6::Test Qt6::Widgets Qt6::Multimedia Qt6::Network bitfox_core)
    add_dependencies(MainWindowTest bitfox-core)
    bitfox_enable_strict_warnings(MainWindowTest)

    foreach(BITFOX_CORE_TEST IN ITEMS UciEngineTest MainWindowTest)
    add_custom_command(TARGET ${BITFOX_CORE_TEST} POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "${BITFOX_CORE}" "$<TARGET_FILE_DIR:${BITFOX_CORE_TEST}>"
        VERBATIM)
    if(APPLE)
        add_custom_command(TARGET ${BITFOX_CORE_TEST} POST_BUILD
            COMMAND ${CMAKE_INSTALL_NAME_TOOL}
                    -id "@rpath/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}"
                    "$<TARGET_FILE_DIR:${BITFOX_CORE_TEST}>/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}"
            COMMAND ${CMAKE_INSTALL_NAME_TOOL}
                    -change "${ENGINE_LIB_DIR_REAL}/deps/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}"
                    "@rpath/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}"
                    "$<TARGET_FILE:${BITFOX_CORE_TEST}>"
            VERBATIM)
        set_target_properties(${BITFOX_CORE_TEST} PROPERTIES BUILD_RPATH "@loader_path")
    elseif(UNIX)
        set_target_properties(${BITFOX_CORE_TEST} PROPERTIES BUILD_RPATH "$ORIGIN")
    endif()
    endforeach()

    add_executable(ReleaseManagerTest
        tests/ReleaseManagerTest.cpp
        src/engine/ReleaseManager.cpp)
    target_include_directories(ReleaseManagerTest PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}/src")
    target_link_libraries(ReleaseManagerTest PRIVATE
        Qt6::Network
        Qt6::Test)
    bitfox_enable_strict_warnings(ReleaseManagerTest)

    add_executable(AppUpdaterTest tests/AppUpdaterTest.cpp src/update/AppUpdater.cpp)
    target_include_directories(AppUpdaterTest PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/src")
    target_link_libraries(AppUpdaterTest PRIVATE Qt6::Network Qt6::Test)
    bitfox_enable_strict_warnings(AppUpdaterTest)

    set_target_properties(UciEngineTest MainWindowTest ReleaseManagerTest AppUpdaterTest PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/tests")

    add_test(NAME UciEngineTest COMMAND "$<TARGET_FILE:UciEngineTest>")
    add_test(NAME MainWindowTest COMMAND "$<TARGET_FILE:MainWindowTest>")
    add_test(NAME ReleaseManagerTest COMMAND "$<TARGET_FILE:ReleaseManagerTest>")
    add_test(NAME AppUpdaterTest COMMAND "$<TARGET_FILE:AppUpdaterTest>")
    add_test(NAME bitfox-board-runtime-layout
        COMMAND "${CMAKE_COMMAND}"
            "-DBITFOX_BOARD=$<TARGET_FILE:bitfox-board>"
            "-DBITFOX_CORE=${BITFOX_RUNTIME_CORE}"
            -P "${CMAKE_CURRENT_SOURCE_DIR}/cmake/VerifyRuntimeLayout.cmake")
    set_tests_properties(UciEngineTest MainWindowTest ReleaseManagerTest AppUpdaterTest
        PROPERTIES TIMEOUT 30)
    set_tests_properties(UciEngineTest MainWindowTest PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")
endif()
