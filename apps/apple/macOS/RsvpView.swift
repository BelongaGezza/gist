import SwiftUI

/// Placeholder RSVP view — wired to real engine in M3.
struct RsvpView: View {
    let itemId: String
    @State private var word = "GIST"
    @State private var wpm: Double = 250
    @State private var isPlaying = false

    var body: some View {
        VStack(spacing: 32) {
            // Word display with ORP marker
            Text(word)
                .font(.system(size: 48, weight: .regular, design: .serif))
                .frame(maxWidth: .infinity)
                .padding()

            // WPM control
            VStack {
                Text("\(Int(wpm)) WPM")
                    .font(.caption)
                    .monospacedDigit()
                Slider(value: $wpm, in: 100...1000, step: 10)
                    .frame(width: 200)
                    .accessibilityLabel("Reading speed")
            }

            // Play/pause
            Button {
                isPlaying.toggle()
            } label: {
                Image(systemName: isPlaying ? "pause.fill" : "play.fill")
                    .font(.title)
            }
            .keyboardShortcut(.space, modifiers: [])
            .accessibilityLabel(isPlaying ? "Pause" : "Play")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .navigationTitle("RSVP")
    }
}
