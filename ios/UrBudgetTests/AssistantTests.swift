import XCTest
@testable import UrBudget

final class AssistantMarkdownTests: XCTestCase {
    func testLargeReportTableBecomesLazyPiecesWithoutLosingData() {
        var source = "# Report\n\n| Payee | Amount |\n| --- | ---: |\n"
        for index in 0..<10_000 { source += "| Payee \(index) | \(index) |\n" }
        source += "\nAll rows included."
        let document = AssistantMarkdown.documentBlocks(source)
        XCTAssertEqual(document.first?.kind, "heading")
        XCTAssertEqual(document.last?.text, "All rows included.")
        let pieces = document.filter { $0.kind == "table" }
        XCTAssertEqual(pieces.count, 10_000 / AssistantMarkdown.documentTableChunk)
        XCTAssertTrue(pieces.allSatisfy { $0.rows.count - 1 <= AssistantMarkdown.documentTableChunk })
        XCTAssertFalse(pieces[0].continued)
        XCTAssertTrue(pieces.dropFirst().allSatisfy(\.continued))
        XCTAssertTrue(pieces.allSatisfy { $0.rows[0] == ["Payee", "Amount"] && $0.alignments == ["leading", "trailing"] })
        let rows = pieces.flatMap { $0.rows.dropFirst() }
        XCTAssertEqual(rows.count, 10_000)
        XCTAssertEqual(rows.last, ["Payee 9999", "9999"])
        // Every piece shares the whole table's column widths, so the pieces line up.
        XCTAssertEqual(Set(pieces.map(\.widths)).count, 1)
    }

    func testChatKeepsCodeHeadingsListsAndCompleteParagraphs() {
        let source = "# Summary\n\n- First item\n  2. Nested step\n- [x] Done\n```text\nexact | values\n```\n---\n**Final** [source](https://example.com)\nsecond line\n> quoted\n> still quoted"
        let blocks = AssistantMarkdown.blocks(source)
        XCTAssertEqual(blocks.map(\.kind), ["heading", "item", "item", "item", "code", "rule", "text", "quote"])
        XCTAssertEqual(blocks[0].level, 1)
        XCTAssertEqual(blocks[1].text, "First item")
        XCTAssertEqual(blocks[1].marker, "•")
        XCTAssertEqual(blocks[2].marker, "2.")
        XCTAssertEqual(blocks[2].level, 1)
        XCTAssertEqual(blocks[3].marker, "☑")
        XCTAssertEqual(blocks[4].text, "exact | values")
        XCTAssertEqual(blocks[6].text, "**Final** [source](https://example.com)\nsecond line")
        XCTAssertEqual(blocks[7].text, "quoted\nstill quoted")
        XCTAssertEqual(AssistantMarkdown.documentBlocks(source), blocks)
    }

    func testTablesPadShortRowsAndKeepEscapedBars() {
        let blocks = AssistantMarkdown.blocks("| A | B | C |\n|:-:|---|--:|\n| 1 | a \\| b |\n")
        XCTAssertEqual(blocks.count, 1)
        XCTAssertEqual(blocks[0].alignments, ["center", "leading", "trailing"])
        XCTAssertEqual(blocks[0].rows[1], ["1", "a | b", ""])
        XCTAssertFalse(AssistantMarkdown.blocks("Total | 12\nnot a divider").contains { $0.kind == "table" })
    }

    func testOnlyWebLinksStayLinks() {
        let safe = AssistantMarkdown.inline("[site](https://urbudget.app) and **bold**")
        XCTAssertEqual(String(safe.characters), "site and bold")
        XCTAssertEqual(safe.runs.compactMap(\.link).map(\.absoluteString), ["https://urbudget.app"])
        let unsafe = AssistantMarkdown.inline("[call](tel:123) [run](javascript:alert(1))")
        XCTAssertTrue(unsafe.runs.allSatisfy { $0.link == nil })
        XCTAssertEqual(String(AssistantMarkdown.inline("plain text").characters), "plain text")
    }

    func testPrintedReportEscapesEverything() {
        let html = AssistantMarkdown.html(title: "<Report>", meta: "Budget · 2026-10",
                                          body: "## Spend <script>x</script>\n\n| A | B |\n| --- | --- |\n| **1** | [x](https://example.com) |")
        XCTAssertFalse(html.contains("<script>"))
        XCTAssertTrue(html.contains("&lt;Report&gt;"))
        XCTAssertTrue(html.contains("&lt;script&gt;"))
        XCTAssertTrue(html.contains("<strong>1</strong>"))
        XCTAssertTrue(html.contains("x (https://example.com)"))
        XCTAssertTrue(html.contains("<thead>"))
    }
}

