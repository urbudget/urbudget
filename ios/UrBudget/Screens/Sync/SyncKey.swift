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

extension SyncKey {
    /// Whether `date` is close enough to `now` to read "just now": under a minute ago, or
    /// ahead of this device's clock.
    static func isJustNow(_ date: Date, now: Date) -> Bool {
        now.timeIntervalSince(date) < 60
    }

    /// How long ago `date` was, as the app's language says it: "5 minutes ago". Never "in 30
    /// seconds": a time ahead of this device's clock reads as a second ago.
    static func ago(_ date: Date, now: Date, locale: Locale = .app) -> String {
        let formatter = RelativeDateTimeFormatter()
        formatter.locale = locale
        formatter.unitsStyle = .full
        formatter.dateTimeStyle = .numeric
        return formatter.localizedString(for: min(date, now.addingTimeInterval(-1)), relativeTo: now)
    }
}

/// A device that syncs with the account, as `sync_devices` lists it (v2.rs `devices`):
/// `{id, name|null, addedAt, lastSeenAt|null, current}`. The list only shows: a device keeps the
/// key it was given until sync is reset.
struct SyncDevice: Identifiable, Equatable {
    let id: String
    /// The name the device gave itself, or "" for one that registered before names were kept.
    let name: String
    let addedAt: Date?
    let lastSeenAt: Date?
    /// The device in hand.
    let current: Bool

    init?(_ row: JSON) {
        id = row.text("id")
        guard !id.isEmpty else { return nil }
        name = row.text("name").trimmingCharacters(in: .whitespacesAndNewlines)
        addedAt = SyncKey.date(row.text("addedAt"))
        lastSeenAt = SyncKey.date(row.text("lastSeenAt"))
        current = row.flag("current")
    }

    /// The rows of `sync_devices`' answer in the engine's order, the device in hand first.
    static func list(_ answer: JSON) -> [SyncDevice] {
        let devices = answer.rows("devices").compactMap(SyncDevice.init)
        return devices.filter(\.current) + devices.filter { !$0.current }
    }

    /// What a device's row is called.
    enum Title: Equatable {
        case named(String)
        /// The device in hand, which has no name yet: "This iPhone".
        case thisDevice
        /// Another device with no name yet: "A device added on 3 November".
        case added(Date)
        case unnamed
    }

    var title: Title {
        if !name.isEmpty { return .named(name) }
        if current { return .thisDevice }
        if let addedAt { return .added(addedAt) }
        return .unnamed
    }

    /// The line under a device's name.
    enum Detail: Equatable {
        /// "This iPhone", under the name of the device in hand.
        case thisDevice
        case syncedJustNow
        /// "Last synced 5 minutes ago."
        case lastSynced(Date)
        /// Added, and not seen since.
        case notYet
        /// Nothing to add: the device in hand, already called "This iPhone".
        case none
    }

    func detail(now: Date) -> Detail {
        if current { return name.isEmpty ? .none : .thisDevice }
        guard let lastSeenAt else { return .notYet }
        return SyncKey.isJustNow(lastSeenAt, now: now) ? .syncedJustNow : .lastSynced(lastSeenAt)
    }
}

/// Account & sync's one status line while sync is on: what the last sync (`cloud_status().sync.last`)
/// left to say, the thing that needs the person first.
enum SyncLine: Equatable {
    case syncing
    /// This many budgets wait for the person, each with its row under Budgets.
    case needsYou(Int)
    case offline
    /// The last sync didn't finish: the engine's words for why, "" when it gave none.
    case failed(String)
    /// Changes made here haven't been sent yet.
    case waiting
    /// Nothing to do: when it last synced, nil when it hasn't yet.
    case on(Date?)

    static func line(syncing: Bool, pending: Bool, report: JSON?) -> SyncLine {
        if syncing { return .syncing }
        guard let report else { return .on(nil) }
        let waiting = SyncBudgets.waitingForPerson(report)
        if waiting > 0 { return .needsYou(waiting) }
        switch report.text("state") {
        case "offline": return .offline
        case "waiting": return .waiting
        case "ok":
            let states = report.rows("budgets").map { $0.text("state") }
            if pending || states.contains(where: { ["waiting", "queued"].contains($0) }) { return .waiting }
            return .on(SyncKey.date(report.text("at")))
        default: return .failed(report.text("message"))
        }
    }
}

