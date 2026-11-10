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
        // A link's host follows its text, so friendly words can't hide where it goes.
        XCTAssertEqual(String(safe.characters), "site (urbudget.app) and bold")
        XCTAssertEqual(safe.runs.compactMap(\.link).map(\.absoluteString), ["https://urbudget.app"])
        XCTAssertEqual(String(AssistantMarkdown.inline("[**your** bank](https://bank.example/login) now").characters), "your bank (bank.example) now")
        // Text that already names the host isn't given it twice, and a printed report writes the address itself.
        XCTAssertEqual(String(AssistantMarkdown.inline("[urbudget.app/help](https://urbudget.app/help)").characters), "urbudget.app/help")
        XCTAssertEqual(String(AssistantMarkdown.inline("[site](https://urbudget.app)", showHosts: false).characters), "site")
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
        XCTAssertEqual(AssistantWith(month: "2026-10", selected: ["t1"]).screen["platform"], "ios")
    }

    func testWhatGoesWithAMessageAndTheScreenItSends() {
        let budget: JSON = ["accounts": [["id": "a1", "name": "Checking"]], "categories": [["id": "c1", "name": "Groceries"]]]
        // From a category's sheet on Budget, with transactions selected.
        var with = AssistantWith(page: "budget", month: "2026-11", category: "c1", selected: ["t1", "t2", "t3"])
        XCTAssertEqual(with.parts(budget: budget).map(\.kind), [.place, .selection])
        XCTAssertEqual(with.parts(budget: budget).map(\.text), ["Budget · November 2026 · Groceries", "3 selected transactions"])
        var screen = with.screen
        XCTAssertEqual(screen["platform"], "ios")
        XCTAssertEqual(screen["page"], "budget")
        XCTAssertEqual(screen["month"], "2026-11")
        XCTAssertEqual(screen["categoryId"], "c1")
        XCTAssertFalse(screen.has("accountId"))
        XCTAssertEqual(screen["selected"], ["t1", "t2", "t3"])
        XCTAssertTrue(screen["import"].isNull)
        XCTAssertEqual(screen.text("today"), Dates.today)
        // The place taken off: no page, and no month, account or category either. The rest still goes.
        with.placeOff = true
        XCTAssertEqual(with.parts(budget: budget).map(\.kind), [.selection])
        screen = with.screen
        XCTAssertEqual(screen["page"], "")
        XCTAssertFalse(screen.has("month"))
        XCTAssertFalse(screen.has("categoryId"))
        XCTAssertEqual(screen["selected"], ["t1", "t2", "t3"])

        // An account's register names the account; it shows no month, so none goes.
        let register = AssistantWith(page: "accounts", month: "2026-11", account: "a1")
        XCTAssertEqual(register.parts(budget: budget).map(\.text), ["Accounts · Checking"])
        XCTAssertEqual(register.screen["accountId"], "a1")
        XCTAssertFalse(register.screen.has("month"))

        // Over an import, the file being imported goes until it's taken off.
        var importing = AssistantWith(page: "import", account: "a1", importing: ["account": "a1", "content": "Date,Amount", "session": [:]])
        XCTAssertEqual(importing.parts(budget: budget).map(\.text), ["Import bank file · Checking", "The file being imported"])
        XCTAssertEqual(importing.screen["import"].text("account"), "a1")
        importing.importOff = true
        XCTAssertEqual(importing.parts(budget: budget).map(\.kind), [.place])
        XCTAssertTrue(importing.screen["import"].isNull)

        // Opened from More, it is on the month Budget shows.
        let more = AssistantWith(month: "2026-11")
        XCTAssertEqual(more.parts(budget: nil).map(\.text), ["Assistant · November 2026"])
        XCTAssertEqual(more.screen["page"], "assistant")
        XCTAssertEqual(more.screen["month"], "2026-11")
    }

    func testASentMessageShowsWhatWentWithIt() {
        let with: JSON = ["page": "budget", "month": "2026-11", "category": "Groceries", "transactions": 3, "import": true]
        XCTAssertEqual(AssistantWith.sent(with), "Budget · November 2026 · Groceries · 3 selected transactions · The file being imported")
        XCTAssertEqual(AssistantWith.sent(["page": "transactions", "month": "", "transactions": 1, "import": false]), "Transactions · 1 selected transaction")
        // A page this version doesn't know is left unnamed; the rest still shows.
        XCTAssertEqual(AssistantWith.sent(["page": "somewhereNew", "month": "", "account": "Checking", "transactions": 0, "import": false]), "Checking")
        // Nothing went with it, or it's from before this was kept.
        XCTAssertNil(AssistantWith.sent(["page": "", "month": "", "transactions": 0, "import": false]))
        XCTAssertNil(AssistantWith.sent(.null))
    }

    func testASelectionOver200IsCutToItsFirst200() {
        let few = (0..<200).map { "t\($0)" }
        XCTAssertEqual(AssistantSelection.capped(few).ids, few)
        XCTAssertNil(AssistantSelection.capped(few).total)
        let many = (0..<350).map { "t\($0)" }
        let capped = AssistantSelection.capped(many)
        XCTAssertEqual(capped.ids, Array(many.prefix(200)))
        XCTAssertEqual(capped.total, 350)
        XCTAssertEqual(AssistantSelection.limit, 200)
    }

    func testReportDetailsLineAndHowItJoinsTheReason() {
        let line = AssistantReportDetails.line(version: "1.0.1", provider: "chatgpt", model: "", tools: ["get_month", "get_spending"], language: "en")
        XCTAssertEqual(line, "App: UrBudget iPhone 1.0.1 · Service: ChatGPT · Model: default · Tools: get_month, get_spending · Language: en")
        XCTAssertEqual(AssistantReportDetails.line(version: "1.0.1", provider: "anthropic", model: "claude-x", tools: [], language: "ar"),
                       "App: UrBudget iPhone 1.0.1 · Service: Anthropic · Model: claude-x · Tools: none · Language: ar")
        XCTAssertTrue(AssistantReportDetails.line(version: "1", provider: "openai", model: "", tools: [], language: "en").contains("Service: OpenAI"))
        // After a blank line. Details the person cleared send the reason alone.
        XCTAssertEqual(AssistantReportDetails.reason(" Wrong total ", details: line), "Wrong total\n\n" + line)
        XCTAssertEqual(AssistantReportDetails.reason("Wrong total", details: "  "), "Wrong total")
        XCTAssertEqual(AssistantReportDetails.reason("", details: line), line)
        // The details are cut to what's left of the reason's 1,000 characters; the person's words never are.
        let long = String(repeating: "a", count: 950)
        let joined = AssistantReportDetails.reason(long, details: line)
        XCTAssertEqual(joined.utf16.count, 1000)
        XCTAssertTrue(joined.hasPrefix(long + "\n\nApp: UrBudget iPhone 1.0.1"))
        let full = String(repeating: "b", count: 999)
        XCTAssertEqual(AssistantReportDetails.reason(full, details: line), full)
        XCTAssertEqual(AssistantReportDetails.reason(String(repeating: "c", count: 1000), details: line).utf16.count, 1000)
    }

    func testHistoryIsSearchedByTitle() {
        let threads: [JSON] = [["id": "t1", "title": "Cover overspending"], ["id": "t2", "title": "October report"], ["id": "t3", "title": "Café spending"]]
        XCTAssertEqual(AssistantHistory.matching(threads, "").count, 3)
        XCTAssertEqual(AssistantHistory.matching(threads, "  ").count, 3)
        XCTAssertEqual(AssistantHistory.matching(threads, "SPEND").map { $0.text("id") }, ["t1", "t3"])
        XCTAssertEqual(AssistantHistory.matching(threads, "cafe").map { $0.text("id") }, ["t3"])
        XCTAssertTrue(AssistantHistory.matching(threads, "groceries").isEmpty)
    }

    func testTextIsAddedToAMessageBeingWrittenAndNeverOverIt() {
        XCTAssertEqual(AssistantDraft.adding("Help me with Groceries this month.", to: ""), "Help me with Groceries this month.")
        XCTAssertEqual(AssistantDraft.adding("Revise: ", to: "  \n"), "Revise: ")
        XCTAssertEqual(AssistantDraft.adding("Revise the proposal: ", to: "A long message"), "A long message\nRevise the proposal: ")
        XCTAssertEqual(AssistantDraft.adding("Again", to: "First\n"), "First\nAgain")
        // The same request twice doesn't write its words twice.
        XCTAssertEqual(AssistantDraft.adding("Import this statement.", to: "Import this statement."), "Import this statement.")
        XCTAssertTrue(AssistantDraft().isEmpty)
        XCTAssertFalse(AssistantDraft(selected: ["t1"]).isEmpty)
    }

    func testTheConversationFollowsOnlyWhileItsEndIsOnScreen() {
        // The end is on screen: it follows, whatever it did before.
        XCTAssertTrue(AssistantFollow.next(false, top: -400, restingTop: -380, bottom: 600, viewport: 600))
        // New text grew the content under a top that didn't move: still following.
        XCTAssertTrue(AssistantFollow.next(true, top: -400, restingTop: -400, bottom: 760, viewport: 600))
        // The content moved down from where it rested: the person scrolled up, however slowly.
        XCTAssertFalse(AssistantFollow.next(true, top: -380, restingTop: -400, bottom: 780, viewport: 600))
        // Scrolled up, new text doesn't pull them back.
        XCTAssertFalse(AssistantFollow.next(false, top: -300, restingTop: -400, bottom: 900, viewport: 600))
        // The first measure has nothing before it.
        XCTAssertTrue(AssistantFollow.next(true, top: 0, restingTop: .greatestFiniteMagnitude, bottom: 900, viewport: 0))
        // Where it rests: where it is with the end on screen, and otherwise only ever higher.
        XCTAssertEqual(AssistantFollow.resting(-380, top: -400, bottom: 600, viewport: 600), -400)
        XCTAssertEqual(AssistantFollow.resting(-400, top: -395, bottom: 800, viewport: 600), -400)
        XCTAssertEqual(AssistantFollow.resting(-400, top: -460, bottom: 800, viewport: 600), -460)
    }

    func testUsageIsOneLineHiddenWhenNothingWasCounted() {
        let sent = 2400.formatted(.number.locale(.app))
        XCTAssertEqual(AssistantUsage.line(["input": 2400, "output": 60, "cached": 800]), "Tokens: \(sent) sent (800 from cache), 60 received")
        XCTAssertNil(AssistantUsage.line(["input": 0, "output": 0, "cached": 0]))
        XCTAssertNil(AssistantUsage.line(.null))
    }

    func testAnErrorOffersOnlyWhatItsActionNames() {
        // An earlier error comes without an action, whatever its code.
        let earlier: JSON = ["kind": "error", "text": "It failed.", "code": "signIn"]
        XCTAssertNil(AssistantErrorAction.of(earlier))
        let named: [(String, AssistantErrorAction)] = [("signIn", .signIn), ("settings", .settings), ("usage", .usage), ("retry", .retry),
                                                       ("newConversation", .newConversation)]
        for (name, action) in named {
            var last = earlier
            last["action"] = .string(name)
            XCTAssertEqual(AssistantErrorAction.of(last), action)
        }
        // "none" is the service saying asking again wouldn't help; a newer action is left alone.
        for name in ["none", "", "somethingNewer"] {
            var last = earlier
            last["action"] = .string(name)
            XCTAssertNil(AssistantErrorAction.of(last))
        }
        var absent = earlier
        absent["action"] = .null
        XCTAssertNil(AssistantErrorAction.of(absent))
    }

    func testTheNoticeNamesWhoTheServiceSendsTo() {
        XCTAssertEqual(AssistantNotice.recipient("chatgpt"), "OpenAI")
        XCTAssertEqual(AssistantNotice.recipient("openai"), "OpenAI")
        XCTAssertEqual(AssistantNotice.recipient("anthropic"), "Anthropic")
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

    func testSeveralTicksGoInOneRevision() {
        // Each choice is made on the choices as they stand, so the one revision sent after the
        // last of them carries them all.
        var choices: JSON = .null
        choices = ProposalChoices.toggle(choices, key: "t:a", skipped: false)
        choices = ProposalChoices.toggle(choices, key: "t:b", skipped: false)
        choices = ProposalChoices.change(choices, key: "t:c", change: ["category": "groceries"])
        choices = ProposalChoices.toggle(choices, key: "t:a", skipped: true)
        XCTAssertEqual(choices, ["skip": ["t:b"], "set": [["key": "t:c", "category": "groceries"]]])
    }

    func testSelectAllNoneAndOnlyTheOnesToCheck() {
        let rows: [JSON] = [["key": "t:a"], ["key": "t:b", "tone": "warn"], ["key": "t:c"], ["label": "unkeyed"]]
        let none = ProposalChoices.select(.null, rows: rows) { _ in false }
        XCTAssertEqual(none, ["skip": ["t:a", "t:b", "t:c"], "set": []])
        XCTAssertEqual(ProposalChoices.select(none, rows: rows) { _ in true }, ProposalChoices.original)
        XCTAssertEqual(ProposalChoices.select(.null, rows: rows) { _ in true }, ProposalChoices.original)
        XCTAssertEqual(ProposalChoices.select(none, rows: rows) { $0.text("tone") == "warn" }, ["skip": ["t:a", "t:c"], "set": []])
        // A row put back loses an amount of nothing given to it; other choices stay.
        let chosen: JSON = ["skip": ["t:a"], "set": [["key": "t:a", "amount": 0], ["key": "t:c", "amount": -500]]]
        XCTAssertEqual(ProposalChoices.select(chosen, rows: rows) { _ in true }, ["skip": [], "set": [["key": "t:c", "amount": -500]]])
    }

    func testChoicesShowOnTheRowsBeforeTheServiceHasThem() {
        let rows: [JSON] = [["label": "Careem", "amount": -3800, "key": "t:a", "edit": ["category": true, "categoryId": "transport", "amount": true]],
                            ["label": "Jarir", "amount": -9900, "key": "t:b", "skipped": true],
                            ["label": "Total"]]
        let pending: JSON = ["skip": ["t:a"], "set": [["key": "t:b", "amount": -5000, "category": .null]]]
        let shown = ProposalChoices.shown(rows, pending: pending, sent: ["skip": ["t:b"], "set": []]) { $0 ?? "Ready to Assign" }
        // Left out at once, as it was otherwise.
        XCTAssertTrue(shown[0].flag("skipped"))
        XCTAssertEqual(shown[0].long("amount"), -3800)
        // Put back, with the amount and the category just given.
        XCTAssertFalse(shown[1].flag("skipped"))
        XCTAssertEqual(shown[1].long("amount"), -5000)
        XCTAssertTrue(shown[1].flag("edited"))
        XCTAssertEqual(shown[1].text("given"), "Ready to Assign")
        XCTAssertFalse(shown[1]["edit"].has("categoryId"))
        XCTAssertEqual(shown[2], rows[2])
        // Apply says how much is chosen now.
        XCTAssertEqual(ProposalChoices.partial(shown, choosing: true), ProposalChoices.Partial(chosen: 1, total: 2))
        // What the card was already made with isn't laid over it a second time.
        let same = ProposalChoices.shown(rows, pending: pending, sent: pending) { _ in "x" }
        XCTAssertEqual(same[1].long("amount"), -9900)
        XCTAssertTrue(same[1].text("given").isEmpty)
        // A category is named, and the row's editor starts from it.
        let named = ProposalChoices.shown(rows, pending: ["skip": [], "set": [["key": "t:a", "category": "groceries"]]], sent: .null) { $0 == "groceries" ? "Groceries" : "?" }
        XCTAssertEqual(named[0].text("given"), "Groceries")
        XCTAssertEqual(named[0]["edit"].text("categoryId"), "groceries")
        XCTAssertEqual(named[0].long("amount"), -3800)
        XCTAssertNil(ProposalChoices.partial(named, choosing: true))
    }
}

