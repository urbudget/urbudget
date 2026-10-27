package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class BankStatementViewTest {
    @org.junit.Before fun resetFormat() { Money.configure(JSONObject().put("currency", "EUR")) }

    private val session = JSONObject("""{"scope":{"session":"s1","budget":"b","account":"checking","file":"f"},"revision":3,
        "structure":{"format":"csv","dateOrder":{"order":"dmy","confirmed":false},"flipSigns":false,"wholeBatches":["e1"]},
        "edits":{"cells":{},"fields":{},"include":[]},"versions":{}}""")

    /** What the engine sends a phone (`bankStatement` with `problemsOnly`). */
    private val statement = JSONObject("""{"scope":{"session":"s1"},"revision":3,"format":"csv","order":"dmy",
        "sources":[{"label":"A1","currency":"EUR","lines":3},{"label":"B2","lines":1}],"source":0,
        "lines":[
          {"id":"30","version":2,"cells":["02/10/2026","Grocer","n/a"],"fields":{"date":"2026-10-02","payee":"Grocer","memo":"","amount":"n/a"},
           "status":{"kind":"attention","field":"amount","column":2,"reason":"Invalid amount: n/a"}},
          {"id":"60","version":0,"cells":["03/10/2026","Cinema","-12.00"],"fields":{"date":"2026-10-03","payee":"Cinema","memo":"","amount":"-12.00"},
           "status":{"kind":"leftOut","reason":"pending","detail":"Pending"}},
          {"id":"90","version":1,"cells":["04/10/2026","","-3.00"],"fields":{"date":"2026-10-04","payee":"","memo":"","amount":"-3.00"},
           "status":{"kind":"ready"},"fixed":true,"batch":0}],
        "counts":{"ready":2,"attention":1,"leftOut":{"pending":1}},
        "dateOrder":{"order":"dmy","example":"01/02/2026","unconfirmed":true},
        "batches":[{"entry":"e1","line":null,"payments":[2],"addsUp":false,"asOne":false}],
        "remember":{"dateFormat":"dmy","dateConfirmed":false,"flipSigns":false}}""")

    @Test fun aStatementReadsAsThePhoneShowsIt() {
        val st = parseStatement(statement)
        assertEquals(listOf("attention", "leftOut", "ready"), st.lines.map { it.kind })
        assertEquals("Invalid amount: n/a", st.lines[0].problem)
        assertEquals("amount", st.lines[0].field)
        assertTrue(st.lines[1].includable)
        assertFalse(st.lines[0].includable)
        assertEquals("Not booked yet (Pending)", leftOutText(st.lines[1]))
        // A line without a payee goes by what the file writes on it.
        assertEquals("04/10/2026 · -3.00", st.lines[2].title)
        assertEquals(0, st.lines[2].batch)
        assertEquals(listOf(StatementSource("A1", "EUR", 3), StatementSource("B2", null, 1)), st.sources)
        assertEquals(1, st.leftOutCount)
        assertTrue(st.dateUnconfirmed)
        // Something to answer and a line to fix: it opens at the lines, and waits for the answer.
        assertTrue(st.needsChecking)
        assertFalse(st.canContinue)
        assertNull(st.signsByType)
        assertEquals(listOf("30"), st.attentionLines.map { it.id })
        assertEquals(listOf("60"), st.leftOutLines.map { it.id })
        assertEquals(listOf("90"), st.changedLines.map { it.id })
        assertEquals(2, st.places["90"])
        val answered = parseStatement(JSONObject(statement.toString()).apply {
            put("dateOrder", JSONObject().put("order", "dmy").put("example", "01/02/2026").put("unconfirmed", false))
            put("signs", JSONObject().put("byType", true).put("unconfirmed", true))
        })
        // The signs still to say: it waits for them too.
        assertEquals(true, answered.signsByType)
        assertFalse(answered.canContinue)
        assertEquals("2 lines read · 1 needs attention and won't be imported · 1 left out", statementCounts(st))
        assertEquals(StatementBatch("e1", null, listOf(2), false, false), st.batches[0])
        assertNull(st.elsewhere)
    }

    @Test fun fixesAreMadeAgainstTheSessionAndEachLineAsItIs() {
        val line = parseStatement(statement).lines[0]
        // Only the parts changed, each against the value it has now.
        val fixes = fieldFixes(session, line, mapOf("date" to "2026-10-02", "payee" to " Grocer ", "memo" to "", "amount" to " -12.40 "))!!
        assertEquals(3L, fixes.getLong("baseRevision"))
        assertEquals("s1", fixes.getJSONObject("scope").getString("session"))
        assertEquals(session.getJSONObject("structure").toString(), fixes.getJSONObject("baseStructure").toString())
        val fields = fixes.getJSONArray("fields")
        assertEquals(1, fields.length())
        val amount = fields.getJSONObject(0)
        assertEquals(listOf("30", 2L, "amount", "n/a", "-12.40"), listOf(amount.getString("line"), amount.getLong("lineVersion"), amount.getString("field"), amount.getString("from"), amount.getString("to")))
        assertNull(fieldFixes(session, line, mapOf("amount" to "n/a")))
        assertEquals(true, includeFix(session, line, true).getJSONArray("include").getJSONObject(0).getBoolean("include"))
        assertEquals(2L, revertFix(session, line).getJSONArray("revert").getJSONObject(0).getLong("lineVersion"))
        // Reading the file another way changes a copy of how it reads now.
        val order = restructured(session) { put("dateOrder", JSONObject().put("order", "mdy").put("confirmed", true)) }
        assertEquals("mdy", order.getJSONObject("structure").getJSONObject("dateOrder").getString("order"))
        assertEquals("dmy", session.getJSONObject("structure").getJSONObject("dateOrder").getString("order"))
        assertEquals("[\"e1\",\"e2\"]", wholeBatchFix(session, "e2", true).getJSONObject("structure").getJSONArray("wholeBatches").toString())
        assertEquals("[]", wholeBatchFix(session, "e1", false).getJSONObject("structure").getJSONArray("wholeBatches").toString())
    }

    @Test fun theAssistantsFixesSayWhichAppliedAndWhyOthersDidnt() {
        val all = fixOutcome(JSONObject("""{"session":{},"results":[{"line":"30","fix":"field","applied":true},{"fix":"structure","applied":true}]}"""))
        assertEquals(FixOutcome(listOf("30"), 2, "Applied the assistant's 2 fixes."), all)
        val some = fixOutcome(JSONObject("""{"session":{},"results":[{"line":"30","fix":"field","applied":true},
            {"line":"60","fix":"field","applied":false,"problem":"This line changed since this fix was made."}]}"""))
        assertEquals(FixOutcome(listOf("30"), 1, "Applied 1 of 2 fixes. One no longer fits: This line changed since this fix was made."), some)
        assertEquals(0, fixOutcome(JSONObject("""{"results":[{"line":"60","applied":false}]}""")).applied)
    }

    @Test fun aDateTheOrdersReadDifferentlyIsNamedBothWays() {
        assertEquals("1 February 2026" to "2 January 2026", dateReadings("01/02/2026"))
        assertEquals("3 April 2026" to "4 March 2026", dateReadings("3.4.26"))
        assertNull(dateReadings("2026-10-01"))
    }

    @Test fun moneyBackFromAShopSaysWhatItMayBeARefundOf() {
        val categories = mapOf("shopping" to "Shopping")
        val one = JSONObject("""{"id":"t1","date":"2026-10-07","payee":"Amazon EU","amount":-2399,"categoryId":"shopping","purchases":1}""")
        assertEquals("Refund of Amazon EU · €23.99 on 2026-10-07 · Shopping", refundText(one, categories, "EUR"))
        val many = JSONObject("""{"id":"t1","date":"2026-10-07","payee":"Amazon EU","amount":-2399,"purchases":2}""")
        assertEquals("Possible refund · 2 purchases from Amazon EU match", refundText(many, categories, "EUR"))
    }

    @Test fun aWorkbookIsReadAtTheSheetAskedForAndSaysItsSheets() {
        val zip = byteArrayOf(0x50, 0x4B, 0x03, 0x04, 1, 2, 3)
        var asked: String? = "unset"
        val read = decodeStatementFile(zip, "Resumen") { _, sheet ->
            asked = sheet
            JSONObject().put("text", "Date,Amount\n").put("sheet", "Resumen").put("sheets", JSONArray(listOf("Movimientos", "Resumen")))
        }
        assertEquals("Resumen", asked)
        assertEquals(DecodedStatement("Date,Amount\n", "Resumen", listOf("Movimientos", "Resumen")), read)
        // Plain text has no sheets.
        assertEquals(DecodedStatement("Date,Amount\n"), decodeStatementFile("Date,Amount\n".toByteArray()) { _, _ -> error("not a workbook") })
    }

    @Test fun aReviewRowSaysWhatItMayBeARefundOf() = kotlinx.coroutines.test.runTest {
        val budget = JSONObject("""{"currency":"EUR","categories":[{"id":"shopping","name":"Shopping"}],"transactions":[]}""")
        val rows = listOf(
            JSONObject("""{"id":"r1","date":"2026-10-09","payee":"Amazon EU","amount":2399,"candidates":[],
                "refundOf":{"id":"t1","date":"2026-10-07","payee":"Amazon EU","amount":-2399,"categoryId":"shopping","purchases":1}}"""),
            JSONObject("""{"id":"r2","date":"2026-10-09","payee":"Amazon EU","amount":2399,"duplicate":true,"candidates":[],
                "refundOf":{"id":"t1","date":"2026-10-07","payee":"Amazon EU","amount":-2399,"purchases":1}}"""),
        )
        val prepared = prepareBankReview(rows, budget, true)
        assertEquals("Refund of Amazon EU · €23.99 on 2026-10-07 · Shopping", prepared[0].refund)
        // An already imported row has nothing to decide.
        assertNull(prepared[1].refund)
        assertEquals(1, summarizeBankReview(prepared).refunds)
        // The refund goes back with the row, for the import to check again.
        val choice = bankReviewChoices(prepared, emptyMap(), true).getJSONObject(0).getJSONObject("row")
        assertEquals("t1", choice.getJSONObject("refundOf").getString("id"))
    }
}