final class AssistantPresentationTests: XCTestCase {
    func testStreamBufferPublishesOnlyChanges() {
        let buffer = AssistantStreamBuffer()
        XCTAssertNil(buffer.drain())
        buffer.append("Hel")
        buffer.append("lo")
        buffer.append("Weighing", thinking: true)
        XCTAssertEqual(buffer.drain()?.reply, "Hello")
        XCTAssertNil(buffer.drain())
        buffer.clearReply()
        let cleared = buffer.drain()
        XCTAssertEqual(cleared?.reply, "")
        XCTAssertEqual(cleared?.thinking, "Weighing")
        buffer.clear()
        XCTAssertNil(buffer.drain())
    }

    func testUnchangedMessagesKeepTheirPreparedMarkdown() {
        let first: JSON = ["id": "t_1", "title": "Spending", "entries": [["kind": "user", "text": "Hi"], ["kind": "assistant", "text": "# Hello"]]]
        let shown = AssistantThreadPresentation.make(first, previous: nil)
        XCTAssertEqual(shown.entries.map(\.kind), ["user", "assistant"])
        XCTAssertEqual(shown.entries[1].blocks.first?.kind, "heading")
        XCTAssertTrue(shown.entries[0].blocks.isEmpty)
        var second = first
        second["entries"] = .array(first["entries"].array + [["kind": "step", "text": "Read October"]])
        let next = AssistantThreadPresentation.make(second, previous: shown)
        XCTAssertEqual(next.entries.count, 3)
        XCTAssertEqual(next.entries[1], shown.entries[1])
        let other = AssistantThreadPresentation.make(["id": "t_2", "entries": [["kind": "assistant", "text": "# Hello"]]], previous: shown)
        XCTAssertEqual(other.entries.count, 1)
    }

    func testConversationExportWritesProposalsOut() {
        Money.configure(budget: ["currency": "USD"])
        let thread: JSON = ["title": "Cover overspending", "entries": [
            ["kind": "user", "text": "Help", "attachments": [["name": "statement.pdf", "size": 2048]]],
            ["kind": "proposal", "proposal": ["title": "Move money", "status": "proposed", "card": [
                "stats": [["label": "Total", "amount": 1500], ["label": "Categories", "text": "2"]],
                "rows": [["label": "Dining", "detail": "From Fun", "amount": -1500, "before": 2000, "after": 500, "badge": "Covered"]],
                "note": "Checked again before applying."]]],
        ]]
        let text = AssistantExport.conversationMarkdown(thread, currency: "USD")
        XCTAssertTrue(text.hasPrefix("# Cover overspending\n\n## User\n\nHelp\n\nAttachment: statement.pdf (2048 bytes)\n"))
        XCTAssertTrue(text.contains("## Proposal: Move money (proposed)"))
        XCTAssertTrue(text.contains("- Total: $15.00\n- Categories: 2\n"))
        XCTAssertTrue(text.contains("- Dining: From Fun -$15.00 ($20.00 → $5.00) · Covered\n"))
        XCTAssertTrue(text.contains("\nChecked again before applying.\n"))
    }

    func testFileNamesAndLimits() {
        XCTAssertEqual(AssistantExport.fileName("Rent: Oct/Nov?", extension: "md"), "Rent_ Oct_Nov_.md")
        XCTAssertEqual(AssistantExport.fileName("", extension: "md"), "Conversation.md")
        XCTAssertEqual(AssistantExport.fileName(String(repeating: "a", count: 80), extension: "md").count, 63)
        XCTAssertEqual(assistantLimited("ab😀c", utf16: 3), "ab")
        XCTAssertEqual(assistantLimited("ab😀c", utf16: 4), "ab😀")
        XCTAssertEqual(AssistantScreen.context(month: "2026-10", selected: ["t1"])["platform"], "ios")
    }
}