/// A card is drawn from what the service's view of it says: the actions it offers, what it has to
/// say, and where it stands.
final class AssistantCardTests: XCTestCase {
    func testACardShowsTheActionsItKnowsInTheServicesOrder() {
        let waiting: JSON = ["id": "p1", "kind": "budget", "status": "proposed", "actions": ["apply", "askChanges", "useOriginal", "discard", "show"]]
        XCTAssertEqual(AssistantCard.actions(waiting), [.apply, .askChanges, .useOriginal, .discard, .show])
        XCTAssertEqual(AssistantCard.main(waiting), .apply)
        XCTAssertEqual(AssistantCard.buttons(waiting, canUndo: false), [.apply, .askChanges, .useOriginal, .discard, .show])
        // One this version doesn't know is skipped: Android's own, a report's Print, a newer service's.
        let texts: JSON = ["kind": "bankTexts", "status": "proposed", "actions": ["applyBankTexts", "askChanges", "discard"]]
        XCTAssertEqual(AssistantCard.actions(texts), [.askChanges, .discard])
        XCTAssertNil(AssistantCard.main(texts))
        let report: JSON = ["kind": "report", "status": "proposed", "made": true, "actions": ["preview", "print", "excel", "remove", "somethingNewer"]]
        XCTAssertEqual(AssistantCard.actions(report), [.preview, .excel, .remove])
        XCTAssertEqual(AssistantCard.main(report), .preview)
        // Remove is in the card's menu, not among its buttons.
        XCTAssertEqual(AssistantCard.buttons(report, canUndo: false), [.preview, .excel])
        let figures: JSON = ["kind": "figures", "status": "proposed", "made": true, "actions": ["remove", "show"]]
        XCTAssertNil(AssistantCard.main(figures))
        XCTAssertEqual(AssistantCard.buttons(figures, canUndo: false), [.show])
        // A question's answers are its rows.
        let question: JSON = ["kind": "question", "status": "proposed", "actions": ["answer"]]
        XCTAssertNil(AssistantCard.main(question))
        XCTAssertTrue(AssistantCard.buttons(question, canUndo: false).isEmpty)
        XCTAssertEqual(AssistantCard.main(["status": "failed", "actions": ["askAgain", "discard"]]), .askAgain)
        XCTAssertTrue(AssistantCard.actions(["status": "applied"]).isEmpty)
        // A kind this version doesn't know is drawn plainly; a bank-text card is known, with no Apply of its own here.
        XCTAssertTrue(AssistantCard.known(["kind": "delete"]))
        XCTAssertTrue(AssistantCard.known(["kind": "bankTexts"]))
        XCTAssertFalse(AssistantCard.known(["kind": "somethingNewer"]))
    }

