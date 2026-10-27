import SwiftUI

/// One block of a reply or report: a paragraph, heading, list item, quote, code, rule or table.
/// Inline formatting stays in `text` and is read by `AssistantMarkdown.inline` when drawn.
struct AssistantMarkdownBlock: Equatable, Sendable {
    /// "text", "heading", "item", "quote", "code", "rule" or "table".
    var kind: String
    var text = ""
    /// A heading's level (1…6), or a list item's nesting (0…3).
    var level = 0
    /// A list item's bullet ("•", "3.", "☐" or "☑").
    var marker = ""
    /// A table's rows, its header first.
    var rows: [[String]] = []
    /// Each column's alignment from the divider row: "leading", "center" or "trailing".
    var alignments: [String] = []
    /// Each column's width in characters, measured over the whole table so its pieces line up.
    var widths: [Int] = []
    /// A later piece of a long table in a report: drawn without the header row.
    var continued = false
}

/// Small native Markdown reader for provider replies and reports, as Android's AssistantMarkdown:
/// paragraphs, headings, lists, quotes, code, rules and tables. No HTML runs, and only web
/// addresses become links.
enum AssistantMarkdown {
    /// Table rows per page in a chat reply.
    static let chatTablePage = 30
    /// Table rows per lazily drawn piece of a report.
    static let documentTableChunk = 40

    static func blocks(_ source: String) -> [AssistantMarkdownBlock] {
        let lines = source.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\r", with: "\n")
            .components(separatedBy: "\n")
        var blocks: [AssistantMarkdownBlock] = []
        var paragraph: [String] = []
        var quote: [String] = []
        func flush() {
            if !paragraph.isEmpty { blocks.append(AssistantMarkdownBlock(kind: "text", text: paragraph.joined(separator: "\n"))) }
            if !quote.isEmpty { blocks.append(AssistantMarkdownBlock(kind: "quote", text: quote.joined(separator: "\n"))) }
            paragraph = []
            quote = []
        }
        var index = 0
        while index < lines.count {
            let line = lines[index]
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty {
                flush()
                index += 1
            } else if let fence = ["```", "~~~"].first(where: { trimmed.hasPrefix($0) }) {
                flush()
                index += 1
                var code: [String] = []
                while index < lines.count && !lines[index].trimmingCharacters(in: .whitespaces).hasPrefix(fence) {
                    code.append(lines[index])
                    index += 1
                }
                if index < lines.count { index += 1 }
                blocks.append(AssistantMarkdownBlock(kind: "code", text: code.joined(separator: "\n")))
            } else if line.contains("|"), index + 1 < lines.count, let alignments = dividerAlignments(lines[index + 1]),
                      alignments.count == cells(line).count {
                flush()
                var rows = [cells(line)]
                index += 2
                while index < lines.count && lines[index].contains("|") && !lines[index].trimmingCharacters(in: .whitespaces).isEmpty {
                    rows.append(cells(lines[index]))
                    index += 1
                }
                blocks.append(table(rows, alignments))
            } else if isRule(trimmed) {
                flush()
                blocks.append(AssistantMarkdownBlock(kind: "rule"))
                index += 1
            } else if let heading = heading(trimmed) {
                flush()
                blocks.append(heading)
                index += 1
            } else if let item = listItem(line) {
                flush()
                blocks.append(item)
                index += 1
            } else if trimmed.hasPrefix(">") {
                if !paragraph.isEmpty { flush() }
                var text = trimmed.dropFirst()
                if text.hasPrefix(" ") { text = text.dropFirst() }
                quote.append(String(text))
                index += 1
            } else {
                if !quote.isEmpty { flush() }
                paragraph.append(trimmed)
                index += 1
            }
        }
        flush()
        return blocks
    }

    /// A report's blocks: a long table becomes pieces, so only the rows on screen are drawn.
    static func documentBlocks(_ source: String) -> [AssistantMarkdownBlock] {
        blocks(source).flatMap { block -> [AssistantMarkdownBlock] in
            guard block.kind == "table", block.rows.count - 1 > documentTableChunk else { return [block] }
            let header = block.rows[0]
            let body = Array(block.rows.dropFirst())
            return stride(from: 0, to: body.count, by: documentTableChunk).map { start in
                var piece = block
                piece.rows = [header] + body[start..<min(body.count, start + documentTableChunk)]
                piece.continued = start > 0
                return piece
            }
        }
    }

