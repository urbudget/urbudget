package app.urbudget

import android.app.Activity
import com.android.billingclient.api.*
import org.json.JSONArray
import org.json.JSONObject

interface BillingCall {
    val args: JSONObject
    fun resolve(value: JSONObject = JSONObject())
    fun reject(message: String)
}

internal fun matchesActivePurchase(products: List<String>, accountId: String?, productId: String, expectedAccountId: String) =
    productId in products && accountId == expectedAccountId

/** What the billing client tells the person. The host app words each one in its interface language. */
enum class BillingProblem { NO_PURCHASE, UNAVAILABLE, SCREEN_CLOSED, PRODUCT_NOT_LOADED, PRODUCT_MISSING, PLAN_NOT_IN_PLAY,
    SIGN_IN_TO_SUBSCRIBE, PURCHASE_IN_PROGRESS, PLAN_UNAVAILABLE, SIGN_IN_TO_RESTORE, CLOSED_BEFORE_RESTORE, OPEN_PLAY_TO_MANAGE,
    PURCHASE_SCREEN_CLOSED, CONNECTION_CLOSED }

/** The English wording; [code] is Google Play's response code where the problem has one. */
fun englishBillingText(problem: BillingProblem, code: Int): String = when (problem) {
    BillingProblem.NO_PURCHASE -> "Google Play returned no purchase. Try restoring purchases."
    BillingProblem.UNAVAILABLE -> "Google Play billing is unavailable ($code). Try again in Google Play, or restore purchases later."
    BillingProblem.SCREEN_CLOSED -> "The billing screen closed. Reopen Account & sync to continue."
    BillingProblem.PRODUCT_NOT_LOADED -> "Google Play could not load this product ($code). Try again later."
    BillingProblem.PRODUCT_MISSING -> "This product is not available in Google Play yet."
    BillingProblem.PLAN_NOT_IN_PLAY -> "The monthly sync plan is unavailable in Google Play."
    BillingProblem.SIGN_IN_TO_SUBSCRIBE -> "Sign in to your UrBudget account before subscribing."
    BillingProblem.PURCHASE_IN_PROGRESS -> "A purchase is already in progress."
    BillingProblem.PLAN_UNAVAILABLE -> "The monthly sync plan is unavailable."
    BillingProblem.SIGN_IN_TO_RESTORE -> "Sign in before restoring purchases."
    BillingProblem.CLOSED_BEFORE_RESTORE -> "The billing screen closed. Restore purchases after reopening UrBudget."
    BillingProblem.OPEN_PLAY_TO_MANAGE -> "Open Google Play to manage your subscriptions."
    BillingProblem.PURCHASE_SCREEN_CLOSED -> "The purchase screen closed. Restore purchases after reopening UrBudget."
    BillingProblem.CONNECTION_CLOSED -> "The billing connection closed."
}

