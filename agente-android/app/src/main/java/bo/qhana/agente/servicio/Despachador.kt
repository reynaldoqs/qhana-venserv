package bo.qhana.agente.servicio

import android.util.Log
import bo.qhana.agente.AgenteApp
import bo.qhana.agente.EstadoApp
import bo.qhana.agente.protocolo.CodigoError
import bo.qhana.agente.protocolo.ConReq
import bo.qhana.agente.protocolo.Fallo
import bo.qhana.agente.protocolo.Mensaje
import bo.qhana.agente.protocolo.Ok
import bo.qhana.agente.protocolo.Ping
import bo.qhana.agente.protocolo.Pong
import bo.qhana.agente.protocolo.Refrescar
import bo.qhana.agente.protocolo.SmsLista
import bo.qhana.agente.protocolo.SmsListar
import bo.qhana.agente.protocolo.Ussd
import bo.qhana.agente.protocolo.UssdCancelar
import bo.qhana.agente.protocolo.UssdIniciar
import bo.qhana.agente.protocolo.UssdResponder
import bo.qhana.agente.sms.LectorSms
import bo.qhana.agente.ussd.FalloUssd
import bo.qhana.agente.ussd.GestorUssd
import bo.qhana.agente.ussd.RespuestaUssd
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * Traduce peticiones del control a llamadas a los módulos y devuelve la respuesta.
 * Cada petición corre en su propia corrutina: un USSD largo no bloquea un `ping`.
 */
class Despachador(
    private val scope: CoroutineScope,
    private val ussd: GestorUssd,
    private val sms: LectorSms,
    private val enviar: (Mensaje) -> Boolean,
    private val alRefrescar: () -> Unit,
) {
    fun manejar(m: Mensaje) {
        if (m !is ConReq) return
        scope.launch(Dispatchers.Default) {
            val respuesta: Mensaje = try {
                when (m) {
                    is Ping -> Pong(m.req)
                    is UssdIniciar -> {
                        EstadoApp.anotar("→ ${m.codigo} (slot ${m.slot}, ${m.modo})")
                        ussd.iniciar(m.slot, m.codigo, m.modo, m.timeoutMs).aMensaje(m.req)
                    }
                    is UssdResponder -> {
                        EstadoApp.anotar("→ responder \"${m.texto}\"")
                        ussd.responder(m.sesion, m.texto, m.timeoutMs).aMensaje(m.req)
                    }
                    is UssdCancelar -> {
                        ussd.cancelar(m.sesion)
                        EstadoApp.anotar("→ cancelar")
                        Ok(m.req)
                    }
                    is SmsListar -> SmsLista(m.req, sms.listar(m.slot, m.desdeMs, m.limite))
                    is Refrescar -> {
                        alRefrescar()
                        Ok(m.req)
                    }
                    else -> Fallo(m.req, CodigoError.PARAMETRO, "Petición no soportada por el agente: ${m::class.simpleName}")
                }
            } catch (e: FalloUssd) {
                EstadoApp.anotar("✗ ${e.codigo}: ${e.message}")
                Fallo(m.req, e.codigo, e.message ?: "")
            } catch (e: SecurityException) {
                Fallo(m.req, CodigoError.SIN_PERMISO, e.message ?: "permiso denegado")
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.e(AgenteApp.TAG, "error manejando ${m::class.simpleName}", e)
                Fallo(m.req, CodigoError.DESCONOCIDO, e.toString())
            }
            if (!enviar(respuesta)) Log.w(AgenteApp.TAG, "sin conexión: se perdió la respuesta a ${m.req}")
        }
    }

    private fun RespuestaUssd.aMensaje(req: String): Ussd {
        EstadoApp.actualizar { it.copy(ultimoUssd = texto) }
        EstadoApp.anotar("← ${texto.lineSequence().firstOrNull().orEmpty().take(60)} (${ms} ms${if (abierta) ", abierta" else ""})")
        return Ussd(req, sesion, texto, abierta, ms)
    }
}
