package bo.qhana.agente.ussd

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Clasificación de textos de la ventana: progreso del sistema, error de red del sistema o respuesta del operador. */
class PerfilDialogoTest {
    /** Textos reales de Tigo (modem/README.md, control/recetas/tigo-*.yaml) y otros plausibles del operador. */
    private val textosDelOperador = listOf(
        "Linea:69759675\nSaldo Total: 24 Bs vence el 25/12/2026 23:59:59\nSaldo Principal: 14 Bs vence el 25/12/2026 23:59:59",
        "Bienvenido al servicio de transferencias\n1.-Transferir por unica vez\n2.-Ultimos movimientos",
        "Ingrese numero de destino",
        "Ingrese monto\n1.-1 Bs\n2.-2 Bs\n3.-5 Bs\n4.-10 Bs\n5.-20 Bs",
        "Confirma transferir por unica vez 1.00 Bs a 69869869 con un costo de 0.20?\n1.-Si\n2.-No",
        "Le ha transferido 1.00 a 69869869",
        "Bienvenido al servicio de transferencias\nUsted alcanzo el maximo numero de transferecias diarias\n1.-Ultimos movimientos",
        "Deseas comprar recarga por Bs 0.20  para el 62478239. Introduce tu PIN para confirmar:",
        "Recarga Tigo por Bs 0.20. Tu saldo total Bs 11.06. Factura: 123",
        "Tu saldo total Tigo Money es de: Bs 211.06",
        "Enviando tu recarga…",
        "Enviando tu recarga, espera un SMS de confirmacion",
        "No se pudo realizar la recarga. Intenta mas tarde",
        "Lo sentimos, no se pudo procesar tu solicitud",
        "Error de red Tigo Money, intenta nuevamente",
        "Ejecutamos tu pedido: recarga de Bs 5 enviada",
    )

    @Test
    fun progresoDelSistema() {
        for (t in listOf(
            "USSD code running…", "USSD code running...", "Running USSD code…", "Running MMI code",
            "Ejecutando código USSD…", "Ejecutando codigo USSD...", "Código USSD en ejecución…", "Ejecutando USSD…",
            "Sending USSD…",
        )) {
            assertTrue(t, PerfilDialogo.esProgreso(t))
        }
    }

    @Test
    fun textosDelOperadorNoSonProgreso() {
        for (t in textosDelOperador) assertFalse(t, PerfilDialogo.esProgreso(t))
    }

    @Test
    fun errorDeRedDelSistema() {
        for (t in listOf(
            "Connection problem or invalid MMI code.",
            "Problema de conexión o código MMI no válido.",
            "Problema de conexion o codigo MMI no valido",
            "Error de conexión o código MMI no válido",
            "Código MMI inválido",
            "UNKNOWN APPLICATION",
        )) {
            assertTrue(t, PerfilDialogo.esErrorRed(t))
        }
    }

    @Test
    fun textosDelOperadorNoSonErrorDeRed() {
        // Vuelven como texto: los juzga la receta con `error_si` (p. ej. tigo-pda-recargar busca "no se pudo").
        for (t in textosDelOperador) assertFalse(t, PerfilDialogo.esErrorRed(t))
    }
}