/// Which budgets of the last sync Account & sync lists: only the ones that need something.
/// The rest are counted in one line that leads to the budget library.
enum SyncBudgets {
    /// Simply syncing: counted, never listed.
    static let syncing: Set<String> = ["synced", "uploaded", "downloaded", "merged", "waiting", "queued"]
    /// Stopped until the person does something, as `SyncCoordinator.conflicts` counts them.
    static let attention: Set<String> = ["conflict", "error", "update_app", "recovery", "subscription_required", "deletion_conflict"]
    /// Said elsewhere on the screen (sync is locked, the account signed out) or in the budget
    /// library (Trash, a budget not downloaded): neither listed nor counted.
    static let quiet: Set<String> = ["locked", "signed_out", "paused", "trashed", "purged", "evicted", ""]

    /// A row's one action.
    enum Action: Equatable {
        /// Review differences.
        case review
        /// A budget kept only on this device: add it to the account's sync.
        case startSyncing
        /// The old sync only: a budget in the account that isn't on this device.
        case download
        /// A kept copy is recovered in the budget library.
        case library
        case none
    }

    static func action(_ entry: JSON, protocolNumber: Int64) -> Action {
        switch entry.text("state") {
        case "conflict": return .review
        case "local_only": return .startSyncing
        case "recovery": return .library
        case "cloud_only": return protocolNumber == 2 ? .none : .download
        default: return .none
        }
    }

    /// Whether a budget has a row: it waits for the person, there is something to do with it
    /// here, or the engine has something to say about a state this app doesn't know.
    static func listed(_ entry: JSON, protocolNumber: Int64) -> Bool {
        let state = entry.text("state")
        if syncing.contains(state) || quiet.contains(state) { return false }
        if attention.contains(state) || action(entry, protocolNumber: protocolNumber) != .none { return true }
        return state != "cloud_only" && !entry.text("message").isEmpty
    }

    static func needing(_ report: JSON?, protocolNumber: Int64) -> [JSON] {
        (report?.rows("budgets") ?? []).filter { listed($0, protocolNumber: protocolNumber) }
    }

    static func syncingCount(_ report: JSON?) -> Int {
        (report?.rows("budgets") ?? []).count { syncing.contains($0.text("state")) }
    }

    static func waitingForPerson(_ report: JSON?) -> Int {
        (report?.rows("budgets") ?? []).count { attention.contains($0.text("state")) }
    }

    /// The old sync only: the budgets whose cloud copy can be removed, which is every one that has one.
    static func removable(_ report: JSON?, protocolNumber: Int64) -> [JSON] {
        guard protocolNumber != 2 else { return [] }
        return (report?.rows("budgets") ?? []).filter { !["local_only", ""].contains($0.text("state")) && !$0.text("id").isEmpty }
    }

    /// A row's one sentence: the engine's own words where it has some.
    enum Sentence: Equatable {
        case message(String)
        /// Changed on two devices.
        case conflict
        /// A copy kept after a reset.
        case keptCopy
        case deviceOnly
        /// Nothing more than what the state is called.
        case state(String)
    }

    static func sentence(_ entry: JSON) -> Sentence {
        let message = entry.text("message")
        if !message.isEmpty { return .message(message) }
        switch entry.text("state") {
        case "conflict": return .conflict
        case "recovery": return .keptCopy
        case "local_only": return .deviceOnly
        case let state: return .state(state)
        }
    }
}

/// The Sync subscription as Account & sync places it, from the account overview (`/me`): beside
/// the account while there is something to do or to know, and under More otherwise.
enum SyncPlan {
    static func active(_ overview: JSON?) -> Bool {
        overview?["sync"]["active"].bool ?? false
    }

    /// The subscription can be bought: there is none, or sync is given by UrBudget (the App
    /// Review account's), where the purchase can still be tried.
    static func offered(_ overview: JSON?) -> Bool {
        guard let overview, overview["sync"].isObject else { return false }
        return !active(overview) || overview["subscription"].text("provider") == "comp"
    }

    /// The App Store couldn't take the payment.
    static func unpaid(_ overview: JSON?) -> Bool {
        overview?["subscription"].text("status") == "unpaid"
    }

    /// An active plan that won't renew: it has a day it ends and none it renews.
    static func ending(_ overview: JSON?) -> Bool {
        guard let plan = overview?["subscription"], active(overview) else { return false }
        return SyncKey.date(plan.text("renewsAt")) == nil && SyncKey.date(plan.text("endsAt")) != nil
    }

    /// Whether the plan has its place beside the account: no plan, one that is ending, a payment
    /// that failed. A plan in good standing takes no room there.
    static func needsAttention(_ overview: JSON?) -> Bool {
        guard let overview, overview["sync"].isObject else { return false }
        return offered(overview) || unpaid(overview) || ending(overview)
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
