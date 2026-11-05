package bo.qhana.agente.ui

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.util.Log
import bo.qhana.agente.AgenteApp
import bo.qhana.agente.EstadoApp
import bo.qhana.agente.config.Ajustes
import bo.qhana.agente.config.UrlControl
import bo.qhana.agente.servicio.AgenteService

/**
 * Única entrada de la configuración por ADB (protocolo: "Conexión"):
 *
 *     adb shell am start -n bo.qhana.agente/.ui.ActividadConfig --es url ws://… --es token … --es adb_serial <serial> [--ez reconectar true]
 *
 * Está protegida con `android.permission.DUMP` en el manifiesto: la tiene `adb shell` y no las apps comunes, así que
 * ninguna otra app del teléfono puede apuntar el agente a otro servidor. No tiene interfaz: aplica los extras, abre
 * [MainActivity] (que ignora esos extras) y termina.
 */
class ActividadConfig : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        runCatching { aplicar(intent) }.onFailure { Log.e(AgenteApp.TAG, "no pude aplicar la configuración", it) }
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        finish()
    }

    private fun aplicar(i: Intent) {
        val ajustes = Ajustes(this)
        val url = i.getStringExtra("url")?.trim()?.takeIf { it.isNotEmpty() }
        val token = i.getStringExtra("token")?.trim()?.takeIf { it.isNotEmpty() }
        val serial = i.getStringExtra("adb_serial")?.trim()?.takeIf { it.isNotEmpty() }
        // El control lo pide con el botón "Reconectar" (p. ej. tras dormir el PC): conectar ya, sin esperar la pausa.
        val pedido = i.getBooleanExtra("reconectar", false)

        // Con url o token (aunque sean los mismos) se reconecta ya, como al provisionar.
        var cambio = false
        if (serial != null && serial != ajustes.adbSerial) {
            ajustes.adbSerial = serial // el próximo `hola` lo lleva
            cambio = true
        }
        if (url != null) {
            if (UrlControl.valida(url)) {
                ajustes.url = url
                cambio = true
            } else {
                // Protocolo: una url que no empieza con ws:// o wss:// se ignora y queda la anterior.
                EstadoApp.anotar("URL ignorada (debe ser ws:// o wss://)")
                Log.w(AgenteApp.TAG, "url inválida ignorada: $url")
            }
        }
        if (token != null) {
            ajustes.token = token
            cambio = true
        }
        if (cambio || pedido) {
            EstadoApp.anotar(if (cambio) "configuración actualizada por ADB" else "reconexión pedida por ADB")
            AgenteService.arrancar(this, reconectar = true)
        } else {
            AgenteService.arrancar(this)
        }
    }
}
