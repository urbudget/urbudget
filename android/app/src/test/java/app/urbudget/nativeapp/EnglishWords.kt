package app.urbudget.nativeapp

import org.w3c.dom.Element
import java.io.File
import java.util.Locale
import javax.xml.parsers.DocumentBuilderFactory

/** One locale's string resources, read from the app's own XML the way Android reads them. */
class StringFile(val file: File) {
    val strings = LinkedHashMap<String, String>()
    val plurals = LinkedHashMap<String, Map<String, String>>()
    /** Strings marked translatable="false": names and the like, the same in every language. */
    val fixed = HashSet<String>()

    init {
        val document = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(file)
        val root = document.documentElement
        val nodes = root.childNodes
        for (i in 0 until nodes.length) {
            val node = nodes.item(i) as? Element ?: continue
            val name = node.getAttribute("name")
            when (node.tagName) {
                "string" -> {
                    require(name !in strings) { "$name appears twice in $file" }
                    strings[name] = unescape(node.textContent)
                    if (node.getAttribute("translatable") == "false") fixed.add(name)
                }
                "plurals" -> {
                    require(name !in plurals) { "$name appears twice in $file" }
                    val items = LinkedHashMap<String, String>()
                    val children = node.getElementsByTagName("item")
                    for (j in 0 until children.length) {
                        val item = children.item(j) as Element
                        items[item.getAttribute("quantity")] = unescape(item.textContent)
                    }
                    plurals[name] = items
                }
            }
        }
    }

    companion object {
        /** The app's resources, from the module directory Gradle runs unit tests in. */
        fun of(folder: String) = StringFile(File("src/main/res/$folder/strings.xml"))

        /** Android's string resource escapes: \n, \t, \', \", \\, \@, \?, \uXXXX; runs of white space read as one. */
        fun unescape(raw: String): String {
            val out = StringBuilder()
            var i = 0
            var quoted = false
            var space = false
            while (i < raw.length) {
                val c = raw[i]
                when {
                    c == '\\' && i + 1 < raw.length -> {
                        space = false
                        when (val next = raw[i + 1]) {
                            'n' -> out.append('\n')
                            't' -> out.append('\t')
                            'u' -> { out.append(raw.substring(i + 2, i + 6).toInt(16).toChar()); i += 4 }
                            else -> out.append(next)
                        }
                        i += 2
                        continue
                    }
                    c == '"' -> quoted = !quoted
                    !quoted && c.isWhitespace() -> { if (!space) out.append(' '); space = true }
                    else -> { out.append(c); space = false }
                }
                i++
            }
            return out.toString().trim()
        }
    }
}

/**
 * The English strings, as unit tests read the app's words: from values/strings.xml, by the ids in
 * the generated R class, formatted as Android formats them.
 */
object EnglishWords : WordSource {
    private val english = StringFile.of("values")
    private val names: Map<Int, String> = listOf("string", "plurals")
        .mapNotNull { runCatching { Class.forName("${R::class.java.name}\$$it") }.getOrNull() }
        .flatMap { it.fields.toList() }.associate { it.getInt(null) to it.name }

    /** Makes [Words] read English, for tests of code that words things. */
    fun install() { Words.source = this }

    override fun text(id: Int, args: Array<out Any>): String {
        val raw = english.strings[names[id]] ?: error("No English string for id $id")
        return if (args.isEmpty()) raw else String.format(Locale.ROOT, raw, *args)
    }

    override fun plural(id: Int, count: Int, args: Array<out Any>): String {
        val forms = english.plurals[names[id]] ?: error("No English plurals for id $id")
        val raw = (if (count == 1) forms["one"] else null) ?: forms.getValue("other")
        return String.format(Locale.ROOT, raw, *args)
    }
}
