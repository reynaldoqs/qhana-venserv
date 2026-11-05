package bo.qhana.agente.ussd

import android.accessibilityservice.AccessibilityService
import android.os.Bundle
import android.util.Log
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import bo.qhana.agente.AgenteApp
import bo.qhana.agente.EstadoApp
import bo.qhana.agente.servicio.AgenteService
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import java.util.concurrent.atomic.AtomicLong

/** La ventana de USSD tal como se ve ahora. */
class Dialogo(
    val paquete: String,
    val perfil: String,
    val texto: String,
    val progreso: Boolean,
    val entrada: AccessibilityNodeInfo?,
    val positivo: AccessibilityNodeInfo?,
    val negativo: AccessibilityNodeInfo?,
)

/**
 * Servicio de accesibilidad: solo mira las ventanas de los paquetes de telefonía (ver [PerfilDialogo]).
 * No decide nada: expone "qué dice la ventana" y "presiona/escribe", y [UssdPantalla] orquesta.
 */
class ServicioAccesibilidad : AccessibilityService() {

    override fun onServiceConnected() {
        instancia = this
        EstadoApp.actualizar { it.copy(accesibilidad = true) }
        Log.i(AgenteApp.TAG, "accesibilidad conectada")
        // Android reconecta este servicio solo tras matar el proceso; con él viene el agente, sin esperar a un SMS,
        // a un reinicio ni a que alguien abra la app. El sistema lo une como proceso persistente, así que puede
        // iniciar el servicio en primer plano desde aquí.
        runCatching { AgenteService.arrancar(applicationContext) }
            .onFailure { Log.w(AgenteApp.TAG, "no pude arrancar el agente desde accesibilidad", it) }
    }

    override fun onAccessibilityEvent(e: AccessibilityEvent?) {
        val pkg = e?.packageName?.toString() ?: return
        if (pkg !in PerfilDialogo.PAQUETES) return
        if (e.eventType == AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED) ventanasNuevas.incrementAndGet()
        _cambios.tryEmit(Unit)
    }

    override fun onInterrupt() = Unit

    override fun onDestroy() {
        instancia = null
        EstadoApp.actualizar { it.copy(accesibilidad = false) }
        super.onDestroy()
    }

    /** Busca la ventana de USSD visible. null si no hay ninguna. */
    fun leerDialogo(): Dialogo? {
        val raices = buildList {
            runCatching { windows }.getOrNull()?.forEach { w -> w.root?.let { add(it) } }
            rootInActiveWindow?.let { add(it) }
        }
        for (raiz in raices) {
            val pkg = raiz.packageName?.toString() ?: continue
            if (pkg !in PerfilDialogo.PAQUETES) continue
            for (perfil in PerfilDialogo.TODOS) {
                if (pkg !in perfil.paquetes) continue
                analizar(raiz, pkg, perfil)?.let { return it }
            }
        }
        return null
    }

    private fun analizar(raiz: AccessibilityNodeInfo, pkg: String, perfil: PerfilDialogo): Dialogo? {
        val texto = perfil.idsTexto.firstNotNullOfOrNull { id -> porId(raiz, id)?.text?.toString()?.takeIf { it.isNotBlank() } }
            ?: textoMasLargo(raiz)
            ?: return null
        val entrada = perfil.idsEntrada.firstNotNullOfOrNull { porId(raiz, it) } ?: buscar(raiz) { n ->
            n.className?.toString()?.contains("EditText") == true
        }
        val positivo = perfil.idsPositivo.firstNotNullOfOrNull { porId(raiz, it) }
            ?: botonConTexto(raiz, PerfilDialogo.TEXTOS_POSITIVO)
        val negativo = perfil.idsNegativo.firstNotNullOfOrNull { porId(raiz, it) }
            ?: botonConTexto(raiz, PerfilDialogo.TEXTOS_NEGATIVO)
        val esProgreso = PerfilDialogo.esProgreso(texto.trim()) && entrada == null
        // Sin botones ni campo no es un diálogo de USSD (otra pantalla del paquete de telefonía).
        if (!esProgreso && entrada == null && positivo == null && negativo == null) return null
        return Dialogo(pkg, perfil.nombre, texto.trim(), esProgreso, entrada, positivo, negativo)
    }

    private fun porId(raiz: AccessibilityNodeInfo, id: String): AccessibilityNodeInfo? =
        runCatching { raiz.findAccessibilityNodeInfosByViewId(id) }.getOrNull()?.firstOrNull()

    private fun buscar(n: AccessibilityNodeInfo?, cond: (AccessibilityNodeInfo) -> Boolean): AccessibilityNodeInfo? {
        if (n == null) return null
        if (cond(n)) return n
        for (i in 0 until n.childCount) buscar(n.getChild(i), cond)?.let { return it }
        return null
    }

    private fun recorrer(n: AccessibilityNodeInfo?, f: (AccessibilityNodeInfo) -> Unit) {
        if (n == null) return
        f(n)
        for (i in 0 until n.childCount) recorrer(n.getChild(i), f)
    }

    private fun textoMasLargo(raiz: AccessibilityNodeInfo): String? {
        var mejor: String? = null
        recorrer(raiz) { n ->
            val c = n.className?.toString().orEmpty()
            val t = n.text?.toString()
            if (t != null && c.endsWith("TextView") && (mejor == null || t.length > mejor!!.length)) mejor = t
        }
        return mejor?.takeIf { it.isNotBlank() }
    }

    private fun botonConTexto(raiz: AccessibilityNodeInfo, textos: List<String>): AccessibilityNodeInfo? =
        buscar(raiz) { n ->
            n.className?.toString()?.contains("Button") == true &&
                n.text?.toString()?.trim()?.lowercase() in textos
        }

    // ---------------------------------------------------------------- acciones

    fun escribir(campo: AccessibilityNodeInfo, texto: String): Boolean {
        campo.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
        val args = Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, texto)
        }
        return campo.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
    }

    /** Lo que tiene ahora el campo, leído de nuevo de la ventana (no de la caché de accesibilidad). */
    fun leerCampo(campo: AccessibilityNodeInfo): String? {
        runCatching { campo.refresh() }
        return campo.text?.toString()
    }

    /** Click en el nodo o, si no es clickeable, en el primer ancestro que lo sea. */
    fun presionar(nodo: AccessibilityNodeInfo?): Boolean {
        var n = nodo
        while (n != null) {
            if (n.isClickable) return n.performAction(AccessibilityNodeInfo.ACTION_CLICK)
            n = n.parent
        }
        return false
    }

    fun atras(): Boolean = performGlobalAction(GLOBAL_ACTION_BACK)

    companion object {
        @Volatile
        var instancia: ServicioAccesibilidad? = null
            private set

        /** Cuenta los cambios de ventana del paquete de telefonía (para detectar respuestas con el mismo texto). */
        val ventanasNuevas = AtomicLong(0)

        private val _cambios = MutableSharedFlow<Unit>(extraBufferCapacity = 64, onBufferOverflow = BufferOverflow.DROP_OLDEST)
        val cambios: SharedFlow<Unit> = _cambios
    }
}
