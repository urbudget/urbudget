package app.urbudget.nativeapp

/** Network/state work owns its own service locks; only budget commits occupy the edit queue. */
internal object NativeServicePolicy {
    private val bankState = setOf("simplefin_status", "simplefin_connect", "simplefin_accounts", "simplefin_disconnect", "simplefin_open", "simplefin_sync",
        // Interactive Brokers (services/ibkr.rs): its token and the statement it last sent. Bringing a statement into the budget
        // (ibkr_apply) and importing its deposits (ibkr_import) are budget commits, and keep the edit queue.
        "ibkr_status", "ibkr_connect", "ibkr_fetch", "ibkr_file", "ibkr_disconnect", "ibkr_open", "ibkr_review_compact")
    private val reads = setOf("cloud_status", "account_overview", "billing_context", "sync_status", "sync_has_key",
        // Device approval (cloud/pairing.rs, cloud/recovery.rs): what the account's key is, the recovery code this device
        // still holds unsaved, and whether a shown code is still waiting. Making, using or taking back a code is a command.
        "sync_key_info", "sync_recovery_unsaved", "sync_pair_state",
        "native_budget_status", "database_status", "list_budgets", "list_backups", "backup_retention",
        "simplefin_status", "simplefin_accounts", "simplefin_sync", "ynab_tokens", "ynab_plans", "ibkr_status", "ibkr_review_compact")
    fun coalesce(method: String) = method in reads
    fun independent(method: String) = method in bankState || method in reads || method in setOf("forget_ynab_token", "sync_ynab") ||
        method.startsWith("account_") || method.startsWith("billing_") || method.startsWith("sync_")
    fun budgetResult(method: String) = method in setOf("sync_ynab", "sync_now", "sync_resolve")
    fun review(method: String) = method == "simplefin_sync"
    fun budgetRead(method: String) = review(method) || method in setOf("native_budget_status", "database_status", "list_backups", "backup_retention", "ibkr_review_compact")
}