    func testUndoShowsOnlyOnTheCardTheBudgetsUndoWouldTakeBack() {
        let applied: JSON = ["kind": "budget", "status": "applied", "actions": ["undo", "show"]]
        XCTAssertEqual(AssistantCard.lineActions(applied, canUndo: true), [.undo, .show])
        XCTAssertEqual(AssistantCard.lineActions(applied, canUndo: false), [.show])
        XCTAssertTrue(AssistantCard.openedActions(applied).isEmpty)
        // An undone card keeps Show on its line; applying it again is inside it.
        let undone: JSON = ["kind": "budget", "status": "undone", "actions": ["apply", "askChanges", "discard", "show"]]
        XCTAssertEqual(AssistantCard.lineActions(undone, canUndo: false), [.show])
        XCTAssertEqual(AssistantCard.openedActions(undone), [.apply, .askChanges, .discard])
        XCTAssertEqual(AssistantCard.lineActions(["kind": "budget", "status": "discarded", "actions": ["restore"]], canUndo: false), [.restore])
        // A converted file that was imported can still be saved.
        XCTAssertEqual(AssistantCard.openedActions(["kind": "import", "made": true, "status": "applied", "actions": ["saveCsv"]]), [.saveCsv])
    }

    func testAFinishedCardsLineSaysWhatBecameOfIt() {
        Money.configure(budget: ["currency": "USD", "dateFormat": "iso"])
        for status in ["applied", "discarded", "undone", "replaced"] { XCTAssertTrue(AssistantCard.finished(["status": .string(status)])) }
        for status in ["proposed", "failed", ""] { XCTAssertFalse(AssistantCard.finished(["status": .string(status)])) }
        XCTAssertEqual(AssistantCard.finishedWords(["status": "applied"]), "Applied")
        // With the time the service kept, as the budget writes dates and the device the time.
        let at = LibraryDates.instant("2026-10-21T10:00:00Z")!
        XCTAssertEqual(AssistantCard.finishedWords(["status": "applied", "appliedAt": "2026-10-21T10:00:00Z"]), "Applied " + LibraryDates.label(at))
        XCTAssertEqual(AssistantCard.finishedWords(["status": "applied", "appliedAt": "not a time"]), "Applied")
        XCTAssertEqual(AssistantCard.finishedWords(["status": "undone"]), "Undone")
        XCTAssertEqual(AssistantCard.finishedWords(["status": "replaced", "replacedBy": "p2"]), "Replaced by a newer version")
        XCTAssertEqual(AssistantCard.finishedWords(["status": "discarded"]), "Discarded")
        // A thing the assistant made is removed, not discarded.
        XCTAssertEqual(AssistantCard.finishedWords(["status": "discarded", "made": true]), "Removed")
        // A question that was answered.
        XCTAssertEqual(AssistantCard.asked(["kind": "question", "status": "applied", "title": "Which account?"]), "Asked: Which account?")
        XCTAssertNil(AssistantCard.asked(["kind": "question", "status": "proposed", "title": "Which account?"]))
        XCTAssertNil(AssistantCard.asked(["kind": "budget", "status": "applied", "title": "Move money"]))
    }

