package bo.qhana.agente.config

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UrlControlTest {
    @Test
    fun aceptaWsYWss() {
        for (u in listOf(
            "ws://127.0.0.1:47800/agente", "ws://192.168.1.50:47800/agente", "wss://control.local/agente",
            "WS://127.0.0.1:47800/agente", " ws://127.0.0.1:47800/agente ",
        )) {
            assertTrue(u, UrlControl.valida(u))
        }
    }

    @Test
    fun rechazaOtrosEsquemasYBasura() {
        for (u in listOf(
            "", "http://127.0.0.1:47800/agente", "https://x/agente", "127.0.0.1:47800", "ws://", "ws:// x/agente",
            "ws://127.0.0.1:99999/agente", "javascript:alert(1)", "file:///sdcard/x", "wsx://h/a",
        )) {
            assertFalse(u, UrlControl.valida(u))
        }
    }
}
