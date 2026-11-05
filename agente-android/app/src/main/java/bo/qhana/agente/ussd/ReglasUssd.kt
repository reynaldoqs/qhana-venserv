package bo.qhana.agente.ussd

import java.text.Normalizer

/**
 * Reglas puras (sin Android) de los USSD: qué código se puede marcar, cómo se anota en el registro y cuándo dos
 * textos de ventana son la misma ventana. Se prueban en la JVM (`app/src/test`).
 */
object ReglasUssd {
    /** Un USSD, nunca un número de teléfono (protocolo: "Validaciones del agente antes de marcar"). */
    private val CODIGO = Regex("^[*#][0-9*#]+#$")
    private val SERVICIO = Regex("^[*#]+\\d*")
    private val ESPACIOS = Regex("\\s+")

    fun codigoValido(codigo: String): Boolean = CODIGO.matches(codigo)

    /**
     * El código tal como va al registro visible: deja el servicio (`*555`, `*66`) y tapa los demás dígitos, que pueden
     * ser un número de destino, un monto o un PIN. `*133*62478239*050#` → `*133*••••••••*•••#`.
     */
    fun enmascarar(codigo: String): String {
        if (!codigoValido(codigo)) return "(código de ${codigo.length} caracteres)"
        val cabeza = SERVICIO.find(codigo)?.value.orEmpty()
        return cabeza + codigo.substring(cabeza.length).map { if (it.isDigit()) '•' else it }.joinToString("")
    }

    /**
     * Texto de ventana comparable: misma forma Unicode, espacios (también el no separable) y saltos de línea colapsados
     * en un espacio, sin bordes. Distintas lecturas de la misma ventana pueden diferir solo en eso.
     */
    fun normalizar(texto: String): String =
        Normalizer.normalize(texto, Normalizer.Form.NFC)
            .replace(' ', ' ')
            .replace(ESPACIOS, " ")
            .trim()

    /** true si los dos textos son de la misma ventana (ver [normalizar]). */
    fun mismaVentana(a: String?, b: String?): Boolean = a != null && b != null && normalizar(a) == normalizar(b)
}
