package app.urbudget.nativeapp.ui

import org.junit.Assert.*
import org.junit.Test

class AssistantMarkdownTest {
    @Test fun largeReportTableBecomesIndividualLazyRowsWithoutLosingData() {
        val source = buildString {
            append("# Report\n\n| Payee | Amount |\n| --- | ---: |\n")
            repeat(10_000) { append("| Payee $it | $it |\n") }
            append("\nAll rows included.")
        }
        val document = markdownDocumentBlocks(source)
        assertEquals(10_003, document.size)
        assertEquals("heading", document.first().kind)
        assertEquals(listOf("Payee", "Amount"), document[1].rows.single())
        assertEquals(listOf("Payee 9999", "9999"), document[10_001].rows.single())
        assertEquals("All rows included.", document.last().text)
        assertTrue(document.none { it.kind == "table" })
    }

    @Test fun chatAndReportKeepCodeHeadingsAndCompleteParagraphs() {
        val source = "# Summary\n\n- First item\n```text\nexact | values\n```\n---\n**Final** [source](https://example.com)"
        val blocks = markdownBlocks(source)
        assertEquals(listOf("heading", "text", "code", "rule", "text"), blocks.map { it.kind })
        assertEquals("• First item", blocks[1].text)
        assertEquals("exact | values", blocks[2].text)
        assertEquals(blocks, markdownDocumentBlocks(source))
    }

    @Test fun aReportsTableRowsKnowTheirTableSoTheyScrollSidewaysTogether() {
        val source = "| A | B | C |\n| --- | --- | --- |\n| 1 | 2 |\n\nBetween.\n\n| X |\n| --- |\n| y |\n| z |"
        val document = markdownDocumentBlocks(source)
        assertEquals(listOf("tableHeader", "tableRow", "text", "tableHeader", "tableRow", "tableRow"), document.map { it.kind })
        // One number for each table's rows, and each row as wide as its table's widest.
        assertEquals(listOf(0, 0, -1, 1, 1, 1), document.map { it.table })
        assertEquals(listOf(3, 3, 0, 1, 1, 1), document.map { it.columns })
        // In a chat reply the table stays one block, which scrolls as one.
        assertEquals(listOf("table", "text", "table"), markdownBlocks(source).map { it.kind })
    }

    @Test fun aReplyBeingWrittenNeverShowsAMarkItHasntFinishedMaking() {
        // Bold and code that have begun are drawn as they will be; their marks alone aren't drawn at all.
        assertEquals("Here is where **Novem**", markdownLive("Here is where **Novem"))
        assertEquals("Here is where ", markdownLive("Here is where **"))
        assertEquals("Here is where ", markdownLive("Here is where *"))
        assertEquals("**November 2026**", markdownLive("**November 2026*"))
        assertEquals("- `Fue`", markdownLive("- `Fue"))
        assertEquals("Spent ", markdownLive("Spent `"))
        // A link shows its words, before its address and while it's written.
        assertEquals("See the gui", markdownLive("See [the gui"))
        assertEquals("See the guide", markdownLive("See [the guide]"))
        assertEquals("See the guide", markdownLive("See [the guide](https://urb"))
        assertEquals("See [the guide](https://urbudget.app/guide)", markdownLive("See [the guide](https://urbudget.app/guide)"))
        // A list's, a quote's or a heading's mark waits for its first word.
        listOf("-", "- ", "*", "> ", "##", "## ", "- **", "`", "``").forEach { assertEquals(it, "Before.\n", markdownLive("Before.\n$it")) }
        assertEquals("Before.\n- G", markdownLive("Before.\n- G"))
        // Only the line being written is touched, and a whole reply is left exactly as it is.
        val whole = "A **bold** start.\n\n- One `two`\n- Three\n\n| A | B |\n| --- | ---: |\n| 1 | 2 |\n\nDone [here](https://example.com)."
        assertEquals(whole, markdownLive(whole))
        assertEquals("An open **mark\nand its next line", markdownLive("An open **mark\nand its next line"))
        // Inside a block of code nothing is a mark.
        assertEquals("```\nlet a = **b", markdownLive("```\nlet a = **b"))
        assertEquals(listOf("text", "code"), markdownBlocks(markdownLive("Run:\n```")).map { it.kind })
    }

    @Test fun aTableBeingWrittenIsATableFromItsFirstHeading() {
        fun live(source: String) = markdownBlocks(markdownLive(source))
        // Its headings alone, part way and whole, with and without the line's end.
        listOf("Spending:\n\n| Category | Set as", "Spending:\n\n| Category | Set aside | Spent |", "Spending:\n\n| Category | Set aside | Spent |\n").forEach { source ->
            assertEquals(source, listOf("text", "table"), live(source).map { it.kind })
            assertEquals(source, "Category", live(source).last().rows.single().first())
        }
        // The line under them, as far as it has come: the columns it has finished already line up as they will.
        val part = live("| Category | Set aside | Spent |\n| --- | ---: | -")
        assertEquals(listOf(listOf("Category", "Set aside", "Spent")), part.single().rows)
        assertEquals(listOf(MarkdownAlign.Start, MarkdownAlign.End, MarkdownAlign.Start), part.single().aligns)
        assertEquals("table", live("| Category | Spent |\n|").single().kind)
        // Whole, it's the table the finished reply has; a row part way is a row.
        val source = "| Category | Set aside | Spent |\n| --- | ---: | ---: |\n| Groceries | 1,500.00 | 1,81"
        assertEquals(markdownBlocks(source), live(source))
        assertEquals(listOf("Groceries", "1,500.00", "1,81"), live(source).single().rows.last())
        assertEquals(listOf("**Gro**"), live("| Category |\n| --- |\n| **Gro").single().rows.last())
        // A line of words with a bar in it is not a table.
        assertEquals("Either this | or that", markdownLive("Either this | or that"))
    }

    @Test fun aTablesColumnsAreAsWideAsTheirLongestCellAndLineUpAsTheTableSays() {
        val table = markdownBlocks("| Category | Set aside | Spent |\n| :--- | ---: | :---: |\n| Groceries | 1,500.00 | 1,812.75 |\n| **Dining out** | 400.00 | [520.00](https://example.com) |").single()
        assertEquals(listOf(MarkdownAlign.Start, MarkdownAlign.End, MarkdownAlign.Center), table.aligns)
        // Ten, nine and eight characters, their marks not counted: three such columns fit a phone, where three of 160 didn't.
        assertEquals(listOf(90, 82, 74), table.widths)
        assertTrue(table.widths.sum() < 360)
        // A short column is still wide enough to tap and read; a long one stops and takes more lines.
        assertEquals(listOf(MarkdownColumnNarrowest, MarkdownColumnWidest), markdownColumnWidths(listOf(listOf("#", "x".repeat(80)))))
        // A row with fewer cells than the table's widest still has every column.
        assertEquals(3, markdownColumnWidths(listOf(listOf("A", "B", "C"), listOf("1"))).size)
        // A report's rows, each an item of its own, carry their table's columns.
        val rows = markdownDocumentBlocks("| A | Amount |\n| --- | ---: |\n| Rent | 6,500.00 |")
        assertEquals(listOf(table(rows[0]), table(rows[0])), rows.map(::table))
        assertEquals(listOf(MarkdownAlign.Start, MarkdownAlign.End), rows[1].aligns)
    }

    private fun table(block: AssistantMarkdownBlock) = block.widths to block.aligns
}
