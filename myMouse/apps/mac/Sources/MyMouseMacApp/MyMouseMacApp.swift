import SwiftUI

@main
struct MyMouseMacApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup("myMouse") {
            ContentView()
                .environmentObject(model)
                .frame(minWidth: 860, minHeight: 560)
        }
    }
}
