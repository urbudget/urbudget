import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Appends tokens once and publishes them at a bounded cadence, instead of copying the reply for
/// every token (Android's AssistantStreamBuffer).
final class AssistantStreamBuffer {
    private var reply = ""
    private var reasoning = ""
    private var changed = false

    func append(_ text: String, thinking: Bool = false) {
        guard !text.isEmpty else { return }
        if thinking { reasoning += text } else { reply += text }
        changed = true
    }

    func clearReply() {
        reply = ""
        changed = true
    }

    func clear() {
        reply = ""
        reasoning = ""
        changed = false
    }

    /// The reply and the thinking, when either changed since the last call.
    func drain() -> (reply: String, thinking: String)? {
        guard changed else { return nil }
        changed = false
        return (reply, reasoning)
    }
}

/// One row of the conversation as the service shows it: a message, step, proposal, error or stop.
/// An assistant message carries its Markdown already read, off the main thread.
struct AssistantEntry: Identifiable, Equatable, Sendable {
    let id: Int
    let json: JSON
    let blocks: [AssistantMarkdownBlock]
    var kind: String { json.text("kind") }
}

/// A conversation ready to draw. Unchanged messages keep their prepared Markdown between reloads.
struct AssistantThreadPresentation: Sendable {
    var thread: JSON
    var entries: [AssistantEntry]

    static func make(_ thread: JSON, previous: AssistantThreadPresentation?) -> AssistantThreadPresentation {
        let sameThread = previous?.thread.text("id") == thread.text("id")
        if sameThread, let previous, previous.thread == thread { return previous }
        let entries = thread.rows("entries").enumerated().map { index, entry -> AssistantEntry in
            if sameThread, let old = previous?.entries, index < old.count, old[index].json == entry { return old[index] }
            let blocks = entry.text("kind") == "assistant" ? AssistantMarkdown.blocks(entry.text("text")) : []
            return AssistantEntry(id: index, json: entry, blocks: blocks)
        }
        return AssistantThreadPresentation(thread: thread, entries: entries)
    }
}

/// One part of "With this message", shown above the field with its own ×.
struct AssistantWithPart: Identifiable, Equatable {
    enum Kind: String {
        /// Where the person is: the page, its month, the account or category in view.
        case place
        /// The transactions they selected.
        case selection
        /// The bank file being imported.
        case importing
    }

    var kind: Kind
    var text: String
    var id: String { kind.rawValue }
}

/// What goes with the next message besides its words: where the person is, the transactions they
/// selected, and the bank file being imported. Each part can be taken off before it's sent, and
/// the next message starts with them all again.
struct AssistantWith: Equatable {
    /// The page, by the key the service knows it by ("budget", "transactions", "accounts",
    /// "import"…); "assistant" when it was opened from nowhere in particular.
    var page = "assistant"
    /// The month the app is on (YYYY-MM). It goes only with a page that shows one.
    var month = ""
    /// The account and the category in view, by id.
    var account = ""
    var category = ""
    var selected: [String] = []
    /// The bank file being imported, when the assistant is open over its import.
    var importing: JSON?
    /// The person took where they are, or the file being imported, off this message.
    var placeOff = false
    var importOff = false

    /// The pages that show a month: Budget, and the assistant opened over the app, which is on
    /// the month Budget shows.
    static func showsMonth(_ page: String) -> Bool { page == "budget" || page == "assistant" }

    /// The `screen` a message is sent with. A place taken off is an empty `page` with no month,
    /// account or category, as the service reads it.
    var screen: JSON {
        let file: JSON = importOff ? .null : importing ?? .null
        var out: JSON = ["platform": "ios", "page": .string(placeOff ? "" : page), "today": .string(Dates.today),
                         "selected": JSON(selected), "import": file]
        guard !placeOff else { return out }
        if Self.showsMonth(page) && !month.isEmpty { out["month"] = .string(month) }
        if !account.isEmpty { out["accountId"] = .string(account) }
        if !category.isEmpty { out["categoryId"] = .string(category) }
        return out
    }

    /// The parts still on the message, in the order they're shown. `budget` names the account
    /// and the category.
    func parts(budget: JSON?) -> [AssistantWithPart] {
        var out: [AssistantWithPart] = []
        let accountName = account.isEmpty ? "" : budget?.row("accounts", id: account)?.text("name") ?? ""
        let categoryName = category.isEmpty ? "" : budget?.row("categories", id: category)?.text("name") ?? ""
        if !placeOff, let named = Self.place(page: page, month: Self.showsMonth(page) ? month : "", account: accountName, category: categoryName) {
            out.append(AssistantWithPart(kind: .place, text: named))
        }
        if !selected.isEmpty { out.append(AssistantWithPart(kind: .selection, text: Self.selection(selected.count))) }
        if importing != nil && !importOff { out.append(AssistantWithPart(kind: .importing, text: String(localized: "The file being imported"))) }
        return out
    }

