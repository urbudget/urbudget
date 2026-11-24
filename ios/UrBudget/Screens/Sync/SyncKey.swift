import CoreGraphics
import Foundation

// Sync without a typed passphrase (docs/plans/2026-11-24-sync-device-approval.md). Turning sync on
// makes a recovery code; a device is added by scanning or typing a one-time device code; and one
// engine call, `sync_unlock`, takes whatever was scanned or typed. An account that has a
// passphrase keeps it. The engine makes and checks every code: this file reads its answers.

/// The account's sync key as Account & sync shows it, from `cloud_status().sync` (sync.rs
/// `Status`) and the fresh answer of `sync_key_info`.
enum SyncKey {
    /// Where this device stands with the account's key.
    enum Stage: Equatable {
        /// Not known yet: `sync_key_info` hasn't answered and the last sync left no word.
        case asking
        /// The account has no key: this device can turn sync on.
        case none
        /// The account has a key this device doesn't hold.
        case locked
        case unlocked
    }

    /// `info` is `sync_key_info`'s answer, or nil until it has one (offline, or still asking).
    /// Then the last sync's word that the account has a key is enough to offer unlocking.
    static func stage(_ status: JSON, info: JSON?) -> Stage {
        if status["sync"].flag("unlocked") { return .unlocked }
        if let info { return info.flag("exists") ? .locked : .none }
        return status["sync"]["secret"].isNull ? .asking : .locked
    }

    /// What the key is made from: "code" (a recovery code the app made), "passphrase" (one
    /// someone chose), or "" when the account has no key or this device hasn't heard.
    static func secret(_ status: JSON, info: JSON? = nil) -> String {
        let asked = info?.text("secret") ?? ""
        return asked.isEmpty ? status["sync"].text("secret") : asked
    }

    /// The server holds a recovery copy of the key: a replacement recovery code was made.
    static func hasRecoveryCopy(_ status: JSON, info: JSON? = nil) -> Bool {
        if let info, info.has("recovery") { return info.flag("recovery") }
        return status["sync"].flag("recovery")
    }

    /// This device holds a recovery code the person hasn't said they saved.
    static func recoveryUnsaved(_ status: JSON) -> Bool {
        status["sync"].flag("recoveryUnsaved")
    }

    /// The protocol the account syncs on: `sync_key_info`'s fresh answer where there is one,
    /// else what the last sync saved.
    static func protocolNumber(_ status: JSON, info: JSON? = nil) -> Int64 {
        if let info, info.has("protocol") { return info["protocol"].int }
        return status["sync"]["protocol"].int
    }

    /// A replacement recovery code needs the new sync (recovery.rs `replace`).
    static func canReplaceRecoveryCode(_ status: JSON) -> Bool {
        status["sync"].flag("unlocked") && status["sync"]["protocol"].int == 2
    }

    /// How a code reads (codes.rs `encode`): 24 characters in six groups of four. Shown in an
    /// empty box where one is typed.
    static let codeShape = "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"

    /// Whether a scanned QR code is one of UrBudget's (codes.rs `OFFER` and `REQUEST`, and
    /// whatever a later version adds, which the engine answers with "update first"). The camera
    /// keeps looking past anything else, so a QR code that happens to be in view isn't tried
    /// as a passphrase.
    static func scannable(_ payload: String) -> Bool {
        let text = payload.trimmingCharacters(in: .whitespacesAndNewlines)
        return text.lowercased().hasPrefix("urbudget://") || text.hasPrefix("URBUDGET-")
    }

    /// Whether a scanned code is one a device waiting to be added shows, which a device that
    /// syncs approves, as against one a device that syncs shows.
    static func isRequest(_ payload: String) -> Bool {
        let text = payload.trimmingCharacters(in: .whitespacesAndNewlines)
        return text.lowercased().hasPrefix("urbudget://ask/") || text.hasPrefix("URBUDGET-ASK:")
    }

