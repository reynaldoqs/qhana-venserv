package bo.qhana.agente.servicio

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.os.BatteryManager
import android.os.Build
import android.os.PowerManager
import bo.qhana.agente.BuildConfig
import bo.qhana.agente.config.Ajustes
import bo.qhana.agente.protocolo.Estado
import bo.qhana.agente.protocolo.Hola
import bo.qhana.agente.telefonia.Telefonia
import bo.qhana.agente.ussd.ServicioAccesibilidad

/** Arma los mensajes que describen el teléfono: `hola` y `estado`. */
class EstadoDispositivo(context: Context, private val telefonia: Telefonia, private val ajustes: Ajustes) {
    private val ctx = context.applicationContext

    fun hola() = Hola(
        token = ajustes.token,
        dispositivoId = Ajustes.dispositivoId(ctx),
        fabricante = Build.MANUFACTURER.uppercase(),
        modelo = Build.MODEL,
        android = Build.VERSION.RELEASE,
        sdk = Build.VERSION.SDK_INT,
        appVersion = BuildConfig.VERSION_NAME,
        sims = telefonia.sims(),
        capacidades = buildList {
            add("ussd_api")
            add("ussd_pantalla")
            add("sms")
        },
        adbSerial = ajustes.adbSerial,
    )

    fun estado(): Estado {
        val bat = ctx.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        // null si Android no informa el nivel (protocolo: `bateria` 0–100 o null), nunca un -1 que parezca un dato.
        val nivel: Int? = bat?.let {
            val l = it.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
            val s = it.getIntExtra(BatteryManager.EXTRA_SCALE, 100)
            if (l >= 0 && s > 0) (l * 100 / s).coerceIn(0, 100) else null
        }
        val cargando = (bat?.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0) ?: 0) != 0
        val pm = ctx.getSystemService(PowerManager::class.java)
        return Estado(
            bateria = nivel,
            cargando = cargando,
            pantallaEncendida = pm.isInteractive,
            accesibilidadActiva = ServicioAccesibilidad.instancia != null,
            permisosFaltantes = permisosFaltantes(ctx),
        )
    }

    companion object {
        val PERMISOS: List<String> = buildList {
            add(Manifest.permission.CALL_PHONE)
            add(Manifest.permission.READ_PHONE_STATE)
            add(Manifest.permission.READ_PHONE_NUMBERS)
            add(Manifest.permission.RECEIVE_SMS)
            add(Manifest.permission.READ_SMS)
            if (Build.VERSION.SDK_INT >= 33) add(Manifest.permission.POST_NOTIFICATIONS)
        }

        fun permisosFaltantes(ctx: Context): List<String> = PERMISOS
            .filter { ctx.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
            .map { it.removePrefix("android.permission.") }
    }
}
