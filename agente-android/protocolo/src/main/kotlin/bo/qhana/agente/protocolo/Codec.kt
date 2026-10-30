package bo.qhana.agente.protocolo

import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json

/** Codificador único del protocolo: discriminador "tipo", ignora campos desconocidos. */
object Codec {
    val json = Json {
        classDiscriminator = "tipo"
        ignoreUnknownKeys = true
        encodeDefaults = true
        explicitNulls = true
    }

    fun codificar(m: Mensaje): String = json.encodeToString(Mensaje.serializer(), m)

    /** null si el texto no es un mensaje conocido (tipo nuevo de una versión futura, JSON roto…). */
    fun decodificar(texto: String): Mensaje? = try {
        json.decodeFromString(Mensaje.serializer(), texto)
    } catch (_: SerializationException) {
        null
    } catch (_: IllegalArgumentException) {
        null
    }
}
