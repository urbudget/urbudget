package app.urbudget.nativeapp.ui

import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.pdf.PdfDocument
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import android.text.Layout
import android.text.SpannableStringBuilder
import android.text.StaticLayout
import android.text.TextPaint
import android.text.style.StyleSpan
import android.text.style.RelativeSizeSpan
import org.json.JSONObject
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import java.io.FileOutputStream
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Android's print framework supplies the printer and Save as PDF, without a WebView. */
internal fun printAssistantReport(context: Context, report: JSONObject) {
    val manager = context.getSystemService(Context.PRINT_SERVICE) as PrintManager
    manager.print(report.text("title"), AssistantReportPrint(report), PrintAttributes.Builder()
        .setMediaSize(PrintAttributes.MediaSize.ISO_A4).setColorMode(PrintAttributes.COLOR_MODE_MONOCHROME).build())
}

private class AssistantReportPrint(private val report: JSONObject) : PrintDocumentAdapter() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private data class ReportLayout(val width: Int, val height: Int, val margin: Int, val text: StaticLayout, val pages: List<IntRange>, val paint: TextPaint)
    private var layout: ReportLayout? = null
    private var layoutGeneration = 0

    override fun onLayout(oldAttributes: PrintAttributes?, newAttributes: PrintAttributes, cancellationSignal: CancellationSignal, callback: LayoutResultCallback, extras: Bundle?) {
        if (cancellationSignal.isCanceled) { callback.onLayoutCancelled(); return }
        val generation = ++layoutGeneration
        val job = scope.launch {
            try {
                val prepared = withContext(Dispatchers.Default) { prepare(newAttributes) }
                if (cancellationSignal.isCanceled || generation != layoutGeneration) callback.onLayoutCancelled()
                else {
                    layout = prepared
                    callback.onLayoutFinished(PrintDocumentInfo.Builder(report.text("title").take(80) + ".pdf")
                        .setContentType(PrintDocumentInfo.CONTENT_TYPE_DOCUMENT).setPageCount(prepared.pages.size).build(), oldAttributes != newAttributes)
                }
            } catch (cancel: CancellationException) { callback.onLayoutCancelled() }
            catch (failure: Exception) { callback.onLayoutFailed(failure.message ?: Words[R.string.assistant_report_layout_failed]) }
        }
        cancellationSignal.setOnCancelListener { job.cancel() }
    }

    private fun prepare(newAttributes: PrintAttributes): ReportLayout {
        val size = newAttributes.mediaSize ?: PrintAttributes.MediaSize.ISO_A4
        val width = (size.widthMils * 72f / 1000).toInt()
        val height = (size.heightMils * 72f / 1000).toInt()
        val margins = newAttributes.minMargins ?: PrintAttributes.Margins.NO_MARGINS
        val margin = maxOf(36, (margins.leftMils * 72f / 1000).toInt(), (margins.rightMils * 72f / 1000).toInt(),
            (margins.topMils * 72f / 1000).toInt(), (margins.bottomMils * 72f / 1000).toInt())
        val text = reportText(report)
        val paint = TextPaint().apply { color = Color.BLACK; textSize = 11f; isAntiAlias = true }
        val built = StaticLayout.Builder.obtain(text, 0, text.length, paint, (width - margin * 2).coerceAtLeast(100))
            .setAlignment(Layout.Alignment.ALIGN_NORMAL).setIncludePad(false).setLineSpacing(3f, 1f).build()
        val available = (height - margin * 2 - 20).coerceAtLeast(100)
        val ranges = mutableListOf<IntRange>()
        var first = 0
        while (first < built.lineCount) {
            var end = first
            while (end + 1 < built.lineCount && built.getLineBottom(end + 1) - built.getLineTop(first) <= available) end++
            ranges += first..end; first = end + 1
        }
        return ReportLayout(width, height, margin, built, ranges, paint)
    }

    override fun onWrite(pageRanges: Array<out PageRange>, destination: ParcelFileDescriptor, cancellationSignal: CancellationSignal, callback: WriteResultCallback) {
        val prepared = layout ?: run { callback.onWriteFailed(Words[R.string.assistant_report_not_laid_out]); return }
        val job = scope.launch {
            try {
                val written = withContext(Dispatchers.IO) {
                    val (width, height, margin, text, pages, paint) = prepared
                    val written = mutableListOf<PageRange>()
                    val document = PdfDocument()
                    try {
                        pages.forEachIndexed { index, lines ->
                            ensureActive()
                            if (cancellationSignal.isCanceled) throw CancellationException("Printing cancelled")
                            if (pageRanges.none { index in it.start..it.end }) return@forEachIndexed
                            val page = document.startPage(PdfDocument.PageInfo.Builder(width, height, index + 1).create())
                            page.canvas.save()
                            val top = text.getLineTop(lines.first)
                            val bottom = text.getLineBottom(lines.last)
                            page.canvas.translate(margin.toFloat(), margin.toFloat())
                            page.canvas.clipRect(0, 0, width - margin * 2, bottom - top)
                            page.canvas.translate(0f, -top.toFloat())
                            text.draw(page.canvas)
                            page.canvas.restore()
                            page.canvas.drawText("${index + 1} / ${pages.size}", margin.toFloat(), (height - margin / 2).toFloat(), paint)
                            document.finishPage(page)
                            written += PageRange(index, index)
                        }
                        ensureActive()
                        FileOutputStream(destination.fileDescriptor).use(document::writeTo)
                        written.toTypedArray()
                    } finally { document.close() }
                }
                callback.onWriteFinished(written)
            } catch (cancel: CancellationException) { callback.onWriteCancelled() }
            catch (failure: Exception) { callback.onWriteFailed(failure.message ?: Words[R.string.assistant_report_print_failed]) }
        }
        cancellationSignal.setOnCancelListener { job.cancel() }
    }

    override fun onFinish() { scope.cancel(); super.onFinish() }

    private fun reportText(report: JSONObject): CharSequence = SpannableStringBuilder().apply {
        append(report.text("title") + "\n")
        setSpan(StyleSpan(Typeface.BOLD), 0, length, 0)
        setSpan(RelativeSizeSpan(1.6f), 0, length, 0)
        append(Money.joined(listOf(report.text("budget"), report.text("period"), report.text("created").take(10))) + "\n\n")
        report.text("body").lines().forEach { source ->
            val heading = source.startsWith('#')
            val start = length
            val line = source.replace(headingMarkup, "").replace(boldMarkup, "$1")
                .replace(linkMarkup, "$1 ($2)")
            append(line + "\n")
            if (heading) { setSpan(StyleSpan(Typeface.BOLD), start, length, 0); setSpan(RelativeSizeSpan(1.2f), start, length, 0) }
        }
    }

    companion object {
        private val headingMarkup = Regex("^#{1,6}\\s*")
        private val boldMarkup = Regex("\\*\\*(.+?)\\*\\*")
        private val linkMarkup = Regex("\\[([^]]+)]\\(([^)]+)\\)")
    }
}
