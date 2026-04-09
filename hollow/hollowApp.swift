import SwiftUI

@main
struct hollowApp: App {
    @State private var ingestionService = IngestionService()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(ingestionService)
                .onAppear {
                    ingestionService.start()
                }
        }
    }
}
