package bo.qhana.agente.ui

import android.app.Activity
import android.app.KeyguardManager
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import bo.qhana.agente.BuildConfig
import bo.qhana.agente.EstadoApp
import bo.qhana.agente.config.Ajustes
import bo.qhana.agente.config.UrlControl
import bo.qhana.agente.servicio.AgenteService
import bo.qhana.agente.servicio.EstadoDispositivo
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * Pantalla de estado. Es secundaria: en operación normal todo se maneja desde el control y esta app no necesita
 * estar abierta. Sirve para ver de un vistazo qué falta y arreglarlo con un toque.
 *
 * Es barata a propósito: fondo negro (ahorra batería en OLED), brillo bajo, no mantiene la pantalla encendida
 * y solo escucha el estado mientras está visible.
 * **No** acepta extras de configuración: eso entra solo por [ActividadConfig], protegida para que solo ADB la use.
 */
class MainActivity : Activity() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var escucha: Job? = null
    private var ultima = EstadoApp.Vista()

    private lateinit var ajustes: Ajustes
    private val prefsUi by lazy { getSharedPreferences("ui", MODE_PRIVATE) }
    private lateinit var cabeceraEstado: TextView
    private lateinit var tarjetaEstado: LinearLayout
    private lateinit var tarjetaChips: LinearLayout
    private lateinit var cuerpoConfig: LinearLayout
    private lateinit var cuerpoRegistro: LinearLayout
    private lateinit var textoRegistro: TextView
    private lateinit var campoUrl: EditText
    private lateinit var campoToken: EditText

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Brillo moderado de esta ventana; no se pide FLAG_KEEP_SCREEN_ON (la pantalla se apaga sola).
        window.attributes = window.attributes.apply { screenBrightness = 0.3f }
        window.statusBarColor = FONDO
        window.navigationBarColor = FONDO
        ajustes = Ajustes(this)
        construir()
        pedirPermisos()
        AgenteService.arrancar(this)
    }

    override fun onStart() {
        super.onStart()
        escucha = scope.launch { EstadoApp.vista.collect { ultima = it; pintar(it) } }
    }

    override fun onResume() {
        super.onResume()
        pintar(ultima) // al volver de Ajustes: batería, permisos, bloqueo cambian sin que el servicio avise
    }

    override fun onStop() {
        escucha?.cancel()
        super.onStop()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // Los extras se ignoran. Si llegamos desde ActividadConfig, la configuración pudo cambiar: mostrar la guardada.
        campoUrl.setText(ajustes.url)
        campoToken.setText(ajustes.token)
    }

    private fun permisosQueFaltan() =
        EstadoDispositivo.PERMISOS.filter { checkSelfPermission(it) != android.content.pm.PackageManager.PERMISSION_GRANTED }

    private fun pedirPermisos() {
        val faltan = permisosQueFaltan()
        if (faltan.isEmpty()) return
        prefsUi.edit().putBoolean(K_PERMISOS_PEDIDOS, true).apply()
        requestPermissions(faltan.toTypedArray(), 1)
    }

    /**
     * Botón "Conceder". Si Android ya no muestra el diálogo (se negó con "No volver a preguntar", o dos veces), se abre la
     * ficha de la app en Ajustes para concederlos a mano.
     */
    private fun conceder() {
        val faltan = permisosQueFaltan()
        val yaSePidio = prefsUi.getBoolean(K_PERMISOS_PEDIDOS, false)
        if (yaSePidio && faltan.none { shouldShowRequestPermissionRationale(it) }) {
            abrir(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
        } else {
            pedirPermisos()
        }
    }

    /** Abre una pantalla de Ajustes; algunos fabricantes no tienen todas. */
    private fun abrir(i: Intent, alternativa: Intent? = null) {
        val ok = runCatching { startActivity(i) }.isSuccess
        if (!ok && alternativa != null && runCatching { startActivity(alternativa) }.isSuccess) return
        if (!ok) Toast.makeText(this, "Este teléfono no tiene esa pantalla de ajustes", Toast.LENGTH_SHORT).show()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        pintar(ultima)
    }

    // ───────────── construcción ─────────────

    private fun dp(v: Int) = (v * resources.displayMetrics.density).toInt()

    private fun fondo(color: Int, radio: Int = 16) = GradientDrawable().apply {
        setColor(color)
        cornerRadius = dp(radio).toFloat()
    }

    private fun texto(t: String, tam: Float = 14f, color: Int = TEXTO, negrita: Boolean = false) = TextView(this).apply {
        text = t
        textSize = tam
        setTextColor(color)
        if (negrita) setTypeface(typeface, Typeface.BOLD)
    }

    private fun tarjeta(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        background = fondo(TARJETA)
        setPadding(dp(16), dp(12), dp(16), dp(12))
        layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
            topMargin = dp(12)
        }
    }

    private fun boton(t: String, primario: Boolean = false, alTocar: () -> Unit) = Button(this).apply {
        text = t
        isAllCaps = false
        textSize = 13f
        setTextColor(if (primario) Color.BLACK else TEXTO)
        background = fondo(if (primario) ACENTO else TARJETA_2, 24)
        minHeight = dp(40)
        minimumHeight = dp(40)
        setPadding(dp(16), 0, dp(16), 0)
        setOnClickListener { alTocar() }
    }

    /** Encabezado que muestra u oculta [cuerpo]. */
    private fun plegable(titulo: String, cuerpo: View, abierto: Boolean = false): View {
        cuerpo.visibility = if (abierto) View.VISIBLE else View.GONE
        val flecha = { a: Boolean -> if (a) "▾" else "▸" }
        val cab = texto("${flecha(abierto)}  $titulo", 13f, TENUE, true).apply {
            setPadding(0, dp(16), 0, dp(4))
            setOnClickListener {
                val a = cuerpo.visibility != View.VISIBLE
                cuerpo.visibility = if (a) View.VISIBLE else View.GONE
                text = "${flecha(a)}  $titulo"
            }
        }
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(cab)
            addView(cuerpo)
        }
    }

    private fun construir() {
        val raiz = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(20), dp(16), dp(32))
        }

        val cabecera = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL }
        val titulos = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(texto("Qhana Agente", 22f, TEXTO, true))
            addView(texto("v${BuildConfig.VERSION_NAME} · no hace falta tener esta pantalla abierta", 11f, TENUE))
        }
        cabecera.addView(titulos, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        cabeceraEstado = texto("", 12f, TEXTO, true).apply { setPadding(dp(12), dp(6), dp(12), dp(6)) }
        cabecera.addView(cabeceraEstado)
        raiz.addView(cabecera)

        tarjetaEstado = tarjeta()
        raiz.addView(tarjetaEstado)
        raiz.addView(texto("Chips", 13f, TENUE, true).apply { setPadding(0, dp(16), 0, 0) })
        tarjetaChips = tarjeta().also { (it.layoutParams as LinearLayout.LayoutParams).topMargin = dp(6) }
        raiz.addView(tarjetaChips)

        campoUrl = EditText(this).apply { setText(ajustes.url); hint = "ws://IP:47800/agente"; textSize = 14f; setTextColor(TEXTO); setHintTextColor(TENUE) }
        campoToken = EditText(this).apply {
            setText(ajustes.token); hint = "token"; textSize = 14f; setTextColor(TEXTO); setHintTextColor(TENUE)
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
        }
        cuerpoConfig = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = fondo(TARJETA)
            setPadding(dp(16), dp(12), dp(16), dp(12))
            addView(texto("Servidor", 12f, TENUE, true)); addView(campoUrl)
            addView(texto("Token", 12f, TENUE, true)); addView(campoToken)
            addView(boton("Guardar y reconectar", primario = true) {
                val url = campoUrl.text.toString().trim()
                if (!UrlControl.valida(url)) {
                    // Inválida: no se guarda nada y queda la anterior.
                    campoUrl.error = "Debe empezar con ws:// o wss://"
                    return@boton
                }
                ajustes.url = url
                if (campoToken.text.isNotBlank()) ajustes.token = campoToken.text.toString()
                AgenteService.arrancar(this@MainActivity, reconectar = true)
                Toast.makeText(this@MainActivity, "Guardado: reconectando…", Toast.LENGTH_SHORT).show()
            }.also { it.layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = dp(12) } })
            addView(texto("ID ${Ajustes.dispositivoId(this@MainActivity)} · ADB ${ajustes.adbSerial ?: "—"}", 11f, TENUE).apply { setPadding(0, dp(12), 0, 0); setTextIsSelectable(true) })
        }
        raiz.addView(plegable("Configuración", cuerpoConfig))

        textoRegistro = texto("", 11f, TENUE).apply { typeface = Typeface.MONOSPACE; setTextIsSelectable(true) }
        cuerpoRegistro = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = fondo(TARJETA)
            setPadding(dp(16), dp(12), dp(16), dp(12))
            addView(textoRegistro)
        }
        raiz.addView(plegable("Registro", cuerpoRegistro))

        setContentView(ScrollView(this).apply {
            setBackgroundColor(FONDO)
            addView(raiz, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        })
    }

    // ───────────── pintado ─────────────

    private class Chequeo(val ok: Nivel, val titulo: String, val detalle: String, val accion: String? = null, val alTocar: (() -> Unit)? = null)
    private enum class Nivel(val color: Int) { OK(VERDE), AVISO(AMBAR), ERROR(ROJO) }

    private fun chequeos(v: EstadoApp.Vista): List<Chequeo> {
        val pm = getSystemService(PowerManager::class.java)
        val faltan = EstadoDispositivo.permisosFaltantes(this)
        val bateriaLibre = pm.isIgnoringBatteryOptimizations(packageName)
        val conBloqueo = getSystemService(KeyguardManager::class.java).isDeviceSecure
        val siempreEncendida = Settings.Global.getInt(contentResolver, Settings.Global.STAY_ON_WHILE_PLUGGED_IN, 0) != 0
        return listOf(
            when (v.conexion) {
                "conectado" -> Chequeo(Nivel.OK, "Control", "Conectado a ${v.detalleConexion.ifBlank { v.url }}")
                "conectando", "reconectando" -> Chequeo(Nivel.AVISO, "Control", "Conectando a ${v.url}…")
                "rechazado" -> Chequeo(Nivel.ERROR, "Control", "Rechazado: ${v.detalleConexion}. Revisa el token.")
                else -> Chequeo(Nivel.ERROR, "Control", "Sin conexión con ${v.url.ifBlank { "el control" }}. ¿Cable USB y control encendido?")
            },
            if (v.accesibilidad) Chequeo(Nivel.OK, "Accesibilidad", "Activa: puede leer los menús USSD")
            else Chequeo(Nivel.ERROR, "Accesibilidad", "Apagada: sin ella no puede contestar USSD", "Activar") {
                startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
            },
            if (faltan.isEmpty()) Chequeo(Nivel.OK, "Permisos", "Todos concedidos")
            else Chequeo(Nivel.ERROR, "Permisos", "Faltan: ${faltan.joinToString()}", "Conceder") { conceder() },
            if (bateriaLibre) Chequeo(Nivel.OK, "Batería", "Sin restricciones: Android no la cierra", "Ajustes") {
                abrir(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
            }
            else Chequeo(Nivel.AVISO, "Batería", "Android puede cerrarla para ahorrar", "Quitar límite") {
                abrir(
                    Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:$packageName")),
                    alternativa = Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
                )
            },
            v.sesionUssd?.let { id ->
                Chequeo(Nivel.OK, "USSD", "Sesión abierta (${id.take(8)}). La cierra el control o sola a los 60 s.", "Cancelar") {
                    AgenteService.cancelarUssd(this)
                }
            } ?: Chequeo(Nivel.OK, "USSD", "Sin sesión abierta"),
            when {
                !conBloqueo && siempreEncendida -> Chequeo(Nivel.OK, "Pantalla", "Encendida con USB: así el sistema no congela el agente")
                !conBloqueo -> Chequeo(Nivel.AVISO, "Pantalla", "Se apaga: algunos teléfonos congelan el agente y el USSD falla")
                siempreEncendida -> Chequeo(
                    Nivel.AVISO, "Pantalla",
                    "Con PIN o patrón queda siempre encendida (desgasta el OLED)",
                    "Bloqueo",
                ) { startActivity(Intent(Settings.ACTION_SECURITY_SETTINGS)) }
                else -> Chequeo(
                    Nivel.ERROR, "Pantalla",
                    "Con PIN o patrón y la pantalla apagada, los USSD fallan",
                    "Bloqueo",
                ) { startActivity(Intent(Settings.ACTION_SECURITY_SETTINGS)) }
            },
        )
    }

    private fun pintar(v: EstadoApp.Vista) {
        val lista = chequeos(v)
        val peor = lista.maxOf { it.ok.ordinal }.let { Nivel.values()[it] }
        cabeceraEstado.text = when (peor) {
            Nivel.OK -> "Todo listo"
            Nivel.AVISO -> "Revisar"
            Nivel.ERROR -> "Requiere atención"
        }
        cabeceraEstado.setTextColor(Color.BLACK)
        cabeceraEstado.background = fondo(peor.color, 20)

        tarjetaEstado.removeAllViews()
        lista.forEachIndexed { i, c -> tarjetaEstado.addView(fila(c, separador = i > 0)) }

        tarjetaChips.removeAllViews()
        val conChip = v.sims.filter { it.estado != "ausente" } // ranuras vacías (p. ej. la eSIM) no interesan
        if (conChip.isEmpty()) {
            tarjetaChips.addView(texto("Sin datos de chips (¿falta el permiso de teléfono?).", 13f, TENUE))
        } else {
            conChip.forEachIndexed { i, s ->
                val nivel = if (s.estado == "listo" && (s.voz == null || s.voz == "en_servicio")) Nivel.OK else Nivel.AVISO
                val detalle = listOfNotNull(s.numero, s.red, s.voz?.takeIf { it != "en_servicio" }?.let { "voz $it" }).joinToString(" · ")
                tarjetaChips.addView(fila(Chequeo(nivel, "SIM ${s.slot + 1} · ${s.operador.ifEmpty { "sin operador" }}", "${s.estado}${if (detalle.isEmpty()) "" else " · $detalle"}"), separador = i > 0))
            }
        }

        textoRegistro.text = buildString {
            if (v.ultimoUssd.isNotBlank()) append("Último USSD:\n${v.ultimoUssd}\n\n")
            append(v.registro.take(15).joinToString("\n").ifEmpty { "(sin eventos)" })
        }
    }

    private fun fila(c: Chequeo, separador: Boolean): View {
        val fila = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, dp(if (separador) 10 else 0), 0, 0)
        }
        fila.addView(View(this).apply { background = fondo(c.ok.color, 8) }, LinearLayout.LayoutParams(dp(10), dp(10)).apply { rightMargin = dp(12) })
        val cuerpo = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(texto(c.titulo, 14f, TEXTO, true))
            addView(texto(c.detalle, 12f, TENUE))
        }
        fila.addView(cuerpo, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        if (c.accion != null && c.alTocar != null) fila.addView(boton(c.accion, primario = true) { c.alTocar.invoke() })
        return fila
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private companion object {
        const val K_PERMISOS_PEDIDOS = "permisos_pedidos"
        val FONDO = Color.parseColor("#0D0D0D")
        val TARJETA = Color.parseColor("#1A1A1A")
        val TARJETA_2 = Color.parseColor("#2B2B2B")
        val TEXTO = Color.parseColor("#F9F9F9")
        val TENUE = Color.parseColor("#A7A7A7")
        val ACENTO = Color.parseColor("#E85002")
        val VERDE = Color.parseColor("#3ED598")
        val AMBAR = Color.parseColor("#F5C451")
        val ROJO = Color.parseColor("#FF7B7B")
    }
}
