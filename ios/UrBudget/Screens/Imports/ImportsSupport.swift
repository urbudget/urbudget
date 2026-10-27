import Foundation

/// A file the person picked, read into memory off the main thread.
struct ImportPickedFile: Sendable {
    var name: String
    var data: Data
}

/// Reading picked files with their security scope, inside the limits the shared engine sets.
enum ImportFiles {
    /// One file, refused when larger than `limit` bytes.
    static func read(_ url: URL, limit: Int, tooLarge: String) throws -> ImportPickedFile {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
        guard size <= limit else { throw AppError(tooLarge) }
        let data: Data
        do { data = try Data(contentsOf: url) } catch { throw AppError(String(localized: "The selected file could not be opened.")) }
        guard data.count <= limit else { throw AppError(tooLarge) }
        return ImportPickedFile(name: url.lastPathComponent, data: data)
    }

    /// YNAB's export: at most eight files, 8 MB each and 16 MB together (ynab_files.rs).
    static func readYnab(_ urls: [URL]) throws -> [ImportPickedFile] {
        guard urls.count <= 8 else { throw AppError(String(localized: "Choose at most 8 export files at once.")) }
        var total = 0
        return try urls.map { url in
            let name = url.lastPathComponent
            let file = try read(url, limit: 8 * 1024 * 1024,
                                tooLarge: String(localized: "\(name) is larger than 8 MB. Use YNAB's token import for larger budgets."))
            total += file.data.count
            guard total <= 16 * 1024 * 1024 else {
                throw AppError(String(localized: "These export files total more than 16 MB. Use YNAB's token import for larger budgets."))
            }
            return file
        }
    }

    /// A bank statement's text: UTF-16 or UTF-8 as the byte-order mark says (Excel saves "Unicode
    /// text" as UTF-16), else UTF-8, else the encoding the engine finds the bytes fit (Windows-1250 or
    /// -1252 in Europe, Shift_JIS, GBK, Windows-1251…), as banks still write. The engine gets the bytes
    /// as ISO-8859-1, a character for each.
    static func text(_ data: Data) throws -> String {
        // An Excel or OpenDocument workbook, a ZIP of statements, a PDF: the engine reads a workbook's
        // sheet as CSV and a ZIP's statements, and says what a PDF needs.
        let binary: [[UInt8]] = [[0x50, 0x4B, 0x03, 0x04], [0xD0, 0xCF, 0x11, 0xE0], [0x25, 0x50, 0x44, 0x46]]
        if binary.contains(where: { data.starts(with: $0) }) {
            let latin1 = String(data: data, encoding: .isoLatin1) ?? ""
            return try NativeCore.engine("decodeStatement", params: ["latin1": .string(latin1)]).text("text")
        }
        if data.starts(with: [0xFF, 0xFE]), let text = String(data: data.dropFirst(2), encoding: .utf16LittleEndian) { return text }
        if data.starts(with: [0xFE, 0xFF]), let text = String(data: data.dropFirst(2), encoding: .utf16BigEndian) { return text }
        let bom = Data([0xEF, 0xBB, 0xBF])
        let body = data.starts(with: bom) ? data.dropFirst(3) : data
        if let text = String(data: body, encoding: .utf8) { return text }
        if let latin1 = String(data: body, encoding: .isoLatin1),
           let decoded = try? NativeCore.engine("decodeStatement", params: ["latin1": .string(latin1)]) {
            return decoded.text("text")
        }
        return String(data: body, encoding: .windowsCP1252) ?? String(decoding: body, as: UTF8.self)
    }

    /// A statement's text (see `text`), and a workbook's sheets with the one read: at `sheet` when
    /// it has one by that name, else the one most like a statement.
    static func statement(_ data: Data, sheet: String?, workbook: (String, String?) throws -> JSON = { latin1, sheet in
        try NativeCore.engine("decodeStatement", params: ["latin1": .string(latin1), "sheet": JSON.text(orNull: sheet)])
    }) throws -> DecodedStatement {
        let binary: [[UInt8]] = [[0x50, 0x4B, 0x03, 0x04], [0xD0, 0xCF, 0x11, 0xE0]]
        guard binary.contains(where: { data.starts(with: $0) }) else { return DecodedStatement(text: try text(data)) }
        let read = try workbook(String(data: data, encoding: .isoLatin1) ?? "", sheet)
        return DecodedStatement(text: read.text("text"), sheet: read["sheet"].isNull ? nil : read.text("sheet"), sheets: read["sheets"].array.map(\.string))
    }

