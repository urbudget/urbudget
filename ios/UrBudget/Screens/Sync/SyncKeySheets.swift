import SwiftUI
import UIKit

// Account & sync's sheets for a sync key that nobody types
// (docs/plans/2026-11-24-sync-device-approval.md): the recovery code to save, and adding a device.

/// A recovery code this device still holds, shown until the person says they've saved it: the
/// sheet's only way out. Until then the engine keeps the code, so closing the app loses nothing.
struct RecoveryCodeSheet: View {
    var code: String
    /// Tells the engine the code is saved (`sync_recovery_saved`) and closes the sheet.
    var saved: () async throws -> Void
    @State private var copied = false
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            CompactForm {
                Section {
                    // One line, so copying it gives the code exactly as an older app's passphrase
                    // box needs it. Left to right in Arabic too.
                    Text(code)
                        .font(.title2.monospaced().weight(.semibold))
                        .lineLimit(1)
                        .minimumScaleFactor(0.5)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 10)
                        .textSelection(.enabled)
                        .environment(\.layoutDirection, .leftToRight)
                        // The line under it runs the row's width, not from where the centred code starts.
                        .alignmentGuide(.listRowSeparatorLeading) { $0[.leading] }
                    Button {
                        UIPasteboard.general.string = code
                        copied = true
                    } label: {
                        if copied { Label("Copied", systemImage: "checkmark") } else { Label("Copy", systemImage: "doc.on.doc") }
                    }
                } footer: {
                    Text("This code unlocks sync on a new device when you have no other device at hand. Keep it somewhere safe: UrBudget can't show it again or recover it.")
                }
                if let error { FormErrorView(message: error) }
                Section {
                    Button(action: confirm) {
                        HStack {
                            Text("I've saved it").fontWeight(.semibold).frame(maxWidth: .infinity)
                            if saving { ProgressView() }
                        }
                    }
                    .primaryAction()
                    .disabled(saving)
                }
            }
            .navigationTitle("Save your recovery code")
            .inlineTitle()
            .sensoryFeedback(.success, trigger: copied)
        }
        // A swipe down doesn't close it: the code is gone from this device once it's confirmed.
        .interactiveDismissDisabled()
    }

    private func confirm() {
        guard !saving else { return }
        saving = true
        error = nil
        Task {
            defer { saving = false }
            do { try await saved() } catch { self.error = BudgetStore.readable(error) }
        }
    }
}

/// The engine's QR code, dark on white in either theme and never mirrored.
struct QRCodeImage: View {
    var modules: QRModules

    var body: some View {
        if let image = modules.image() {
            Image(image, scale: 1, label: Text("QR code for adding a device"))
                .interpolation(.none)
                .resizable()
                .aspectRatio(1, contentMode: .fit)
                .frame(maxWidth: 240)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
    }
}

/// On a device that syncs: shows a one-time code for a new device to scan or type, and scans the
/// code a new computer shows. It closes itself once the new device has the key, and takes the
/// shown code back when it's closed.
struct AddDeviceSheet: View {
    /// A device was added: the new one collected the key, or a computer's code was approved.
    var added: () -> Void
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var offer: PairingOffer?
    @State private var expired = false
    @State private var offerError: String?
    @State private var attempt = 0
    @State private var scanning = false
    @State private var scanned: String?
    @State private var approving: String?
    @State private var approvalError: String?
    @State private var busy = false

