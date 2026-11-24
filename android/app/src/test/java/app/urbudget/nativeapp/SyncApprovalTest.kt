package app.urbudget.nativeapp

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.time.Instant

class SyncApprovalTest {
    @org.junit.Before fun english() = EnglishWords.install()

    private fun status(sync: String) = JSONObject("""{"email":"qa@example.test","sync":$sync}""")

    @Test fun theScreenFollowsTheFreshAnswerAndFallsBackOnTheLastSyncsWord() {
        assertEquals(SyncKey.Unknown, syncKey(null, null))
        assertEquals("signed in, nothing heard yet", SyncKey.Unknown, syncKey(status("""{"unlocked":false,"secret":null}"""), null))
        assertEquals(SyncKey.None, syncKey(status("""{"unlocked":false,"secret":null}"""), JSONObject("""{"exists":false,"secret":null,"recovery":false,"protocol":2}""")))
        assertEquals(SyncKey.Locked, syncKey(status("""{"unlocked":false,"secret":null}"""), JSONObject("""{"exists":true,"secret":"code","recovery":false,"protocol":2}""")))
        // Offline, or before the engine answers: a device that last heard of a key doesn't offer to turn sync on.
        assertEquals(SyncKey.Locked, syncKey(status("""{"unlocked":false,"secret":"passphrase"}"""), null))
        // A key that was reset away elsewhere: the fresh answer wins over what the last sync saved.
        assertEquals(SyncKey.None, syncKey(status("""{"unlocked":false,"secret":"code"}"""), JSONObject("""{"exists":false,"secret":null}""")))
        // Holding the key settles it, whatever an older answer said.
        assertEquals(SyncKey.Unlocked, syncKey(status("""{"unlocked":true,"secret":"code"}"""), JSONObject("""{"exists":false}""")))
        assertEquals(SyncKey.Unlocked, syncKey(status("""{"unlocked":true}"""), null))
    }

    @Test fun theUnlockBoxIsLabelledForWhatTheAccountsKeyIsMadeFrom() {
        val code = JSONObject("""{"exists":true,"secret":"code"}""")
        val passphrase = JSONObject("""{"exists":true,"secret":"passphrase"}""")
        assertEquals("code", syncSecret(null, code))
        assertEquals("the fresh answer first", "passphrase", syncSecret(status("""{"secret":"code"}"""), passphrase))
        assertEquals("then what the last sync saved", "code", syncSecret(status("""{"secret":"code"}"""), null))
        assertEquals("code", syncSecret(status("""{"secret":"code"}"""), JSONObject("""{"exists":true,"secret":null}""")))
        assertNull(syncSecret(status("""{"secret":null}"""), null))
        assertNull(syncSecret(null, null))
        assertEquals(R.string.cloud_unlock_field_passphrase, syncUnlockLabel("passphrase"))
        assertEquals(R.string.cloud_unlock_field_code, syncUnlockLabel("code"))
        assertEquals("an account whose key isn't known yet is asked for a code", R.string.cloud_unlock_field_code, syncUnlockLabel(null))
    }

    @Test fun aRecoveryCodeIsTakenFromAnyAnswerThatCarriesOne() {
        assertEquals("ABCD-EFGH-JKMN-PQRS-TVWX-YZ01", recoveryCodeIn(JSONObject("""{"created":true,"protocol":2,"recoveryCode":"ABCD-EFGH-JKMN-PQRS-TVWX-YZ01"}""")))
        assertNull("unlocking with a passphrase", recoveryCodeIn(JSONObject("""{"created":false}""")))
        assertNull(recoveryCodeIn(JSONObject("""{"recoveryCode":null}""")))
        assertNull(recoveryCodeIn(JSONObject("""{"recoveryCode":""}""")))
        assertNull("sync_recovery_unsaved with nothing kept", recoveryCodeIn(null))
        assertNull(recoveryCodeIn("ABCD"))
    }

    @Test fun aCodeShowsOnTwoLinesThatStillReadAsTheCodeItself() {
        val code = "ABCD-EFGH-JKMN-PQRS-TVWX-YZ01"
        assertEquals("ABCD-EFGH-JKMN-\nPQRS-TVWX-YZ01", codeLines(code))
        assertEquals("read across the lines, it is the text the key is made from", code, codeLines(code).replace("\n", ""))
        assertEquals("anything else is shown as it came", "ABCD-EFGH", codeLines(" ABCD-EFGH "))
    }

