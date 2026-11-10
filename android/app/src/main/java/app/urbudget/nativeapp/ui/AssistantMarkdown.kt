package app.urbudget.nativeapp.ui

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

/** Small native Markdown renderer for provider replies and report cards; no HTML execution. */
@Composable
internal fun AssistantMarkdown(source: String, modifier: Modifier = Modifier) {
    val blocks = preparedMarkdown(source) ?: return
    var page by remember(source) { mutableIntStateOf(0) }
    // A reply that was written while the panel was open showed whole, so it stays whole once it's finished:
    // whoever was reading it is where they were, not back on a first page. A long reply met later is paged.
    val whole = remember(source) { MarkdownCache.wasWritten(source) }
    val pages = if (whole) 1 else ((blocks.size + 39) / 40).coerceAtLeast(1)
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        key(page) { (if (whole) blocks else blocks.subList(page * 40, minOf(blocks.size, (page + 1) * 40))).forEach { block -> AssistantMarkdownBlockView(block) } }
        if (pages > 1) MarkdownPages(page, pages) { page = it }
    }
}

/**
 * A reply as it's being written, formatted as the finished one will be. It's laid out again as
 * text arrives, at most about ten times a second and on a worker; what was laid out last stays on
 * screen meanwhile, so the reply never blinks.
 */
@Composable
internal fun AssistantLiveMarkdown(source: String, modifier: Modifier = Modifier) {
    val latest by rememberUpdatedState(source)
    // Scrolled away and back, it starts from what it last showed rather than from nothing.
    val blocks by produceState(remember { MarkdownCache.live?.takeIf { source.startsWith(it.first) }?.second ?: MarkdownCache.get(source).orEmpty() }) {
        snapshotFlow { latest }.collect { text ->
            // Marks its last line hasn't finished making are tidied, so they never show as symbols.
            value = MarkdownCache.get(text) ?: withContext(Dispatchers.Default) { markdownBlocks(markdownLive(text)) }
            MarkdownCache.live = text to value
            delay(100)
        }
    }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) { blocks.forEach { block -> AssistantMarkdownBlockView(block) } }
}

/** A report owns the vertical viewport, so only visible paragraphs/table rows are composed. */
@Composable
internal fun AssistantMarkdownDocument(source: String, modifier: Modifier = Modifier) {
    val rows = preparedMarkdown(source, document = true) ?: return
    // A table's rows are separate items here: they share one sideways scroll, so its columns stay under their headings.
    val scrolls = remember(rows) { HashMap<Int, ScrollState>() }
    LazyColumn(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        itemsIndexed(rows, contentType = { _, block -> block.kind }) { _, block ->
            AssistantMarkdownBlockView(block, if (block.table >= 0) scrolls.getOrPut(block.table) { ScrollState(0) } else null)
        }
    }
}

/**
 * Replies laid out lately, by their text. A reply that was laid out while it was written, or as
 * the conversation was read (`warmAssistantMarkdown`), shows at once when it takes its place in
 * the conversation, with no empty frame between.
 */
private object MarkdownCache {
    private val replies = object : LinkedHashMap<String, List<AssistantMarkdownBlock>>(16, .75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, List<AssistantMarkdownBlock>>) = size > 24
    }
    /** The live reply as it was last laid out. */
    @Volatile var live: Pair<String, List<AssistantMarkdownBlock>>? = null
    /** The latest replies that were written live here, by their text. */
    private val written = ArrayDeque<String>()
    @Synchronized fun get(source: String): List<AssistantMarkdownBlock>? = replies[source]
    @Synchronized fun put(source: String, blocks: List<AssistantMarkdownBlock>) { replies[source] = blocks }
    @Synchronized fun written(source: String) { written.remove(source); written.addLast(source); if (written.size > 8) written.removeFirst() }
    @Synchronized fun wasWritten(source: String): Boolean = source.trim() in written
}

/** A reply has just been written live, [source] being its whole text: it showed whole as it came, and goes on showing whole now that it's finished. */
internal fun assistantReplyWritten(source: String) {
    if (source.isNotBlank()) MarkdownCache.written(source.trim())
}

/** Lays a reply out ahead of showing it, off the main thread. */
internal fun warmAssistantMarkdown(source: String) {
    if (source.isNotEmpty() && MarkdownCache.get(source) == null) MarkdownCache.put(source, markdownBlocks(source))
}