/// The revisions a proposal card sends `assistant_revise`, as the desktop's ProposalCard builds them.
final class ProposalChoicesTests: XCTestCase {
    func testLeavingARowOutAndPuttingItBackChangesOnlyItsKey() {
        XCTAssertEqual(ProposalChoices.original, ["skip": [], "set": []])
        let one = ProposalChoices.toggle(.null, key: "t:b", skipped: false)
        XCTAssertEqual(one, ["skip": ["t:b"], "set": []])
        XCTAssertEqual(ProposalChoices.toggle(one, key: "t:a", skipped: false), ["skip": ["t:b", "t:a"], "set": []])
        // Put back: out of skip, and an amount of nothing given to it goes too; other choices stay.
        let choices: JSON = ["skip": ["s:0", "s:1"], "set": [["key": "s:0", "amount": 0], ["key": "s:1", "category": .null], ["key": "s:2", "amount": 0]]]
        let back = ProposalChoices.toggle(choices, key: "s:0", skipped: true)
        XCTAssertEqual(back, ["skip": ["s:1"], "set": [["key": "s:1", "category": .null], ["key": "s:2", "amount": 0]]])
        // A category given stays when its row is put back, Ready to Assign (null) included.
        let again = ProposalChoices.toggle(back, key: "s:1", skipped: true)
        XCTAssertEqual(again, ["skip": [], "set": [["key": "s:1", "category": .null], ["key": "s:2", "amount": 0]]])
        XCTAssertTrue(again["set"][0].has("category"))
        XCTAssertTrue(again.serialized().contains("\"category\":null"))
    }

    func testAnEditReplacesItsRowsEntryOverWhatItWasGiven() {
        let one = ProposalChoices.change(["skip": ["r:r1"], "set": []], key: "t:b", change: ["category": "groceries"])
        XCTAssertEqual(one, ["skip": ["r:r1"], "set": [["key": "t:b", "category": "groceries"]]])
        let two = ProposalChoices.change(one, key: "t:a", change: ["category": .null])
        XCTAssertEqual(two, ["skip": ["r:r1"], "set": [["key": "t:b", "category": "groceries"], ["key": "t:a", "category": .null]]])
        // Another amount for t:b keeps the category it was given.
        let three = ProposalChoices.change(two, key: "t:b", change: ["amount": -30000])
        XCTAssertEqual(three, ["skip": ["r:r1"], "set": [["key": "t:a", "category": .null], ["key": "t:b", "category": "groceries", "amount": -30000]]])
        XCTAssertEqual(ProposalChoices.change(.null, key: "s:0", change: ["amount": 500]), ["skip": [], "set": [["key": "s:0", "amount": 500]]])
    }

    func testSavingARowsEditorGivesWhatChangedKeepingTheRowsSign() throws {
        Money.configure(budget: ["currency": "SAR"])
        let home: JSON = ["label": "Home", "amount": -45900, "key": "s:0", "edit": ["category": true, "categoryId": "rent", "amount": true]]
        XCTAssertEqual(try ProposalChoices.edit(home, category: "rent", amount: "300"), ["amount": -30000])
        XCTAssertEqual(try ProposalChoices.edit(home, category: "rent", amount: "+5"), ["amount": 500])
        XCTAssertNil(try ProposalChoices.edit(home, category: "rent", amount: "-459.00"))
        XCTAssertNil(try ProposalChoices.edit(home, category: "rent", amount: " 459 "))
        XCTAssertEqual(try ProposalChoices.edit(home, category: nil, amount: "-459.00"), ["category": .null])
        XCTAssertEqual(try ProposalChoices.edit(home, category: "groceries", amount: "12.5"), ["category": "groceries", "amount": -1250])
        XCTAssertThrowsError(try ProposalChoices.edit(home, category: "rent", amount: "lots"))
        // A category only: the amount isn't read. Ready to Assign is a row with no categoryId.
        let careem: JSON = ["label": "Careem", "amount": -3800, "key": "t:a", "edit": ["category": true, "amount": false]]
        XCTAssertNil(try ProposalChoices.edit(careem, category: nil, amount: "lots"))
        XCTAssertEqual(try ProposalChoices.edit(careem, category: "transport", amount: ""), ["category": "transport"])
        // A positive row stays positive.
        let salary: JSON = ["label": "Salary", "amount": 2000, "key": "t:s", "edit": ["category": false, "amount": true]]
        XCTAssertEqual(try ProposalChoices.edit(salary, category: nil, amount: "15"), ["amount": 1500])
        XCTAssertNil(try ProposalChoices.edit(["label": "New rule"], category: nil, amount: ""))
    }

    func testApplySaysHowMuchOfTheCardItMakesWhenSomeIsLeftOut() {
        let rows: [JSON] = [["key": "t:a"], ["key": "t:b", "skipped": true], ["key": "r:r1"], ["label": "unkeyed"]]
        XCTAssertEqual(ProposalChoices.partial(rows, choosing: true), ProposalChoices.Partial(chosen: 2, total: 3))
        XCTAssertNil(ProposalChoices.partial(rows, choosing: false))
        XCTAssertNil(ProposalChoices.partial(rows.map { row -> JSON in var row = row; row["skipped"] = false; return row }, choosing: true))
        XCTAssertEqual(ProposalChoices.partial(rows.map { row -> JSON in var row = row; row["skipped"] = true; return row }, choosing: true),
                       ProposalChoices.Partial(chosen: 0, total: 3))
    }
}

