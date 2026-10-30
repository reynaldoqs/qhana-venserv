package bo.qhana.agente.servicio

import android.content.Context
import android.os.PowerManager

/**
 * Enciende la pantalla solo mientras hay una operación USSD por pantalla y deja que el sistema la apague después.
 * Así el teléfono no necesita `stayon` (pantalla siempre encendida): se evita el desgaste del OLED y el calor
 * de tener la pantalla y la batería llenas todo el día.
 *
 * Solo sirve si el teléfono no tiene bloqueo de pantalla ("Ninguno" en ajustes): con PIN, patrón o incluso "deslizar",
 * la ventana de USSD queda detrás del bloqueo y la operación vence por tiempo (probado en el Infinix X678B, que tiene
 * patrón). En ese caso hay que dejar `stayon usb`; ver `scripts/provisionar.sh`.
 */
class Pantalla(context: Context) {
    private val pm = context.getSystemService(PowerManager::class.java)

    // SCREEN_BRIGHT_WAKE_LOCK está marcado obsoleto pero sigue siendo la única forma de encender la pantalla desde
    // un servicio sin una Activity en primer plano.
    @Suppress("DEPRECATION")
    private val candado: PowerManager.WakeLock = pm.newWakeLock(
        PowerManager.SCREEN_BRIGHT_WAKE_LOCK or PowerManager.ACQUIRE_CAUSES_WAKEUP or PowerManager.ON_AFTER_RELEASE,
        "qhana:ussd",
    ).apply { setReferenceCounted(false) }

    val encendida: Boolean get() = pm.isInteractive

    /** Enciende (o mantiene) la pantalla. El tope evita dejarla prendida si algo se cuelga y nunca llama a [soltar]. */
    fun encender() = candado.acquire(TOPE_MS)

    fun soltar() {
        if (candado.isHeld) candado.release()
    }

    companion object {
        const val TOPE_MS = 90_000L
        /** Lo que tarda la pantalla en estar lista para recibir la ventana de llamada tras encenderla. */
        const val ESPERA_ENCENDIDO_MS = 700L
    }
}
