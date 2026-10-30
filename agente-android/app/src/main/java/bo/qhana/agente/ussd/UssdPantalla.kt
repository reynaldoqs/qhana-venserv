package bo.qhana.agente.ussd

import android.os.SystemClock
import android.util.Log
import bo.qhana.agente.AgenteApp
import bo.qhana.agente.protocolo.CodigoError
import bo.qhana.agente.telefonia.Telefonia
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.newSingleThreadContext
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.withTimeoutOrNull

/**
 * USSD leyendo y contestando la ventana del sistema con el servicio de accesibilidad.
 * Soporta sesiones de varios pasos. Nunca usa coordenadas: escribe con ACTION_SET_TEXT y presiona
 * los botones por id o por texto.
 */
@OptIn(ExperimentalCoroutinesApi::class, kotlinx.coroutines.DelicateCoroutinesApi::class)
class UssdPantalla(private val telefonia: Telefonia) : MotorUssd {
    override val nombre = "pantalla"

    private val hilo = newSingleThreadContext("ussd-pantalla")
    private var sesionActual: String? = null

    private fun servicio(): ServicioAccesibilidad = ServicioAccesibilidad.instancia
        ?: throw FalloUssd(CodigoError.SIN_ACCESIBILIDAD, "El servicio de accesibilidad del agente no está activo")

    override suspend fun iniciar(sesion: String, slot: Int, subId: Int, codigo: String, timeoutMs: Long): RespuestaUssd =
        withContext(hilo) {
            val s = servicio()
            limpiarVentanasViejas(s)
            sesionActual = sesion
            val t0 = SystemClock.elapsedRealtime()
            try {
                s.startActivity(telefonia.intentMarcar(codigo, slot, subId))
            } catch (e: SecurityException) {
                sesionActual = null
                throw FalloUssd(CodigoError.SIN_PERMISO, "Falta CALL_PHONE: ${e.message}")
            }
            esperarRespuesta(s, sesion, previo = null, ventanasAntes = -1, t0 = t0, timeoutMs = timeoutMs)
        }

    override suspend fun responder(sesion: String, texto: String, timeoutMs: Long): RespuestaUssd = withContext(hilo) {
        val s = servicio()
        if (sesion != sesionActual) throw FalloUssd(CodigoError.SESION_INEXISTENTE, "Sesión $sesion no está abierta")
        val d = s.leerDialogo()?.takeIf { !it.progreso && it.entrada != null }
            ?: run {
                sesionActual = null
                throw FalloUssd(CodigoError.SESION_INEXISTENTE, "No hay ventana USSD esperando respuesta")
            }
        val t0 = SystemClock.elapsedRealtime()
        val ventanasAntes = ServicioAccesibilidad.ventanasNuevas.get()
        if (!s.escribir(d.entrada!!, texto)) Log.w(AgenteApp.TAG, "ACTION_SET_TEXT devolvió false")
        if (!s.presionar(d.positivo)) {
            throw FalloUssd(CodigoError.DESCONOCIDO, "No encontré el botón para enviar la respuesta (perfil ${d.perfil})")
        }
        esperarRespuesta(s, sesion, previo = d.texto, ventanasAntes = ventanasAntes, t0 = t0, timeoutMs = timeoutMs)
    }

    override suspend fun cancelar(sesion: String) {
        withContext(hilo) {
            if (sesion != sesionActual) {
                Log.w(AgenteApp.TAG, "cancelar: $sesion no es la sesión actual ($sesionActual)")
                return@withContext
            }
            sesionActual = null
            val s = ServicioAccesibilidad.instancia
            Log.i(AgenteApp.TAG, "cancelar: accesibilidad=${s != null}")
            if (s != null) cerrarTodo(s)
        }
    }