    // MARK: Lines

    private static func cells(_ line: String) -> [String] {
        var text = line.trimmingCharacters(in: .whitespaces)
        if text.hasPrefix("|") { text.removeFirst() }
        if text.hasSuffix("|") && !text.hasSuffix("\\|") { text.removeLast() }
        // An escaped bar belongs to its cell.
        let placeholder = "\u{0}"
        return text.replacingOccurrences(of: "\\|", with: placeholder).components(separatedBy: "|").map {
            $0.replacingOccurrences(of: placeholder, with: "|").trimmingCharacters(in: .whitespaces)
        }
    }

    /// The column alignments when `line` is a table's divider row ("| --- | ---: |").
    private static func dividerAlignments(_ line: String) -> [String]? {
        guard line.contains("-") else { return nil }
        let parts = cells(line)
        guard !parts.isEmpty else { return nil }
        var alignments: [String] = []
        for part in parts {
            let dashes = part.trimmingCharacters(in: CharacterSet(charactersIn: ":"))
            guard !dashes.isEmpty, dashes.allSatisfy({ $0 == "-" }), part.count - dashes.count <= 2 else { return nil }
            let leading = part.hasPrefix(":")
            let trailing = part.hasSuffix(":")
            alignments.append(leading && trailing ? "center" : trailing ? "trailing" : "leading")
        }
        return alignments
    }

    private static func table(_ rows: [[String]], _ alignments: [String]) -> AssistantMarkdownBlock {
        let columns = rows.map(\.count).max() ?? 0
        let padded = rows.map { $0 + Array(repeating: "", count: columns - $0.count) }
        let widths = (0..<columns).map { column in
            min(28, max(3, padded.map { displayLength($0[column]) }.max() ?? 3))
        }
        let aligned = (0..<columns).map { $0 < alignments.count ? alignments[$0] : "leading" }
        return AssistantMarkdownBlock(kind: "table", rows: padded, alignments: aligned, widths: widths)
    }

    /// Roughly how many characters a cell shows once its markup is drawn.
    private static func displayLength(_ text: String) -> Int {
        text.replacingOccurrences(of: "**", with: "").replacingOccurrences(of: "`", with: "").count
    }

    private static func isRule(_ trimmed: String) -> Bool {
        let compact = trimmed.replacingOccurrences(of: " ", with: "")
        guard compact.count >= 3, let first = compact.first, "-*_".contains(first) else { return false }
        return compact.allSatisfy { $0 == first }
    }

    private static func heading(_ trimmed: String) -> AssistantMarkdownBlock? {
        let hashes = trimmed.prefix { $0 == "#" }.count
        guard (1...6).contains(hashes) else { return nil }
        let rest = trimmed.dropFirst(hashes)
        guard rest.isEmpty || rest.first == " " || rest.first == "\t" else { return nil }
        var text = rest.trimmingCharacters(in: .whitespaces)
        // A closing run of hashes ("## Title ##") isn't part of the title.
        while text.hasSuffix("#") { text.removeLast() }
        return AssistantMarkdownBlock(kind: "heading", text: text.trimmingCharacters(in: .whitespaces), level: hashes)
    }

    private static func listItem(_ line: String) -> AssistantMarkdownBlock? {
        var spaces = 0
        var rest = Substring(line)
        while let first = rest.first, first == " " || first == "\t" {
            spaces += first == "\t" ? 4 : 1
            rest = rest.dropFirst()
        }
        let level = min(3, spaces / 2)
        if let first = rest.first, "-*+".contains(first), rest.dropFirst().first == " " {
            var text = rest.dropFirst(2).trimmingCharacters(in: .whitespaces)
            var marker = "•"
            if text.hasPrefix("[ ] ") { marker = "☐"; text = String(text.dropFirst(4)) }
            else if text.lowercased().hasPrefix("[x] ") { marker = "☑"; text = String(text.dropFirst(4)) }
            return AssistantMarkdownBlock(kind: "item", text: text, level: level, marker: marker)
        }
        let digits = rest.prefix { $0.isASCII && $0.isNumber }
        guard (1...3).contains(digits.count) else { return nil }
        let after = rest.dropFirst(digits.count)
        guard let close = after.first, close == "." || close == ")", after.dropFirst().first == " " else { return nil }
        return AssistantMarkdownBlock(kind: "item", text: after.dropFirst(2).trimmingCharacters(in: .whitespaces), level: level, marker: String(digits) + ".")
    }

