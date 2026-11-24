package app.urbudget.nativeapp

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Test

class SharedServiceReadsTest {
    @Test fun reopeningAScreenJoinsItsDownloadAndCancellationOnlyDetachesTheSubscriber() = runTest {
        val reads = SharedServiceReads(backgroundScope)
        val release = CompletableDeferred<Unit>()
        var requests = 0
        val first = async { reads.request("bank:budget-a:revision-1") { requests++; release.await(); "review" } }
        runCurrent()
        assertEquals(1, requests)
        first.cancelAndJoin()
        val reopened = async { reads.request("bank:budget-a:revision-1") { error("Must join the existing download") } }
        runCurrent()
        release.complete(Unit)
        assertEquals("review", reopened.await())
        assertEquals("fresh", reads.request("bank:budget-a:revision-1") { requests++; "fresh" })
        assertEquals(2, requests)
    }

    @Test fun anotherBudgetRevisionNeverReceivesTheOldReview() = runTest {
        val reads = SharedServiceReads(backgroundScope)
        val release = CompletableDeferred<Unit>()
        val old = async { reads.request("bank:budget-a:revision-1") { release.await(); "old" } }
        runCurrent()
        assertEquals("new", reads.request("bank:budget-a:revision-2") { "new" })
        assertFalse(old.isCompleted)
        release.complete(Unit)
        assertEquals("old", old.await())
    }

    @Test fun onlyIndependentReadsJoinAndBudgetCommitsKeepTheirOrderedPath() {
        for (read in listOf("simplefin_status", "simplefin_accounts", "simplefin_sync", "ynab_tokens", "ynab_plans", "account_overview", "ibkr_status", "ibkr_review_compact")) {
            assertTrue(read, NativeServicePolicy.independent(read))
            assertTrue(read, NativeServicePolicy.coalesce(read))
        }
        for (commit in listOf("simplefin_import", "ibkr_apply", "ibkr_import", "accept_ynab_import", "resolve_ynab", "native_apply_patch", "edit_budget", "open_budget", "restore_backup_file")) {
            assertFalse(commit, NativeServicePolicy.independent(commit))
            assertFalse(commit, NativeServicePolicy.coalesce(commit))
        }
        assertTrue(NativeServicePolicy.independent("sync_ynab"))
        assertTrue(NativeServicePolicy.budgetResult("sync_ynab"))
        assertFalse(NativeServicePolicy.coalesce("sync_ynab"))
        assertFalse(NativeServicePolicy.coalesce("simplefin_connect"))
        // Interactive Brokers' token and downloads keep off the edit queue, and each fetch is its own.
        for (broker in listOf("ibkr_connect", "ibkr_fetch", "ibkr_file", "ibkr_disconnect", "ibkr_open")) {
            assertTrue(broker, NativeServicePolicy.independent(broker))
            assertFalse(broker, NativeServicePolicy.coalesce(broker))
        }
        // Device approval: what its screens ask may be joined; whatever makes, uses or takes back a code or a key runs on its own.
        for (read in listOf("sync_key_info", "sync_recovery_unsaved", "sync_pair_state", "sync_devices")) {
            assertTrue(read, NativeServicePolicy.independent(read))
            assertTrue(read, NativeServicePolicy.coalesce(read))
            assertFalse(read, NativeServicePolicy.budgetRead(read))
        }
        for (command in listOf("sync_setup", "sync_unlock", "sync_reset", "sync_recovery_create", "sync_recovery_saved", "sync_pair_offer", "sync_pair_cancel", "sync_pair_approve")) {
            assertTrue(command, NativeServicePolicy.independent(command))
            assertFalse(command, NativeServicePolicy.coalesce(command))
            assertFalse(command, NativeServicePolicy.budgetResult(command))
        }
        for (budgetRead in listOf("simplefin_sync", "ibkr_review_compact", "native_budget_status", "database_status", "list_backups", "backup_retention")) {
            assertTrue(budgetRead, NativeServicePolicy.budgetRead(budgetRead))
        }
        for (globalRead in listOf("cloud_status", "account_overview", "list_budgets", "simplefin_accounts", "ynab_tokens", "ibkr_status")) {
            assertFalse(globalRead, NativeServicePolicy.budgetRead(globalRead))
        }
    }
}
