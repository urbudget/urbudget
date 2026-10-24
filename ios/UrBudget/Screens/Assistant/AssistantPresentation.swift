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

/// What the assistant is told about the screen a message was sent from.
enum AssistantScreen {
    /// `importing` is the bank file being imported, when the assistant is open over its import.
    static func context(month: String, selected: [String], importing: JSON? = nil) -> JSON {
        ["platform": "ios", "page": importing == nil ? "assistant" : "import", "month": .string(month), "today": .string(Dates.today),
         "selected": JSON(selected), "import": importing ?? .null]
    }
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
                out += "## Proposal: \(proposal.text("title")) (\(proposal.text("status")))\n\n"
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
                let kind = entry.text("kind")
                out += "## \(kind.prefix(1).uppercased() + kind.dropFirst())\n\n\(entry.text("text"))\n\n"
                for file in entry.rows("attachments") {
                    out += "Attachment: \(file.text("name")) (\(file.long("size")) bytes)\n"
                }
            }
        }
        return out
    }

    /// A title as a file name: letters, digits, spaces, "_" and "-" only, at most 60 characters.
    static func fileName(_ title: String, extension suffix: String, fallback: String = "Conversation") -> String {
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
        guard values?.isRegularFile == true else { throw AppError("The generated file is unavailable.") }
        guard (values?.fileSize ?? 0) <= 64 * 1024 * 1024 else { throw AppError("This file is too large to share.") }
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
        guard size <= maxBytes else { throw AppError("Choose a file smaller than 25 MiB.") }
        let data = try Data(contentsOf: url)
        guard data.count <= maxBytes else { throw AppError("Choose a file smaller than 25 MiB.") }
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
    static func photo(_ data: Data, type: UTType?, number: Int) throws -> (name: String, data: Data) {
        let kept: [(UTType, String)] = [(.png, "png"), (.jpeg, "jpg"), (.gif, "gif"), (.webP, "webp")]
        if let type, let match = kept.first(where: { type.conforms(to: $0.0) }) {
            return prepare(name: "Photo \(number).\(match.1)", data: data)
        }
        guard let jpeg = jpeg(data) else { throw AppError("This photo couldn't be read. Choose another, or attach it as a file.") }
        return ("Photo \(number).jpg", jpeg)
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

    /// How much of the card Apply makes ("Apply 2 of 3") while the person is choosing and some of
    /// its rows are left out; nil otherwise.
    static func partial(_ rows: [JSON], choosing: Bool) -> String? {
        let keyed = rows.filter { !$0.text("key").isEmpty }
        let chosen = keyed.filter { !$0.flag("skipped") }.count
        return choosing && chosen < keyed.count ? "Apply \(chosen) of \(keyed.count)" : nil
    }

    private static func revision(_ skip: [String], _ set: [JSON]) -> JSON { ["skip": JSON(skip), "set": .array(set)] }
}
