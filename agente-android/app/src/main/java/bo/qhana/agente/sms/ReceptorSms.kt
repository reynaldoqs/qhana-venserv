package bo.qhana.agente.sms

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.provider.Telephony
import bo.qhana.agente.protocolo.Sms
import bo.qhana.agente.servicio.AgenteService
import bo.qhana.agente.telefonia.Telefonia

/** SMS entrante → cola de [BusSms]; el servicio lo envía al control como `sms_recibido`. */
class ReceptorSms : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Telephony.Sms.Intents.SMS_RECEIVED_ACTION) return
        val partes = Telephony.Sms.Intents.getMessagesFromIntent(intent) ?: return
        if (partes.isEmpty()) return
        val subId = intent.getIntExtra("android.telephony.extra.SUBSCRIPTION_INDEX", intent.getIntExtra("subscription", -1))
        val sms = Sms(
            id = "rx-${partes[0].timestampMillis}-${partes[0].originatingAddress}",
            slot = Telefonia(context).slotDeSubId(subId),
            de = partes[0].originatingAddress.orEmpty(),
            texto = partes.joinToString("") { it.messageBody.orEmpty() },
            fechaMs = partes[0].timestampMillis,
            leido = false,
        )
        BusSms.publicar(sms)
        AgenteService.arrancar(context)
    }
}
