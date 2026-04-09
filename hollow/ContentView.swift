import SwiftUI

struct ContentView: View {
    @State private var status: String = "Initializing..."

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "archivebox")
                .imageScale(.large)
                .foregroundStyle(.tint)
            Text("hollow")
                .font(.title)
            Text(status)
                .foregroundStyle(.secondary)
        }
        .padding()
        .task {
            if HollowBridge.shared.isReady {
                let count = HollowBridge.shared.listFiles().count
                status = "Database ready. \(count) files indexed."
            } else {
                status = "Failed to initialize database."
            }
        }
    }
}

#Preview {
    ContentView()
}
