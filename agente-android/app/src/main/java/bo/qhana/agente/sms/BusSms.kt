package bo.qhana.agente.sms

import android.content.Context
import android.util.Log
import bo.qhana.agente.AgenteApp
import bo.qhana.agente.protocolo.Codec
import bo.qhana.agente.protocolo.Sms
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.serialization.builtins.ListSerializer
import java.io.File

/**
 * Puente receptor → servicio: cola de SMS por enviar al control. Se guarda en disco (`files/sms_pendientes.json`) para
 * sobrevivir a que Android mate el proceso, con tope de [MAXIMO] (se descartan los más viejos).
 *
 * Se consume mirando el primero ([primero]) y quitándolo solo cuando el envío salió ([quitar]): si falla, queda en la
 * cola y se vuelve a intentar al reconectar, sin reintentos en caliente.
 */
object BusSms {
    const val MAXIMO = 500
    private const val ARCHIVO = "sms_pendientes.json"
    private val serializador = ListSerializer(Sms.serializer())

    /** Aviso de "hay SMS nuevos" (sin datos: la cola es la fuente). */
    private val _avisos = MutableSharedFlow<Unit>(extraBufferCapacity = 1, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val avisos: SharedFlow<Unit> = _avisos

    private val pendientes = ArrayDeque<Sms>()
    private var archivo: File? = null

    /** Carga la cola guardada. Idempotente; lo llaman el servicio y el receptor. */
    @Synchronized
    fun cargar(context: Context) {
        if (archivo != null) return
        val f = File(context.applicationContext.filesDir, ARCHIVO)
        archivo = f
        if (!f.exists()) return
        runCatching { Codec.json.decodeFromString(serializador, f.readText()) }
            .onSuccess { guardados -> guardados.forEach { s -> if (pendientes.none { it.id == s.id }) pendientes.addLast(s) } }
            .onFailure { Log.w(AgenteApp.TAG, "no pude leer $ARCHIVO; se descarta", it) }
        recortar()
    }

    @Synchronized
    fun publicar(context: Context, sms: Sms) {
        cargar(context)
        if (pendientes.none { it.id == sms.id }) pendientes.addLast(sms) // el mismo broadcast dos veces no duplica
        recortar()
        guardar()
        _avisos.tryEmit(Unit)
    }

    @Synchronized
    fun primero(): Sms? = pendientes.firstOrNull()

    @Synchronized
    fun quitar(sms: Sms) {
        if (pendientes.remove(sms)) guardar()
    }

    @Synchronized
    fun cantidad(): Int = pendientes.size

    private fun recortar() {
        while (pendientes.size > MAXIMO) pendientes.removeFirst()
    }

    /** Escribe a un temporal y lo renombra: un corte a mitad no deja el archivo roto. */
    private fun guardar() {
        val f = archivo ?: return
        runCatching {
            val tmp = File(f.parentFile, "$ARCHIVO.tmp")
            tmp.writeText(Codec.json.encodeToString(serializador, pendientes.toList()))
            if (!tmp.renameTo(f)) {
                f.delete()
                tmp.renameTo(f)
            }
        }.onFailure { Log.w(AgenteApp.TAG, "no pude guardar $ARCHIVO", it) }
    }
}