    /// The same line on a sent message, from what the service kept with it (the entry's `with`):
    /// nil for a message from before that was kept, or one nothing went with.
    static func sent(_ with: JSON) -> String? {
        guard with.isObject else { return nil }
        var parts: [String] = []
        if let named = Self.place(page: with.text("page"), month: with.text("month"), account: with.text("account"), category: with.text("category")) {
            parts.append(named)
        }
        let transactions = Int(with.long("transactions"))
        if transactions > 0 { parts.append(Self.selection(transactions)) }
        if with.flag("import") { parts.append(String(localized: "The file being imported")) }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    /// "Budget · November 2026", then the account's or the category's name; nil when there's no
    /// place to name (it was taken off, or the page is one this version doesn't know and nothing
    /// else came with it).
    static func place(page: String, month: String, account: String, category: String) -> String? {
        var parts: [String] = []
        if let name = pageName(page) { parts.append(name) }
        if Dates.isMonth(month) { parts.append(Dates.monthLabel(month)) }
        if !account.isEmpty { parts.append(account) }
        if !category.isEmpty { parts.append(category) }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    /// "3 selected transactions".
    static func selection(_ count: Int) -> String { String(localized: "\(count) selected transactions", locale: .app) }

    /// The page's name as the app's own tabs and More list write it.
    static func pageName(_ page: String) -> String? {
        switch page {
        case "budget": return String(localized: "Budget")
        case "transactions": return String(localized: "Transactions")
        case "accounts": return String(localized: "Accounts")
        case "reports": return String(localized: "Reports")
        case "payees": return String(localized: "Payees & rules")
        case "scheduled": return String(localized: "Scheduled")
        case "settings": return String(localized: "Settings & data")
        case "import": return String(localized: "Import bank file")
        case "bankConnections": return String(localized: "Bank connections")
        case "assistant": return String(localized: "Assistant")
        default: return nil
        }
    }
}

/// The transactions that can go with one message.
enum AssistantSelection {
    /// As many as the service takes with a message.
    static let limit = 200

    /// A selection handed over from another screen: its first 200, and how many there were when
    /// that's more, for the sentence that says so.
    static func capped(_ ids: [String]) -> (ids: [String], total: Int?) {
        guard ids.count > limit else { return (ids, nil) }
        return (Array(ids.prefix(limit)), ids.count)
    }
}

/// The message being written in a conversation: its words and the transactions selected for it.
/// The controller keeps one per conversation, so closing the sheet or looking at another
/// conversation doesn't lose a long message.
struct AssistantDraft: Equatable {
    var text = ""
    var selected: [String] = []
    /// How many transactions were handed over when only the first 200 could be attached.
    var total: Int?

    var isEmpty: Bool { text.isEmpty && selected.isEmpty }

    /// `text` added to what's written already, on a line of its own: a request from another
    /// screen or a card's Ask for changes never replaces a message being written. Text that's
    /// there already isn't added twice.
    static func adding(_ text: String, to draft: String) -> String {
        guard !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return text }
        guard !text.isEmpty, !draft.contains(text) else { return draft }
        return draft + (draft.hasSuffix("\n") ? "" : "\n") + text
    }
}

/// When the conversation follows new text: only while its end is on screen. Scrolling up stops
/// it; coming back to the end, or Jump to latest, starts it again.
enum AssistantFollow {
    /// How far below the bottom of the screen the end may be and still count as on screen.
    static let slack: CGFloat = 32
    /// How far the content may settle downwards on its own (a lazy stack measuring its rows)
    /// before that counts as the person scrolling up.
    static let drift: CGFloat = 8

    /// Whether to follow after the content moved or grew. `top` and `bottom` are its edges in the
    /// scroll view's coordinates, `restingTop` where its top rests while following (`resting`),
    /// `viewport` the scroll view's height. With the end on screen it follows. Otherwise new
    /// text, which grows the content and leaves its top where it was, changes nothing; only the
    /// content having moved down from where it rested, which is the person scrolling up however
    /// slowly, stops it.
    static func next(_ following: Bool, top: CGFloat, restingTop: CGFloat, bottom: CGFloat, viewport: CGFloat) -> Bool {
        if bottom <= viewport + slack { return true }
        return following && top <= restingTop + drift
    }

    /// Where a scroll up is measured from after this move. With the end on screen that's where
    /// the top is now; otherwise it only ever goes up, as text is added and scrolled to.
    static func resting(_ resting: CGFloat, top: CGFloat, bottom: CGFloat, viewport: CGFloat) -> CGFloat {
        bottom <= viewport + slack ? top : min(resting, top)
    }
}

/// The saved conversations whose title has what was searched for.
enum AssistantHistory {
    static func matching(_ threads: [JSON], _ search: String) -> [JSON] {
        let query = search.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return threads }
        return threads.filter { $0.text("title").range(of: query, options: [.caseInsensitive, .diacriticInsensitive]) != nil }
    }
}