    /** Si quedó una ventana de USSD de antes (otra app, un error), la cierra para no confundirla con la respuesta. */
    private suspend fun limpiarVentanasViejas(s: ServicioAccesibilidad) {
        if (s.leerDialogo() == null) return
        cerrarTodo(s)
        delay(PAUSA_TRAS_LIMPIAR_MS) // la red tarda en liberar la sesión que acabamos de cerrar
    }

    /**
     * Cierra todas las ventanas de USSD. Pueden estar apiladas (p. ej. el operador vence la sesión y
     * aparece "Connection problem" encima del menú): se da por terminado cuando no hay ninguna
     * durante ~0.8 s seguidos.
     */
    private suspend fun cerrarTodo(s: ServicioAccesibilidad) {
        var vaciasSeguidas = 0
        var rondas = 0
        while (vaciasSeguidas < 2 && rondas < 8) {
            rondas++
            val d = s.leerDialogo()
            when {
                d == null -> { vaciasSeguidas++; delay(400) }
                d.progreso -> { vaciasSeguidas = 0; delay(500) }
                else -> { vaciasSeguidas = 0; cerrar(s, d) }
            }
        }
        if (vaciasSeguidas < 2) Log.w(AgenteApp.TAG, "cerrarTodo: quedó una ventana USSD tras $rondas rondas")
    }

    private suspend fun cerrar(s: ServicioAccesibilidad, d: Dialogo) {
        // Con campo de respuesta, "Cancel" termina la sesión; sin campo, el único botón (OK) cierra.
        val boton = if (d.entrada != null) d.negativo else d.positivo ?: d.negativo
        val ok = s.presionar(boton)
        if (!ok) s.atras()
        // Esperar a que desaparezca (máx. 3 s); si sigue, BACK como último recurso.
        val cerrado = withTimeoutOrNull(3_000) {
            while (s.leerDialogo()?.takeIf { !it.progreso } != null) delay(150)
            true
        } ?: false
        if (!cerrado) {
            s.atras()
            delay(500)
        }
        Log.i(AgenteApp.TAG, "cerrar: boton=${boton?.viewIdResourceName} click=$ok cerrado=$cerrado")
    }

    private suspend fun esperarRespuesta(
        s: ServicioAccesibilidad,
        sesion: String,
        previo: String?,
        ventanasAntes: Long,
        t0: Long,
        timeoutMs: Long,
    ): RespuestaUssd {
        var vioTransicion = false
        val d = try {
            withTimeout(timeoutMs) {
                while (true) {
                    val actual = s.leerDialogo()
                    // Transición = la ventana desapareció o pasó a "ejecutando…". Un cambio de ventana sin
                    // eso no cuenta: el diálogo viejo puede seguir visible unos ms tras presionar Enviar.
                    // (ventanasAntes queda para diagnóstico; ver README, "mismo texto dos veces".)
                    if (actual == null || actual.progreso) {
                        vioTransicion = true
                    } else if (actual.texto != previo || vioTransicion) {
                        return@withTimeout actual
                    }
                    // Despierta con el próximo evento de la ventana o a los 300 ms (algunos OEM no emiten eventos).
                    withTimeoutOrNull(300) { ServicioAccesibilidad.cambios.first() }
                }
                @Suppress("UNREACHABLE_CODE")
                error("inalcanzable")
            }
        } catch (e: TimeoutCancellationException) {
            sesionActual = null
            cerrarTodo(s)
            throw FalloUssd(CodigoError.TIMEOUT, "La ventana USSD no respondió en ${timeoutMs} ms")
        }
        val ms = SystemClock.elapsedRealtime() - t0
        val abierta = d.entrada != null
        if (!abierta) {
            sesionActual = null
            cerrar(s, d)
            if (s.leerDialogo() != null) cerrarTodo(s)
            if (PerfilDialogo.ERROR_RED.containsMatchIn(d.texto)) throw FalloUssd(CodigoError.RED, d.texto)
        }
        return RespuestaUssd(sesion, d.texto, abierta, ms)
    }

    companion object {
        const val PAUSA_TRAS_LIMPIAR_MS = 2_000L
    }
}
