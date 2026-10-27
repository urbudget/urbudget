package app.urbudget.nativeapp

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Arabic must say everything English says, keep every argument, and use all of Arabic's plural forms. */
class StringResourcesTest {
    private val english = StringFile.of("values")
    private val arabic = StringFile.of("values-ar")
    private val argument = Regex("%(?:(\\d+)\\$)?([-#+0,(]*\\d*(?:\\.\\d+)?)([a-zA-Z%])")

    /** The arguments a string formats, as "%1\$s"; "%%" is a literal percent sign, not one. */
    private fun arguments(text: String): Set<String> = argument.findAll(text).filter { it.groupValues[3] != "%" }.map { it.value }.toSet()

    private fun problems(check: (MutableList<String>) -> Unit): List<String> = mutableListOf<String>().also(check)

    @Test fun everyStringIsTranslated() {
        val missing = english.strings.keys.filter { it !in english.fixed && it !in arabic.strings } +
            english.plurals.keys.filter { it !in arabic.plurals }
        assertEquals("Strings without Arabic", emptyList<String>(), missing)
        val extra = arabic.strings.keys.filter { it !in english.strings || it in english.fixed } + arabic.plurals.keys.filter { it !in english.plurals }
        assertEquals("Arabic strings English doesn't have, or that are the same in every language", emptyList<String>(), extra)
    }

    @Test fun noTranslationIsEmpty() {
        val empty = arabic.strings.filterValues { it.isBlank() }.keys +
            arabic.plurals.flatMap { (name, forms) -> forms.filterValues { it.isBlank() }.keys.map { "$name/$it" } }
        assertEquals(emptyList<String>(), empty.toList())
    }

    @Test fun argumentsAreKept() {
        val wrong = problems { out ->
            english.strings.forEach { (name, text) ->
                val translated = arabic.strings[name] ?: return@forEach
                if (arguments(text) != arguments(translated)) out += "$name: ${arguments(text)} became ${arguments(translated)}"
            }
            english.plurals.forEach { (name, forms) ->
                val all = forms.values.flatMap(::arguments).toSet()
                arabic.plurals[name]?.forEach { (quantity, text) ->
                    val kept = arguments(text)
                    // zero, one and two can say the number in words ("معاملتان"); every other argument stays.
                    val needed = if (quantity in setOf("zero", "one", "two")) all - "%1\$s" else all
                    if (!kept.containsAll(needed) || !all.containsAll(kept)) out += "$name/$quantity: $all became $kept"
                }
            }
        }
        assertEquals(emptyList<String>(), wrong)
    }

    @Test fun argumentsAreNumberedTextSoArabicReordersThemAndKeepsWesternDigits() {
        val wrong = problems { out ->
            (english.strings + english.plurals.flatMap { (name, forms) -> forms.map { "$name/${it.key}" to it.value } }).forEach { (name, text) ->
                argument.findAll(text).filter { it.groupValues[3] != "%" }.forEach { match ->
                    if (match.groupValues[1].isEmpty() || match.groupValues[3] != "s" || match.groupValues[2].isNotEmpty()) out += "$name: ${match.value}"
                }
            }
        }
        assertEquals("Use %1\$s-style arguments only", emptyList<String>(), wrong)
    }

    @Test fun pluralsCoverEveryQuantity() {
        val wrong = problems { out ->
            english.plurals.forEach { (name, forms) -> if (!forms.keys.containsAll(listOf("one", "other"))) out += "$name: English has ${forms.keys}" }
            arabic.plurals.forEach { (name, forms) ->
                val quantities = listOf("zero", "one", "two", "few", "many", "other")
                if (!forms.keys.containsAll(quantities)) out += "$name: Arabic has ${forms.keys}, needs $quantities"
            }
        }
        assertEquals(emptyList<String>(), wrong)
    }

    /**
     * An Arabic string starts with an Arabic word, so a Latin name or brand at its head can't turn the
     * line left to right; arguments are isolated and don't count. Arrows point the way Arabic reads.
     */
    @Test fun arabicReadsRightToLeft() {
        val isolated = Regex("[\u2066\u2067\u2068][^\u2069]*\u2069")
        val bareFigure = Regex("\\d[\\d,.]*\\s?[-%+]|[-+]\\d")
        val argumentText = Regex("%\\d+\\\$s")
        // Codes and date patterns are read by the app, not shown.
        val codes = arabic.strings.keys.filter { it == "language_code" || it.startsWith("date_pattern_") }
        val texts = arabic.strings - codes + arabic.plurals.flatMap { (name, forms) -> forms.map { "$name/${it.key}" to it.value } }
        val wrong = problems { out ->
            texts.forEach { (name, text) ->
                val outside = text.replace(isolated, "")
                if ('→' in text) out += "$name points right: $text"
                // After an Arabic word, "2026-10-31" or "1,000%" would come apart unless isolated.
                if (bareFigure.containsMatchIn(outside.replace(argumentText, ""))) out += "$name has a figure to isolate: $text"
                val first = outside.firstOrNull { it.isLetter() } ?: return@forEach
                if (Character.UnicodeScript.of(first.code) != Character.UnicodeScript.ARABIC) out += "$name starts with “$first”: $text"
            }
        }
        assertEquals(emptyList<String>(), wrong)
    }

    @Test fun arabicUsesWesternDigits() {
        val eastern = Regex("[٠-٩۰-۹]")
        val wrong = arabic.strings.filterValues { eastern.containsMatchIn(it) }.keys +
            arabic.plurals.filterValues { forms -> forms.values.any { eastern.containsMatchIn(it) } }.keys
        assertTrue("Arabic-Indic digits in $wrong", wrong.isEmpty())
    }
}