final class AssistantBankReviewTests: XCTestCase {
    func testDefaultChoicesMatchTheDesktop() {
        XCTAssertEqual(AssistantBankReview.defaultChoice(duplicate: true, possibleDuplicate: false, candidates: ["a"], previouslyMatched: []), "skip")
        XCTAssertEqual(AssistantBankReview.defaultChoice(duplicate: false, possibleDuplicate: true, candidates: [], previouslyMatched: []), "review")
        XCTAssertEqual(AssistantBankReview.defaultChoice(duplicate: false, possibleDuplicate: false, candidates: [], previouslyMatched: []), "new")
        XCTAssertEqual(AssistantBankReview.defaultChoice(duplicate: false, possibleDuplicate: false, candidates: ["a"], previouslyMatched: []), "match:a")
        XCTAssertEqual(AssistantBankReview.defaultChoice(duplicate: false, possibleDuplicate: false, candidates: ["a"], previouslyMatched: ["a"]), "review")
        XCTAssertEqual(AssistantBankReview.defaultChoice(duplicate: false, possibleDuplicate: false, candidates: ["a", "b"], previouslyMatched: []), "review")
    }

    func testReviewRowsAndChoices() throws {
        Money.configure(budget: ["currency": "USD"])
        let budget: JSON = ["currency": "USD", "transactions": [["id": "x1", "date": "2026-10-03", "payee": "Shop", "memo": "", "amount": -500]]]
        let items: [JSON] = [
            ["id": "r1", "date": "2026-10-03", "payee": "SHOP 123", "rename": "Shop", "memo": "card", "amount": -500, "candidateSet": 0],
            ["id": "r2", "date": "2026-10-04", "payee": "Cafe", "memo": "", "amount": -300, "candidateSet": 1, "duplicate": true],
            ["id": "r3", "date": "2026-10-05", "payee": "Cafe", "memo": "", "amount": -300, "candidateSet": 1, "duplicate": true, "idless": true],
            ["id": "r4", "date": "2026-10-06", "payee": "Pay", "memo": "", "amount": 9000, "candidateSet": 1, "possibleDuplicate": true, "idless": true],
        ]
        let sets: [JSON] = [["candidates": ["x1"], "previouslyMatchedCandidates": [], "candidateDetails": []],
                            ["candidates": [], "previouslyMatchedCandidates": [], "candidateDetails": []]]
        let rows = try AssistantBankReview.prepare(items, budget: budget, candidateSets: sets)
        XCTAssertEqual(rows.map(\.initial), ["match:x1", "skip", "skip", "review"])
        XCTAssertEqual(rows[0].payee, "Shop")
        XCTAssertEqual(rows[0].bankText, "SHOP 123")
        XCTAssertTrue(rows[0].options.contains { $0.value == "match:x1" && $0.label.hasPrefix("Match: ") && $0.label.contains("Shop") })
        XCTAssertTrue(rows[1].locked)
        XCTAssertFalse(rows[2].locked)
        XCTAssertEqual(rows[2].options.map(\.value), ["skip", "new"])
        XCTAssertFalse(rows[0].source.has("candidateSet"))

        let counts = AssistantBankReview.counts(rows, [:])
        XCTAssertEqual(counts.importing, 1)
        XCTAssertEqual(counts.undecided, 1)
        let summary = AssistantBankReview.summary(rows)
        XCTAssertEqual(summary.text, "4 rows · 2 already imported · 1 renamed by your payee rules")
        XCTAssertTrue(summary.idless)

        let choices = AssistantBankReview.choices(rows, [2: "new", 3: "review"])
        XCTAssertEqual(choices.map { $0.text("action") }, ["match", "skip", "new", "skip"])
        XCTAssertEqual(choices[0]["matchId"], "x1")
        XCTAssertTrue(choices[1]["matchId"].isNull)
        XCTAssertTrue(choices[2]["row"].text("importId").hasPrefix("bank:manual:"))
        XCTAssertFalse(choices[0]["row"].has("importId"))
        XCTAssertThrowsError(try AssistantBankReview.prepare([["id": "r", "candidateSet": 5]], budget: budget, candidateSets: []))
    }
}
