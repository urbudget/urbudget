package app.urbudget.nativeapp

import org.json.JSONObject

/**
 * Anonymous usage counts, as the desktop keeps them (crates/urbudget-services/src/usage.rs): each
 * time the app comes to the front, the engine counts this copy as in use on urbudget.app if the day,
 * week or month is new since it was last counted, saying only what's new, with the app's version and
 * where it came from. Nothing identifies the copy, the phone or the person. Settings & data → Privacy
 * & data → Count this copy as in use turns it off.
 */
object UsageCounts {
    /** Where this copy came from: a preview APK (the debug-signed `app.urbudget.nativeapp`), or Google Play. */
    fun channel(applicationId: String): String = if (applicationId.endsWith(".nativeapp")) "preview" else "play"

    /**
     * What to ask the engine's `usage_report` with, or null when this copy doesn't count itself: the
     * switch is off, or it's a debug build with a release engine, which would count on urbudget.app.
     * A debug engine (-PrustDebug) counts only on the local server URBUDGET_API names.
     */
    fun request(enabled: Boolean, debugBuild: Boolean, debugEngine: Boolean, applicationId: String, version: String): JSONObject? {
        if (!enabled || (debugBuild && !debugEngine)) return null
        return JSONObject().put("platform", "android").put("channel", channel(applicationId)).put("version", version)
    }

    /** This copy's request, from its build and the switch in Settings. */
    fun request(preferences: NativePreferences): JSONObject? =
        request(preferences.usageCounts, BuildConfig.DEBUG, BuildConfig.RUST_DEBUG, BuildConfig.APPLICATION_ID, BuildConfig.VERSION_NAME)
}