/// The tokens a conversation's requests used, as one quiet line. No prices: plans and keys differ.
enum AssistantUsage {
    /// Nil when nothing was counted: a conversation from before counting, or with no reply yet.
    static func line(_ usage: JSON) -> String? {
        let sent = usage.long("input")
        let cached = usage.long("cached")
        let received = usage.long("output")
        guard sent != 0 || cached != 0 || received != 0 else { return nil }
        return String(localized: "Tokens: \(grouped(sent)) sent (\(grouped(cached)) from cache), \(grouped(received)) received")
    }

    /// "12,345": thousands separated, in Western digits as the app's other numbers.
    private static func grouped(_ value: Int64) -> String { Int(clamping: value).formatted(.number.locale(.app)) }
}

/// The line of details a report of a reply offers to send with it, and how it joins the reason.
enum AssistantReportDetails {
    /// The reason's limit, in UTF-16 units as the service counts it.
    static let reasonLimit = 1000

    /// The app's version as the App Store shows it ("1.0.1").
    static var appVersion: String { Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "" }

    /// "App: UrBudget iPhone 1.0.1 · Service: ChatGPT · Model: default · Tools: none · Language: en".
    /// Never translated: it's read by whoever looks into the report.
    static func line(version: String, provider: String, model: String, tools: [String], language: String) -> String {
        let service: String
        switch provider {
        case "anthropic": service = "Anthropic"
        case "openai": service = "OpenAI"
        default: service = "ChatGPT"
        }
        let chosen = model.isEmpty ? "default" : model
        let called = tools.isEmpty ? "none" : tools.joined(separator: ", ")
        return ["App: UrBudget iPhone \(version)", "Service: \(service)", "Model: \(chosen)", "Tools: \(called)", "Language: \(language)"]
            .joined(separator: " · ")
    }

    /// The reason as it's sent: the person's words, a blank line, then the details, cut to what
    /// still fits in the reason's 1,000 characters. Their own words are never cut to make room.
    static func reason(_ reason: String, details: String) -> String {
        let reason = reason.trimmingCharacters(in: .whitespacesAndNewlines)
        let details = details.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !details.isEmpty else { return reason }
        guard !reason.isEmpty else { return assistantLimited(details, utf16: reasonLimit) }
        let room = reasonLimit - reason.utf16.count - 2
        guard room > 0 else { return reason }
        return reason + "\n\n" + assistantLimited(details, utf16: room)
    }
}

/// What the button under an error does, as the service names it in the entry's `action`. Only the
/// conversation's last entry carries one: an earlier error's moment has passed.
enum AssistantErrorAction: String {
    /// Sign in with ChatGPT again, then ask again.
    case signIn
    /// The key, the model or the service needs changing: the assistant's settings.
    case settings
    /// The plan's limit was reached: ChatGPT's usage page.
    case usage
    /// The conversation no longer fits the model: carry it on in a new one.
    case newConversation
    /// Ask again.
    case retry

    /// Nil when the error offers nothing: it has no `action`, or "none" (asking again wouldn't
    /// help), or one this version doesn't know.
    static func of(_ entry: JSON) -> AssistantErrorAction? { AssistantErrorAction(rawValue: entry.text("action")) }
}

/// The notice before a first message to a service: what gets sent, and to whom.
enum AssistantNotice {
    /// The company the chosen service's messages go to: a ChatGPT plan and an OpenAI key are both
    /// OpenAI's. A name, so it isn't translated.
    static func recipient(_ provider: String) -> String { provider == "anthropic" ? "Anthropic" : "OpenAI" }
}

/// Conversations and reports as files to save or share.
enum AssistantExport {
    /// The conversation as Markdown, proposals written out with their rows (Android's conversationMarkdown).
    static func conversationMarkdown(_ thread: JSON, currency: String) -> String {
        var out = "# \(thread.text("title"))\n\n"
        for entry in thread.rows("entries") {
            if entry.text("kind") == "proposal" {
                let proposal = entry["proposal"]
                guard proposal.isObject else { continue }
                out += "## " + String(localized: "Proposal: \(proposal.text("title")) (\(statusWord(proposal.text("status"))))") + "\n\n"
                let card = proposal["card"]
                if card.isObject {
                    for stat in card.rows("stats") {
                        let value = stat["amount"].isNull ? stat.text("text") : money(stat.long("amount"), currency)
                        out += "- \(stat.text("label")): \(value)\n"
                    }
                    for row in card.rows("rows") + (card["total"].isObject ? [card["total"]] : []) {
                        out += "- \(row.text("label")): \(row.text("detail"))"
                        if row.has("amount") { out += " \(money(row.long("amount"), currency))" }
                        if row.has("before") && row.has("after") {
                            out += " (\(money(row.long("before"), currency)) → \(money(row.long("after"), currency)))"
                        }
                        if !row.text("badge").trimmingCharacters(in: .whitespaces).isEmpty { out += " · \(row.text("badge"))" }
                        out += "\n"
                    }
                    for key in ["warning", "note"] where !card.text(key).trimmingCharacters(in: .whitespaces).isEmpty {
                        out += "\n\(card.text(key))\n"
                    }
                }
                out += "\n"
            } else {
                out += "## \(heading(entry.text("kind")))\n\n\(entry.text("text"))\n\n"
                for file in entry.rows("attachments") {
                    // The size as a plain number ("2048"): a localized count would group its digits.
                    let name = file.text("name"), size = String(file.long("size"))
                    out += String(localized: "Attachment: \(name) (\(size) bytes)", comment: "Exported conversation: an attached file's name and size") + "\n"
                }
            }
        }
        return out
    }

