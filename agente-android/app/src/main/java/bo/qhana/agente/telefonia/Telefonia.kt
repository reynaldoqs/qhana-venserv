package bo.qhana.agente.telefonia

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.telecom.PhoneAccountHandle
import android.telecom.TelecomManager
import android.telephony.ServiceState
import android.telephony.SubscriptionInfo
import android.telephony.SubscriptionManager
import android.telephony.TelephonyManager
import bo.qhana.agente.protocolo.EstadoSim
import bo.qhana.agente.protocolo.EstadoVoz
import bo.qhana.agente.protocolo.Sim

/** Todo lo que el agente sabe de las SIMs. Las llamadas que requieren permisos fallan en silencio (campo null). */
@SuppressLint("MissingPermission", "HardwareIds")
class Telefonia(context: Context) {
    private val ctx = context.applicationContext
    private val subs = ctx.getSystemService(SubscriptionManager::class.java)
    private val tel = ctx.getSystemService(TelephonyManager::class.java)
    private val telecom = ctx.getSystemService(TelecomManager::class.java)

    private fun suscripciones(): List<SubscriptionInfo> =
        runCatching { subs.activeSubscriptionInfoList.orEmpty() }.getOrDefault(emptyList())

    fun cantidadSlots(): Int =
        if (Build.VERSION.SDK_INT >= 30) tel.activeModemCount else @Suppress("DEPRECATION") tel.phoneCount

    /** Una entrada por ranura física; las vacías van con estado "ausente" y sub_id -1. */
    fun sims(): List<Sim> {
        val porSlot = suscripciones().associateBy { it.simSlotIndex }
        return (0 until cantidadSlots()).map { slot ->
            val info = porSlot[slot]
            if (info == null) {
                Sim(slot = slot, subId = -1, operador = "", mccMnc = "", estado = estadoSim(slot).let {
                    if (it == EstadoSim.LISTO) EstadoSim.DESCONOCIDO else it
                })
            } else {
                simDe(info)
            }
        }
    }

    private fun simDe(info: SubscriptionInfo): Sim {
        val subId = info.subscriptionId
        val t = tel.createForSubscriptionId(subId)
        val mccMnc = if (Build.VERSION.SDK_INT >= 29) "${info.mccString.orEmpty()}${info.mncString.orEmpty()}"
        else @Suppress("DEPRECATION") "${info.mcc}${"%02d".format(info.mnc)}"
        val numero = runCatching {
            if (Build.VERSION.SDK_INT >= 33) subs.getPhoneNumber(subId) else @Suppress("DEPRECATION") info.number
        }.getOrNull()?.takeIf { it.isNotBlank() }
        val iccid = runCatching { info.iccId }.getOrNull()?.takeIf { it.isNotBlank() }
        val servicio = runCatching { t.serviceState }.getOrNull()
        val red = runCatching { nombreRed(t.voiceNetworkType) }.getOrNull()
        return Sim(
            slot = info.simSlotIndex,
            subId = subId,
            operador = (info.carrierName ?: info.displayName ?: "").toString(),
            mccMnc = mccMnc,
            iccid = iccid,
            numero = numero,
            estado = estadoSim(info.simSlotIndex),
            voz = servicio?.let { estadoVoz(it.state) },
            red = red,
        )
    }

    private fun estadoSim(slot: Int): String = when (runCatching { tel.getSimState(slot) }.getOrDefault(-1)) {
        TelephonyManager.SIM_STATE_READY -> EstadoSim.LISTO
        TelephonyManager.SIM_STATE_ABSENT -> EstadoSim.AUSENTE
        TelephonyManager.SIM_STATE_PIN_REQUIRED -> EstadoSim.PIN
        TelephonyManager.SIM_STATE_PUK_REQUIRED -> EstadoSim.PUK
        TelephonyManager.SIM_STATE_NETWORK_LOCKED, TelephonyManager.SIM_STATE_PERM_DISABLED -> EstadoSim.BLOQUEADA
        else -> EstadoSim.DESCONOCIDO
    }

