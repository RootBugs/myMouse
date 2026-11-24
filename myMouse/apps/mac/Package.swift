// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "MyMouseMac",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "MyMouseMacCore", targets: ["MyMouseMacCore"]),
        .executable(name: "MyMouseMac", targets: ["MyMouseMacApp"]),
    ],
    targets: [
        .target(name: "MyMouseMacCore"),
        .executableTarget(
            name: "MyMouseMacApp",
            dependencies: ["MyMouseMacCore"]
        ),
        .testTarget(
            name: "MyMouseMacCoreTests",
            dependencies: ["MyMouseMacCore"]
        ),
    ]
)
