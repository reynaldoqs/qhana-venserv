package bo.qhana.agente.config

import android.annotation.SuppressLint
import android.content.Context
import android.provider.Settings

/**
 * Configuración persistente del agente. La URL decide el transporte:
 * - USB:   ws://127.0.0.1:47800/agente  (con `adb reverse tcp:47800 tcp:47800`)
 * - Wi-Fi: ws://<IP del PC>:47800/agente
 */
class Ajustes(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences("ajustes", Context.MODE_PRIVATE)

    var url: String
        get() = prefs.getString(K_URL, URL_POR_DEFECTO) ?: URL_POR_DEFECTO
        set(v) = prefs.edit().putString(K_URL, v.trim()).apply()

    var token: String
        get() = prefs.getString(K_TOKEN, TOKEN_POR_DEFECTO) ?: TOKEN_POR_DEFECTO
        set(v) = prefs.edit().putString(K_TOKEN, v.trim()).apply()

    /** Serial de ADB que pasa el script de provisión; null si nunca se provisionó por ADB. */
    var adbSerial: String?
        get() = prefs.getString(K_ADB, null)
        set(v) = prefs.edit().putString(K_ADB, v?.trim()).apply()

    companion object {
        private const val K_ADB = "adb_serial"
        const val URL_POR_DEFECTO = "ws://127.0.0.1:47800/agente"
        const val TOKEN_POR_DEFECTO = "dev"
        private const val K_URL = "url"
        private const val K_TOKEN = "token"

        /** ANDROID_ID de esta app (desde Android 8 es distinto por app: no coincide con el de `adb shell`). */
        @SuppressLint("HardwareIds")
        fun dispositivoId(context: Context): String =
            Settings.Secure.getString(context.contentResolver, Settings.Secure.ANDROID_ID) ?: "desconocido"
    }
}
