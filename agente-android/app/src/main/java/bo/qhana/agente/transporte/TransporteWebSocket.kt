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

/** WebSocket con reconexión exponencial 1, 2, 4… 30 s, para siempre. */
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

    override fun iniciar(saludo: () -> Mensaje, alRecibir: (Mensaje) -> Unit, alConectar: () -> Unit, alPerder: () -> Unit) {
        if (bucle?.isActive == true) return
        bucle = scope.launch {
            var espera = 1_000L
            while (isActive) {
                _estado.value = InfoConexion(EstadoConexion.CONECTANDO, url)
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
                            Log.w(AgenteApp.TAG, "mensaje desconocido: ${text.take(200)}")
                            return
                        }
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
                ws = cliente.newWebSocket(Request.Builder().url(url).build(), listener)
                val motivo = cerrado.receive()
                ws = null
                if (aceptado) alPerder()
                if (!rechazado) _estado.value = InfoConexion(EstadoConexion.DESCONECTADO, motivo)
                val pausa = when {
                    aceptado -> 1_000L
                    rechazado -> 30_000L
                    else -> espera
                }
                espera = if (aceptado) 1_000L else (espera * 2).coerceAtMost(30_000L)
                delay(pausa)
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
