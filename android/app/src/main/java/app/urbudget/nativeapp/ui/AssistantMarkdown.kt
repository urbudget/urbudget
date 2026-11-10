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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
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
            value = MarkdownCache.get(text) ?: withContext(Dispatchers.Default) { markdownBlocks(text) }
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
            val columns = remember(block) { block.rows.maxOfOrNull { it.size } ?: 0 }
            Column {
                Column(Modifier.horizontalScroll(rememberScrollState())) {
                    block.rows.firstOrNull()?.let { MarkdownTableRow(it, true, columns) }
                    body.subList(page * 30, minOf(body.size, (page + 1) * 30)).forEach { MarkdownTableRow(it, false, columns) }
                }
                if (body.size > 30) MarkdownPages(page, (body.size + 29) / 30) { page = it }
            }
        }
        "tableHeader", "tableRow" -> Row(Modifier.horizontalScroll(tableScroll ?: rememberScrollState())) { MarkdownTableRow(block.rows.first(), block.kind == "tableHeader", block.columns) }
        "code" -> Surface(color = MaterialTheme.colorScheme.surfaceContainerHighest, shape = MaterialTheme.shapes.small) {
            Text(block.text, Modifier.horizontalScroll(rememberScrollState()).padding(10.dp), fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodySmall.own)
        }
        "heading" -> Text(remember(block.text) { markdownInline(block.text) }, Modifier.fillMaxWidth(), style = MaterialTheme.typography.titleMedium.own, fontWeight = FontWeight.Bold)
        "rule" -> HorizontalDivider()
        else -> Text(remember(block.text) { markdownInline(block.text) }, Modifier.fillMaxWidth(), style = MaterialTheme.typography.bodyMedium.own)
    }
}

/** A row of a table, as wide as the table's widest row ([columns]) so every row scrolls as far as the others. */
@Composable
private fun MarkdownTableRow(cells: List<String>, heading: Boolean, columns: Int = cells.size) {
    Row {
        repeat(maxOf(columns, cells.size)) { index ->
            val cell = cells.getOrElse(index) { "" }
            Text(remember(cell) { markdownInline(cell) }, Modifier.width(160.dp).padding(6.dp),
                fontWeight = if (heading) FontWeight.Bold else FontWeight.Normal, style = MaterialTheme.typography.bodySmall.own)
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

/** [table] and [columns] are for a table's row that stands as a block of its own: which table of the document it's in, and that table's widest row. */
internal data class AssistantMarkdownBlock(val kind: String, val text: String = "", val rows: List<List<String>> = emptyList(), val table: Int = -1, val columns: Int = 0)

internal fun markdownDocumentBlocks(source: String): List<AssistantMarkdownBlock> {
    var tables = 0
    return markdownBlocks(source).flatMap { block ->
        if (block.kind != "table") return@flatMap listOf(block)
        val table = tables++
        val columns = block.rows.maxOfOrNull { it.size } ?: 0
        block.rows.mapIndexed { index, cells -> AssistantMarkdownBlock(if (index == 0) "tableHeader" else "tableRow", rows = listOf(cells), table = table, columns = columns) }
    }
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
                val table = mutableListOf(cells(line)); index += 2
                while (index < lines.size && lines[index].isNotBlank() && '|' in lines[index]) table += cells(lines[index++])
                blocks += AssistantMarkdownBlock("table", rows = table)
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