    /// A conversation row's heading in the export: "User", "Assistant", "Step"…
    private static func heading(_ kind: String) -> String {
        switch kind {
        case "user": return String(localized: "User", comment: "Heading for the person's messages in an exported conversation")
        case "assistant": return String(localized: "Assistant")
        case "step": return String(localized: "Step", comment: "Heading for a step the assistant took, in an exported conversation")
        case "error": return String(localized: "Error", comment: "Heading for an error in an exported conversation")
        case "stopped": return String(localized: "Stopped", comment: "Heading for a reply that was stopped, in an exported conversation")
        case "cut": return String(localized: "The reply was cut short.")
        default: return kind.prefix(1).uppercased() + kind.dropFirst()
        }
    }

    /// A proposal's status in the export, lowercase as it follows the title.
    private static func statusWord(_ status: String) -> String {
        switch status {
        case "proposed": return String(localized: "proposed", comment: "A proposal's status in an exported conversation")
        case "applied": return String(localized: "applied", comment: "A proposal's status in an exported conversation")
        case "discarded": return String(localized: "discarded", comment: "A proposal's status in an exported conversation")
        case "undone": return String(localized: "undone", comment: "A proposal's status in an exported conversation")
        case "failed": return String(localized: "failed", comment: "A proposal's status in an exported conversation")
        default: return status
        }
    }

    /// A title as a file name: letters, digits, spaces, "_" and "-" only, at most 60 characters.
    /// One with no title is named in the app's language.
    static func fileName(_ title: String, extension suffix: String,
                         fallback: String = String(localized: "file.conversation", defaultValue: "Conversation", comment: "The file name of an exported conversation that has no title.")) -> String {
        let cleaned = String(title.prefix(60)).map { character -> String in
            character.isLetter || character.isNumber || " _-".contains(character) ? String(character) : "_"
        }.joined().trimmingCharacters(in: .whitespaces)
        return (cleaned.isEmpty ? fallback : cleaned) + "." + suffix
    }

    /// Writes `data` to a temporary file for the share sheet.
    static func temporaryFile(named name: String, data: Data) throws -> URL {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("AssistantExports", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let url = folder.appendingPathComponent(name)
        try data.write(to: url, options: .atomic)
        return url
    }

    /// A file the services wrote in the budget's exports folder, checked before it's shared.
    static func savedFile(_ path: String) throws -> URL {
        let url = URL(fileURLWithPath: path)
        let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
        guard values?.isRegularFile == true else { throw AppError(String(localized: "The generated file is unavailable.")) }
        guard (values?.fileSize ?? 0) <= 64 * 1024 * 1024 else { throw AppError(String(localized: "This file is too large to share.")) }
        return url
    }
}

/// Files and photos for a message, in the forms the services read.
enum AssistantAttachments {
    static let limit = 5
    static let maxBytes = 25 * 1024 * 1024
    /// What the services read (assistant/attachments.rs), plus photos in HEIC, which become JPEG.
    static let extensions = ["png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "pdf", "xlsx", "xlsm", "xls", "xlsb", "ods",
                             "csv", "tsv", "txt", "ofx", "qfx", "qif", "json", "md"]

    static var contentTypes: [UTType] {
        var seen = Set<String>()
        return ([.pdf, .png, .jpeg, .gif, .webP, .heic, .commaSeparatedText, .tabSeparatedText, .plainText, .json, .spreadsheet]
            + extensions.compactMap { UTType(filenameExtension: $0) })
            .filter { seen.insert($0.identifier).inserted }
    }

    /// Reads a file chosen in Files, within the 25 MiB a message may carry.
    static func read(_ url: URL) throws -> (name: String, data: Data) {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
        guard size <= maxBytes else { throw AppError(String(localized: "Choose a file smaller than 25 MiB.")) }
        let data = try Data(contentsOf: url)
        guard data.count <= maxBytes else { throw AppError(String(localized: "Choose a file smaller than 25 MiB.")) }
        return (url.lastPathComponent.isEmpty ? "attachment.txt" : url.lastPathComponent, data)
    }

    /// A large photo is scaled to 2,400 pixels and sent as JPEG, as Android does; HEIC always is.
    static func prepare(name: String, data: Data) -> (name: String, data: Data) {
        let suffix = (name as NSString).pathExtension.lowercased()
        let heic = ["heic", "heif"].contains(suffix)
        let large = data.count > 3_500_000 && ["jpg", "jpeg", "png", "webp"].contains(suffix)
        guard heic || large, let jpeg = jpeg(data) else { return (name, data) }
        return ((name as NSString).deletingPathExtension + ".jpg", jpeg)
    }