    /// The format a statement is in: what its content says, as the engine reads it (an MT940
    /// statement saved as .txt), else what its extension says.
    static func statementFormat(name: String, text: String) -> String? {
        let head = String(text.drop(while: { $0 == "\u{feff}" || $0.isWhitespace }).prefix(4096)).uppercased()
        let lines = head.split(whereSeparator: \.isNewline).map { $0.drop(while: { $0 == " " || $0 == "\t" }) }.filter { !$0.isEmpty }
        let ext = (name as NSString).pathExtension.lowercased()
        if head.hasPrefix("OFXHEADER") || head.contains("<OFX>") { return ext == "qfx" ? "qfx" : "ofx" }
        if head.contains("CAMT.053") || head.contains("BKTOCSTMRSTMT>") { return "camt053" }
        if lines.contains(where: { $0.hasPrefix(":20:") }) && lines.contains(where: { $0.hasPrefix(":61:") }) { return "mt940" }
        if let first = lines.first, ["!TYPE:", "!ACCOUNT", "!OPTION:", "!CLEAR:"].contains(where: { first.hasPrefix($0) }) { return "qif" }
        switch ext {
        case "csv", "ofx", "qfx", "qif": return ext
        case "xlsx", "xlsm", "xlsb", "xls", "ods", "zip": return "csv"
        case "xml": return "camt053"
        case "sta", "940", "mt940": return "mt940"
        default: return nil
        }
    }

}

/// A statement's text, and a workbook's sheets with the one read.
struct DecodedStatement: Equatable, Sendable {
    var text: String
    var sheet: String?
    var sheets: [String] = []
}

// MARK: A bank file's lines

/// Words for a bank file's lines (bank_statement.rs, the desktop's import/model.ts).
enum BankStatementText {
    /// What a line left out is, by why.
    static let leftOut: [String: String] = [
        "pending": String(localized: "Not booked yet"), "declined": String(localized: "Declined or cancelled"),
        "otherAccount": String(localized: "Another account's"), "otherCurrency": String(localized: "In another currency"),
        "ownBalances": String(localized: "Between your own balances"), "balance": String(localized: "A balance or a total"),
        "aboveHeader": String(localized: "Above the header"), "noDate": String(localized: "No date"), "noAmount": String(localized: "No amount"),
        "zero": String(localized: "Nothing moved"), "corrected": String(localized: "Corrected by the bank"),
        "repeatedHeader": String(localized: "The header row again"), "inWholeBatch": String(localized: "In a batch imported as one entry"),
        "imported": String(localized: "Imported before"),
    ]
    /// Lines left out for these reasons can be included anyway (`Reason::includable`).
    static let includable: Set<String> = ["pending", "declined", "otherCurrency", "ownBalances", "balance", "noDate", "noAmount"]
    /// The parts of a transaction a line gives, as a phone edits them.
    static let fields: [(id: String, label: String)] = [
        ("date", String(localized: "Date")), ("payee", String(localized: "Payee")), ("memo", String(localized: "Memo")), ("amount", String(localized: "Amount")),
    ]
    /// What the assistant is asked about the lines of an import that need attention: ready to send, not sent.
    static let fixImport = String(localized: "Fix the lines of this import that need attention. Use only what the file itself supports, and ask me about anything that isn't clear.")

    /// Why a line is left out, with what the file says of it.
    static func leftOut(_ line: StatementLine) -> String {
        let name = leftOut[line.reason] ?? String(localized: "Left out")
        let detail = line.detail
        if line.reason == "otherAccount" && !detail.isEmpty { return String(localized: "Account \(detail)") }
        if line.reason == "otherCurrency" && !detail.isEmpty { return String(localized: "In \(detail)") }
        if !detail.isEmpty && ["pending", "declined", "ownBalances"].contains(line.reason) { return String(localized: "\(name) (\(detail))") }
        return name
    }