    func testACardSaysWhatItHasToSayInItsTone() {
        XCTAssertNil(AssistantCard.message(["status": "proposed"]))
        let updated = AssistantCard.message(["message": ["tone": "info", "text": "Updated for your budget as it is now."]])
        XCTAssertEqual(updated?.tone, "info")
        XCTAssertEqual(updated?.text, "Updated for your budget as it is now.")
        XCTAssertEqual(AssistantCard.message(["message": ["tone": "error", "text": "This can't be applied any more."]])?.tone, "error")
        XCTAssertNil(AssistantCard.message(["message": ["tone": "warn", "text": ""]]))
        // Ask for a new version sends the card's title and why it couldn't be applied.
        let failed: JSON = ["title": "Cover Dining", "status": "failed", "message": ["tone": "error", "text": "Fun has only 5.00 left."]]
        XCTAssertEqual(AssistantCard.askAgain(failed), "Prepare “Cover Dining” again. It couldn't be applied: Fun has only 5.00 left.")
    }

    func testWhichCardsFollowTheBudget() {
        XCTAssertTrue(AssistantCard.followsBudget(["kind": "budget", "status": "proposed"]))
        XCTAssertTrue(AssistantCard.followsBudget(["kind": "transactions", "status": "undone"]))
        XCTAssertTrue(AssistantCard.followsBudget(["kind": "delete", "status": "applied"]))
        XCTAssertFalse(AssistantCard.followsBudget(["kind": "budget", "status": "discarded"]))
        XCTAssertFalse(AssistantCard.followsBudget(["kind": "report", "made": true, "status": "proposed"]))
        XCTAssertFalse(AssistantCard.followsBudget(["kind": "question", "status": "proposed"]))
        XCTAssertFalse(AssistantCard.followsBudget(["kind": "fix", "status": "proposed"]))
    }