    /// A photo from the library: kept as it is when the services read its format, otherwise JPEG.
    /// The library gives no file name, so it's "Photo 2" in the app's language.
    static func photo(_ data: Data, type: UTType?, number: Int) throws -> (name: String, data: Data) {
        let kept: [(UTType, String)] = [(.png, "png"), (.jpeg, "jpg"), (.gif, "gif"), (.webP, "webp")]
        let title = String(localized: "Photo \(number)", locale: .app, comment: "The file name of a photo attached from the library: Photo 1, Photo 2…")
        if let type, let match = kept.first(where: { type.conforms(to: $0.0) }) {
            return prepare(name: "\(title).\(match.1)", data: data)
        }
        guard let jpeg = jpeg(data) else { throw AppError(String(localized: "This photo couldn't be read. Choose another, or attach it as a file.")) }
        return ("\(title).jpg", jpeg)
    }

    /// The image at most `maxPixel` on its longer side, upright, as JPEG at 85% quality.
    static func jpeg(_ data: Data, maxPixel: Int = 2400) -> Data? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
                                        kCGImageSourceCreateThumbnailWithTransform: true,
                                        kCGImageSourceThumbnailMaxPixelSize: maxPixel]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(output, UTType.jpeg.identifier as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(destination, image, [kCGImageDestinationLossyCompressionQuality: 0.85] as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { return nil }
        return output as Data
    }

    /// The SF Symbol for an attachment's kind.
    static func symbol(_ kind: String) -> String {
        switch kind {
        case "image": return "photo"
        case "pdf": return "doc.richtext"
        case "sheet", "csv": return "tablecells"
        default: return "doc.text"
        }
    }
}

/// Keeps `text` within `limit` UTF-16 code units (the service's and browser's measure), whole characters only.
func assistantLimited(_ text: String, utf16 limit: Int) -> String {
    guard text.utf16.count > limit else { return text }
    var out = ""
    var count = 0
    for character in text {
        let size = character.utf16.count
        if count + size > limit { break }
        out.append(character)
        count += size
    }
    return out
}

/// What a proposal card can offer, as the service names it in the card's `actions`. The card
/// draws a button for each one it knows, in the order given, and leaves out any other: a newer
/// service's action, Android's own (applying bank texts), or a report's Print, which is on the
/// report's own page here.
enum AssistantCardAction: String, Equatable, Sendable {
    case apply, applyToImport, askChanges, useOriginal, discard, remove, restore, undo, askAgain, show
    case preview, excel, open, save, reviewImport, saveCsv, answer
}

/// A proposal card as the service's view of it says to draw it (its `kind`, `status`, `made`,
/// `actions`, `message` and `appliedAt`). Nothing here works out from kind and status what a
/// card offers: that is the service's `actions`.
enum AssistantCard {
    /// The kinds of card this version draws in full. Any other is drawn plainly, with a line
    /// saying an update is needed.
    static let kinds: Set<String> = ["budget", "transactions", "payees", "notes", "categories", "schedules", "delete", "fix", "bankTexts",
                                     "report", "spreadsheet", "import", "figures", "question"]

    static func known(_ proposal: JSON) -> Bool { kinds.contains(proposal.text("kind")) }

    /// The actions the card offers that this version knows, in the service's order.
    static func actions(_ proposal: JSON) -> [AssistantCardAction] {
        proposal["actions"].array.compactMap { AssistantCardAction(rawValue: $0.string) }
    }

    /// The card's main button: the first action the service gives, when this version knows it.
    /// Remove is in the card's menu and a question's answers are its rows, so neither is one.
    static func main(_ proposal: JSON) -> AssistantCardAction? {
        guard let first = proposal["actions"].array.first, let action = AssistantCardAction(rawValue: first.string),
              action != .remove, action != .answer else { return nil }
        return action
    }

    /// The buttons of a card that waits, in the service's order, the main one included. Undo
    /// shows only while the card's change is the one the budget's Undo would take back.
    static func buttons(_ proposal: JSON, canUndo: Bool) -> [AssistantCardAction] {
        actions(proposal).filter { action in
            switch action {
            case .remove, .answer: return false
            case .undo: return canUndo
            default: return true
            }
        }
    }

    /// A card that's done with: applied, set aside, undone or replaced. It folds to one line.
    static func finished(_ proposal: JSON) -> Bool {
        ["applied", "discarded", "undone", "replaced"].contains(proposal.text("status"))
    }

    /// The actions a finished card keeps on its line.
    private static let onLine: [AssistantCardAction] = [.undo, .restore, .show]

    /// The buttons a finished card keeps on its line: Undo (as in `buttons`), Restore and Show.
    static func lineActions(_ proposal: JSON, canUndo: Bool) -> [AssistantCardAction] {
        buttons(proposal, canUndo: canUndo).filter { onLine.contains($0) }
    }