    /// "2 lines read · 1 needs attention and won't be imported · 1 left out"
    static func counts(_ statement: BankStatement) -> String {
        var parts = [String(localized: "\(statement.ready) lines read", locale: .app)]
        if statement.attention > 0 { parts.append(String(localized: "\(statement.attention) need attention and won't be imported", locale: .app)) }
        if statement.leftOutCount > 0 { parts.append(String(localized: "\(statement.leftOutCount) left out", locale: .app)) }
        return ImportsText.line(parts)
    }

    /// "1 need attention · 2 left out, not imported.", above the review of the lines that read.
    static func notImported(_ statement: BankStatement) -> String {
        var parts: [String] = []
        if statement.attention > 0 { parts.append(String(localized: "\(statement.attention) need attention", locale: .app)) }
        if statement.leftOutCount > 0 { parts.append(String(localized: "\(statement.leftOutCount) left out", locale: .app)) }
        let lines = ImportsText.line(parts)
        return String(localized: "\(lines), not imported.")
    }

    /// "A1 · EUR · 3 lines": an account in the file, its currency when it says one, and its lines.
    static func source(_ source: StatementSource) -> String {
        var parts = [source.label]
        if let currency = source.currency { parts.append(currency) }
        parts.append(String(localized: "\(source.lines) lines", locale: .app))
        return ImportsText.line(parts)
    }

    /// A date order ("dmy" or "mdy") with what the ambiguous date reads as in it: "Day first (01/02/2026 is 1 February 2026)".
    static func order(_ order: String, example: String, reading: String?) -> String {
        let monthFirst = order == "mdy"
        guard let reading else { return monthFirst ? String(localized: "Month first") : String(localized: "Day first") }
        return monthFirst ? String(localized: "Month first (\(example) is \(reading))") : String(localized: "Day first (\(example) is \(reading))")
    }

    /// A date the orders read differently, read day first and month first: "1 February 2026" and "2 January 2026",
    /// with Arabic month names in Arabic.
    static func readings(_ example: String) -> (dmy: String, mdy: String)? {
        guard let pattern = try? NSRegularExpression(pattern: #"(\d{1,2})\D(\d{1,2})\D(\d{2,4})"#),
              let match = pattern.firstMatch(in: example, range: NSRange(example.startIndex..., in: example)),
              let a = Range(match.range(at: 1), in: example).flatMap({ Int(example[$0]) }),
              let b = Range(match.range(at: 2), in: example).flatMap({ Int(example[$0]) }),
              let y = Range(match.range(at: 3), in: example).map({ String(example[$0]) }), let written = Int(y) else { return nil }
        let year = y.count == 2 ? 2000 + written : written
        let calendar = Calendar(identifier: .gregorian)
        let named = DateFormatter()
        named.locale = AppLanguage.isRightToLeft ? Locale.app : Locale(identifier: "en_GB")
        named.calendar = calendar
        named.timeZone = TimeZone(identifier: "UTC")
        // Arabic: the day, month name and year in the order of the person's Arabic region, with Western digits.
        if AppLanguage.isRightToLeft { named.setLocalizedDateFormatFromTemplate("dMMMMy") } else { named.dateFormat = "d MMMM yyyy" }
        func read(_ day: Int, _ month: Int) -> String? {
            let parts = DateComponents(calendar: calendar, timeZone: named.timeZone, year: year, month: month, day: day)
            guard parts.isValidDate, let date = parts.date else { return nil }
            return named.string(from: date)
        }
        guard let dmy = read(a, b), let mdy = read(b, a) else { return nil }
        return (dmy, mdy)
    }

    /// What money in may be: a refund of a purchase, with its category when the purchases agree on one.
    static func refund(_ refund: JSON, categories: [String: String], currency: String) -> String {
        let payee = refund.text("payee")
        let category = refund.text("categoryId")
        let purchases = refund.long("purchases")
        if purchases > 1 && category.isEmpty { return String(localized: "Possible refund · \(purchases) purchases from \(payee) match", locale: .app) }
        let amount = Money.format(-refund.long("amount"), currency: currency)
        let date = Money.date(refund.text("date"))
        guard let name = categories[category] else { return String(localized: "Refund of \(payee) · \(amount) on \(date)") }
        return String(localized: "Refund of \(payee) · \(amount) on \(date) · \(name)")
    }

    /// What became of fixes applied to an import (`bankApplyFixes`): the lines changed, how many applied, and what to say.
    static func outcome(_ applied: JSON) -> (lines: [String], applied: Int, notice: String) {
        let results = applied.rows("results")
        let refused = results.filter { !$0.flag("applied") }
        let done = results.count - refused.count
        let notice: String
        if let first = refused.first {
            let problem = first.text("problem").isEmpty ? String(localized: "the line changed") : first.text("problem")
            let total = results.count
            let refusedCount = refused.count
            // "Applied 1 of 2 fixes. One no longer fits: …"
            notice = String(localized: "Applied \(done) of \(total) fixes.", locale: .app) + " "
                + String(localized: "\(refusedCount) no longer fit: \(problem)", locale: .app)
        } else {
            // One reads "Applied the assistant's fix."
            notice = String(localized: "Applied the assistant's \(done) fixes.", locale: .app)
        }
        return (results.filter { $0.flag("applied") && !$0["line"].isNull }.map { $0.text("line") }, done, notice)
    }
}

/// A line of a bank file as the import reads it now; `kind` is "ready", "attention" or "leftOut".
struct StatementLine: Identifiable, Equatable, Sendable {
    var id: String
    var version: Int64
    var cells: [String]
    var fields: [String: String]
    var kind: String
    /// The part that doesn't read, and why.
    var field = ""
    var problem = ""
    /// Why it's left out, and what the file says of it (its state, currency, account).
    var reason = ""
    var detail = ""
    var included = false
    var fixed = false
    var batch: Int?

