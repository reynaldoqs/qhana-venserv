package bo.qhana.agente.servicio

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import bo.qhana.agente.AgenteApp

/** Arranca el agente al encender el teléfono o al actualizar la app. */
class ReceptorArranque : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        runCatching { AgenteService.arrancar(context) }
            .onFailure { Log.w(AgenteApp.TAG, "no pude arrancar el agente (${intent.action})", it) }
    }
}
