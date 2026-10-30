package bo.qhana.agente.servicio

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** Arranca el agente al encender el teléfono o al actualizar la app. */
class ReceptorArranque : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        AgenteService.arrancar(context)
    }
}