    private fun estadoVoz(s: Int) = when (s) {
        ServiceState.STATE_IN_SERVICE -> EstadoVoz.EN_SERVICIO
        ServiceState.STATE_OUT_OF_SERVICE -> EstadoVoz.SIN_SERVICIO
        ServiceState.STATE_EMERGENCY_ONLY -> EstadoVoz.SOLO_EMERGENCIA
        ServiceState.STATE_POWER_OFF -> EstadoVoz.APAGADO
        else -> null
    }

    private fun nombreRed(t: Int): String? = when (t) {
        TelephonyManager.NETWORK_TYPE_LTE -> "LTE"
        TelephonyManager.NETWORK_TYPE_NR -> "NR"
        TelephonyManager.NETWORK_TYPE_UMTS, TelephonyManager.NETWORK_TYPE_HSDPA, TelephonyManager.NETWORK_TYPE_HSUPA,
        TelephonyManager.NETWORK_TYPE_HSPA, TelephonyManager.NETWORK_TYPE_HSPAP -> "WCDMA"
        TelephonyManager.NETWORK_TYPE_GSM, TelephonyManager.NETWORK_TYPE_EDGE, TelephonyManager.NETWORK_TYPE_GPRS -> "GSM"
        TelephonyManager.NETWORK_TYPE_IWLAN -> "IWLAN"
        TelephonyManager.NETWORK_TYPE_UNKNOWN -> null
        else -> "otro($t)"
    }

    /** sub_id de la SIM en ese slot, o null si el slot está vacío. */
    fun subIdDeSlot(slot: Int): Int? = suscripciones().firstOrNull { it.simSlotIndex == slot }?.subscriptionId

    fun slotDeSubId(subId: Int): Int? = suscripciones().firstOrNull { it.subscriptionId == subId }?.simSlotIndex

    fun telephonyDe(subId: Int): TelephonyManager = tel.createForSubscriptionId(subId)

    /** Cuenta de llamadas (PhoneAccount) que corresponde a la SIM, para que ACTION_CALL no pregunte qué chip usar. */
    fun cuentaDeSubId(subId: Int): PhoneAccountHandle? {
        val cuentas = runCatching { telecom.callCapablePhoneAccounts }.getOrDefault(emptyList())
        if (Build.VERSION.SDK_INT >= 30) {
            cuentas.firstOrNull { runCatching { tel.getSubscriptionId(it) }.getOrDefault(-1) == subId }?.let { return it }
        }
        val iccid = suscripciones().firstOrNull { it.subscriptionId == subId }?.let { runCatching { it.iccId }.getOrNull() }
        return cuentas.firstOrNull { it.id == subId.toString() || (!iccid.isNullOrBlank() && it.id.startsWith(iccid)) }
    }

    /** Intent para marcar un código USSD por una SIM concreta. */
    fun intentMarcar(codigo: String, slot: Int, subId: Int): Intent {
        val i = Intent(Intent.ACTION_CALL, Uri.parse("tel:" + Uri.encode(codigo)))
        i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        cuentaDeSubId(subId)?.let { i.putExtra(TelecomManager.EXTRA_PHONE_ACCOUNT_HANDLE, it) }
        // Extras que leen distintos fabricantes para elegir el chip.
        for (k in EXTRAS_SLOT) i.putExtra(k, slot)
        i.putExtra("com.android.phone.force.slot", true)
        i.putExtra("subscription", subId)
        i.putExtra("Subscription", subId)
        return i
    }

    companion object {
        private val EXTRAS_SLOT = listOf(
            "com.android.phone.extra.slot", "slot", "simSlot", "sim_slot", "simslot", "slot_id", "slotId",
            "slotIdx", "phone", "com.android.phone.DialingMode", "simSlotIndex",
        )
    }
}