    @Test fun theQrCodeIsReadRowByRowAndOnlyWhenItIsWhole() {
        val size = 21
        // Dark where the column is even on the first row only, and at the last square.
        val modules = CharArray(size * size) { if (it < size && it % 2 == 0 || it == size * size - 1) '1' else '0' }.concatToString()
        val grid = qrGrid(JSONObject().put("size", size).put("modules", modules))!!
        assertEquals(size, grid.size)
        assertTrue(grid.dark(0, 0)); assertFalse(grid.dark(1, 0)); assertTrue(grid.dark(20, 0))
        assertFalse("x runs along a row, y down the rows", grid.dark(0, 1))
        assertTrue(grid.dark(20, 20)); assertFalse(grid.dark(19, 20))
        assertNull(qrGrid(null))
        assertNull(qrGrid(JSONObject()))
        assertNull("a row short", qrGrid(JSONObject().put("size", size).put("modules", modules.dropLast(size))))
        assertNull("the size doesn't match", qrGrid(JSONObject().put("size", size + 4).put("modules", modules)))
        assertNull("something that isn't a square", qrGrid(JSONObject().put("size", size).put("modules", modules.replaceRange(5, 6, "x"))))
        assertNull("smaller than any QR code", qrGrid(JSONObject().put("size", 2).put("modules", "1001")))
    }

    @Test fun aCodeIsShownUntilItExpiresAndNoLongerThanCodesLast() {
        val now = Instant.parse("2026-11-24T10:00:00Z")
        assertEquals(PAIR_LIFETIME_MS, pairingLifetime("2026-11-24T10:05:00.000Z", now))
        assertEquals("the server's time less the answer's journey", 298_500L, pairingLifetime("2026-11-24T10:04:58.500Z", now))
        assertEquals("the engine writes an offset", 240_000L, pairingLifetime("2026-11-24T10:04:00+00:00", now))
        // A clock that's wrong here can neither stretch a code nor end it as it arrives.
        assertEquals(PAIR_LIFETIME_MS, pairingLifetime("2026-11-24T10:09:00Z", now))
        assertEquals(PAIR_LIFETIME_MS, pairingLifetime("2026-11-24T09:59:00Z", now))
        assertEquals(PAIR_LIFETIME_MS, pairingLifetime("soon", now))
        assertEquals(PAIR_LIFETIME_MS, pairingLifetime("", now))
        assertEquals(PAIR_LIFETIME_MS, pairingLifetime(null, now))
    }

    @Test fun onlyTheServersWordThatTheKeyWasCollectedMeansADeviceWasAdded() {
        assertEquals(PairState.Waiting, pairState(JSONObject("""{"waiting":true,"collected":false}""")))
        assertEquals(PairState.Collected, pairState(JSONObject("""{"waiting":false,"collected":true}""")))
        // Ran out, or taken back: a new code, never "Device added".
        assertEquals(PairState.Gone, pairState(JSONObject("""{"waiting":false,"collected":false}""")))
        assertEquals("an engine from before `collected`", PairState.Gone, pairState(JSONObject("""{"waiting":false}""")))
        assertEquals(PairState.Waiting, pairState(JSONObject("""{"waiting":true}""")))
        assertEquals("no answer is asked again", PairState.Waiting, pairState(null))
    }

    @Test fun aResetIsDescribedForTheProtocolTheAccountIsOn() {
        assertEquals(2, syncProtocol(status("""{"protocol":2,"unlocked":true}"""), null))
        assertEquals(1, syncProtocol(status("""{"protocol":1,"unlocked":true}"""), null))
        // A device that never synced hasn't saved the account's protocol: the fresh answer says.
        assertEquals(2, syncProtocol(status("""{"protocol":1,"unlocked":false}"""), JSONObject("""{"exists":true,"secret":"code","protocol":2}""")))
        assertEquals(1, syncProtocol(status("""{"unlocked":false}"""), JSONObject("""{"exists":true,"secret":"passphrase","protocol":1}""")))
        assertEquals(1, syncProtocol(null, null))
    }

    @Test fun theTimeLeftCountsDownInMinutesAndSeconds() {
        assertEquals("5:00", pairingCountdown(PAIR_LIFETIME_MS))
        assertEquals("4:07", pairingCountdown(247_000))
        assertEquals("a started second still shows", "4:07", pairingCountdown(246_001))
        assertEquals("0:01", pairingCountdown(1))
        assertEquals("0:00", pairingCountdown(0))
        assertEquals("0:00", pairingCountdown(-5_000))
    }