@Composable
private fun preparedMarkdown(source: String, document: Boolean = false): List<AssistantMarkdownBlock>? {
    // A reply laid out already shows at once, and a short one is laid out here; only a long one new to the screen, or a report, waits for a worker.
    val ready = remember(source, document) {
        if (document) null else MarkdownCache.get(source) ?: if (source.length <= 6_000) markdownBlocks(source).also { MarkdownCache.put(source, it) } else null
    }
    if (ready != null) return ready
    val prepared by produceState<Pair<String, List<AssistantMarkdownBlock>>?>(null, source, document) {
        value = withContext(Dispatchers.Default) { source to if (document) markdownDocumentBlocks(source) else markdownBlocks(source).also { MarkdownCache.put(source, it) } }
    }
    return prepared?.takeIf { it.first == source }?.second
}

/** Text that isn't the interface's own (what the assistant or the user wrote, a name on a card) reads in its own direction, whatever the interface's. */
internal val TextStyle.own get() = copy(textDirection = TextDirection.Content)

/** [tableScroll] is the sideways scroll a table's row shares with the table's other rows, when each row is an item of its own. */
@Composable
private fun AssistantMarkdownBlockView(block: AssistantMarkdownBlock, tableScroll: ScrollState? = null) {
    when (block.kind) {
        "table" -> {
            var page by remember(block) { mutableIntStateOf(0) }
            val body = block.rows.subList(minOf(1, block.rows.size), block.rows.size)
            Column {
                Column(Modifier.horizontalScroll(rememberScrollState())) {
                    block.rows.firstOrNull()?.let { MarkdownTableRow(it, true, block) }
                    body.subList(page * 30, minOf(body.size, (page + 1) * 30)).forEach { MarkdownTableRow(it, false, block) }
                }
                if (body.size > 30) MarkdownPages(page, (body.size + 29) / 30) { page = it }
            }
        }
        "tableHeader", "tableRow" -> Row(Modifier.horizontalScroll(tableScroll ?: rememberScrollState())) { MarkdownTableRow(block.rows.first(), block.kind == "tableHeader", block) }
        "code" -> Surface(color = MaterialTheme.colorScheme.surfaceContainerHighest, shape = MaterialTheme.shapes.small) {
            Text(block.text, Modifier.horizontalScroll(rememberScrollState()).padding(10.dp), fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodySmall.own)
        }
        "heading" -> Text(remember(block.text) { markdownInline(block.text) }, Modifier.fillMaxWidth(), style = MaterialTheme.typography.titleMedium.own, fontWeight = FontWeight.Bold)
        "rule" -> HorizontalDivider()
        else -> Text(remember(block.text) { markdownInline(block.text) }, Modifier.fillMaxWidth(), style = MaterialTheme.typography.bodyMedium.own)
    }
}

/**
 * A row of a table. Every row has the table's columns ([table]'s `widths`), each as wide as its
 * longest cell needs, so a table of a few short columns fits a phone without scrolling sideways and
 * every row scrolls as far as the others. A column the table says is right-aligned (amounts) ends
 * where its heading ends.
 */
@Composable
private fun MarkdownTableRow(cells: List<String>, heading: Boolean, table: AssistantMarkdownBlock) {
    // The widths are for the text's size as the app sets it: larger text gets wider columns.
    val scale = LocalDensity.current.fontScale
    // A column's side is the table's, whatever language a cell is in: a name in English and one in Arabic start at the same edge.
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    Row {
        repeat(maxOf(table.widths.size, cells.size)) { index ->
            val cell = cells.getOrElse(index) { "" }
            Text(remember(cell) { markdownInline(cell) }, Modifier.width((table.widths.getOrElse(index) { MarkdownColumnWidest }).dp * scale).padding(6.dp),
                fontWeight = if (heading) FontWeight.Bold else FontWeight.Normal, style = MaterialTheme.typography.bodySmall.own,
                textAlign = when (table.aligns.getOrNull(index)) {
                    MarkdownAlign.End -> if (rtl) TextAlign.Left else TextAlign.Right
                    MarkdownAlign.Center -> TextAlign.Center
                    else -> if (rtl) TextAlign.Right else TextAlign.Left
                })
        }
    }
}

