package bo.qhana.agente.ussd

import android.os.SystemClock

/** Respuesta de un paso USSD. `abierta` = el operador espera otra respuesta. */
data class RespuestaUssd(val sesion: String, val texto: String, val abierta: Boolean, val ms: Long)

/** Falla con un código del protocolo (ocupado, timeout, red, sin_accesibilidad…). */
class FalloUssd(val codigo: String, detalle: String) : Exception(detalle)

/**
 * Hora límite (en `elapsedRealtime`) para contestar una petición. Cubre todo el paso: el enfriamiento de la API,
 * encender la pantalla, limpiar ventanas viejas, esperar la respuesta y cerrar la ventana. Así el agente nunca
 * contesta mucho después del `timeout_ms` del control (que ya no lo esperaría).
 */
class Plazo(val limite: Long, val totalMs: Long) {
    fun restante(): Long = limite - SystemClock.elapsedRealtime()

    companion object {
        fun desde(timeoutMs: Long): Plazo {
            val t = timeoutMs.coerceIn(1_000L, 300_000L)
            return Plazo(SystemClock.elapsedRealtime() + t, t)
        }
    }
}

/**
 * Una forma de ejecutar USSD. Hay dos: [UssdApi] (un paso, sin ventana) y [UssdPantalla]
 * (varios pasos, leyendo la ventana del sistema con accesibilidad). Un módem u otro canal futuro
 * implementaría esta misma interfaz.
 */
interface MotorUssd {
    val nombre: String

    /**
     * Marca el código por la SIM indicada y devuelve la primera respuesta. Si el [plazo] se acabó antes de marcar,
     * falla con `timeout` **sin marcar**.
     */
    suspend fun iniciar(sesion: String, slot: Int, subId: Int, codigo: String, plazo: Plazo): RespuestaUssd

    /** Contesta en una sesión abierta. Solo los motores con sesiones lo soportan. */
    suspend fun responder(sesion: String, texto: String, plazo: Plazo): RespuestaUssd

    /** Cierra la sesión si quedó abierta. No debe lanzar. */
    suspend fun cancelar(sesion: String)

    companion object {
        /** Con menos que esto por delante no se marca ni se envía: no alcanzaría a llegar la respuesta. */
        const val MINIMO_PARA_ENVIAR_MS = 2_000L

        /** Margen tras el plazo para cerrar la ventana antes de contestar (el control espera `timeout_ms` + 5 s). */
        const val GRACIA_LIMPIEZA_MS = 3_000L
    }
}
