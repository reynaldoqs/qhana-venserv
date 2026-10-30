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

        /** Ventana "ejecutando código USSD…": se ignora, no es la respuesta. */
        val PROGRESO = Regex(
            "(running ussd|ussd code running|ejecutando c[oó]digo|c[oó]digo ussd en ejecuci|ejecutando ussd|sending|enviando)",
            RegexOption.IGNORE_CASE,
        )

        /** Mensajes del sistema cuando el operador rechaza o corta: se reportan como error `red`. */
        val ERROR_RED = Regex(
            "(connection problem|invalid mmi|problema de conexi|c[oó]digo mmi no v|mmi no v[aá]lido|unknown application|" +
                "error de red|network error|no se pudo|unable to process)",
            RegexOption.IGNORE_CASE,
        )

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
