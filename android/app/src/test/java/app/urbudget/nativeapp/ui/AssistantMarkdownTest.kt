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
}
