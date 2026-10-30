package bo.qhana.agente.ussd

/** Respuesta de un paso USSD. `abierta` = el operador espera otra respuesta. */
data class RespuestaUssd(val sesion: String, val texto: String, val abierta: Boolean, val ms: Long)

/** Falla con un código del protocolo (ocupado, timeout, red, sin_accesibilidad…). */
class FalloUssd(val codigo: String, detalle: String) : Exception(detalle)

/**
 * Una forma de ejecutar USSD. Hay dos: [UssdApi] (un paso, sin ventana) y [UssdPantalla]
 * (varios pasos, leyendo la ventana del sistema con accesibilidad). Un módem u otro canal futuro
 * implementaría esta misma interfaz.
 */
interface MotorUssd {
    val nombre: String

    /** Marca el código por la SIM indicada y devuelve la primera respuesta. */
    suspend fun iniciar(sesion: String, slot: Int, subId: Int, codigo: String, timeoutMs: Long): RespuestaUssd

    /** Contesta en una sesión abierta. Solo los motores con sesiones lo soportan. */
    suspend fun responder(sesion: String, texto: String, timeoutMs: Long): RespuestaUssd

    /** Cierra la sesión si quedó abierta. No debe lanzar. */
    suspend fun cancelar(sesion: String)
}
