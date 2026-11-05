package bo.qhana.agente.ussd

import android.os.SystemClock
import android.util.Log
import bo.qhana.agente.AgenteApp
import bo.qhana.agente.protocolo.CodigoError
import bo.qhana.agente.telefonia.Telefonia
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.withTimeoutOrNull

/**
 * USSD leyendo y contestando la ventana del sistema con el servicio de accesibilidad.
 * Soporta sesiones de varios pasos. Nunca usa coordenadas: escribe con ACTION_SET_TEXT y presiona
 * los botones por id o por texto.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class UssdPantalla(private val telefonia: Telefonia) : MotorUssd {
    override val nombre = "pantalla"

    /** Todo lo de la ventana pasa de a uno. Comparte los hilos de IO: no hay hilo propio que cerrar al destruir el servicio. */
    private val hilo = Dispatchers.IO.limitedParallelism(1)

    @Volatile private var sesionActual: String? = null

    /** Último texto que devolvimos en [sesionActual]: `responder` solo escribe si la ventana visible sigue siendo esa. */
    @Volatile private var ultimoTexto: String? = null

    private fun servicio(): ServicioAccesibilidad = ServicioAccesibilidad.instancia
        ?: throw FalloUssd(CodigoError.SIN_ACCESIBILIDAD, "El servicio de accesibilidad del agente no está activo")

    private fun olvidarSesion() {
        sesionActual = null
        ultimoTexto = null
    }

    override suspend fun iniciar(sesion: String, slot: Int, subId: Int, codigo: String, plazo: Plazo): RespuestaUssd =
        withContext(hilo) {
            val s = servicio()
            // Valida código y cuenta telefónica antes de tocar ninguna ventana.
            val intent = telefonia.intentMarcar(codigo, slot, subId)
            withTimeoutOrNull(plazo.restante().coerceAtLeast(1)) { limpiarVentanasViejas(s) }
            if (s.leerDialogo()?.takeIf { !it.progreso } != null) {
                // Marcar con otra ventana encima haría que esa se lea como la respuesta.
                throw FalloUssd(CodigoError.OCUPADO, "Quedó una ventana USSD que no se pudo cerrar; no se marcó")
            }
            if (plazo.restante() < MotorUssd.MINIMO_PARA_ENVIAR_MS) {
                throw FalloUssd(CodigoError.TIMEOUT, "No quedó tiempo para marcar (plazo de ${plazo.totalMs} ms); no se marcó")
            }
            olvidarSesion()
            sesionActual = sesion
            val t0 = SystemClock.elapsedRealtime()
            try {
                s.startActivity(intent)
            } catch (e: SecurityException) {
                olvidarSesion()
                throw FalloUssd(CodigoError.SIN_PERMISO, "Falta CALL_PHONE: ${e.message}")
            }
            esperarRespuesta(s, sesion, previo = null, t0 = t0, plazo = plazo)
        }

    override suspend fun responder(sesion: String, texto: String, plazo: Plazo): RespuestaUssd = withContext(hilo) {
        val s = servicio()
        if (sesion != sesionActual) throw FalloUssd(CodigoError.SESION_INEXISTENTE, "Sesión $sesion no está abierta")
        val d = s.leerDialogo()?.takeIf { !it.progreso && it.entrada != null }
            ?: run {
                olvidarSesion()
                throw FalloUssd(CodigoError.SESION_INEXISTENTE, "No hay ventana USSD esperando respuesta")
            }
        // Solo se escribe en la ventana de esta sesión. Otra (un USSD que mandó la red, una promo) no se toca: lo que
        // escribamos ahí (un monto, un PIN) iría a otro menú. Se compara con la misma extracción que dio el texto.
        if (!ReglasUssd.mismaVentana(d.texto, ultimoTexto)) {
            Log.w(AgenteApp.TAG, "responder: la ventana cambió; no se escribe")
            throw FalloUssd(CodigoError.RED, "la ventana cambió")
        }
        if (plazo.restante() < MotorUssd.MINIMO_PARA_ENVIAR_MS) {
            throw FalloUssd(CodigoError.TIMEOUT, "No quedó tiempo para enviar la respuesta (plazo de ${plazo.totalMs} ms); no se envió")
        }
        val t0 = SystemClock.elapsedRealtime()
        if (!s.escribir(d.entrada!!, texto)) Log.w(AgenteApp.TAG, "ACTION_SET_TEXT devolvió false")
        // Releer el campo: si no quedó exactamente lo pedido (otro teclado, otra ventana, texto a medias), no se envía.
        val listo = campoConTexto(s, texto)
            ?: throw FalloUssd(CodigoError.RED, "El campo de respuesta no quedó con el texto pedido; no se presionó Enviar")
        if (!s.presionar(listo.positivo)) {
            throw FalloUssd(CodigoError.DESCONOCIDO, "No encontré el botón para enviar la respuesta (perfil ${listo.perfil})")
        }
        esperarRespuesta(s, sesion, previo = listo.texto, t0 = t0, plazo = plazo)
    }

    override suspend fun cancelar(sesion: String) {
        withContext(hilo) {
            if (sesion != sesionActual) {
                Log.w(AgenteApp.TAG, "cancelar: $sesion no es la sesión actual ($sesionActual)")
                return@withContext
            }
            olvidarSesion()
            val s = ServicioAccesibilidad.instancia
            Log.i(AgenteApp.TAG, "cancelar: accesibilidad=${s != null}")
            if (s != null) cerrarTodo(s)
        }
    }

    /**
     * La ventana de esta sesión con [texto] en el campo, leída de nuevo tras escribir. Reintenta unos instantes por si
     * la ventana tarda en reflejar el cambio. null si no quedó así.
     */
    private suspend fun campoConTexto(s: ServicioAccesibilidad, texto: String): Dialogo? {
        repeat(5) { intento ->
            if (intento > 0) delay(100)
            val d = s.leerDialogo()
            if (d != null && !d.progreso && d.entrada != null && ReglasUssd.mismaVentana(d.texto, ultimoTexto) &&
                s.leerCampo(d.entrada)?.trim() == texto.trim()
            ) return d
        }
        return null
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

    /** Limpieza después de saber la respuesta: acotada para contestar a más tardar [MotorUssd.GRACIA_LIMPIEZA_MS] tras el plazo. */
    private suspend fun acotado(plazo: Plazo, f: suspend () -> Unit) {
        val hasta = plazo.restante().coerceAtLeast(0) + MotorUssd.GRACIA_LIMPIEZA_MS
        if (withTimeoutOrNull(hasta) { f(); true } == null) {
            Log.w(AgenteApp.TAG, "limpieza cortada por el plazo; la próxima operación cierra lo que quede")
        }
    }

    private suspend fun esperarRespuesta(
        s: ServicioAccesibilidad,
        sesion: String,
        previo: String?,
        t0: Long,
        plazo: Plazo,
    ): RespuestaUssd {
        var vioTransicion = false
        val espera = plazo.restante().coerceAtLeast(1)
        val d = try {
            withTimeout(espera) {
                while (true) {
                    val actual = s.leerDialogo()
                    // Transición = la ventana desapareció o pasó a "ejecutando…". Un cambio de ventana sin
                    // eso no cuenta: el diálogo viejo puede seguir visible unos ms tras presionar Enviar.
                    // (Ver README, "mismo texto dos veces".)
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
            olvidarSesion()
            acotado(plazo) { cerrarTodo(s) }
            throw FalloUssd(CodigoError.TIMEOUT, "La ventana USSD no respondió en ${espera} ms")
        }
        val ms = SystemClock.elapsedRealtime() - t0
        val abierta = d.entrada != null
        if (abierta) {
            ultimoTexto = d.texto
        } else {
            olvidarSesion()
            acotado(plazo) {
                cerrar(s, d)
                if (s.leerDialogo() != null) cerrarTodo(s)
            }
            if (PerfilDialogo.esErrorRed(d.texto)) throw FalloUssd(CodigoError.RED, d.texto)
        }
        return RespuestaUssd(sesion, d.texto, abierta, ms)
    }

    companion object {
        const val PAUSA_TRAS_LIMPIAR_MS = 2_000L
    }
}