    /// The buttons a finished card shows once it's opened: whatever else it still offers.
    static func openedActions(_ proposal: JSON) -> [AssistantCardAction] {
        buttons(proposal, canUndo: false).filter { !onLine.contains($0) }
    }

    /// The words on a finished card's line, before its title: "Applied 21/10/2026, 14:30" in the
    /// budget's date format (plain "Applied" when the service kept no time), "Undone", "Replaced
    /// by a newer version", and "Discarded", or "Removed" for a thing the assistant made.
    static func finishedWords(_ proposal: JSON) -> String {
        switch proposal.text("status") {
        case "applied":
            guard let at = LibraryDates.instant(proposal.text("appliedAt")) else { return String(localized: "Applied") }
            return String(localized: "Applied \(LibraryDates.label(at))")
        case "undone": return String(localized: "Undone")
        case "replaced": return String(localized: "Replaced by a newer version")
        default: return proposal.flag("made") ? String(localized: "Removed") : String(localized: "Discarded")
        }
    }

    /// A question that was answered: its line reads "Asked: …" in place of a state and a title.
    static func asked(_ proposal: JSON) -> String? {
        guard proposal.text("kind") == "question", proposal.text("status") == "applied" else { return nil }
        return String(localized: "Asked: \(proposal.text("title"))")
    }

    /// What the card has to say about itself: its words and their tone ("info", "warn" or
    /// "error"). Nil when it says nothing.
    static func message(_ proposal: JSON) -> (tone: String, text: String)? {
        let text = proposal["message"].text("text")
        guard !text.isEmpty else { return nil }
        return (tone: proposal["message"].text("tone"), text: text)
    }

    /// What Ask for a new version sends: the card's title, and why it couldn't be applied.
    static func askAgain(_ proposal: JSON) -> String {
        let title = proposal.text("title")
        let reason = proposal["message"].text("text")
        return String(localized: "Prepare “\(title)” again. It couldn't be applied: \(reason)")
    }

    /// A change to the budget that the budget's own changes move: one that waits is made again
    /// on the budget each time the conversation is read, and the budget's Undo and Redo take an
    /// applied one back and put it back. A conversation with one is read again when the budget changes.
    static func followsBudget(_ proposal: JSON) -> Bool {
        !proposal.flag("made") && !["question", "fix", "bankTexts"].contains(proposal.text("kind"))
            && ["proposed", "undone", "applied"].contains(proposal.text("status"))
    }

    /// How long each row's bar is on a figures card: the row's share of the largest amount on
    /// the card, by absolute value, from 0 to 1. A card with no amounts has bars of no length.
    static func barShares(_ rows: [JSON]) -> [Double] {
        let sizes = rows.map { Double($0.long("amount").magnitude) }
        guard let largest = sizes.max(), largest > 0 else { return rows.map { _ in 0 } }
        return sizes.map { $0 / largest }
    }

    /// How many transactions a delete card deletes: its rows still chosen, or every one of them
    /// (`more` counts those left off the card) when none can be left out.
    static func deleting(_ rows: [JSON], more: Int) -> Int {
        let keyed = rows.filter { !$0.text("key").isEmpty }
        return keyed.isEmpty ? rows.count + more : keyed.filter { !$0.flag("skipped") }.count
    }

    /// What a `fix` card is marked with when its import took only some of its fixes: "Fixes
    /// applied: 1 of 3. Not applied: …", naming the rows that didn't take, up to five. Nil when
    /// every one took. `results` is what the import says became of each fix (`bankApplyFixes`),
    /// in the order of the card's `fixes`.
    static func fixNote(_ results: [JSON], proposal: JSON) -> String? {
        let total = results.count
        let applied = results.filter { $0.flag("applied") }.count
        guard applied < total else { return nil }
        let rows = proposal["card"].rows("rows")
        let fields = proposal["fixes"].rows("fields")
        var places: [String: Int] = [:]
        var labels: [String] = []
        for result in results {
            let kind = result.text("fix")
            let place = places[kind, default: 0]
            places[kind] = place + 1
            guard !result.flag("applied") else { continue }
            let line = result.text("line")
            // The row's key on the card, as the service makes it for each kind of fix.
            var key = ""
            switch kind {
            case "structure": key = "x"
            case "include": key = "i:\(line)"
            case "field": if fields.indices.contains(place) { key = "f:\(line):\(fields[place].text("field"))" }
            default: break
            }
            var label = rows.first { !key.isEmpty && $0.text("key") == key }?.text("label") ?? ""
            if label.isEmpty && !line.isEmpty { label = String(localized: "Line \(line)") }
            if !label.isEmpty && !labels.contains(label) { labels.append(label) }
        }
        let comma = AppLanguage.isRightToLeft ? "، " : ", "
        var list = labels.prefix(5).joined(separator: comma)
        if labels.count > 5 { list += comma + "…" }
        return String(localized: "Fixes applied: \(applied) of \(total). Not applied: \(list)", locale: .app)
    }
}

/// The notes the assistant keeps for the open budget (its `assistantNotes`), as the assistant's
/// settings list and save them: always the whole list.
enum AssistantNotes {
    /// A note's length, in the characters the service counts.
    static let length = 200

