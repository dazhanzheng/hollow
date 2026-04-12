import SwiftUI

struct OnboardingModelView: View {
    @Environment(\.dismiss) private var dismiss

    private let systemRAM: UInt64 = ProcessInfo.processInfo.physicalMemory / (1024 * 1024 * 1024)

    var body: some View {
        VStack(spacing: 24) {
            // Header
            VStack(spacing: 8) {
                Image(systemName: "brain")
                    .font(.system(size: 40))
                    .foregroundStyle(.tint)
                Text("Set Up Semantic Search")
                    .font(.title2.weight(.semibold))
                Text("Download an embedding model to search files by meaning, not just keywords. Models run entirely on your Mac — nothing leaves your device.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }

            // Model cards
            VStack(spacing: 12) {
                modelCard(
                    title: "Standard",
                    subtitle: "Recommended for all Macs",
                    size: "~600 MB download",
                    ram: "~400 MB RAM",
                    recommended: true,
                    warning: nil
                )

                modelCard(
                    title: "High Quality",
                    subtitle: "Better accuracy, higher resource usage",
                    size: "~4 GB download",
                    ram: "~3 GB RAM",
                    recommended: false,
                    warning: systemRAM < 32
                        ? "Your Mac has \(systemRAM) GB RAM — this model may slow down other apps while embedding."
                        : nil
                )
            }

            // Skip button
            Button("Skip for now") {
                markOnboardingDone()
                dismiss()
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
            .font(.caption)
        }
        .padding(32)
        .frame(width: 480)
    }

    private func modelCard(
        title: String,
        subtitle: String,
        size: String,
        ram: String,
        recommended: Bool,
        warning: String?
    ) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(title).font(.body.weight(.medium))
                        if recommended {
                            Text("RECOMMENDED")
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(.white)
                                .padding(.horizontal, 6)
                                .padding(.vertical, 2)
                                .background(.tint, in: Capsule())
                        }
                    }
                    Text(subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    HStack(spacing: 12) {
                        Text(size)
                        Text(ram)
                    }
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                }

                Spacer()

                Button("Download") {
                    // Download will be connected when download infrastructure is ready
                    markOnboardingDone()
                    dismiss()
                }
                .buttonStyle(.bordered)
                .tint(recommended ? .accentColor : nil)
            }

            if let warning {
                HStack(spacing: 4) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                    Text(warning)
                        .font(.caption2)
                        .foregroundStyle(.orange)
                }
            }
        }
        .padding(12)
        .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
    }

    private func markOnboardingDone() {
        UserDefaults.standard.set(true, forKey: "hasShownModelOnboarding")
    }
}