    func testFiguresBarsAreSharesOfTheLargestAmount() {
        let rows: [JSON] = [["label": "Rent", "amount": -120000], ["label": "Food", "amount": -30000], ["label": "Refund", "amount": 60000], ["label": "Note"]]
        XCTAssertEqual(AssistantCard.barShares(rows), [1, 0.25, 0.5, 0])
        XCTAssertEqual(AssistantCard.barShares([["label": "A", "amount": 0], ["label": "B"]]), [0, 0])
        XCTAssertTrue(AssistantCard.barShares([]).isEmpty)
    }

    func testADeleteCardCountsTheTransactionsStillChosen() {
        let rows: [JSON] = [["key": "t:a"], ["key": "t:b", "skipped": true], ["key": "t:c"]]
        XCTAssertEqual(AssistantCard.deleting(rows, more: 0), 2)
        XCTAssertEqual(AssistantCard.deleting([["label": "A"], ["label": "B"]], more: 3), 5)
    }

    func testAFixCardIsMarkedWithTheFixesThatDidNotTake() {
        let proposal: JSON = [
            "id": "p1", "kind": "fix",
            "card": ["rows": [["label": "Line 3 · date: 1/2 → 2026-02-01", "key": "f:3:date"], ["label": "Line 4 · amount: n/a → -12.40", "key": "f:4:amount"],
                              ["label": "Include line 9", "key": "i:9"]]],
            "fixes": ["fields": [["line": "3", "field": "date"], ["line": "4", "field": "amount"]], "include": [["line": "9"]]],
        ]
        let all: [JSON] = [["line": "3", "fix": "field", "applied": true], ["line": "4", "fix": "field", "applied": true], ["line": "9", "fix": "include", "applied": true]]
        XCTAssertNil(AssistantCard.fixNote(all, proposal: proposal))
        let some: [JSON] = [["line": "3", "fix": "field", "applied": true],
                            ["line": "4", "fix": "field", "applied": false, "problem": "This value changed since this fix was made."],
                            ["line": "9", "fix": "include", "applied": false]]
        XCTAssertEqual(AssistantCard.fixNote(some, proposal: proposal), "Fixes applied: 1 of 3. Not applied: Line 4 · amount: n/a → -12.40, Include line 9")
        // A fix with no row on the card is named by its line; after five of them, "…".
        let many: [JSON] = (1...7).map { number -> JSON in ["line": .string(String(number)), "fix": "revert", "applied": false] }
        XCTAssertEqual(AssistantCard.fixNote(many, proposal: proposal), "Fixes applied: 0 of 7. Not applied: Line 1, Line 2, Line 3, Line 4, Line 5, …")
    }