    /** A frame showing `grid` head on: each square `scale` pixels, on a light margin of four squares. */
    private fun frame(grid: QrGrid, scale: Int = 6, margin: Int = 4, dark: Byte = 20, light: Byte = (-26).toByte()): Triple<ByteArray, Int, Int> {
        val side = (grid.size + 2 * margin) * scale
        val pixels = ByteArray(side * side) { light }
        for (row in 0 until grid.size) for (column in 0 until grid.size) if (grid.dark(column, row))
            for (y in 0 until scale) for (x in 0 until scale) pixels[((row + margin) * scale + y) * side + (column + margin) * scale + x] = dark
        return Triple(pixels, side, side)
    }

    @Test fun theCameraReadsTheCodeTheEngineDraws() {
        // A real `sync_pair_request` answer from the shared engine: its code, and its QR code's squares.
        val code = "B942-5Z5D-3V45-4Z8B-ER39-9PGR"
        val grid = qrGrid(JSONObject().put("size", 29).put("modules", "1111111011010111001100111111110000010011000010011001000001101110101110010001100010111011011101000010001110110101110110111010010100011110001011101100000101100111011010010000011111111010101010101010111111100000000011001000010100000000101000110000110110100001001010010000101100000111101001101000100110100001101100111011010101001001011001110100000110010100101111110110011001001011011111001001001100111011011010110111110110000101001000011110000110011010100001011100010000010011100001011100010100101000101001101100011100001111101111111011001110101101000001100010101100000111011111100001110101110000111001111111100010000000010010110010110001111011111110110100010000101010000100000100111010001011000101111011101001101101110111111101110111010000111001001000110010101110101011001011101010011111000001000001101101010001000111111110111101100110110000111"))!!
        val (pixels, width, height) = frame(grid)
        assertEquals("URBUDGET-ASK:1:" + code.replace("-", ""), decodeSyncCode(pixels, width, height))
        // A frame with nothing in it, and some other QR code, are both passed over.
        assertNull(decodeSyncCode(ByteArray(width * height) { (-26).toByte() }, width, height))
        val other = com.google.zxing.qrcode.QRCodeWriter().encode("https://example.com/not-ours", com.google.zxing.BarcodeFormat.QR_CODE, 0, 0)
        val foreign = QrGrid(other.width, BooleanArray(other.width * other.width) { other[it % other.width, it / other.width] })
        val (theirs, theirWidth, theirHeight) = frame(foreign)
        assertNull(decodeSyncCode(theirs, theirWidth, theirHeight))
    }

    // Account & sync, simplified (section 14 of the plan).

    private val now = Instant.parse("2026-11-24T12:00:00Z")
    private val utc = java.time.ZoneOffset.UTC

    @Test fun aMomentReadsAsTheEngineOrTheServerWroteIt() {
        assertEquals(Instant.parse("2026-11-24T11:58:00Z"), syncInstant("2026-11-24T11:58:00.000Z"))
        assertEquals(Instant.parse("2026-11-24T11:58:00Z"), syncInstant("2026-11-24T11:58:00+00:00"))
        assertEquals("a database's own time is UTC", Instant.parse("2026-11-24T11:58:00Z"), syncInstant("2026-11-24 11:58:00"))
        for (nothing in listOf(null, "", "null", "yesterday")) assertNull(nothing, syncInstant(nothing))
    }

    @Test fun lastSyncedCountsInMinutesThenHoursThenDays() {
        fun ago(seconds: Long) = lastSynced(now.minusSeconds(seconds), now)
        assertEquals("Last synced just now", ago(20))
        assertEquals("a clock behind the server's", "Last synced just now", ago(-90))
        assertEquals("Last synced 1 minute ago", ago(60))
        assertEquals("Last synced 5 minutes ago", ago(5 * 60 + 30))
        assertEquals("Last synced 59 minutes ago", ago(59 * 60 + 59))
        assertEquals("Last synced 1 hour ago", ago(3600))
        assertEquals("Last synced 47 hours ago", ago(47 * 3600L + 60))
        assertEquals("Last synced 2 days ago", ago(48 * 3600L))
        assertEquals("Last synced 40 days ago", ago(40 * 86400L))
        assertNull("a device that hasn't synced since names were kept", lastSynced(null, now))
    }