@Composable
private fun MarkdownPages(page: Int, pages: Int, select: (Int) -> Unit) {
    Row {
        TextButton({ select(page - 1) }, enabled = page > 0) { Text(stringResource(R.string.assistant_previous)) }
        Text(stringResource(R.string.assistant_page, "${page + 1}", "$pages"), Modifier.padding(12.dp))
        TextButton({ select(page + 1) }, enabled = page + 1 < pages) { Text(stringResource(R.string.assistant_next)) }
    }
}

/** How a table's column lines its cells up, as the line under its headings says (`---`, `:---:`, `---:`). */
internal enum class MarkdownAlign { Start, Center, End }

/**
 * [table] and [columns] are for a table's row that stands as a block of its own: which table of the document it's in, and that table's widest row.
 * [widths] are a table's columns in dp, one for each, and [aligns] how each lines its cells up; a row that stands on its own carries its table's.
 */
internal data class AssistantMarkdownBlock(val kind: String, val text: String = "", val rows: List<List<String>> = emptyList(), val table: Int = -1, val columns: Int = 0,
                                           val widths: List<Int> = emptyList(), val aligns: List<MarkdownAlign> = emptyList())

internal fun markdownDocumentBlocks(source: String): List<AssistantMarkdownBlock> {
    var tables = 0
    return markdownBlocks(source).flatMap { block ->
        if (block.kind != "table") return@flatMap listOf(block)
        val table = tables++
        val columns = block.rows.maxOfOrNull { it.size } ?: 0
        block.rows.mapIndexed { index, cells ->
            AssistantMarkdownBlock(if (index == 0) "tableHeader" else "tableRow", rows = listOf(cells), table = table, columns = columns, widths = block.widths, aligns = block.aligns)
        }
    }
}

/** The narrowest and the widest a table's column is drawn, in dp: a longer cell takes more lines. */
internal const val MarkdownColumnNarrowest = 56
internal const val MarkdownColumnWidest = 220

/**
 * How wide each column of a table is drawn, in dp, from its longest cell: about what that cell's
 * text takes at the table's size, with the cell's own padding. Counted, not measured, so it's the
 * same wherever the table is laid out and costs nothing as a reply's table grows row by row.
 */
internal fun markdownColumnWidths(rows: List<List<String>>): List<Int> {
    val columns = rows.maxOfOrNull { it.size } ?: 0
    return List(columns) { column ->
        val longest = rows.maxOf { cells -> cells.getOrNull(column)?.let(::markdownCellLength) ?: 0f }
        (longest * 7.6f + 14f).toInt().coerceIn(MarkdownColumnNarrowest, MarkdownColumnWidest)
    }
}

/** A cell's length as it's drawn: without its marks, and with a wide character (Chinese, Japanese, Korean) counted as nearly two. */
private fun markdownCellLength(cell: String): Float =
    cell.replace(inlineMarkup) { it.groupValues[1].ifEmpty { it.groupValues[2].ifEmpty { it.groupValues[3] } } }.sumOf { if (it.code >= 0x2E80) 1.8 else 1.0 }.toFloat()

/**
 * A reply that's still being written, as it's laid out meanwhile: the same text, less the marks
 * its last line hasn't finished making, which would otherwise show as symbols until the rest of
 * them came. Bold and code that have begun are drawn as they'll be; a link shows its words; a
 * list's or heading's mark waits for its first word; and a table's headings are a table's from
 * their first cell, before the line under them has come. Whole lines are left as they are, so
 * the finished reply is laid out exactly as the live one was.
 */
