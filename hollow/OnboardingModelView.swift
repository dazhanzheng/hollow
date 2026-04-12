import SwiftUI

struct OnboardingModelView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var downloader = ModelDownloader()

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
                standardModelCard
                highQualityModelCard
            }

            // Download progress
            if downloader.isDownloading {
                VStack(spacing: 8) {
                    ProgressView(value: downloader.progress)
                    HStack {
                        Text("Downloading model… \(Int(downloader.progress * 100))%")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .monospacedDigit()
                        Spacer()
                        Button("Cancel") { downloader.cancel() }
                            .font(.caption)
                            .buttonStyle(.plain)
                            .foregroundStyle(.red)
                    }
                }
            }

            // Error display
            if let error = downloader.error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            // Skip button
            Button("Skip for now") {
                markOnboardingDone()
                dismiss()
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
            .font(.caption)
            .disabled(downloader.isDownloading)
        }
        .padding(32)
        .frame(width: 480)
    }

    // MARK: - Standard (0.6B) Card

    private var standardModelCard: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text("Standard").font(.body.weight(.medium))
                        Text("RECOMMENDED")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(.tint, in: Capsule())
                    }
                    Text("Recommended for all Macs")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    HStack(spacing: 12) {
                        Text("~586 MB download")
                        Text("~400 MB RAM")
                    }
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                }

                Spacer()

                Button("Download") {
                    Task {
                        try? await downloader.downloadDefaultModel()
                        if downloader.error == nil {
                            markOnboardingDone()
                            dismiss()
                        }
                    }
                }
                .buttonStyle(.bordered)
                .tint(.accentColor)
                .disabled(downloader.isDownloading)
            }
        }
        .padding(12)
        .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
    }

    // MARK: - High Quality (4B) Card

    private var highQualityModelCard: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("High Quality").font(.body.weight(.medium))
                    Text("Better accuracy, higher resource usage")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    HStack(spacing: 12) {
                        Text("~8 GB download")
                        Text("~3 GB RAM")
                    }
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                }

                Spacer()

                Text("Coming soon")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            if systemRAM < 32 {
                HStack(spacing: 4) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                    Text("Your Mac has \(systemRAM) GB RAM — this model may slow down other apps while embedding.")
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