    var includable: Bool { kind == "leftOut" && BankStatementText.includable.contains(reason) }
    /// Its payee, else what the file writes on it.
    var title: String {
        let payee = fields["payee"] ?? ""
        if !payee.trimmingCharacters(in: .whitespaces).isEmpty { return payee }
        let written = cells.filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
        return written.isEmpty ? String(localized: "Line \(id)") : ImportsText.line(written)
    }
}

struct StatementSource: Equatable, Sendable {
    var label: String
    var currency: String?
    var lines: Int
}

struct StatementBatch: Equatable, Sendable {
    var entry: String
    var line: Int?
    var payments: [Int]
    var addsUp: Bool?
    var asOne: Bool
}

/// A bank file as an import session reads it (bank_statement.rs `Statement`); a phone gets the lines that need it.
struct BankStatement: Equatable, Sendable {
    var session: String
    var revision: Int64
    var format: String
    var lines: [StatementLine]
    var ready: Int
    var attention: Int
    var leftOut: [String: Int]
    var sources: [StatementSource]
    var source: Int?
    /// The date order, an ambiguous date, and whether it's still to confirm.
    var dateOrder: String?
    var dateExample: String?
    var dateUnconfirmed: Bool
    /// Whether money out is told by the transaction types, when the file leaves that open; nil when it doesn't.
    var signsByType: Bool?
    var signsUnconfirmed: Bool
    /// The chosen account's currency, when it isn't the budget's.
    var currency: String?
    var batches: [StatementBatch]
    /// What the account keeps for its next file, once this one is imported.
    var remember: JSON
    /// Each list the phone shows, worked out once for this reading of the file.
    var attentionLines: [StatementLine] = []
    var leftOutLines: [StatementLine] = []
    var changedLines: [StatementLine] = []
    /// Where each line is among the file's, as a batch names its payments.
    var places: [String: Int] = [:]

    var leftOutCount: Int { leftOut.values.reduce(0, +) }
    /// It opens at the lines by itself for: lines to fix, none that read, or a question to answer.
    var needsChecking: Bool { attention > 0 || ready == 0 || dateUnconfirmed || signsUnconfirmed }
    /// Nothing goes on to the review on a guess the file leaves open.
    var canContinue: Bool { ready > 0 && !dateUnconfirmed && !signsUnconfirmed }
    /// Every line is in a currency other than the budget's, which one.
    var elsewhere: String? {
        guard ready == 0 && currency == nil else { return nil }
        return lines.first { $0.reason == "otherCurrency" && !$0.detail.isEmpty }?.detail
    }