    /// The budget's notes; none when it has no `assistantNotes`.
    static func list(_ budget: JSON?) -> [String] {
        (budget?["assistantNotes"].array ?? []).map(\.string).filter { !$0.isEmpty }
    }

    /// `text` cut to a note's length, whole characters only.
    static func limited(_ text: String) -> String {
        var out = ""
        var count = 0
        for character in text {
            let size = character.unicodeScalars.count
            if count + size > length { break }
            out.append(character)
            count += size
        }
        return out
    }

    /// `note` as the service keeps one: on one line with single spaces, and at most 200 characters.
    static func cleaned(_ note: String) -> String {
        limited(note.split(whereSeparator: \.isWhitespace).joined(separator: " ")).trimmingCharacters(in: .whitespaces)
    }

    /// The list with `note` at its end; nil when there's nothing to add, or it's there already.
    static func adding(_ note: String, to notes: [String]) -> [String]? {
        let note = cleaned(note)
        guard !note.isEmpty, !notes.contains(note) else { return nil }
        return notes + [note]
    }

    /// The list without the note at `index`.
    static func removing(_ index: Int, from notes: [String]) -> [String] {
        guard notes.indices.contains(index) else { return notes }
        var out = notes
        out.remove(at: index)
        return out
    }
}

/// Starting points other screens offer: the assistant opened with a request in the message
/// field for the person to send. Nothing is sent by itself.
enum AssistantStart {
    /// What the Budget tab's overspent notice asks.
    static var cover: String { String(localized: "Cover my overspent categories this month.") }

    /// The transactions an import added that still have no category, and the accounts they're in.
    struct Imported: Equatable, Sendable {
        var ids: [String] = []
        var accounts: [String] = []
    }

    /// The transactions in `after` that weren't in `before` and still have no category: none at
    /// all, or the Uncategorized placeholder (the desktop's uncategorizedImports). Payee rules may
    /// have categorized the rest. Tracking accounts use no categories, and transfers need none.
    static func uncategorized(before: JSON, after: JSON) -> Imported {
        let known = Set(before.rows("transactions").map { $0.text("id") })
        var budgeted: [String: String] = [:]
        for account in after.rows("accounts") where account.text("kind") != "tracking" && account.flag("onBudget", true) {
            budgeted[account.text("id")] = account.text("name")
        }
        var placeholders: Set<String> = ["local-uncategorized"]
        for category in after.rows("categories") where ["Uncategorized", "Uncategorized Transactions"].contains(category.text("name")) {
            placeholders.insert(category.text("id"))
        }
        var found = Imported()
        var seen: Set<String> = []
        for transaction in after.rows("transactions") {
            let account = transaction.text("accountId")
            guard !known.contains(transaction.text("id")), let name = budgeted[account], transaction.text("transferAccountId").isEmpty else { continue }
            let open = transaction.rows("splits").allSatisfy { part in
                part.text("transferAccountId").isEmpty && (part.text("categoryId").isEmpty || placeholders.contains(part.text("categoryId")))
            }
            guard open else { continue }
            found.ids.append(transaction.text("id"))
            if seen.insert(account).inserted { found.accounts.append(name) }
        }
        return found
    }

    /// What the assistant is asked about them, ready to send with them attached (the desktop's
    /// categorizeText). A message takes 200: over that, it says these are the first of them.
    static func categorize(_ imported: Imported) -> String {
        let place = joined(imported.accounts)
        let count = imported.ids.count
        if count > AssistantSelection.limit {
            return String(localized: "Categorize these 200 of the \(count) new transactions in \(place).", locale: .app)
        }
        return String(localized: "Categorize the \(count) new transactions in \(place).", locale: .app)
    }

    /// "Checking", "Checking and Savings", "Checking, Savings and Cash".
    static func joined(_ names: [String]) -> String {
        guard names.count > 1, let last = names.last else { return names.first ?? "" }
        let items = names.dropLast().joined(separator: AppLanguage.isRightToLeft ? "، " : ", ")
        return String(localized: "\(items) and \(last)")
    }

    /// "Categorize with the assistant", for the message that says an import added transactions:
    /// it opens the assistant on a conversation of its own with those transactions attached and
    /// the request in the field. Nil when every one of them has a category.
    @MainActor static func offer(_ imported: Imported) -> ToastAction? {
        guard !imported.ids.isEmpty else { return nil }
        let request = AssistantStart.categorize(imported)
        return ToastAction(label: String(localized: "Categorize with the assistant")) {
            AppActions.shared.asking = AssistantRequest(page: "transactions", text: request, selected: imported.ids)
        }
    }
}

/// What the person chose on a proposal card, as `assistant_revise` takes it (the desktop's
/// ProposalCard, Android's ProposalChoices): `skip` lists the keys of rows left out, `set` the rows
/// given another category (null: Ready to Assign) or amount. Each choice is made from the card's
/// current `choices`.
enum ProposalChoices {
    /// The assistant's own version: nothing left out or changed.
    static let original: JSON = ["skip": [], "set": []]

