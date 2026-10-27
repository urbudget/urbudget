package app.urbudget.nativeapp.ui

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
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Small native Markdown renderer for provider replies and report cards; no HTML execution. */
@Composable
internal fun AssistantMarkdown(source: String, modifier: Modifier = Modifier) {
    val blocks = preparedMarkdown(source) ?: return
    var page by remember(source) { mutableIntStateOf(0) }
    val pages = ((blocks.size + 39) / 40).coerceAtLeast(1)
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        key(page) { blocks.subList(page * 40, minOf(blocks.size, (page + 1) * 40)).forEach { block -> AssistantMarkdownBlockView(block) } }
        if (pages > 1) MarkdownPages(page, pages) { page = it }
    }
}

/** A report owns the vertical viewport, so only visible paragraphs/table rows are composed. */
@Composable
internal fun AssistantMarkdownDocument(source: String, modifier: Modifier = Modifier) {
    val rows = preparedMarkdown(source, document = true) ?: return
    LazyColumn(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        itemsIndexed(rows, contentType = { _, block -> block.kind }) { _, block -> AssistantMarkdownBlockView(block) }
    }
}

@Composable
private fun preparedMarkdown(source: String, document: Boolean = false): List<AssistantMarkdownBlock>? {
    val prepared by produceState<Pair<String, List<AssistantMarkdownBlock>>?>(null, source, document) {
        value = withContext(Dispatchers.Default) { source to if (document) markdownDocumentBlocks(source) else markdownBlocks(source) }
    }
    return prepared?.takeIf { it.first == source }?.second
}

@Composable
private fun AssistantMarkdownBlockView(block: AssistantMarkdownBlock) {
    when (block.kind) {
        "table" -> {
            var page by remember(block) { mutableIntStateOf(0) }
            val body = block.rows.subList(minOf(1, block.rows.size), block.rows.size)
            Column {
                Column(Modifier.horizontalScroll(rememberScrollState())) {
                    block.rows.firstOrNull()?.let { MarkdownTableRow(it, true) }
                    body.subList(page * 30, minOf(body.size, (page + 1) * 30)).forEach { MarkdownTableRow(it, false) }
                }
                if (body.size > 30) MarkdownPages(page, (body.size + 29) / 30) { page = it }
            }
        }
        "tableHeader", "tableRow" -> Row(Modifier.horizontalScroll(rememberScrollState())) { MarkdownTableRow(block.rows.first(), block.kind == "tableHeader") }
        "code" -> Surface(color = MaterialTheme.colorScheme.surfaceContainerHighest, shape = MaterialTheme.shapes.small) {
            Text(block.text, Modifier.horizontalScroll(rememberScrollState()).padding(10.dp), fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodySmall)
        }
        "heading" -> Text(remember(block.text) { markdownInline(block.text) }, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        "rule" -> HorizontalDivider()
        else -> Text(remember(block.text) { markdownInline(block.text) }, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun MarkdownTableRow(cells: List<String>, heading: Boolean) {
    Row {
        cells.forEach { cell -> Text(remember(cell) { markdownInline(cell) }, Modifier.width(160.dp).padding(6.dp),
            fontWeight = if (heading) FontWeight.Bold else FontWeight.Normal, style = MaterialTheme.typography.bodySmall) }
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

internal data class AssistantMarkdownBlock(val kind: String, val text: String = "", val rows: List<List<String>> = emptyList())

internal fun markdownDocumentBlocks(source: String): List<AssistantMarkdownBlock> = markdownBlocks(source).flatMap { block ->
    if (block.kind == "table") block.rows.mapIndexed { index, cells -> AssistantMarkdownBlock(if (index == 0) "tableHeader" else "tableRow", rows = listOf(cells)) }
    else listOf(block)
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
