package bo.qhana.agente.servicio

import bo.qhana.agente.servicio.ReparadorAccesibilidad.Companion.con
import bo.qhana.agente.servicio.ReparadorAccesibilidad.Companion.contiene
import bo.qhana.agente.servicio.ReparadorAccesibilidad.Companion.debeReparar
import bo.qhana.agente.servicio.ReparadorAccesibilidad.Companion.sin
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReparadorAccesibilidadTest {
    private val largo = "bo.qhana.agente/bo.qhana.agente.ussd.ServicioAccesibilidad"
    private val corto = "bo.qhana.agente/.ussd.ServicioAccesibilidad"
    private val otro = "com.otro/.Lector"

    @Test
    fun reconoceLaFormaCortaYLarga() {
        assertTrue(contiene(corto, largo))
        assertTrue(contiene("$otro:$corto", largo))
        assertFalse(contiene(otro, largo))
        assertFalse(contiene(null, largo))
        assertFalse(contiene("null", largo))
    }

    @Test
    fun apagaYPrendeSinTocarLosDemas() {
        assertEquals(otro, sin("$otro:$corto", largo))
        assertEquals("", sin(corto, largo))
        assertEquals("$otro:$largo", con(sin("$otro:$corto", largo), largo))
        assertEquals(largo, con(sin(null, largo), largo))
    }

    @Test
    fun soloReparaConPermisoTrasVariasVueltasYSinRepetirSeguido() {
        val mucho = ReparadorAccesibilidad.ESPERA_ENTRE_MS
        assertTrue(debeReparar(2, mucho, permiso = true))
        assertFalse(debeReparar(1, mucho, permiso = true), "una vuelta sin unir puede ser el arranque")
        assertFalse(debeReparar(5, mucho, permiso = false))
        assertFalse(debeReparar(5, mucho - 1, permiso = true), "como mucho una vez cada 5 min")
    }

    private fun assertFalse(c: Boolean, msj: String) = org.junit.Assert.assertFalse(msj, c)
}
