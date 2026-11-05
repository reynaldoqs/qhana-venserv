package bo.qhana.agente.ussd

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.telephony.TelephonyManager
import bo.qhana.agente.protocolo.CodigoError
import bo.qhana.agente.telefonia.Telefonia
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeout
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * USSD por la API oficial (`sendUssdRequest`, Android 8+). Un solo paso: Android cierra la sesión
 * al recibir la respuesta, así que nunca queda abierta.
 */
class UssdApi(private val telefonia: Telefonia) : MotorUssd {
    override val nombre = "api"
    private val handler = Handler(Looper.getMainLooper())

    override suspend fun iniciar(sesion: String, slot: Int, subId: Int, codigo: String, plazo: Plazo): RespuestaUssd {
        val espera = plazo.restante()
        if (espera < MotorUssd.MINIMO_PARA_ENVIAR_MS) {
            throw FalloUssd(CodigoError.TIMEOUT, "No quedó tiempo para marcar (plazo de ${plazo.totalMs} ms); no se marcó")
        }
        val t0 = SystemClock.elapsedRealtime()
        val tm = telefonia.telephonyDe(subId)
        val texto = try {
            withTimeout(espera) {
                suspendCancellableCoroutine<String> { cont ->
                    val cb = object : TelephonyManager.UssdResponseCallback() {
                        override fun onReceiveUssdResponse(t: TelephonyManager, req: String, resp: CharSequence) {
                            if (cont.isActive) cont.resume(resp.toString())
                        }

                        override fun onReceiveUssdResponseFailed(t: TelephonyManager, req: String, code: Int) {
                            if (cont.isActive) cont.resumeWithException(
                                FalloUssd(CodigoError.RED, "El operador rechazó o no respondió el USSD (código $code)")
                            )
                        }
                    }
                    try {
                        tm.sendUssdRequest(codigo, cb, handler)
                    } catch (e: SecurityException) {
                        cont.resumeWithException(FalloUssd(CodigoError.SIN_PERMISO, "Falta CALL_PHONE: ${e.message}"))
                    }
                }
            }
        } catch (e: TimeoutCancellationException) {
            throw FalloUssd(CodigoError.TIMEOUT, "Sin respuesta en ${espera} ms")
        }
        return RespuestaUssd(sesion, texto.trim(), abierta = false, ms = SystemClock.elapsedRealtime() - t0)
    }

    override suspend fun responder(sesion: String, texto: String, plazo: Plazo): RespuestaUssd =
        throw FalloUssd(CodigoError.SESION_INEXISTENTE, "El modo api no mantiene sesiones; usa modo pantalla")

    override suspend fun cancelar(sesion: String) = Unit
}