/** Play owns payment UI for the sync subscription. Only the server verifies, grants and acknowledges purchases. */
class PlayBillingClient(
    private val activity: Activity,
    private val words: (BillingProblem, Int) -> String = ::englishBillingText,
) : AutoCloseable {
    private fun BillingCall.reject(problem: BillingProblem, code: Int = 0) = reject(words(problem, code))
    private data class ActivePurchase(val invoke: BillingCall, val productId: String, val accountId: String)
    private var buying: ActivePurchase? = null
    private var connecting = false
    @Volatile private var closed = false
    private val waiting = mutableListOf<Pair<BillingCall, () -> Unit>>()
    private val client = BillingClient.newBuilder(activity)
        .setListener { result, purchases ->
            val active = buying
            if (active != null) {
                val invoke = active.invoke
                when (result.responseCode) {
                    BillingClient.BillingResponseCode.OK -> {
                        val matching = purchases.orEmpty().filter {
                            matchesActivePurchase(it.products, it.accountIdentifiers?.obfuscatedAccountId, active.productId, active.accountId)
                        }
                        // An earlier pending purchase can complete while this checkout is open.
                        // Only this checkout's receipt may finish its invocation.
                        if (matching.isNotEmpty()) {
                            buying = null
                            invoke.resolve(purchaseResult(matching))
                        } else if (purchases.isNullOrEmpty()) {
                            buying = null
                            invoke.reject(BillingProblem.NO_PURCHASE)
                        }
                    }
                    BillingClient.BillingResponseCode.USER_CANCELED -> {
                        buying = null
                        invoke.resolve(JSONObject().put("cancelled", true).put("purchases", JSONArray()))
                    }
                    BillingClient.BillingResponseCode.ITEM_ALREADY_OWNED -> { buying = null; restore(invoke) }
                    else -> { buying = null; fail(invoke, result) }
                }
            }
            // A completion delivered without an outstanding purchase call is picked up by
            // queryPurchasesAsync when the frontend resumes/restores. Never acknowledge here.
        }
        // Play requires one-time pending purchases to be enabled, though only the subscription is sold.
        .enablePendingPurchases(PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
        .enableAutoServiceReconnection()
        .build()

    private fun fail(invoke: BillingCall, result: BillingResult) =
        invoke.reject(BillingProblem.UNAVAILABLE, result.responseCode)

    private fun connected(invoke: BillingCall, action: () -> Unit) {
        activity.runOnUiThread {
            if (closed) { invoke.reject(BillingProblem.SCREEN_CLOSED); return@runOnUiThread }
            if (client.isReady) { action(); return@runOnUiThread }
            waiting.add(invoke to action)
            if (connecting) return@runOnUiThread
            connecting = true
            client.startConnection(object : BillingClientStateListener {
                override fun onBillingSetupFinished(result: BillingResult) {
                    activity.runOnUiThread {
                        connecting = false
                        val queue = waiting.toList()
                        waiting.clear()
                        queue.forEach { (call, work) ->
                            if (result.responseCode == BillingClient.BillingResponseCode.OK) work() else fail(call, result)
                        }
                    }
                }
                override fun onBillingServiceDisconnected() { /* automatic reconnection */ }
            })
        }
    }

    private fun query(id: String, invoke: BillingCall, unavailable: ((String) -> Unit)? = null, done: (ProductDetails) -> Unit) {
        val params = QueryProductDetailsParams.newBuilder().setProductList(listOf(
            QueryProductDetailsParams.Product.newBuilder().setProductId(id).setProductType(BillingClient.ProductType.SUBS).build()
        )).build()
        client.queryProductDetailsAsync(params) { result, answer ->
            if (closed) { invoke.reject(BillingProblem.SCREEN_CLOSED); return@queryProductDetailsAsync }
            if (result.responseCode != BillingClient.BillingResponseCode.OK) {
                if (unavailable != null) unavailable(words(BillingProblem.PRODUCT_NOT_LOADED, result.responseCode))
                else fail(invoke, result)
            }
            else {
                val details = answer.productDetailsList.firstOrNull { it.productId == id }
                if (details == null) {
                    val message = words(BillingProblem.PRODUCT_MISSING, 0)
                    if (unavailable != null) unavailable(message) else invoke.reject(message)
                } else done(details)
            }
        }
    }

    // Launch uses only the configured monthly base plan: no implicit introductory or discount offer.
    private fun subscriptionOffer(details: ProductDetails, basePlanId: String) =
        details.subscriptionOfferDetails?.firstOrNull {
            it.basePlanId == basePlanId && it.offerId == null &&
                it.pricingPhases.pricingPhaseList.size == 1 &&
                it.pricingPhases.pricingPhaseList[0].billingPeriod == "P1M" &&
                it.pricingPhases.pricingPhaseList[0].recurrenceMode == ProductDetails.RecurrenceMode.INFINITE_RECURRING
        }

    fun products(invoke: BillingCall) {
        val args = invoke.args
        connected(invoke) {
            fun unavailable(message: String) = invoke.resolve(JSONObject().put("errors", JSONObject().put("sync", message)))
            query(args.getString("syncId"), invoke, { unavailable(it) }) { sync ->
                val monthly = subscriptionOffer(sync, args.getString("basePlanId"))
                if (monthly == null) unavailable(words(BillingProblem.PLAN_NOT_IN_PLAY, 0))
                else invoke.resolve(JSONObject().put("errors", JSONObject())
                    .put("sync", JSONObject().put("id", sync.productId).put("price", monthly.pricingPhases.pricingPhaseList[0].formattedPrice)))
            }
        }
    }

    fun purchase(invoke: BillingCall) {
        val args = invoke.args
        if (!args.getString("accountId").matches(Regex("[a-f0-9]{64}"))) {
            invoke.reject(BillingProblem.SIGN_IN_TO_SUBSCRIBE); return
        }
        connected(invoke) {
            if (buying != null) { invoke.reject(BillingProblem.PURCHASE_IN_PROGRESS); return@connected }
            // Fetch again immediately before launch; cached ProductDetails can become stale.
            query(args.getString("productId"), invoke) { details ->
                val product = BillingFlowParams.ProductDetailsParams.newBuilder().setProductDetails(details)
                val offer = subscriptionOffer(details, args.getString("basePlanId"))
                if (offer == null) { invoke.reject(BillingProblem.PLAN_UNAVAILABLE); return@query }
                offer.offerToken?.let { product.setOfferToken(it) }
                activity.runOnUiThread {
                    // The screen may have closed while Google Play was fetching product details.
                    // Its delayed callback must never launch checkout on the disposed Activity.
                    if (closed) { invoke.reject(BillingProblem.SCREEN_CLOSED); return@runOnUiThread }
                    if (buying != null) { invoke.reject(BillingProblem.PURCHASE_IN_PROGRESS); return@runOnUiThread }
                    buying = ActivePurchase(invoke, args.getString("productId"), args.getString("accountId"))
                    val result = client.launchBillingFlow(activity, BillingFlowParams.newBuilder()
                        .setObfuscatedAccountId(args.getString("accountId"))
                        .setProductDetailsParamsList(listOf(product.build())).build())
                    if (result.responseCode != BillingClient.BillingResponseCode.OK) {
                        buying = null
                        if (result.responseCode == BillingClient.BillingResponseCode.ITEM_ALREADY_OWNED) restore(invoke)
                        else fail(invoke, result)
                    }
                }
            }
        }
    }

    private fun purchaseResult(purchases: List<Purchase>): JSONObject {
        val values = JSONArray()
        purchases.forEach { purchase ->
            values.put(JSONObject().put("token", purchase.purchaseToken)
                .put("products", JSONArray(purchase.products))
                .put("state", when (purchase.purchaseState) {
                    Purchase.PurchaseState.PURCHASED -> "purchased"
                    Purchase.PurchaseState.PENDING -> "pending"
                    else -> "unknown"
                }))
        }
        return JSONObject().put("cancelled", false).put("purchases", values)
    }

    fun restore(invoke: BillingCall) {
        val args = invoke.args
        if (!args.getString("accountId").matches(Regex("[a-f0-9]{64}"))) { invoke.reject(BillingProblem.SIGN_IN_TO_RESTORE); return }
        connected(invoke) {
            client.queryPurchasesAsync(QueryPurchasesParams.newBuilder().setProductType(BillingClient.ProductType.SUBS).build()) { result, subs ->
                if (closed) { invoke.reject(BillingProblem.CLOSED_BEFORE_RESTORE); return@queryPurchasesAsync }
                if (result.responseCode != BillingClient.BillingResponseCode.OK) fail(invoke, result)
                else invoke.resolve(purchaseResult(subs.filter { it.accountIdentifiers?.obfuscatedAccountId == args.getString("accountId") }))
            }
        }
    }

    fun manage(invoke: BillingCall) {
        activity.runOnUiThread {
            if (closed) { invoke.reject(BillingProblem.SCREEN_CLOSED); return@runOnUiThread }
            try {
                activity.startActivity(android.content.Intent(android.content.Intent.ACTION_VIEW,
                    android.net.Uri.parse("https://play.google.com/store/account/subscriptions?package=${activity.packageName}")))
                invoke.resolve()
            } catch (_: Exception) { invoke.reject(BillingProblem.OPEN_PLAY_TO_MANAGE) }
        }
    }

    override fun close() {
        if (closed) return
        closed = true
        buying?.invoke?.reject(BillingProblem.PURCHASE_SCREEN_CLOSED)
        buying = null
        waiting.forEach { it.first.reject(BillingProblem.CONNECTION_CLOSED) }
        waiting.clear()
        client.endConnection()
    }
}