    @Test fun aDevicesRowSaysWhichItIsAndWhenItLastSynced() {
        val devices = syncDevices(JSONObject("""{"devices":[
            {"id":"a","name":"Pixel 9","addedAt":"2026-10-01T08:00:00.000Z","lastSeenAt":"2026-11-24T11:59:30.000Z","current":true},
            {"id":"b","name":"Ahmad’s MacBook","addedAt":"2026-10-02T08:00:00.000Z","lastSeenAt":"2026-11-24T11:55:00.000Z","current":false},
            {"id":"c","name":null,"addedAt":"2026-11-03T08:00:00.000Z","lastSeenAt":null,"current":false},
            {"id":"d","name":"  ","addedAt":null,"lastSeenAt":"2026-11-21T12:00:00.000Z","current":false}]}"""))
        assertEquals("in the engine's order, this phone first", listOf("a", "b", "c", "d"), devices.map { it.id })
        assertEquals(listOf("Pixel 9", "Ahmad’s MacBook", "A device added on 3 November", "A device with no name"), devices.map { deviceTitle(it, utc) })
        assertEquals(listOf("This phone", "Last synced 5 minutes ago", null, "Last synced 3 days ago"), devices.map { deviceDetail(it, now) })
        // Before this phone has a name in the account, it is just "This phone".
        val unnamed = SyncDevice("a", null, null, null, current = true)
        assertEquals("This phone", deviceTitle(unnamed, utc))
        assertNull(deviceDetail(unnamed, now))
        assertTrue(syncDevices(null).isEmpty())
        assertTrue(syncDevices(JSONObject("""{"devices":null}""")).isEmpty())
    }

    private fun synced(report: String, protocol: Int = 2) = status("""{"unlocked":true,"protocol":$protocol,"last":$report}""")
    private fun budget(state: String, message: String? = null) =
        """{"id":"$state","name":"Home","state":"$state","message":${message?.let { "\"$it\"" } ?: "null"},"conflicts":[],"notes":[]}"""

    @Test fun theStatusLineSaysSyncIsOnOrWhatNeedsThePerson() {
        assertEquals("Sync is on. Last synced 2 minutes ago.", syncLineText(synced("""{"at":"2026-11-24T11:58:00+00:00","state":"ok","budgets":[${budget("synced")}]}"""), now))
        assertEquals("nothing has synced yet", "Sync is on.", syncLineText(status("""{"unlocked":true,"last":null}"""), now))
        assertEquals("Sync is on. Changes are waiting to sync.", syncLineText(synced("""{"at":"2026-11-24T11:58:00+00:00","state":"waiting","budgets":[${budget("queued")}]}"""), now))
        fun line(report: String) = syncLine(synced(report))
        assertEquals(SyncLine.Review, line("""{"state":"attention","budgets":[${budget("error")},${budget("conflict")}]}"""))
        assertEquals(SyncLine.Review, line("""{"state":"attention","budgets":[${budget("deletion_conflict")}]}"""))
        assertEquals(SyncLine.Update, line("""{"state":"ok","budgets":[${budget("update_app")}]}"""))
        assertEquals(SyncLine.NeedsPlan, line("""{"state":"subscription_required","budgets":[]}"""))
        assertEquals(SyncLine.NeedsPlan, line("""{"state":"attention","budgets":[${budget("subscription_required")}]}"""))
        assertEquals(SyncLine.Attention, line("""{"state":"attention","budgets":[${budget("recovery")}]}"""))
        assertEquals(SyncLine.Offline, line("""{"state":"offline","message":"OFFLINE: no route","budgets":[${budget("synced")}]}"""))
        assertEquals("Can’t reach UrBudget right now. Sync will try again.", syncLineText(synced("""{"state":"offline","message":"OFFLINE: no route","budgets":[]}"""), now))
        // A sync that stopped says why in the engine's words, and plainly when it gave none.
        assertEquals("The server is busy.", syncLineText(synced("""{"state":"error","message":"The server is busy.","budgets":[]}"""), now))
        assertEquals("The last sync didn’t finish.", syncLineText(synced("""{"state":"error","message":null,"budgets":[]}"""), now))
        assertNull("a sync that failed isn't when it last synced", lastSyncedAt(synced("""{"at":"2026-11-24T11:58:00+00:00","state":"error","budgets":[]}""")))
    }