    // MARK: Inline

    /// Bold, italic, strikethrough, code and links, with links kept only for web addresses.
    static func inline(_ text: String) -> AttributedString {
        guard text.contains(where: { "*_`[~\\<".contains($0) }) else { return AttributedString(text) }
        let options = AttributedString.MarkdownParsingOptions(allowsExtendedAttributes: false,
                                                              interpretedSyntax: .inlineOnlyPreservingWhitespace,
                                                              failurePolicy: .returnPartiallyParsedIfPossible)
        guard var value = try? AttributedString(markdown: text, options: options) else { return AttributedString(text) }
        let unsafe = value.runs.compactMap { run -> Range<AttributedString.Index>? in
            guard let link = run.link else { return nil }
            return ["http", "https"].contains(link.scheme?.lowercased() ?? "") ? nil : run.range
        }
        for range in unsafe { value[range].link = nil }
        return value
    }

    // MARK: Printing

    /// The report as a self-contained HTML page for the print system. Every piece of text is
    /// escaped, so a reply can't add markup, scripts or remote images.
    static func html(title: String, meta: String, body: String) -> String {
        // An Arabic app prints right to left: quote bars and list indents move to the right.
        let rtl = AppLanguage.isRightToLeft
        let rtlStyle = rtl ? " blockquote { border-left: none; padding-left: 0; border-right: 2pt solid #999; padding-right: 8pt; }" : ""
        let direction = rtl ? " dir='rtl'" : ""
        let indentSide = rtl ? "margin-right" : "margin-left"
        var out = """
        <!doctype html><html\(direction)><head><meta charset="utf-8"><style>
        body { font-family: -apple-system, Helvetica, sans-serif; font-size: 11pt; color: #000; line-height: 1.35; }
        h1 { font-size: 19pt; margin: 0 0 4pt; } h2 { font-size: 14pt; margin: 14pt 0 4pt; }
        h3, h4, h5, h6 { font-size: 12pt; margin: 12pt 0 3pt; }
        p { margin: 4pt 0; } .meta { color: #444; font-size: 9pt; margin-bottom: 14pt; }
        .item { margin: 2pt 0; } .marker { display: inline-block; min-width: 14pt; color: #444; }
        table { border-collapse: collapse; margin: 6pt 0; font-size: 9.5pt; }
        thead { display: table-header-group; } tr { page-break-inside: avoid; }
        th, td { border: 0.5pt solid #999; padding: 3pt 6pt; vertical-align: top; }
        th { background: #eee; }
        pre { font-family: Menlo, monospace; font-size: 9pt; background: #f2f2f2; padding: 6pt; white-space: pre-wrap; }
        code { font-family: Menlo, monospace; font-size: 9.5pt; }
        blockquote { border-left: 2pt solid #999; margin: 6pt 0; padding-left: 8pt; color: #333; }
        hr { border: none; border-top: 0.5pt solid #999; margin: 10pt 0; }\(rtlStyle)
        </style></head><body>
        """
        out += "<h1>\(escape(title))</h1>"
        if !meta.isEmpty { out += "<p class=\"meta\">\(escape(meta))</p>" }
        for block in blocks(body) {
            switch block.kind {
            case "heading":
                let level = min(6, max(2, block.level + 1))
                out += "<h\(level)>\(inlineHTML(block.text))</h\(level)>"
            case "code": out += "<pre>\(escape(block.text))</pre>"
            case "rule": out += "<hr>"
            case "quote": out += "<blockquote>\(inlineHTML(block.text))</blockquote>"
            case "item":
                out += "<p class=\"item\" style=\"\(indentSide): \(block.level * 14)pt\"><span class=\"marker\">\(escape(block.marker))</span>\(inlineHTML(block.text))</p>"
            case "table":
                out += "<table><thead><tr>"
                for (column, cell) in (block.rows.first ?? []).enumerated() {
                    out += "<th style=\"text-align: \(cssAlignment(block, column))\">\(inlineHTML(cell))</th>"
                }
                out += "</tr></thead><tbody>"
                for row in block.rows.dropFirst() {
                    out += "<tr>"
                    for (column, cell) in row.enumerated() {
                        out += "<td style=\"text-align: \(cssAlignment(block, column))\">\(inlineHTML(cell))</td>"
                    }
                    out += "</tr>"
                }
                out += "</tbody></table>"
            default: out += "<p>\(inlineHTML(block.text))</p>"
            }
        }
        return out + "</body></html>"
    }

