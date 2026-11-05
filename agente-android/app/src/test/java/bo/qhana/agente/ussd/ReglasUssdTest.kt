package bo.qhana.agente.ussd

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReglasUssdTest {
    @Test
    fun codigosUssdValidos() {
        for (c in listOf("*105#", "*66#1#", "*555#", "*#66#", "*133*62478239*050#", "#100#", "*#06#")) {
            assertTrue(c, ReglasUssd.codigoValido(c))
        }
    }

    @Test
    fun numerosYBasuraNoSeMarcan() {
        for (c in listOf(
            "", "78815232", "+59178815232", "*105", "105#", "*#", "#", "tel:*105#", "*1 05#", "*105#;", "*105#\n",
            "*105#,1", "*105#*", "*10a5#", "*105#+",
        )) {
            assertFalse(c, ReglasUssd.codigoValido(c))
        }
    }

    @Test
    fun enmascararDejaSoloElServicio() {
        assertEquals("*105#", ReglasUssd.enmascarar("*105#"))
        assertEquals("*555#", ReglasUssd.enmascarar("*555#"))
        assertEquals("*66#•#", ReglasUssd.enmascarar("*66#1#"))
        assertEquals("*133*••••••••*•••#", ReglasUssd.enmascarar("*133*62478239*050#"))
        assertEquals("(código de 8 caracteres)", ReglasUssd.enmascarar("78815232"))
    }

    @Test
    fun mismaVentanaToleraEspaciosYUnicode() {
        val menu = "Bienvenido al servicio de transferencias\n1.-Transferir por unica vez\n2.-Ultimos movimientos"
        assertTrue(ReglasUssd.mismaVentana(menu, "Bienvenido al servicio de transferencias 1.-Transferir por unica vez  2.-Ultimos movimientos "))
        assertTrue(ReglasUssd.mismaVentana("Ingrese monto\r\n1.-1 Bs", "  Ingrese monto\n1.-1 Bs"))
        // "ó" compuesta vs. "o" + acento combinante
        assertTrue(ReglasUssd.mismaVentana("Recarga a otro número", "Recarga a otro número"))
        // Doble espacio real de Tigo ("Bs 0.20  para el") contra el mismo texto con uno solo.
        assertTrue(ReglasUssd.mismaVentana(
            "Deseas comprar recarga por Bs 0.20  para el 62478239. Introduce tu PIN para confirmar:",
            "Deseas comprar recarga por Bs 0.20 para el 62478239. Introduce tu PIN para confirmar:",
        ))
    }

    @Test
    fun otraVentanaNoEsLaMisma() {
        assertFalse(ReglasUssd.mismaVentana("Ingrese numero de destino", "Ingrese monto"))
        assertFalse(ReglasUssd.mismaVentana(
            "Deseas comprar recarga por Bs 0.20  para el 62478239. Introduce tu PIN para confirmar:",
            "Deseas comprar recarga por Bs 0.20  para el 62478238. Introduce tu PIN para confirmar:",
        ))
        assertFalse(ReglasUssd.mismaVentana("Ingrese monto", null))
        assertFalse(ReglasUssd.mismaVentana(null, null))
    }
}
