package app.urbudget.nativeapp

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.time.Instant

class SyncApprovalTest {
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
}
