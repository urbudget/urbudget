package app.urbudget.nativeapp.ui

/**
 * Opens a file the system's picker chose, to replace all of it. The picker can hand back one that's
 * already there, and "w" leaves the end of a longer file after what's written, which desktop and iPhone
 * then refuse. A provider that won't take "wt" gets "w": a backup with a tail beats no backup.
 */
internal fun <T> replacing(open: (mode: String) -> T): T = runCatching { open("wt") }.getOrElse { open("w") }
