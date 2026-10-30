package bo.qhana.agente.servicio

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.telephony.SubscriptionManager
import android.util.Log
import androidx.core.app.ServiceCompat
import bo.qhana.agente.AgenteApp
import bo.qhana.agente.EstadoApp
import bo.qhana.agente.config.Ajustes
import bo.qhana.agente.protocolo.Sim
import bo.qhana.agente.protocolo.Sims
import bo.qhana.agente.protocolo.SmsRecibido
import bo.qhana.agente.sms.BusSms
import bo.qhana.agente.sms.LectorSms
import bo.qhana.agente.telefonia.Telefonia
import bo.qhana.agente.transporte.EstadoConexion
import bo.qhana.agente.transporte.Transporte
import bo.qhana.agente.transporte.TransporteWebSocket
import bo.qhana.agente.ui.MainActivity
import bo.qhana.agente.ussd.GestorUssd
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * Servicio en primer plano que vive mientras el teléfono esté encendido. Une los módulos:
 * transporte ↔ despachador ↔ (USSD, SMS, telefonía) y envía `estado`/`sims` periódicamente.
 */
class AgenteService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private lateinit var ajustes: Ajustes
    private lateinit var telefonia: Telefonia
    private lateinit var estadoDisp: EstadoDispositivo
    private lateinit var ussd: GestorUssd
    private var transporte: Transporte? = null
    private var vigiaEstado: Job? = null
    private var ultimasSims: List<Sim> = emptyList()
    private var escuchaSims: SubscriptionManager.OnSubscriptionsChangedListener? = null
    private var cpuDespierta: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        ajustes = Ajustes(this)
        telefonia = Telefonia(this)
        estadoDisp = EstadoDispositivo(this, telefonia, ajustes)
        ussd = GestorUssd.crear(telefonia, scope, Pantalla(this))
        // Con la pantalla apagada el procesador se duerme y el WebSocket deja de atender (el control lo da por muerto tras
        // ~20 s y falla el USSD). Con esto la pantalla puede estar apagada y el agente sigue respondiendo. Gasta batería,
        // pero el teléfono trabaja enchufado.
        cpuDespierta = getSystemService(PowerManager::class.java)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "qhana:agente")
            .apply { setReferenceCounted(false); acquire() }
        enPrimerPlano("iniciando…")
        escucharSims()
        scope.launch { BusSms.entrantes.collect { enviarSms() } }
        scope.launch {
            while (isActive) {
                refrescar(soloSiCambia = true)
                delay(30_000)
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACCION_RECONECTAR || transporte == null) conectar()
        return START_STICKY
    }

    private fun conectar() {
        transporte?.detener()
        val url = ajustes.url
        val t = TransporteWebSocket(url, scope)
        transporte = t
        val despachador = Despachador(scope, ussd, LectorSms(this, telefonia), t::enviar) { refrescar(soloSiCambia = false) }
        EstadoApp.actualizar { it.copy(url = url) }
        vigiaEstado?.cancel()
        vigiaEstado = scope.launch {
            t.estado.collect { info ->
                EstadoApp.actualizar { it.copy(conexion = info.estado.name.lowercase(), detalleConexion = info.detalle) }
                enPrimerPlano(
                    when (info.estado) {
                        EstadoConexion.CONECTADO -> "conectado a ${info.detalle}"
                        EstadoConexion.RECHAZADO -> "rechazado: ${info.detalle}"
                        else -> "${info.estado.name.lowercase()} · $url"
                    }
                )
            }
        }
        t.iniciar(
            saludo = { estadoDisp.hola().also { ultimasSims = it.sims } },
            alRecibir = despachador::manejar,
            alConectar = {
                EstadoApp.anotar("conectado")
                t.enviar(estadoDisp.estado())
                enviarSms()
            },
            alPerder = {
                EstadoApp.anotar("conexión perdida")
                // Protocolo: al reconectar se descartan las sesiones abiertas.
                scope.launch { ussd.cancelarTodo() }
            },
        )
    }

    private fun refrescar(soloSiCambia: Boolean) {
        val sims = telefonia.sims()
        val estado = estadoDisp.estado()
        EstadoApp.actualizar { it.copy(sims = sims, permisosFaltantes = estado.permisosFaltantes, accesibilidad = estado.accesibilidadActiva) }
        val t = transporte ?: return
        if (!soloSiCambia || sims != ultimasSims) {
            if (t.enviar(Sims(sims))) ultimasSims = sims
        }
        t.enviar(estado)
    }

    private fun enviarSms() {
        val t = transporte ?: return
        for (s in BusSms.tomarPendientes()) {
            if (t.enviar(SmsRecibido(s))) {
                EstadoApp.anotar("SMS de ${s.de} enviado al control")
            } else {
                BusSms.publicar(s)
                return
            }
        }
    }

    private fun escucharSims() {
        val sm = getSystemService(SubscriptionManager::class.java)
        val l = object : SubscriptionManager.OnSubscriptionsChangedListener() {
            override fun onSubscriptionsChanged() {
                refrescar(soloSiCambia = true)
            }
        }
        runCatching {
            if (Build.VERSION.SDK_INT >= 30) sm.addOnSubscriptionsChangedListener(mainExecutor, l)
            else @Suppress("DEPRECATION") sm.addOnSubscriptionsChangedListener(l)
            escuchaSims = l
        }.onFailure { Log.w(AgenteApp.TAG, "no pude escuchar cambios de SIM", it) }
    }

    private fun enPrimerPlano(texto: String) {
        val abrir = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
        val n: Notification = Notification.Builder(this, AgenteApp.CANAL)
            .setSmallIcon(android.R.drawable.stat_sys_phone_call)
            .setContentTitle("Qhana Agente")
            .setContentText(texto)
            .setOngoing(true)
            .setContentIntent(abrir)
            .build()
        if (Build.VERSION.SDK_INT >= 34) {
            ServiceCompat.startForeground(this, NOTIF_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIF_ID, n)
        }
        getSystemService(NotificationManager::class.java).notify(NOTIF_ID, n)
    }

    override fun onDestroy() {
        transporte?.detener()
        escuchaSims?.let { runCatching { getSystemService(SubscriptionManager::class.java).removeOnSubscriptionsChangedListener(it) } }
        scope.cancel()
        cpuDespierta?.let { if (it.isHeld) it.release() }
        super.onDestroy()
    }

    companion object {
        private const val NOTIF_ID = 1
        const val ACCION_RECONECTAR = "bo.qhana.agente.RECONECTAR"

        fun arrancar(context: Context, reconectar: Boolean = false) {
            val i = Intent(context, AgenteService::class.java)
            if (reconectar) i.action = ACCION_RECONECTAR
            context.startForegroundService(i)
        }
    }
}