    var body: some View {
        NavigationStack {
            CompactForm {
                Section {
                    Text("Scan this code with the new phone, or type its characters on the new computer where it asks to unlock sync.")
                        .font(.subheadline).foregroundStyle(Color.secondary)
                    shownCode
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                } footer: {
                    if let offer, !expired {
                        Text("It works once, until \(offer.expiresAt.formatted(Date.FormatStyle(date: .omitted, time: .shortened).locale(.app))). Use it only on a device you're holding: anyone with this code can read your budgets.")
                    }
                }
                Section {
                    Button { approvalError = nil; scanning = true } label: {
                        HStack {
                            Label("Scan a computer's code", systemImage: "qrcode.viewfinder")
                            if busy { Spacer(); ProgressView() }
                        }
                    }
                    .disabled(busy)
                    if let approvalError { FormErrorView(message: approvalError) }
                } footer: {
                    Text("A computer shows a code of its own where it asks to unlock sync.")
                }
            }
            .navigationTitle("Add a device")
            .inlineTitle()
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(busy) } }
        }
        .task(id: attempt) { await show() }
        .onDisappear { withdraw() }
        // What was scanned waits for the scanner to close, so the question isn't lost behind it.
        .sheet(isPresented: $scanning, onDismiss: {
            if let scanned { approving = scanned }
            scanned = nil
        }) {
            CodeScannerSheet { scanned = $0 }
        }
        // The key goes to whoever shows that code, so the person says yes first.
        .alert("Add this device?", isPresented: Binding(get: { approving != nil }, set: { if !$0 { approving = nil } }), presenting: approving) { code in
            Button("Add device") { approve(code) }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("Only approve a device you are holding. It will be able to read every budget in this account.")
        }
    }

    @ViewBuilder private var shownCode: some View {
        VStack(spacing: 12) {
            if expired {
                Text("This code has expired.").foregroundStyle(Color.secondary)
                Button("Show a new code") { attempt += 1 }.buttonStyle(.bordered)
            } else if let offer {
                if let qr = offer.qr { QRCodeImage(modules: qr) }
                Text(offer.code)
                    .font(.body.monospaced().weight(.semibold))
                    .lineLimit(1)
                    .minimumScaleFactor(0.5)
                    .textSelection(.enabled)
                    .environment(\.layoutDirection, .leftToRight)
            } else if let offerError {
                FormErrorView(message: offerError)
                Button("Try again") { attempt += 1 }.buttonStyle(.bordered)
            } else {
                ProgressView().padding(.vertical, 24)
            }
        }
        .multilineTextAlignment(.center)
    }

    /// Makes a code and watches for the new device to collect the key sealed for it.
    private func show() async {
        withdraw()
        expired = false
        offerError = nil
        do {
            guard let made = PairingOffer(try await store.service("sync_pair_offer")) else {
                throw AppError(String(localized: "UrBudget couldn't make a code. Try again."))
            }
            // Closed while the code was being made: nothing shows it, so it's taken back.
            guard !Task.isCancelled else { return cancel(made.id) }
            offer = made
            await watch(made)
        } catch {
            if !Task.isCancelled { offerError = BudgetStore.readable(error) }
        }
    }

    private func watch(_ shown: PairingOffer) async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(3))
            guard !Task.isCancelled else { return }
            // An answer that didn't arrive is asked for again: the code still stands.
            let state = try? await store.service("sync_pair_state", ["id": .string(shown.id)])
            guard !Task.isCancelled else { return }
            switch shown.standing(state) {
            case .waiting:
                continue
            case .collected:
                offer = nil
                added()
                dismiss()
                return
            case .expired:
                // Kept until it's replaced or the sheet closes, so it's taken back all the same.
                expired = true
                return
            }
        }
    }

    /// Takes the shown code back, so a late scan of it finds nothing.
    private func withdraw() {
        guard let id = offer?.id else { return }
        offer = nil
        cancel(id)
    }

    private func cancel(_ id: String) {
        let store = store
        Task { _ = try? await store.service("sync_pair_cancel", ["id": .string(id)]) }
    }

    private func approve(_ code: String) {
        guard !busy else { return }
        busy = true
        approvalError = nil
        Task {
            defer { busy = false }
            do {
                _ = try await store.service("sync_pair_approve", ["code": .string(code)])
                added()
                dismiss()
            } catch {
                approvalError = BudgetStore.readable(error)
            }
        }
    }
}
