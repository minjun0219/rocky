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
        // 아바타 조각 그림 — `swift run` 은 SwiftPM 리소스 번들에서, .app 은 bundle.sh 가 Contents/Resources 에 복사한 것에서 읽는다.
        .executableTarget(name: "RockyMac", dependencies: ["RockyKit"], resources: [.copy("Resources/rocky")]),
        .testTarget(name: "RockyKitTests", dependencies: ["RockyKit"]),
    ]
)
