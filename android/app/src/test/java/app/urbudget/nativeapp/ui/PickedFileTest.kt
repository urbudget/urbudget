package app.urbudget.nativeapp.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.io.FileNotFoundException

class PickedFileTest {
    @Test fun aFileIsOpenedToBeReplacedWhole() {
        val modes = mutableListOf<String>()
        assertEquals("stream", replacing { mode -> modes += mode; "stream" })
        assertEquals(listOf("wt"), modes)
    }

    @Test fun aProviderThatRefusesToTruncateStillGetsTheFile() {
        val modes = mutableListOf<String>()
        assertEquals("stream", replacing { mode ->
            modes += mode
            if (mode == "wt") throw FileNotFoundException("Unsupported mode") else "stream"
        })
        assertEquals(listOf("wt", "w"), modes)
    }

    @Test fun aFileThatWontOpenAtAllStillSaysSo() {
        val refused = assertThrows(SecurityException::class.java) { replacing<String> { throw SecurityException("Not allowed") } }
        assertEquals("Not allowed", refused.message)
    }
}