internal fun markdownLive(source: String): String {
    val written = source.split('\n')
    // Inside a block of code nothing is a mark.
    if (written.count { it.trimStart().startsWith("```") } % 2 == 1) return source
    val last = written.lastIndex
    var line = written[last]
    // A link shows its words while its address is written, and before it.
    line = line.replace(linkPart, "$1")
    // Half of a closing or opening pair of stars is not a star to show.
    if (line.endsWith("*") && !line.endsWith("**") && line.trim() != "*") line = line.dropLast(1)
    if (boldMark.findAll(line).count() % 2 == 1) line = if (line.endsWith("**")) line.dropLast(2) else "$line**"
    if (line.count { it == '`' } % 2 == 1) line = if (line.endsWith("`")) line.dropLast(1) else "$line`"
    // A mark with nothing after it yet: a list's, a quote's, a heading's, the start of a block of code.
    if (line.trim().matches(loneMark)) line = ""
    val lines = if (line == written[last]) written else written.subList(0, last) + line
    // The table being written: the run of lines with cells that ends the text, or ends just before its last, empty line.
    val end = if (lines[last].isEmpty() && last > 0) last - 1 else last
    var start = end
    while (start >= 0 && lines[start].isNotBlank() && '|' in lines[start]) start--
    start++
    val tidied = if (start > end || !lines[start].trimStart().startsWith("|")) lines else {
        fun cells(row: String) = row.trim().removePrefix("|").removeSuffix("|").split('|').map(String::trim)
        when {
            // Only its headings so far: they're a table's already, before the line under them has come.
            end == start -> lines.subList(0, end + 1) + "|---|" + lines.subList(end + 1, lines.size)
            // That line is part way: it stands whole, with what it already says of how each column lines up.
            end == start + 1 && end == last && lines[end].matches(tableDividerPart) && !cells(lines[end]).all { it.matches(tableDivider) } ->
                lines.subList(0, end) + cells(lines[end]).joinToString("|", "|", "|") { if (it.matches(tableDivider)) it else "---" }
            else -> lines
        }
    }
    return if (tidied === written) source else tidied.joinToString("\n")
}

internal fun markdownBlocks(source: String): List<AssistantMarkdownBlock> {
    val lines = source.lines()
    val blocks = mutableListOf<AssistantMarkdownBlock>()
    var index = 0
    fun cells(line: String) = line.trim().removePrefix("|").removeSuffix("|").split('|').map(String::trim)
    while (index < lines.size) {
        val line = lines[index]
        when {
            line.isBlank() -> index++
            line.trimStart().startsWith("```") -> {
                index++; val code = mutableListOf<String>()
                while (index < lines.size && !lines[index].trimStart().startsWith("```")) code += lines[index++]
                if (index < lines.size) index++
                blocks += AssistantMarkdownBlock("code", code.joinToString("\n"))
            }
            '|' in line && index + 1 < lines.size && cells(lines[index + 1]).all { it.matches(tableDivider) } -> {
                val table = mutableListOf(cells(line))
                val aligns = cells(lines[index + 1]).map { if (!it.endsWith(':')) MarkdownAlign.Start else if (it.startsWith(':')) MarkdownAlign.Center else MarkdownAlign.End }
                index += 2
                while (index < lines.size && lines[index].isNotBlank() && '|' in lines[index]) table += cells(lines[index++])
                blocks += AssistantMarkdownBlock("table", rows = table, widths = markdownColumnWidths(table), aligns = aligns)
            }
            line.trim().matches(horizontalRule) -> { blocks += AssistantMarkdownBlock("rule"); index++ }
            line.startsWith('#') -> { blocks += AssistantMarkdownBlock("heading", line.replace(headingPrefix, "")); index++ }
            else -> {
                blocks += AssistantMarkdownBlock("text", line.replace(listPrefix, "• ").removePrefix("> ")); index++
            }
        }
    }
    return blocks
}

private val inlineMarkup = Regex("\\*\\*([^*]+)\\*\\*|`([^`]+)`|\\[([^]]+)]\\((https?://[^)\\s]+)\\)")
private val tableDivider = Regex(":?-{3,}:?")
/** The line under a table's headings, as far as it has been written. */
private val tableDividerPart = Regex("[\\s|:-]*")
/** A link at the end of a line that's still being written: its words, with or without the start of its address. */
private val linkPart = Regex("\\[([^\\]]*)(?:](?:\\([^)\\s]*)?)?$")
private val boldMark = Regex("\\*\\*")
private val loneMark = Regex("[-*+>]|#{1,6}|`{1,2}")
private val horizontalRule = Regex("[-*_]{3,}")
private val headingPrefix = Regex("^#{1,6}\\s*")
private val listPrefix = Regex("^\\s*[-*]\\s+")
private fun markdownInline(text: String): AnnotatedString = buildAnnotatedString {
    var cursor = 0
    inlineMarkup.findAll(text).forEach { match ->
        append(text.substring(cursor, match.range.first))
        when {
            match.groups[1] != null -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(match.groupValues[1]) }
            match.groups[2] != null -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace)) { append(match.groupValues[2]) }
            else -> withLink(LinkAnnotation.Url(match.groupValues[4])) { append(match.groupValues[3]) }
        }
        cursor = match.range.last + 1
    }
    append(text.substring(cursor))
}