    /// `choices` with the row `key` left out, or put back when it was; one put back loses an amount
    /// of nothing given to it.
    static func toggle(_ choices: JSON, key: String, skipped: Bool) -> JSON {
        let skip = choices["skip"].array.map(\.string)
        let set = choices.rows("set")
        guard skipped else { return revision(skip + [key], set) }
        return revision(skip.filter { $0 != key }, set.filter { !($0.text("key") == key && !$0["amount"].isNull && $0.long("amount") == 0) })
    }

    /// `choices` with `change` (a category and/or an amount) given to the row `key`, over what it
    /// was given before.
    static func change(_ choices: JSON, key: String, change: JSON) -> JSON {
        let set = choices.rows("set")
        var given = set.first(where: { $0.text("key") == key }) ?? [:]
        for (field, value) in change.object { given[field] = value }
        given["key"] = .string(key)
        return revision(choices["skip"].array.map(\.string), set.filter { $0.text("key") != key } + [given])
    }

    /// What saving a row's editor gives it: `category` (nil: Ready to Assign) when it isn't the one
    /// the row has, and the typed amount when it isn't the row's; nil when nothing changed. An
    /// amount typed without a sign keeps the row's sign. Throws when the amount doesn't read.
    static func edit(_ row: JSON, category: String?, amount: String) throws -> JSON? {
        let edit = row["edit"]
        guard edit.isObject else { return nil }
        var change: JSON = [:]
        let had = edit.text("categoryId")
        if edit.flag("category") && category != (had.isEmpty ? nil : had) { change["category"] = category.map(JSON.string) ?? .null }
        if edit.flag("amount") {
            let was: Int64? = row["amount"].isNull ? nil : row.long("amount")
            let typed = amount.trimmingCharacters(in: .whitespaces)
            var cents = try Money.parse(typed)
            if !typed.hasPrefix("-") && !typed.hasPrefix("+") && (was ?? 0) < 0 { cents = -abs(cents) }
            if cents != was { change["amount"] = .int(cents) }
        }
        return change.object.isEmpty ? nil : change
    }

    /// How much of a card Apply makes: `chosen` of its `total` parts ("Apply 2 of 3").
    struct Partial: Equatable {
        var chosen: Int
        var total: Int
    }

    /// How much of the card Apply makes while the person is choosing and some of its rows are left
    /// out; nil otherwise. The card words it.
    static func partial(_ rows: [JSON], choosing: Bool) -> Partial? {
        let keyed = rows.filter { !$0.text("key").isEmpty }
        let chosen = keyed.filter { !$0.flag("skipped") }.count
        return choosing && chosen < keyed.count ? Partial(chosen: chosen, total: keyed.count) : nil
    }

    /// `choices` with each row of the card chosen that `keep` says to, and every other left out:
    /// Select all, Select none, and Only the ones to check. A row put back loses an amount of
    /// nothing given to it, as `toggle` has it.
    static func select(_ choices: JSON, rows: [JSON], keep: (JSON) -> Bool) -> JSON {
        var out = revision(choices["skip"].array.map(\.string), choices.rows("set"))
        for row in rows {
            let key = row.text("key")
            guard !key.isEmpty else { continue }
            let skipped = out["skip"].array.contains { $0.string == key }
            if keep(row) == skipped { out = toggle(out, key: key, skipped: skipped) }
        }
        return out
    }

    /// The card's rows as the person's choices leave them, before the service has made the card
    /// again: a row left out or put back, and an amount or a category given, shown at once.
    /// `pending` is the choices still to send and `sent` the ones the card was made with: only
    /// what differs between them is laid over a row. A category given is named by `category`
    /// (nil: Ready to Assign) in the row's `given`, until the card says it itself.
    static func shown(_ rows: [JSON], pending: JSON, sent: JSON, category: (String?) -> String) -> [JSON] {
        let skip = Set(pending["skip"].array.map(\.string))
        let given = pending.rows("set")
        let had = sent.rows("set")
        return rows.map { row -> JSON in
            let key = row.text("key")
            guard !key.isEmpty else { return row }
            var out = row
            out["skipped"] = .bool(skip.contains(key))
            guard let change = given.first(where: { $0.text("key") == key }), change != had.first(where: { $0.text("key") == key }) else { return out }
            if !change["amount"].isNull {
                out["amount"] = change["amount"]
                out["edited"] = true
            }
            if change.has("category") {
                let id: String? = change["category"].isNull ? nil : change["category"].string
                var edit = out["edit"]
                if let id { edit["categoryId"] = .string(id) } else { edit.remove("categoryId") }
                out["edit"] = edit
                out["given"] = .string(category(id))
                out["edited"] = true
            }
            return out
        }
    }

    private static func revision(_ skip: [String], _ set: [JSON]) -> JSON { ["skip": JSON(skip), "set": .array(set)] }
}