    /// A time the server wrote, with or without fractions of a second.
    static func date(_ iso: String) -> Date? {
        let parser = ISO8601DateFormatter()
        parser.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return parser.date(from: iso) ?? ISO8601DateFormatter().date(from: iso)
    }
}

/// A recovery code to show until the person says they've saved it: the `recoveryCode` in the
/// answer of `sync_setup`, `sync_reset`, `sync_recovery_create` or `sync_recovery_unsaved`.
/// Nil when the answer carries none.
struct RecoveryCode: Identifiable, Equatable {
    let code: String
    var id: String { code }

    init?(_ answer: JSON) {
        let code = answer.text("recoveryCode")
        guard !code.isEmpty else { return nil }
        self.code = code
    }
}

/// A device code this device is showing for a new one (`sync_pair_offer`): its QR code, its 24
/// characters, and when the server stops keeping the sealed key.
struct PairingOffer: Identifiable, Equatable {
    let id: String
    let code: String
    let qr: QRModules?
    let expiresAt: Date

    /// How long a code is good for when the answer doesn't say (pairing.rs `MINUTES`).
    static let lifetime: TimeInterval = 5 * 60

    init?(_ answer: JSON, now: Date = Date()) {
        id = answer.text("id")
        code = answer.text("code")
        guard !id.isEmpty, !code.isEmpty else { return nil }
        qr = QRModules(answer["qr"])
        expiresAt = SyncKey.date(answer.text("expiresAt")) ?? now.addingTimeInterval(Self.lifetime)
    }

    enum Standing: Equatable {
        /// The sealed key is still on the server for the new device.
        case waiting
        /// The new device collected it: the device is added.
        case collected
        /// The code ran out or was taken back: a new one can be shown.
        case expired
    }

    /// `state` is `sync_pair_state`'s answer, `{waiting, collected}`: collected once the new
    /// device has taken the key, and neither once the code ran out or was taken back. The server
    /// says which, so this device's clock decides nothing while it answers. With no answer
    /// (nil: the request didn't arrive) the code stands until its time is up.
    func standing(_ state: JSON?, now: Date = Date()) -> Standing {
        guard let state else { return now >= expiresAt ? .expired : .waiting }
        if state.flag("collected") { return .collected }
        return state.flag("waiting") ? .waiting : .expired
    }
}

/// The engine's QR code (codes.rs `qr`): `size` squares a side, their rows end to end in
/// `modules`, "1" for dark. Each app draws them itself, so all three draw the same code.
struct QRModules: Equatable {
    let size: Int
    private let dark: [Bool]

    /// The light squares scanners expect around a code.
    static let quietZone = 4

    init?(_ qr: JSON) {
        let size = Int(qr["size"].int)
        let squares = Array(qr.text("modules").utf8)
        // A QR code is 21 to 177 squares a side.
        guard (21...177).contains(size), squares.count == size * size else { return nil }
        var dark: [Bool] = []
        dark.reserveCapacity(squares.count)
        for square in squares {
            switch square {
            case UInt8(ascii: "1"): dark.append(true)
            case UInt8(ascii: "0"): dark.append(false)
            default: return nil
            }
        }
        self.size = size
        self.dark = dark
    }

    /// Whether the square in `row` (from the top) and `column` (from the left) is dark.
    /// Anything outside the code is light.
    func isDark(row: Int, column: Int) -> Bool {
        guard (0..<size).contains(row), (0..<size).contains(column) else { return false }
        return dark[row * size + column]
    }

    /// The code as an image of one pixel a square, black on white whatever the app's theme,
    /// with the quiet zone around it. Drawn without smoothing, it stays sharp at any size.
    func image() -> CGImage? {
        let side = size + Self.quietZone * 2
        var pixels = [UInt8](repeating: 255, count: side * side)
        for row in 0..<size {
            for column in 0..<size where dark[row * size + column] {
                pixels[(row + Self.quietZone) * side + column + Self.quietZone] = 0
            }
        }
        guard let provider = CGDataProvider(data: Data(pixels) as CFData) else { return nil }
        return CGImage(width: side, height: side, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: side,
                       space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    }
}
