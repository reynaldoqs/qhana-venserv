package bo.qhana.agente.protocolo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CodecTest {
    @Test
    fun decodificaPeticionDelControl() {
        val m = Codec.decodificar(
            """{"tipo":"ussd_iniciar","req":"r1","slot":1,"codigo":"*105#","modo":"api","timeout_ms":20000,"extra":"x"}"""
        )
        assertEquals(UssdIniciar("r1", 1, "*105#", "api", 20000), m)
    }

    @Test
    fun valoresPorDefecto() {
        val m = Codec.decodificar("""{"tipo":"ussd_iniciar","req":"r1","slot":0,"codigo":"*105#"}""") as UssdIniciar
        assertEquals(ModoUssd.PANTALLA, m.modo)
        assertEquals(30_000L, m.timeoutMs)
    }

    @Test
    fun codificaConTipoYSnakeCase() {
        val s = Codec.codificar(Ussd("r1", "s1", "hola", abierta = true, ms = 12))
        assertTrue(s.contains("\"tipo\":\"ussd\""))
        val sim = Codec.codificar(Sims(listOf(Sim(0, 4, "TIGO", "73603"))))
        assertTrue(sim.contains("\"sub_id\":4") && sim.contains("\"mcc_mnc\":\"73603\""))
    }

    @Test
    fun tipoDesconocidoDaNull() {
        assertNull(Codec.decodificar("""{"tipo":"algo_futuro","req":"x"}"""))
        assertNull(Codec.decodificar("no es json"))
    }

    @Test
    fun idaYVuelta() {
        val todos: List<Mensaje> = listOf(
            Hola(token = "t", dispositivoId = "d", fabricante = "F", modelo = "M", android = "14", sdk = 34,
                appVersion = "0.1.0", sims = emptyList(), capacidades = listOf("ussd_api")),
            Estado(50, true, true, false, listOf("CALL_PHONE")),
            Fallo("r", CodigoError.OCUPADO, "x"), Ok("r"), Pong("r"),
            SmsRecibido(Sms("1", 0, "300", "hola", 1L)), SmsLista("r", emptyList()),
            Bienvenido("s", 1L), Rechazado("m"), UssdResponder("r", "s", "2"), UssdCancelar("r", "s"),
            SmsListar("r"), Refrescar("r"), Ping("r"),
        )
        for (m in todos) assertEquals(m, Codec.decodificar(Codec.codificar(m)))
    }
}
