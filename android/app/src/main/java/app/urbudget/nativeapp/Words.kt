package app.urbudget.nativeapp

import android.content.res.Resources
import androidx.annotation.PluralsRes
import androidx.annotation.StringRes

/** Where the app's words come from: its string resources, in the language in use. */
interface WordSource {
    fun text(@StringRes id: Int, args: Array<out Any>): String
    fun plural(@PluralsRes id: Int, count: Int, args: Array<out Any>): String
}

/** The string resources of [resources]: the activity's, which follow the app's language on every Android version. */
class ResourceWords(private val resources: Resources) : WordSource {
    override fun text(id: Int, args: Array<out Any>): String = if (args.isEmpty()) resources.getString(id) else resources.getString(id, *args)
    override fun plural(id: Int, count: Int, args: Array<out Any>): String = resources.getQuantityString(id, count, *args)
}

/**
 * The interface's words for code outside composition: report and status text, and the messages
 * raised below the screens. The activity points it at its resources whenever the language changes,
 * unit tests at the English file. Screens use `stringResource` instead.
 *
 * Every argument is text (`%1$s`), never a number the resources would format: Arabic shows the
 * same Western digits as the budget (docs/LOCALIZATION.md), so counts arrive already written.
 */
object Words {
    @Volatile var source: WordSource? = null

    private fun words() = checkNotNull(source) { "The app's words are not loaded yet." }

    operator fun get(@StringRes id: Int, vararg args: Any): String = words().text(id, args)

    /** A counted phrase: the count fills `%1$s` and picks the plural form; `more` fills the arguments after it. */
    fun count(@PluralsRes id: Int, count: Long, vararg more: Any): String = countAs(id, count, count.toString(), *more)

    /** As [count], with the count written as [countText] (grouped, say: "1,234"). */
    fun countAs(@PluralsRes id: Int, count: Long, countText: String, vararg more: Any): String =
        words().plural(id, count.coerceIn(Int.MIN_VALUE.toLong(), Int.MAX_VALUE.toLong()).toInt(), arrayOf(countText, *more))
}
