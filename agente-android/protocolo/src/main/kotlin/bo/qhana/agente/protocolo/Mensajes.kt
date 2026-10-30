package bo.qhana.agente.protocolo

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Mensajes del protocolo v1 (ver docs/protocolo.md). Un solo árbol sellado con discriminador "tipo":
 * así el mismo decodificador sirve para ambas direcciones y los tests pueden ir y volver.
 */
const val VERSION_PROTOCOLO = 1

@Serializable
sealed interface Mensaje

/** Mensajes que llevan id de petición. */
sealed interface ConReq : Mensaje {
    val req: String
}

// ---------------------------------------------------------------- tipos compartidos

@Serializable
data class Sim(
    val slot: Int,
    @SerialName("sub_id") val subId: Int,
    val operador: String,
    @SerialName("mcc_mnc") val mccMnc: String,
    val iccid: String? = null,
    val numero: String? = null,
    val estado: String = EstadoSim.DESCONOCIDO,
    val voz: String? = null,
    val red: String? = null,
)

object EstadoSim {
    const val LISTO = "listo"
    const val AUSENTE = "ausente"
    const val PIN = "pin"
    const val PUK = "puk"
    const val BLOQUEADA = "bloqueada"
    const val DESCONOCIDO = "desconocido"
}

object EstadoVoz {
    const val EN_SERVICIO = "en_servicio"
    const val SIN_SERVICIO = "sin_servicio"
    const val SOLO_EMERGENCIA = "solo_emergencia"
    const val APAGADO = "apagado"
}

@Serializable
data class Sms(
    val id: String,
    val slot: Int? = null,
    val de: String,
    val texto: String,
    @SerialName("fecha_ms") val fechaMs: Long,
    val leido: Boolean = false,
)

object CodigoError {
    const val OCUPADO = "ocupado"
    const val SIN_PERMISO = "sin_permiso"
    const val SIN_ACCESIBILIDAD = "sin_accesibilidad"
    const val SIM_INVALIDA = "sim_invalida"
    const val TIMEOUT = "timeout"
    const val RED = "red"
    const val SESION_INEXISTENTE = "sesion_inexistente"
    const val PARAMETRO = "parametro"
    const val DESCONOCIDO = "desconocido"
}

object ModoUssd {
    const val API = "api"
    const val PANTALLA = "pantalla"
}

// ---------------------------------------------------------------- agente → control

@Serializable
@SerialName("hola")
data class Hola(
    val protocolo: Int = VERSION_PROTOCOLO,
    val token: String,
    @SerialName("dispositivo_id") val dispositivoId: String,
    val fabricante: String,
    val modelo: String,
    val android: String,
    val sdk: Int,
    @SerialName("app_version") val appVersion: String,
    val sims: List<Sim>,
    val capacidades: List<String>,
    /**
     * Serial de ADB del teléfono, si se lo pasaron al provisionar (`--es adb_serial`). Extensión al
     * contrato v1: ANDROID_ID es distinto para cada app desde Android 8, así que el control no puede
     * cruzarlo con `adb shell settings get secure android_id`; este campo resuelve el cruce.
     */
    @SerialName("adb_serial") val adbSerial: String? = null,
) : Mensaje

@Serializable
@SerialName("sims")
data class Sims(val sims: List<Sim>) : Mensaje

@Serializable
@SerialName("estado")
data class Estado(
    val bateria: Int,
    val cargando: Boolean,
    @SerialName("pantalla_encendida") val pantallaEncendida: Boolean,
    @SerialName("accesibilidad_activa") val accesibilidadActiva: Boolean,
    @SerialName("permisos_faltantes") val permisosFaltantes: List<String>,
) : Mensaje

@Serializable
@SerialName("ussd")
data class Ussd(
    override val req: String,
    val sesion: String,
    val texto: String,
    val abierta: Boolean,
    val ms: Long,
) : ConReq

@Serializable
@SerialName("ok")
data class Ok(override val req: String) : ConReq

@Serializable
@SerialName("error")
data class Fallo(
    override val req: String,
    val codigo: String,
    val detalle: String,
) : ConReq

@Serializable
@SerialName("sms_recibido")
data class SmsRecibido(val sms: Sms) : Mensaje

@Serializable
@SerialName("sms_lista")
data class SmsLista(override val req: String, val sms: List<Sms>) : ConReq

@Serializable
@SerialName("pong")
data class Pong(override val req: String) : ConReq

// ---------------------------------------------------------------- control → agente

@Serializable
@SerialName("bienvenido")
data class Bienvenido(val servidor: String, @SerialName("hora_ms") val horaMs: Long) : Mensaje

@Serializable
@SerialName("rechazado")
data class Rechazado(val motivo: String) : Mensaje

@Serializable
@SerialName("ussd_iniciar")
data class UssdIniciar(
    override val req: String,
    val slot: Int,
    val codigo: String,
    val modo: String = ModoUssd.PANTALLA,
    @SerialName("timeout_ms") val timeoutMs: Long = 30_000,
) : ConReq

@Serializable
@SerialName("ussd_responder")
data class UssdResponder(
    override val req: String,
    val sesion: String,
    val texto: String,
    @SerialName("timeout_ms") val timeoutMs: Long = 30_000,
) : ConReq

@Serializable
@SerialName("ussd_cancelar")
data class UssdCancelar(override val req: String, val sesion: String) : ConReq

@Serializable
@SerialName("sms_listar")
data class SmsListar(
    override val req: String,
    val slot: Int? = null,
    @SerialName("desde_ms") val desdeMs: Long? = null,
    val limite: Int = 50,
) : ConReq

@Serializable
@SerialName("refrescar")
data class Refrescar(override val req: String) : ConReq

@Serializable
@SerialName("ping")
data class Ping(override val req: String) : ConReq
