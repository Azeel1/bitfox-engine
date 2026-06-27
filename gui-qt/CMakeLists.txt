cmake_minimum_required(VERSION 3.21)
project(BitfoxBoard VERSION 1.0.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)

find_package(Qt6 REQUIRED COMPONENTS Widgets Multimedia)
qt_standard_project_setup()

set(ENGINE_LIB_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../engine/target/release"
    CACHE PATH "Directory containing the engine core library (libbitfox)")
get_filename_component(ENGINE_LIB_DIR_REAL "${ENGINE_LIB_DIR}" REALPATH)
set(BITFOX_CORE "${ENGINE_LIB_DIR}/${CMAKE_SHARED_LIBRARY_PREFIX}bitfox${CMAKE_SHARED_LIBRARY_SUFFIX}")
add_custom_target(bitfox-core
    COMMAND cargo build --release
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/../engine"
    COMMENT "Building Bitfox engine core"
    VERBATIM)

qt_add_executable(bitfox-board
    src/main.cpp
    src/ui/MainWindow.cpp
    src/ui/SidePanel.cpp
    src/ui/Icons.cpp
    src/board/BoardView.cpp
    src/board/CapturedView.cpp
    src/core/CoreBoard.cpp
    src/audio/SoundBank.cpp
    src/engine/UciEngine.cpp
    src/engine/EngineCatalog.cpp
    src/game/GameController.cpp
    resources/resources.qrc
)

target_include_directories(bitfox-board PRIVATE src include)
target_link_libraries(bitfox-board PRIVATE Qt6::Widgets Qt6::Multimedia)
target_link_libraries(bitfox-board PRIVATE "${BITFOX_CORE}")
add_dependencies(bitfox-board bitfox-core)
set_target_properties(bitfox-board PROPERTIES
    BUILD_RPATH "@loader_path;${ENGINE_LIB_DIR}"
    INSTALL_RPATH "@loader_path")

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
