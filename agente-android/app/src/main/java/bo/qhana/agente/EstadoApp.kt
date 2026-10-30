package bo.qhana.agente

import bo.qhana.agente.protocolo.Sim
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

/** Estado observable para la pantalla. Lo escribe el servicio; lo lee MainActivity. */
object EstadoApp {
    data class Vista(
        val conexion: String = "detenido",
        val detalleConexion: String = "",
        val url: String = "",
        val sims: List<Sim> = emptyList(),
        val permisosFaltantes: List<String> = emptyList(),
        val accesibilidad: Boolean = false,
        val ultimoUssd: String = "",
        val registro: List<String> = emptyList(),
    )

    private val _vista = MutableStateFlow(Vista())
    val vista: StateFlow<Vista> = _vista.asStateFlow()

    fun actualizar(f: (Vista) -> Vista) = _vista.update(f)

    fun anotar(linea: String) = _vista.update {
        val hora = java.text.SimpleDateFormat("HH:mm:ss", java.util.Locale.US).format(java.util.Date())
        it.copy(registro = (listOf("$hora $linea") + it.registro).take(30))
    }
}
