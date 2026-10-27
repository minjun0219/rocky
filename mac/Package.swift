// swift-tools-version: 5.9
// rocky 맥 앱 — 데몬(rockyd)의 REST/SSE 클라이언트. 순수 로직은 RockyKit(Foundation 만), 화면은 RockyMac(SwiftUI).
// 규칙은 docs/features/mac.md.
import PackageDescription

let package = Package(
    name: "RockyMac",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "RockyMac", targets: ["RockyMac"]),
    ],
    targets: [
        .target(name: "RockyKit"),
        .executableTarget(name: "RockyMac", dependencies: ["RockyKit"]),
        .testTarget(name: "RockyKitTests", dependencies: ["RockyKit"]),
    ]
)
