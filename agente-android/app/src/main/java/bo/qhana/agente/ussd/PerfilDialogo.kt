package bo.qhana.agente.ussd

/**
 * Cómo reconocer la ventana de USSD de un fabricante. Se prueban en orden; el primero que encuentra
 * el texto gana. Para un modelo nuevo: `adb shell uiautomator dump` con la ventana abierta y agregar un perfil.
 */
data class PerfilDialogo(
    val nombre: String,
    val paquetes: Set<String>,
    val idsTexto: List<String>,
    val idsEntrada: List<String>,
    val idsPositivo: List<String>,
    val idsNegativo: List<String>,
) {
    companion object {
        val TEXTOS_POSITIVO = listOf("send", "enviar", "responder", "reply", "ok", "aceptar", "responder")
        val TEXTOS_NEGATIVO = listOf("cancel", "cancelar", "dismiss", "descartar", "cerrar", "close")

        /**
         * Ventana "ejecutando código USSD…" del sistema: se ignora, no es la respuesta. Solo textos del sistema y al
         * principio de la ventana: un texto del operador como "Enviando tu recarga…" es una respuesta, no progreso.
         */
        val PROGRESO = Regex(
            "^\\s*(ussd code running|mmi code running|running (ussd|mmi)( code)?|" +
                "ejecutando (el )?c[oó]digo|c[oó]digo (ussd|mmi) en ejecuci[oó]n|ejecutando (ussd|mmi)|" +
                "sending (ussd|mmi)|enviando (solicitud |el c[oó]digo )?(ussd|mmi))",
            RegexOption.IGNORE_CASE,
        )

        /**
         * Mensajes del sistema (no del operador) cuando la red rechaza o corta la sesión: se reportan como error `red`.
         * Un texto del operador como "No se pudo realizar la recarga" vuelve como texto: lo juzga la receta (`error_si`).
         */
        val ERROR_RED = Regex(
            "(connection problem|invalid mmi|mmi code (is )?invalid|problema de conexi[oó]n|error de conexi[oó]n|" +
                "c[oó]digo mmi (no v[aá]lido|inv[aá]lido)|mmi no v[aá]lido|unknown application|" +
                "fixed dialing numbers|n[uú]meros de marcaci[oó]n fija)",
            RegexOption.IGNORE_CASE,
        )

        /** Los textos de progreso del sistema son cortos; uno largo es del operador. */
        fun esProgreso(texto: String): Boolean = texto.length <= 80 && PROGRESO.containsMatchIn(texto)

        fun esErrorRed(texto: String): Boolean = ERROR_RED.containsMatchIn(texto)

        /** AOSP y derivados (Infinix/XOS observado 2026-10-29). */
        val AOSP = PerfilDialogo(
            nombre = "aosp",
            paquetes = setOf("com.android.phone", "com.android.server.telecom", "com.mediatek.phone"),
            idsTexto = listOf("com.android.phone:id/text_message", "android:id/message"),
            idsEntrada = listOf("com.android.phone:id/input_field"),
            idsPositivo = listOf("com.android.phone:id/btn_positive", "android:id/button1"),
            idsNegativo = listOf("com.android.phone:id/btn_negative", "android:id/button2"),
        )

        val SAMSUNG = PerfilDialogo(
            nombre = "samsung",
            paquetes = setOf("com.samsung.android.app.telephonyui", "com.android.phone"),
            idsTexto = listOf("android:id/message", "com.android.phone:id/text_message"),
            idsEntrada = emptyList(),
            idsPositivo = listOf("android:id/button1"),
            idsNegativo = listOf("android:id/button2"),
        )

        val TODOS = listOf(AOSP, SAMSUNG)
        val PAQUETES = TODOS.flatMap { it.paquetes }.toSet()
    }
}
