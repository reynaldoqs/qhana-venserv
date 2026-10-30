package bo.qhana.agente.sms

import bo.qhana.agente.protocolo.Sms
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow

/** Puente receptor → servicio. Guarda hasta 100 SMS mientras no hay conexión. */
object BusSms {
    private val _entrantes = MutableSharedFlow<Sms>(replay = 0, extraBufferCapacity = 100, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val entrantes: SharedFlow<Sms> = _entrantes
    private val pendientes = ArrayDeque<Sms>()

    @Synchronized
    fun publicar(sms: Sms) {
        pendientes.addLast(sms)
        while (pendientes.size > 100) pendientes.removeFirst()
        _entrantes.tryEmit(sms)
    }

    @Synchronized
    fun tomarPendientes(): List<Sms> = pendientes.toList().also { pendientes.clear() }

    @Synchronized
    fun marcarEnviado(sms: Sms) {
        pendientes.remove(sms)
    }
}
