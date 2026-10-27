package app.urbudget.nativeapp

import android.app.Activity
import app.urbudget.BillingCall
import app.urbudget.BillingProblem
import app.urbudget.PlayBillingClient
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import org.json.JSONObject
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** Play displays the sync subscription's localized price and checkout; the shared server verifier grants access. */
class BillingGateway(activity: Activity) : AutoCloseable {
    private val client = PlayBillingClient(activity) { problem, code -> billingText(problem, code) }
    private suspend fun config() = withContext(Dispatchers.IO) { NativeServices.call("billing_context") as JSONObject }
    private suspend fun invoke(args: JSONObject, operation: (BillingCall) -> Unit): JSONObject = withContext(Dispatchers.Main) {
        suspendCancellableCoroutine { continuation ->
            operation(object : BillingCall {
                override val args = args
                override fun resolve(value: JSONObject) { if (continuation.isActive) continuation.resume(value) }
                override fun reject(message: String) { if (continuation.isActive) continuation.resumeWithException(IllegalStateException(message)) }
            })
        }
    }
    suspend fun products(): JSONObject = invoke(config(), client::products)
    suspend fun subscribe(): JSONObject {
        val config = config()
        val result = invoke(JSONObject().put("productId", config.getString("syncId")).put("accountId", config.getString("accountId"))
            .put("basePlanId", config.getString("basePlanId")), client::purchase)
        return withContext(Dispatchers.IO) { NativeServices.call("billing_verify", result) as JSONObject }
    }
    suspend fun restore(): JSONObject {
        val result = invoke(config(), client::restore)
        return withContext(Dispatchers.IO) { NativeServices.call("billing_verify", result) as JSONObject }
    }
    suspend fun manage() { invoke(JSONObject(), client::manage) }
    override fun close() { client.close() }
}

/** Google Play's problems in the interface language; the response code stays a plain number. */
internal fun billingText(problem: BillingProblem, code: Int): String = when (problem) {
    BillingProblem.NO_PURCHASE -> Words[R.string.billing_no_purchase]
    BillingProblem.UNAVAILABLE -> Words[R.string.billing_unavailable, code.toString()]
    BillingProblem.SCREEN_CLOSED -> Words[R.string.billing_screen_closed]
    BillingProblem.PRODUCT_NOT_LOADED -> Words[R.string.billing_product_not_loaded, code.toString()]
    BillingProblem.PRODUCT_MISSING -> Words[R.string.billing_product_missing]
    BillingProblem.PLAN_NOT_IN_PLAY -> Words[R.string.billing_plan_not_in_play]
    BillingProblem.SIGN_IN_TO_SUBSCRIBE -> Words[R.string.billing_sign_in_to_subscribe]
    BillingProblem.PURCHASE_IN_PROGRESS -> Words[R.string.billing_purchase_in_progress]
    BillingProblem.PLAN_UNAVAILABLE -> Words[R.string.billing_plan_unavailable]
    BillingProblem.SIGN_IN_TO_RESTORE -> Words[R.string.billing_sign_in_to_restore]
    BillingProblem.CLOSED_BEFORE_RESTORE -> Words[R.string.billing_closed_before_restore]
    BillingProblem.OPEN_PLAY_TO_MANAGE -> Words[R.string.billing_open_play_to_manage]
    BillingProblem.PURCHASE_SCREEN_CLOSED -> Words[R.string.billing_purchase_screen_closed]
    BillingProblem.CONNECTION_CLOSED -> Words[R.string.billing_connection_closed]
}