    @Test fun onlyBudgetsThatNeedSomethingAreListedAndTheRestAreCounted() {
        val all = listOf("synced", "conflict", "uploaded", "recovery", "local_only", "error", "downloaded", "merged", "waiting", "queued",
            "deletion_conflict", "update_app", "subscription_required", "trashed", "evicted", "paused", "cloud_only")
        val sorted = syncBudgets(synced("""{"state":"attention","budgets":[${all.joinToString(",") { budget(it) }}]}"""))
        assertEquals(listOf(BudgetNeed.Review, BudgetNeed.KeptCopy, BudgetNeed.DeviceOnly, BudgetNeed.Failed, BudgetNeed.ReviewDeletion, BudgetNeed.Update, BudgetNeed.NeedsPlan),
            sorted.rows.map { it.need })
        assertEquals("in the engine's order", listOf("conflict", "recovery", "local_only", "error", "deletion_conflict", "update_app", "subscription_required"), sorted.rows.map { it.entry.optString("id") })
        assertEquals("synced, uploaded, downloaded, merged, waiting and queued", 6, sorted.syncing)
        // The older sync downloads a budget that is only in the account from here.
        val older = syncBudgets(synced("""{"state":"ok","budgets":[${budget("cloud_only")},${budget("synced")}]}""", protocol = 1))
        assertEquals(listOf(BudgetNeed.Download), older.rows.map { it.need })
        assertEquals(1, older.syncing)
        val none = syncBudgets(status("""{"unlocked":false,"last":null}"""))
        assertTrue(none.rows.isEmpty() && none.syncing == 0)
        assertTrue(syncBudgets(null).rows.isEmpty())
    }

    @Test fun aListedBudgetSaysOnePlainSentence() {
        fun sentence(state: String, message: String? = null) = budgetSentence(syncBudgets(synced("""{"state":"attention","budgets":[${budget(state, message)}]}""", protocol = 1)).rows.single())
        assertEquals("It changed on two devices. Choose what to keep.", sentence("conflict"))
        assertEquals("the engine's own words for what to choose", "The copies differ. Choose which version to keep.", sentence("conflict", "The copies differ. Choose which version to keep."))
        assertEquals("Kept only on this phone.", sentence("local_only"))
        assertEquals("It couldn’t sync.", sentence("error"))
        assertEquals("The file is damaged.", sentence("error", "The file is damaged."))
        assertEquals("In your account, but not on this phone.", sentence("cloud_only"))
        // A kept copy is worded here: the engine's sentence for it says how sync works.
        assertEquals("A copy kept on this phone. Recover it from the Budget library if you need what’s in it.",
            sentence("recovery", "The account's encryption key changed. Recover this preserved local budget as a new copy."))
        assertEquals("Update UrBudget to sync this budget.", sentence("update_app", "Version 9 needed."))
        assertEquals("Sync needs an active subscription", sentence("subscription_required"))
    }

    @Test fun thePlanTakesRoomOnlyWhenThereIsSomethingToDoOrKnow() {
        fun overview(active: Boolean, plan: String) = JSONObject("""{"subscription":$plan,"sync":{"active":$active}}""")
        val unlocked = status("""{"unlocked":true}""")
        assertEquals("not known yet", PlanNotice.None, planNotice(unlocked, null))
        assertEquals(PlanNotice.None, planNotice(unlocked, overview(true, """{"provider":"google","status":"active","renewsAt":"2026-12-03T00:00:00.000Z","endsAt":null}""")))
        assertEquals(PlanNotice.Subscribe, planNotice(unlocked, overview(false, "null")))
        assertEquals(PlanNotice.Subscribe, planNotice(unlocked, overview(false, """{"provider":"google","status":"canceled","renewsAt":null,"endsAt":"2026-11-01T00:00:00.000Z"}""")))
        val ending = overview(true, """{"provider":"google","status":"active","renewsAt":null,"endsAt":"2026-12-03T00:00:00.000Z"}""")
        assertEquals(PlanNotice.Ending, planNotice(unlocked, ending))
        assertEquals(java.time.LocalDate.of(2026, 12, 3), planDay(ending, "endsAt", utc))
        assertNull(planDay(ending, "renewsAt", utc))
        assertEquals(PlanNotice.PaymentFailed, planNotice(unlocked, overview(true, """{"provider":"google","status":"past_due","renewsAt":"2026-12-03T00:00:00.000Z","endsAt":null}""")))
        // Offline, the plan is only mentioned when the last sync was turned away for want of one.
        assertEquals(PlanNotice.Subscribe, planNotice(synced("""{"state":"subscription_required","budgets":[]}"""), null))
        assertEquals(PlanNotice.Subscribe, planNotice(synced("""{"state":"attention","budgets":[${budget("subscription_required")}]}"""), null))
        assertEquals(PlanNotice.None, planNotice(synced("""{"state":"ok","budgets":[${budget("synced")}]}"""), null))
    }
}