    init(_ json: JSON) {
        func optional(_ value: JSON) -> Int? { value.isNull ? nil : Int(value.int) }
        lines = json.rows("lines").map { line in
            let status = line["status"]
            let kind = status.text("kind").isEmpty ? "ready" : status.text("kind")
            var fields: [String: String] = [:]
            for (id, _) in BankStatementText.fields { fields[id] = line["fields"].text(id) }
            return StatementLine(id: line.text("id"), version: line.long("version"), cells: line["cells"].array.map(\.string), fields: fields, kind: kind,
                                 field: status.text("field"), problem: kind == "attention" ? status.text("reason") : "",
                                 reason: kind == "leftOut" ? status.text("reason") : "", detail: status.text("detail"),
                                 included: line.flag("included"), fixed: line.flag("fixed"), batch: optional(line["batch"]))
        }
        session = json["scope"].text("session")
        revision = json.long("revision")
        format = json.text("format")
        let counts = json["counts"]
        ready = Int(counts.long("ready"))
        attention = Int(counts.long("attention"))
        leftOut = counts["leftOut"].object.mapValues { Int($0.int) }
        sources = json.rows("sources").map { StatementSource(label: $0.text("label"), currency: $0.text("currency").isEmpty ? nil : $0.text("currency"), lines: Int($0.long("lines"))) }
        source = optional(json["source"])
        let date = json["dateOrder"]
        dateOrder = date.isNull ? nil : date.text("order")
        dateExample = date.text("example").isEmpty ? nil : date.text("example")
        dateUnconfirmed = date.flag("unconfirmed")
        signsByType = json["signs"].isNull ? nil : json["signs"].flag("byType")
        signsUnconfirmed = json["signs"].flag("unconfirmed")
        currency = json.text("currency").isEmpty ? nil : json.text("currency")
        batches = json.rows("batches").map { batch in
            StatementBatch(entry: batch.text("entry"), line: optional(batch["line"]), payments: batch["payments"].array.map { Int($0.int) },
                           addsUp: batch["addsUp"].isNull ? nil : batch.flag("addsUp"), asOne: batch.flag("asOne"))
        }
        remember = json["remember"].isObject ? json["remember"] : [:]
        attentionLines = lines.filter { $0.kind == "attention" }
        leftOutLines = lines.filter { $0.kind == "leftOut" }
        changedLines = lines.filter { $0.kind == "ready" && ($0.fixed || $0.included) }
        places = Dictionary(lines.enumerated().map { ($0.element.id, $0.offset) }, uniquingKeysWith: { first, _ in first })
    }
}

/// What a review of an import's rows was asked for: a reply is kept only while the screen still
/// shows that import, and a review shown or imported only for the import it was made of.
struct BankReviewRequest: Equatable, Sendable {
    /// Reviews asked for so far: a later one replaces this one.
    var number: Int
    /// Files chosen so far: another file (or none) is another import, its session at revision 0 too.
    var generation: Int
    var session: String
    var revision: Int64
    var account: String

    /// Of the same import as `other`, as read after the same changes, into the same account.
    func sameImport(as other: BankReviewRequest) -> Bool {
        generation == other.generation && session == other.session && revision == other.revision && account == other.account
    }
}

/// Changes to an import session, made against its current revision (bank_statement.rs `Fixes`).
enum BankFixes {
    static func make(_ session: JSON, _ build: (inout JSON) -> Void = { _ in }) -> JSON {
        var fixes: JSON = ["scope": session["scope"], "baseRevision": session["revision"], "baseStructure": session["structure"]]
        build(&fixes)
        return fixes
    }

    /// The session read another way: its structure with `change` made to a copy.
    static func restructured(_ session: JSON, _ change: (inout JSON) -> Void) -> JSON {
        make(session) { fixes in
            var structure = session["structure"]
            change(&structure)
            fixes["structure"] = structure
        }
    }

    /// A line's parts given new values: those changed, each against the value it has now; nil when none changed.
    static func fields(_ session: JSON, _ line: StatementLine, _ values: [String: String]) -> JSON? {
        let changed = BankStatementText.fields.compactMap { field -> JSON? in
            guard let value = values[field.id]?.trimmingCharacters(in: .whitespaces), value != (line.fields[field.id] ?? "") else { return nil }
            return ["line": .string(line.id), "lineVersion": .int(line.version), "field": .string(field.id),
                    "from": .string(line.fields[field.id] ?? ""), "to": .string(value)]
        }
        guard !changed.isEmpty else { return nil }
        return make(session) { $0["fields"] = .array(changed) }
    }

