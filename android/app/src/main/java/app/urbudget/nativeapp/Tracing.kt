package app.urbudget.nativeapp

/** A named section in a Perfetto trace. It must end on the thread it began on, so never put a suspension point inside. */
internal inline fun <T> traced(name: String, block: () -> T): T {
    android.os.Trace.beginSection(name)
    try { return block() } finally { android.os.Trace.endSection() }
}
