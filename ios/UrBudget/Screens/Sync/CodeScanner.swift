import AVFoundation
import SwiftUI
import VisionKit

/// Scanning the QR code UrBudget shows on another device, as a sheet. Where the camera can't be
/// used (the Simulator has none, and the person may have refused it) the code is typed instead:
/// the 24 characters under a QR code do the same.
struct CodeScannerSheet: View {
    /// Given what the camera read or what was typed, once, as the sheet closes.
    var found: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var camera = Camera.asking
    @State private var typing = false
    @State private var typed = ""

    private enum Camera { case asking, ready, refused, unavailable }

    var body: some View {
        NavigationStack {
            Group {
                switch camera {
                case .asking:
                    ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity).appGround()
                case .ready where !typing:
                    scanner
                default:
                    typedCode
                }
            }
            .navigationTitle("Scan a code")
            .inlineTitle()
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
        }
        .task { camera = await Self.askForCamera() }
    }

    private var scanner: some View {
        ZStack(alignment: .bottom) {
            QRCamera(found: finish) { camera = .unavailable }
                .ignoresSafeArea(edges: .bottom)
            VStack(spacing: 10) {
                Text("Point the camera at the code UrBudget shows on the other device.")
                    .font(.subheadline)
                    .multilineTextAlignment(.center)
                Button("Type the code instead") { typing = true }
                    .font(.subheadline.weight(.semibold))
            }
            .padding(16)
            .frame(maxWidth: .infinity)
            .background(.regularMaterial)
        }
    }

    private var typedCode: some View {
        let entered = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        return CompactForm {
            if camera == .refused {
                Section {
                    Text("UrBudget isn't allowed to use the camera. You can allow it in Settings.")
                        .font(.subheadline).foregroundStyle(Color.secondary)
                    Button("Open Settings") { openAppSettings() }
                }
            } else if camera == .unavailable {
                Section {
                    Text("The camera can't scan on this device.")
                        .font(.subheadline).foregroundStyle(Color.secondary)
                }
            }
            Section {
                TextField("Code", text: $typed, prompt: Text(verbatim: SyncKey.codeShape))
                    .font(.body.monospaced())
                    .plainTextEntry()
                    .environment(\.layoutDirection, .leftToRight)
                    .submitLabel(.done)
                    .onSubmit { if !entered.isEmpty { finish(entered) } }
            } footer: {
                Text("Type the 24 characters shown under the code on the other device.")
            }
            Section {
                Button { finish(entered) } label: { Text("Continue").fontWeight(.semibold).frame(maxWidth: .infinity) }
                    .primaryAction()
                    .disabled(entered.isEmpty)
            }
        }
    }

    private func finish(_ text: String) {
        found(text)
        dismiss()
    }

    /// Asks for the camera the first time. A device that can't scan at all is never asked.
    private static func askForCamera() async -> Camera {
        guard DataScannerViewController.isSupported else { return .unavailable }
        guard await AVCaptureDevice.requestAccess(for: .video) else { return .refused }
        return DataScannerViewController.isAvailable ? .ready : .unavailable
    }
}

/// The camera, reading one of UrBudget's QR codes. Any other code in view is passed over.
private struct QRCamera: UIViewControllerRepresentable {
    var found: (String) -> Void
    /// The camera stopped being usable while the sheet was open.
    var failed: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(found: found, failed: failed) }

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(recognizedDataTypes: [.barcode(symbologies: [.qr])], qualityLevel: .balanced,
                                                recognizesMultipleItems: false, isHighFrameRateTrackingEnabled: false,
                                                isPinchToZoomEnabled: true, isGuidanceEnabled: false, isHighlightingEnabled: true)
        scanner.delegate = context.coordinator
        return scanner
    }

    func updateUIViewController(_ scanner: DataScannerViewController, context: Context) {
        if !scanner.isScanning { try? scanner.startScanning() }
    }

    static func dismantleUIViewController(_ scanner: DataScannerViewController, coordinator: Coordinator) {
        scanner.stopScanning()
    }

    @MainActor final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        private let found: (String) -> Void
        private let failed: () -> Void
        private var done = false

        init(found: @escaping (String) -> Void, failed: @escaping () -> Void) {
            self.found = found
            self.failed = failed
        }

        func dataScanner(_ dataScanner: DataScannerViewController, didAdd addedItems: [RecognizedItem], allItems: [RecognizedItem]) {
            guard !done else { return }
            for item in addedItems {
                guard case .barcode(let code) = item, let text = code.payloadStringValue, SyncKey.scannable(text) else { continue }
                done = true
                dataScanner.stopScanning()
                found(text)
                return
            }
        }

        func dataScanner(_ dataScanner: DataScannerViewController, becameUnavailableWithError error: DataScannerViewController.ScanningUnavailable) {
            if !done { failed() }
        }
    }
}