    static func include(_ session: JSON, _ line: StatementLine, _ on: Bool) -> JSON {
        make(session) { $0["include"] = [["line": .string(line.id), "lineVersion": .int(line.version), "include": .bool(on)]] }
    }

    /// The line put back as the file has it.
    static func revert(_ session: JSON, _ line: StatementLine) -> JSON {
        make(session) { $0["revert"] = [["line": .string(line.id), "lineVersion": .int(line.version)]] }
    }

    /// A batch's payments imported as its one entry, or (`on` false) each again.
    static func wholeBatch(_ session: JSON, _ entry: String, _ on: Bool) -> JSON {
        restructured(session) { structure in
            let was = structure["wholeBatches"].array.map(\.string)
            structure["wholeBatches"] = JSON(on ? (was.contains(entry) ? was : was + [entry]) : was.filter { $0 != entry })
        }
    }
}

/// Text the import and sync screens put together.
enum ImportsText {
    /// Parts of a detail line, joined with " · ". In Arabic the line reads right to left and each
    /// part keeps its own direction (a date, a payee in Latin letters); English is joined as it is.
    static func line(_ parts: [String]) -> String {
        guard AppLanguage.isRightToLeft else { return parts.joined(separator: " · ") }
        return "\u{200F}" + parts.map { "\u{2068}" + $0 + "\u{2069}" }.joined(separator: " · ")
    }
}

/// Engine requests too large for the JSON value tree.
enum ImportsEngine {
    private static let digits: [[UInt8]] = (0...255).map { Array(String($0).utf8) }

    /// `importYnabFiles` takes each file as an array of byte values. Writing that request as text
    /// directly avoids building millions of JSON values on a phone.
    static func importYnabFiles(_ files: [ImportPickedFile], dateOrder: String, currency: String) throws -> JSON {
        var request = [UInt8]()
        request.reserveCapacity(files.reduce(256) { $0 + $1.data.count * 4 + $1.name.utf8.count })
        func append(_ text: String) { request.append(contentsOf: Array(text.utf8)) }
        append(#"{"version":1,"method":"importYnabFiles","params":{"files":["#)
        for (index, file) in files.enumerated() {
            if index > 0 { request.append(UInt8(ascii: ",")) }
            append(#"{"name":"#)
            append(JSON.string(file.name).serialized())
            append(#","bytes":["#)
            var first = true
            for byte in file.data {
                if !first { request.append(UInt8(ascii: ",")) }
                first = false
                request.append(contentsOf: digits[Int(byte)])
            }
            append("]}")
        }
        append(#"],"dateOrder":"#)
        append(dateOrder.isEmpty ? "null" : JSON.string(dateOrder).serialized())
        append(#","currency":"#)
        append(JSON.string(currency).serialized())
        append("}}")
        request.append(0)
        let reply: Data = request.withUnsafeBufferPointer { buffer in
            buffer.baseAddress!.withMemoryRebound(to: CChar.self, capacity: buffer.count) { pointer in
                guard let reply = urbudget_invoke(pointer) else { return Data(#"{"ok":false,"error":"The engine returned nothing."}"#.utf8) }
                defer { urbudget_free(reply) }
                return Data(bytes: reply, count: strlen(reply))
            }
        }
        let value = try JSON.parse(reply)
        guard value["ok"].bool else { throw AppError(value.text("error", String(localized: "The shared engine couldn't read these files."))) }
        return value["value"]
    }
}

extension BudgetStore {
    /// An engine command from a review (`bankApply`), refused when the budget changed since the
    /// review was shown at `revision`. Runs in the edit queue, so nothing slips in between.
    func importsCommand(_ method: String, _ params: JSON, since revision: String, stale: String) async throws {
        try await perform { repository in
            guard await repository.revision == revision else { throw AppError(stale) }
            return ((), try await repository.command(method, params))
        }
    }

    /// Replaces the open budget with an imported one, refused when it changed since the preview.
    func importsReplace(with data: Data, since revision: String, stale: String) async throws {
        try await perform { repository in
            guard await repository.revision == revision else { throw AppError(stale) }
            return ((), try await repository.importBudget(data))
        }
    }
}
