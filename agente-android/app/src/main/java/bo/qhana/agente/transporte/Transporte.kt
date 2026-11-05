package bo.qhana.agente.transporte

import bo.qhana.agente.protocolo.Mensaje
import kotlinx.coroutines.flow.StateFlow

enum class EstadoConexion { DETENIDO, CONECTANDO, CONECTADO, RECHAZADO, DESCONECTADO }

data class InfoConexion(val estado: EstadoConexion, val detalle: String = "")

/**
 * Canal hacia el control. Hoy: WebSocket ([TransporteWebSocket]) sobre USB con `adb reverse` o por Wi-Fi;
 * cambiar de uno a otro es solo cambiar la URL. Otro canal (MQTT, etc.) implementaría esta interfaz.
 */
interface Transporte {
    val estado: StateFlow<InfoConexion>

    /**
     * Conecta y reconecta solo. [saludo] se llama en cada conexión para armar el `hola`. [alPerder] corre antes de
     * volver a conectar (protocolo: al reconectar se cierra antes cualquier sesión USSD abierta).
     */
    fun iniciar(saludo: () -> Mensaje, alRecibir: (Mensaje) -> Unit, alConectar: () -> Unit, alPerder: suspend () -> Unit)

    fun detener()

    /** false si no hay conexión: el mensaje se pierde (el control repregunta lo que necesite). */
    fun enviar(m: Mensaje): Boolean
}
