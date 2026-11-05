package bo.qhana.agente.servicio

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager
import android.provider.Settings
import android.util.Log
import bo.qhana.agente.AgenteApp
import bo.qhana.agente.EstadoApp
import bo.qhana.agente.ussd.ServicioAccesibilidad
import kotlinx.coroutines.delay

/**
 * Vuelve a conectar el servicio de accesibilidad cuando Android lo deja "caído": si se fuerza a detener la app
 * (Ajustes o el limpiador de XOS) con el servicio unido, queda activado en Ajustes pero sin unir, y Android no lo
 * vuelve a unir hasta que alguien lo apague y lo prenda. Eso hace este reparador, con el permiso
 * `WRITE_SECURE_SETTINGS` (solo se da por adb: `provisionar`). Sin el permiso no hace nada.
 *
 * Solo repara si el servicio **está activado** en Ajustes (si alguien lo apagó a propósito, no lo prende), después de
 * [VUELTAS_ANTES] revisiones seguidas sin unir (no pisa un arranque ni el corte de un `uiautomator dump`) y como mucho
 * una vez cada [ESPERA_ENTRE_MS].
 */
class ReparadorAccesibilidad(private val ctx: Context) {
    private var sinUnir = 0
    private var ultimaMs = 0L

    /** Llamar en cada revisión periódica (cada 30 s). */
    suspend fun revisar() {
        val unido = ServicioAccesibilidad.instancia != null
        val activado = activadoEnAjustes()
        sinUnir = if (unido || !activado) 0 else sinUnir + 1
        if (!debeReparar(sinUnir, System.currentTimeMillis() - ultimaMs, tienePermiso())) return
        ultimaMs = System.currentTimeMillis()
        sinUnir = 0
        runCatching { reparar() }
            .onSuccess { EstadoApp.anotar("accesibilidad caída: la apagué y prendí de nuevo") }
            .onFailure { Log.w(AgenteApp.TAG, "no pude reparar la accesibilidad", it) }
    }

    private fun servicio() = ComponentName(ctx, ServicioAccesibilidad::class.java).flattenToString()

    private fun lista(): String? = Settings.Secure.getString(ctx.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES)

    private fun activadoEnAjustes() = contiene(lista(), servicio())

    private fun tienePermiso() =
        ctx.checkSelfPermission(Manifest.permission.WRITE_SECURE_SETTINGS) == PackageManager.PERMISSION_GRANTED

    private suspend fun reparar() {
        val cr = ctx.contentResolver
        val actual = lista()
        Log.i(AgenteApp.TAG, "accesibilidad activada pero sin unir: la apago y la prendo")
        Settings.Secure.putString(cr, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES, sin(actual, servicio()))
        delay(1_500)
        Settings.Secure.putString(cr, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES, con(sin(actual, servicio()), servicio()))
        Settings.Secure.putInt(cr, Settings.Secure.ACCESSIBILITY_ENABLED, 1)
    }

    companion object {
        /** Revisiones seguidas sin unir antes de reparar (cada 30 s: ~1 min). */
        const val VUELTAS_ANTES = 2
        const val ESPERA_ENTRE_MS = 5 * 60_000L

        fun debeReparar(sinUnir: Int, msDesdeUltima: Long, permiso: Boolean) =
            permiso && sinUnir >= VUELTAS_ANTES && msDesdeUltima >= ESPERA_ENTRE_MS

        private fun partes(lista: String?) =
            lista.orEmpty().split(':').map { it.trim() }.filter { it.isNotEmpty() && it != "null" }

        /** Compara la forma larga y la corta (`pkg/.Clase`) del nombre. */
        private fun igual(a: String, b: String) = largo(a) == largo(b)

        private fun largo(s: String): String {
            val (pkg, clase) = s.split('/', limit = 2).takeIf { it.size == 2 } ?: return s
            return if (clase.startsWith(".")) "$pkg/$pkg$clase" else s
        }

        fun contiene(lista: String?, servicio: String) = partes(lista).any { igual(it, servicio) }

        /** La lista sin el servicio; deja los demás servicios activos tal cual. */
        fun sin(lista: String?, servicio: String) = partes(lista).filterNot { igual(it, servicio) }.joinToString(":")

        fun con(lista: String?, servicio: String) = (partes(lista) + servicio).joinToString(":")
    }
}
