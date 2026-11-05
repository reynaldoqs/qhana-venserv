package bo.qhana.agente.config

import okhttp3.HttpUrl.Companion.toHttpUrlOrNull

/** Validación de la URL del control (pura, sin Android; se prueba en la JVM). */
object UrlControl {
    private val ESQUEMA = Regex("^wss?://", RegexOption.IGNORE_CASE)

    /**
     * true si es `ws://…` o `wss://…` y OkHttp la puede abrir. Una URL que no cumple se ignora y queda la anterior
     * (protocolo: "Conexión"), así un dato roto no deja al agente sin poder conectar.
     */
    fun valida(url: String): Boolean {
        val u = url.trim()
        if (!ESQUEMA.containsMatchIn(u) || u.any { it.isWhitespace() }) return false
        // OkHttp abre ws:// y wss:// como http:// y https://: misma validación que usará al conectar.
        val comoHttp = u.replaceFirst(Regex("^ws", RegexOption.IGNORE_CASE), "http")
        return comoHttp.toHttpUrlOrNull() != null
    }
}
