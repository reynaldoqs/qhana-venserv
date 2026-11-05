package bo.qhana.agente.transporte

import android.util.Log
import bo.qhana.agente.AgenteApp
import bo.qhana.agente.protocolo.Bienvenido
import bo.qhana.agente.protocolo.Codec
import bo.qhana.agente.protocolo.Mensaje
import bo.qhana.agente.protocolo.Rechazado
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.util.concurrent.TimeUnit
import kotlin.random.Random

/** WebSocket con reconexión exponencial 1, 2, 4… 30 s (con azar), para siempre. */
class TransporteWebSocket(
    private val url: String,
    private val scope: CoroutineScope,
) : Transporte {
    private val cliente = OkHttpClient.Builder()
        .pingInterval(15, TimeUnit.SECONDS)
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .build()

    private val _estado = MutableStateFlow(InfoConexion(EstadoConexion.DETENIDO))
    override val estado: StateFlow<InfoConexion> = _estado.asStateFlow()

    @Volatile private var ws: WebSocket? = null
    private var bucle: Job? = null

    override fun iniciar(saludo: () -> Mensaje, alRecibir: (Mensaje) -> Unit, alConectar: () -> Unit, alPerder: suspend () -> Unit) {
        if (bucle?.isActive == true) return
        bucle = scope.launch {
            var espera = 1_000L
            while (isActive) {
                _estado.value = InfoConexion(EstadoConexion.CONECTANDO, url)
                // Una URL que OkHttp no acepta no debe tumbar el proceso (y volver a tumbarlo al reiniciar).
                val peticion = runCatching { Request.Builder().url(url).build() }
                    .onFailure { Log.w(AgenteApp.TAG, "URL inválida: $url", it) }
                    .getOrNull()
                if (peticion == null) {
                    _estado.value = InfoConexion(EstadoConexion.DESCONECTADO, "URL inválida: $url")
                    delay(30_000)
                    continue
                }
                val cerrado = Channel<String>(1)
                var aceptado = false
                var rechazado = false
                val listener = object : WebSocketListener() {
                    override fun onOpen(webSocket: WebSocket, response: Response) {
                        webSocket.send(Codec.codificar(saludo()))
                    }

                    override fun onMessage(webSocket: WebSocket, text: String) {
                        val m = Codec.decodificar(text)
                        if (m == null) {
                            // Tipo desconocido o mal formado: si trae `req`, se contesta `parametro` para que el control
                            // no espere hasta su timeout. Solo el tipo al registro: el texto podría llevar un PIN.
                            val fallo = Codec.falloNoEntendido(text)
                            Log.w(AgenteApp.TAG, "mensaje no entendido (${text.length} caracteres)${if (fallo != null) ": ${fallo.detalle}" else ""}")
                            if (fallo != null) webSocket.send(Codec.codificar(fallo))
                            return
                        }
                        runCatching {
                            when (m) {
                                is Bienvenido -> {
                                    aceptado = true
                                    _estado.value = InfoConexion(EstadoConexion.CONECTADO, m.servidor)
                                    alConectar()
                                }
                                is Rechazado -> {
                                    rechazado = true
                                    _estado.value = InfoConexion(EstadoConexion.RECHAZADO, m.motivo)
                                }
                                else -> alRecibir(m)
                            }
                        }.onFailure { Log.e(AgenteApp.TAG, "error atendiendo un mensaje", it) }
                    }

                    override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                        webSocket.close(1000, null)
                        cerrado.trySend("cerrado por el servidor ($code $reason)")
                    }

                    override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                        cerrado.trySend("cerrado ($code)")
                    }

                    override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                        cerrado.trySend(t.message ?: t.javaClass.simpleName)
                    }
                }
                ws = cliente.newWebSocket(peticion, listener)
                val motivo = cerrado.receive()
                ws = null
                if (!rechazado) _estado.value = InfoConexion(EstadoConexion.DESCONECTADO, motivo)
                // Antes de volver a conectar: el protocolo pide cerrar las sesiones USSD abiertas.
                if (aceptado) runCatching { alPerder() }.onFailure { Log.w(AgenteApp.TAG, "alPerder falló", it) }
                val base = when {
                    aceptado -> 1_000L
                    rechazado -> 30_000L
                    else -> espera
                }
                espera = if (aceptado) 1_000L else (espera * 2).coerceAtMost(30_000L)
                // ±25 % de azar: varios teléfonos que perdieron el control a la vez no vuelven todos en el mismo instante.
                delay((base * (0.75 + Random.nextDouble() * 0.5)).toLong())
            }
        }
    }

    override fun detener() {
        bucle?.cancel()
        bucle = null
        ws?.close(1000, "detenido")
        ws = null
        _estado.value = InfoConexion(EstadoConexion.DETENIDO)
    }

    override fun enviar(m: Mensaje): Boolean {
        if (_estado.value.estado != EstadoConexion.CONECTADO) return false
        return ws?.send(Codec.codificar(m)) ?: false
    }
}
