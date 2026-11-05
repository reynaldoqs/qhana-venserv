package bo.qhana.agente.ussd

import android.os.SystemClock
import android.util.Log
import bo.qhana.agente.AgenteApp
import bo.qhana.agente.EstadoApp
import bo.qhana.agente.protocolo.CodigoError
import bo.qhana.agente.protocolo.ModoUssd
import bo.qhana.agente.servicio.Pantalla
import bo.qhana.agente.telefonia.Telefonia
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.util.UUID

/**
 * Reglas comunes a cualquier motor: una sola sesión por teléfono, sesión ligada a su motor,
 * cierre automático tras [INACTIVIDAD_MS] sin uso. Si el motor necesita pantalla, la enciende al empezar y la
 * suelta cuando no queda sesión abierta (ver [Pantalla]). Cada paso respeta un [Plazo] total de `timeout_ms`.
 */
class GestorUssd(
    private val telefonia: Telefonia,
    private val motores: Map<String, MotorUssd>,
    private val scope: CoroutineScope,
    private val pantalla: Pantalla? = null,
) {
    private class Sesion(val id: String, val motor: MotorUssd, ultimoUso: Long) {
        @Volatile var ultimoUso: Long = ultimoUso
    }

    private val enUso = Mutex()

    @Volatile private var abierta: Sesion? = null
        set(v) {
            field = v
            EstadoApp.actualizar { it.copy(sesionUssd = v?.id) }
        }

    /**
     * Tras un fallo por la API, la red puede seguir con la sesión ocupada unos segundos (observado en
     * Tigo con VoLTE: "cannot accept second ussd request"). El siguiente USSD espera hasta esta hora.
     */
    @Volatile private var redLibreDesde = 0L

    init {
        scope.launch {
            while (isActive) {
                delay(5_000)
                val s = abierta
                if (s != null && inactiva(s)) cerrarPorInactividad(s)
            }
        }
    }

    private fun inactiva(s: Sesion) = SystemClock.elapsedRealtime() - s.ultimoUso > INACTIVIDAD_MS

    /** Vuelve a mirar con el candado tomado: un `responder` pudo usarla mientras se esperaba el candado. */
    private suspend fun cerrarPorInactividad(s: Sesion) = enUso.withLock {
        if (abierta !== s || !inactiva(s)) return@withLock
        Log.i(AgenteApp.TAG, "sesión ${s.id} sin uso por ${INACTIVIDAD_MS / 1000} s: se cierra")
        abierta = null
        try {
            s.motor.cancelar(s.id)
        } finally {
            soltarPantallaSiLibre()
        }
    }

    suspend fun iniciar(slot: Int, codigo: String, modo: String, timeoutMs: Long): RespuestaUssd {
        val plazo = Plazo.desde(timeoutMs)
        val motor = motores[modo] ?: throw FalloUssd(CodigoError.PARAMETRO, "Modo desconocido: $modo")
        // Un USSD, nunca un número de teléfono: una orden mal formada no debe terminar en una llamada de voz.
        if (!ReglasUssd.codigoValido(codigo)) {
            throw FalloUssd(CodigoError.PARAMETRO, "El código no es un USSD (debe cumplir ^[*#][0-9*#]+#$)")
        }
        val subId = telefonia.subIdDeSlot(slot) ?: throw FalloUssd(CodigoError.SIM_INVALIDA, "No hay SIM activa en el slot $slot")
        if (!enUso.tryLock()) throw FalloUssd(CodigoError.OCUPADO, "Hay otra operación USSD en curso")
        try {
            abierta?.let { throw FalloUssd(CodigoError.OCUPADO, "Hay una sesión abierta: ${it.id}") }
            val espera = redLibreDesde - SystemClock.elapsedRealtime()
            if (espera > timeoutMs / 2 || espera > plazo.restante() - MotorUssd.MINIMO_PARA_ENVIAR_MS) {
                throw FalloUssd(CodigoError.OCUPADO, "La red sigue ocupada por un USSD por API fallido; reintenta en ${(espera + 999) / 1000} s")
            }
            if (espera > 0) delay(espera)
            if (motor.nombre == ModoUssd.PANTALLA) encenderPantalla()
            if (plazo.restante() < MotorUssd.MINIMO_PARA_ENVIAR_MS) {
                throw FalloUssd(CodigoError.TIMEOUT, "No quedó tiempo para marcar (plazo de ${plazo.totalMs} ms); no se marcó")
            }
            val id = UUID.randomUUID().toString()
            val r = try {
                motor.iniciar(id, slot, subId, codigo, plazo)
            } catch (e: FalloUssd) {
                if (motor.nombre == ModoUssd.API && e.codigo == CodigoError.RED) {
                    redLibreDesde = SystemClock.elapsedRealtime() + ENFRIAMIENTO_API_MS
                }
                throw e
            }
            if (r.abierta) abierta = Sesion(id, motor, SystemClock.elapsedRealtime())
            return r
        } finally {
            soltarPantallaSiLibre()
            enUso.unlock()
        }
    }

    suspend fun responder(sesion: String, texto: String, timeoutMs: Long): RespuestaUssd {
        val plazo = Plazo.desde(timeoutMs)
        if (!enUso.tryLock()) throw FalloUssd(CodigoError.OCUPADO, "Hay otra operación USSD en curso")
        try {
            val s = abierta?.takeIf { it.id == sesion }
                ?: throw FalloUssd(CodigoError.SESION_INEXISTENTE, "Sesión $sesion no está abierta")
            // En uso desde ya: el vigía de inactividad no la cierra mientras esperamos al operador.
            s.ultimoUso = SystemClock.elapsedRealtime()
            if (s.motor.nombre == ModoUssd.PANTALLA) encenderPantalla()
            try {
                val r = s.motor.responder(sesion, texto, plazo)
                if (r.abierta) s.ultimoUso = SystemClock.elapsedRealtime() else abierta = null
                return r
            } catch (e: FalloUssd) {
                // La sesión ya no sirve: se olvida y se cierra lo que quede en pantalla (p. ej. la ventana ajena que
                // hizo fallar con "la ventana cambió"), sin demorar la respuesta.
                abierta = null
                cerrarEnFondo(s)
                throw e
            }
        } finally {
            soltarPantallaSiLibre()
            enUso.unlock()
        }
    }

    suspend fun cancelar(sesion: String) = enUso.withLock {
        val s = abierta?.takeIf { it.id == sesion }
            ?: throw FalloUssd(CodigoError.SESION_INEXISTENTE, "Sesión $sesion no está abierta")
        abierta = null
        try {
            s.motor.cancelar(s.id)
        } finally {
            soltarPantallaSiLibre()
        }
    }

    /** Al perder la conexión, al reconectar, al detener el agente o desde la pantalla de la app. */
    suspend fun cancelarTodo() = enUso.withLock {
        val s = abierta ?: return@withLock
        abierta = null
        try {
            s.motor.cancelar(s.id)
        } finally {
            soltarPantallaSiLibre()
        }
    }

    private fun cerrarEnFondo(s: Sesion) {
        scope.launch {
            enUso.withLock {
                if (abierta != null) return@withLock // ya empezó otra sesión: no tocar sus ventanas
                runCatching { s.motor.cancelar(s.id) }
                soltarPantallaSiLibre()
            }
        }
    }

    private suspend fun encenderPantalla() {
        val p = pantalla ?: return
        val estabaApagada = !p.encendida
        p.encender()
        if (estabaApagada) delay(Pantalla.ESPERA_ENCENDIDO_MS)
    }

    private fun soltarPantallaSiLibre() {
        if (abierta == null) pantalla?.soltar()
    }

    companion object {
        const val INACTIVIDAD_MS = 60_000L
        const val ENFRIAMIENTO_API_MS = 30_000L  // medido con Tigo: la red sigue ocupada entre 11 y >25 s

        fun crear(telefonia: Telefonia, scope: CoroutineScope, pantalla: Pantalla? = null) = GestorUssd(
            telefonia,
            mapOf(ModoUssd.API to UssdApi(telefonia), ModoUssd.PANTALLA to UssdPantalla(telefonia)),
            scope,
            pantalla,
        )
    }
}