    private static func cssAlignment(_ block: AssistantMarkdownBlock, _ column: Int) -> String {
        let rtl = AppLanguage.isRightToLeft
        switch column < block.alignments.count ? block.alignments[column] : "leading" {
        case "center": return "center"
        case "trailing": return rtl ? "left" : "right"
        default: return rtl ? "right" : "left"
        }
    }

    /// Inline formatting as HTML; a link is written out as its text and address, as Android prints it.
    static func inlineHTML(_ text: String) -> String {
        let value = inline(text)
        var out = ""
        for run in value.runs {
            var piece = escape(String(value[run.range].characters))
            if let intent = run.inlinePresentationIntent {
                if intent.contains(.code) { piece = "<code>\(piece)</code>" }
                if intent.contains(.stronglyEmphasized) { piece = "<strong>\(piece)</strong>" }
                if intent.contains(.emphasized) { piece = "<em>\(piece)</em>" }
                if intent.contains(.strikethrough) { piece = "<s>\(piece)</s>" }
            }
            if let link = run.link { piece += " (\(escape(link.absoluteString)))" }
            out += piece
        }
        return out.replacingOccurrences(of: "\n", with: "<br>")
    }

    static func escape(_ text: String) -> String {
        var out = ""
        out.reserveCapacity(text.count)
        for character in text {
            switch character {
            case "&": out += "&amp;"
            case "<": out += "&lt;"
            case ">": out += "&gt;"
            case "\"": out += "&quot;"
            case "'": out += "&#39;"
            default: out.append(character)
            }
        }
        return out
    }
}

// MARK: Views

/// A reply's blocks. A very long reply is paged so one message never draws thousands of views.
struct AssistantMarkdownView: View {
    var blocks: [AssistantMarkdownBlock]
    var pageSize = 60
    @State private var page = 0

    var body: some View {
        let pages = max(1, (blocks.count + pageSize - 1) / pageSize)
        let current = min(page, pages - 1)
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(current * pageSize..<min(blocks.count, (current + 1) * pageSize)), id: \.self) { index in
                AssistantMarkdownBlockView(block: blocks[index])
            }
            if pages > 1 { AssistantPager(page: current, pages: pages) { page = $0 } }
        }
    }
}

/// A report: the scroll view owns the page, so only the visible paragraphs and table pieces are drawn.
struct AssistantMarkdownDocument<Header: View>: View {
    var blocks: [AssistantMarkdownBlock]
    @ViewBuilder var header: Header

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                header.padding(.bottom, 12)
                ForEach(Array(blocks.indices), id: \.self) { index in
                    AssistantMarkdownBlockView(block: blocks[index], document: true)
                        .padding(.top, index == 0 || blocks[index].continued ? 0 : 10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .textSelection(.enabled)
        }
    }
}

struct AssistantMarkdownBlockView: View {
    var block: AssistantMarkdownBlock
    var document = false
    @ScaledMetric(relativeTo: .body) private var indent: CGFloat = 18

