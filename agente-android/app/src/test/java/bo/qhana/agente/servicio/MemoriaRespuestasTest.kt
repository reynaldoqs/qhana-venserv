package bo.qhana.agente.servicio

import bo.qhana.agente.protocolo.Ok
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class MemoriaRespuestasTest {
    @Test
    fun reqRepetidoDevuelveLaMismaRespuesta() {
        val m = MemoriaRespuestas()
        val (d1, nueva1) = m.registrar("r1")
        assertTrue(nueva1)
        val (d2, nueva2) = m.registrar("r1")
        assertFalse(nueva2)
        assertSame(d1, d2)
        d1.complete(Ok("r1"))
        assertEquals(Ok("r1"), m.registrar("r1").first.getCompleted())
    }

    @Test
    fun guardaSoloLasUltimas() {
        val m = MemoriaRespuestas(maximo = 3)
        for (i in 1..4) m.registrar("r$i")
        assertTrue("r1 salió de la memoria", m.registrar("r1").second)
        assertFalse(m.registrar("r4").second)
    }
}
