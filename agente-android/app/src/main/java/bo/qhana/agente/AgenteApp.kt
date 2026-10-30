package bo.qhana.agente

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager

class AgenteApp : Application() {
    override fun onCreate() {
        super.onCreate()
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CANAL, "Agente", NotificationManager.IMPORTANCE_LOW)
        )
    }

    companion object {
        const val CANAL = "agente"
        const val TAG = "QhanaAgente"
    }
}