    var body: some View {
        switch block.kind {
        case "heading":
            Text(AssistantMarkdown.inline(block.text))
                .font(block.level <= 1 ? .title3.bold() : block.level == 2 ? .headline : .subheadline.bold())
                .padding(.top, 4)
                .accessibilityAddTraits(.isHeader)
        case "code":
            // Code reads left to right in every language.
            ScrollView(.horizontal, showsIndicators: false) {
                Text(block.text).font(.footnote.monospaced()).padding(10).textSelection(.enabled)
            }
            .environment(\.layoutDirection, .leftToRight)
            .background(Color.insetBackground, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        case "rule":
            Divider().padding(.vertical, 2)
        case "table":
            AssistantTableView(block: block, paged: !document)
        case "item":
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(block.marker).monospacedDigit().foregroundStyle(Color.secondary).accessibilityHidden(block.marker == "•")
                Text(AssistantMarkdown.inline(block.text)).fixedSize(horizontal: false, vertical: true)
            }
            .padding(.leading, CGFloat(block.level) * indent)
        case "quote":
            HStack(alignment: .top, spacing: 10) {
                RoundedRectangle(cornerRadius: 1.5).fill(Color.secondary.opacity(0.4)).frame(width: 3)
                Text(AssistantMarkdown.inline(block.text)).foregroundStyle(Color.secondary).fixedSize(horizontal: false, vertical: true)
            }
            .fixedSize(horizontal: false, vertical: true)
        default:
            Text(AssistantMarkdown.inline(block.text)).fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// A table with its columns lined up; it scrolls sideways when wider than the screen.
private struct AssistantTableView: View {
    var block: AssistantMarkdownBlock
    var paged: Bool
    @ScaledMetric(relativeTo: .footnote) private var unit: CGFloat = 7.6
    @State private var page = 0

    var body: some View {
        let header = block.rows.first ?? []
        let rows = Array(block.rows.dropFirst())
        let size = AssistantMarkdown.chatTablePage
        let pages = paged ? max(1, (rows.count + size - 1) / size) : 1
        let current = min(page, pages - 1)
        let shown = paged ? Array(rows[min(rows.count, current * size)..<min(rows.count, (current + 1) * size)]) : rows
        VStack(alignment: .leading, spacing: 6) {
            ScrollView(.horizontal, showsIndicators: false) {
                VStack(alignment: .leading, spacing: 0) {
                    if !block.continued {
                        row(header, heading: true)
                        Divider()
                    }
                    ForEach(Array(shown.indices), id: \.self) { index in
                        row(shown[index], heading: false)
                        if index < shown.count - 1 { Divider().opacity(0.6) }
                    }
                }
                .background(Color.cardBackground.opacity(0.5))
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            }
            if pages > 1 { AssistantPager(page: current, pages: pages) { page = $0 } }
        }
    }

    private func row(_ cells: [String], heading: Bool) -> some View {
        HStack(alignment: .top, spacing: 0) {
            ForEach(Array(cells.indices), id: \.self) { column in
                Text(AssistantMarkdown.inline(cells[column]))
                    .font(heading ? .footnote.weight(.semibold) : .footnote)
                    .monospacedDigit()
                    .multilineTextAlignment(textAlignment(column))
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(width: max(44, CGFloat(column < block.widths.count ? block.widths[column] : 12) * unit), alignment: alignment(column))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 6)
            }
        }
        .background(heading ? Color.insetBackground : Color.clear)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(heading ? .isHeader : [])
    }

    private func alignment(_ column: Int) -> Alignment {
        switch column < block.alignments.count ? block.alignments[column] : "leading" {
        case "center": return .center
        case "trailing": return .trailing
        default: return .leading
        }
    }

    private func textAlignment(_ column: Int) -> TextAlignment {
        switch column < block.alignments.count ? block.alignments[column] : "leading" {
        case "center": return .center
        case "trailing": return .trailing
        default: return .leading
        }
    }
}

/// Previous · 2 / 5 · Next, for long tables, replies and proposal rows.
struct AssistantPager: View {
    var page: Int
    var pages: Int
    var previous = String(localized: "Previous")
    var next = String(localized: "Next")
    var select: (Int) -> Void

    var body: some View {
        HStack {
            Button(previous) { select(page - 1) }.disabled(page <= 0)
            Spacer()
            Text("\(page + 1) / \(pages)").font(.footnote).monospacedDigit().foregroundStyle(Color.secondary)
                .accessibilityLabel("Page \(page + 1) of \(pages)")
            Spacer()
            Button(next) { select(page + 1) }.disabled(page + 1 >= pages)
        }
        .font(.subheadline)
        .buttonStyle(.borderless)
    }
}
