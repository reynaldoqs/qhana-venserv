package bo.qhana.agente.protocolo

import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

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

    /**
     * Para un texto que [decodificar] no entiende (tipo desconocido o campos inválidos) pero que trae `req`: el `error`
     * `parametro` que lo contesta, así quien preguntó no espera hasta su timeout. null si no trae `req` (un evento) o
     * no es un objeto JSON.
     */
    fun falloNoEntendido(texto: String): Fallo? {
        val obj = try {
            json.parseToJsonElement(texto) as? JsonObject
        } catch (_: SerializationException) {
            null
        } catch (_: IllegalArgumentException) {
            null
        } ?: return null
        val req = (obj["req"] as? JsonPrimitive)?.takeIf { it.isString }?.content ?: return null
        val tipo = (obj["tipo"] as? JsonPrimitive)?.content ?: "?"
        return Fallo(req, CodigoError.PARAMETRO, "Mensaje no entendido por el agente (tipo \"$tipo\")")
    }
}