    func testTheNotesListIsEditedAsAWhole() {
        XCTAssertTrue(AssistantNotes.list(nil).isEmpty)
        XCTAssertTrue(AssistantNotes.list(["currency": "USD"]).isEmpty)
        let notes = AssistantNotes.list(["assistantNotes": ["Never take money from Holiday fund", "Rent is due on the 1st"]])
        XCTAssertEqual(notes, ["Never take money from Holiday fund", "Rent is due on the 1st"])
        // Added at the end, on one line with single spaces.
        XCTAssertEqual(AssistantNotes.adding("  Groceries are\n shared  ", to: notes), notes + ["Groceries are shared"])
        // Nothing to add, or there already.
        XCTAssertNil(AssistantNotes.adding("   ", to: notes))
        XCTAssertNil(AssistantNotes.adding("Rent is due on the 1st", to: notes))
        XCTAssertEqual(AssistantNotes.removing(0, from: notes), ["Rent is due on the 1st"])
        XCTAssertEqual(AssistantNotes.removing(5, from: notes), notes)
        // A note is at most 200 characters.
        XCTAssertEqual(AssistantNotes.cleaned(String(repeating: "a", count: 250)).count, 200)
        XCTAssertEqual(AssistantNotes.limited("Short"), "Short")
    }

    func testAnImportsUncategorizedTransactionsAndWhatIsAskedAboutThem() {
        let before: JSON = ["accounts": [["id": "a1", "name": "Checking", "kind": "cash"], ["id": "a2", "name": "Car loan", "kind": "tracking"]],
                            "categories": [["id": "c1", "name": "Groceries"], ["id": "u", "name": "Uncategorized"]],
                            "transactions": [["id": "old", "accountId": "a1", "splits": [["amount": -100]]]]]
        var after = before
        after["transactions"] = .array(before["transactions"].array + [
            ["id": "n1", "accountId": "a1", "splits": [["amount": -500]]],
            ["id": "n2", "accountId": "a1", "splits": [["amount": -700, "categoryId": "c1"]]],
            ["id": "n3", "accountId": "a1", "splits": [["amount": -900, "categoryId": "u"]]],
            ["id": "n4", "accountId": "a2", "splits": [["amount": -100]]],
            ["id": "n5", "accountId": "a1", "transferAccountId": "a2", "splits": [["amount": -100]]],
        ])
        // New, in a budget account, not a transfer, and with no category or only the placeholder.
        let fresh = AssistantStart.uncategorized(before: before, after: after)
        XCTAssertEqual(fresh.ids, ["n1", "n3"])
        XCTAssertEqual(fresh.accounts, ["Checking"])
        XCTAssertTrue(AssistantStart.uncategorized(before: after, after: after).ids.isEmpty)
        XCTAssertEqual(AssistantStart.categorize(fresh), "Categorize the 2 new transactions in Checking.")
        XCTAssertEqual(AssistantStart.categorize(AssistantStart.Imported(ids: ["n1"], accounts: ["Checking", "Savings"])),
                       "Categorize the new transaction in Checking and Savings.")
        XCTAssertEqual(AssistantStart.joined(["A", "B", "C"]), "A, B and C")
        // A message takes 200 of them.
        let many = AssistantStart.Imported(ids: (0..<230).map { "t\($0)" }, accounts: ["Checking"])
        XCTAssertEqual(AssistantStart.categorize(many), "Categorize these 200 of the 230 new transactions in Checking.")
    }
}
