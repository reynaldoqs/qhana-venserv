package bo.qhana.agente.sms

import android.content.Context
import android.net.Uri
import android.provider.Telephony
import bo.qhana.agente.protocolo.Sms
import bo.qhana.agente.telefonia.Telefonia

/** Lee la bandeja de entrada (requiere READ_SMS). */
class LectorSms(context: Context, private val telefonia: Telefonia) {
    private val resolver = context.applicationContext.contentResolver

    fun listar(slot: Int?, desdeMs: Long?, limite: Int): List<Sms> {
        val subId = slot?.let { telefonia.subIdDeSlot(it) ?: return emptyList() }
        val filtros = mutableListOf<String>()
        val args = mutableListOf<String>()
        if (desdeMs != null) { filtros += "${Telephony.Sms.DATE} > ?"; args += desdeMs.toString() }
        if (subId != null) { filtros += "${Telephony.Sms.SUBSCRIPTION_ID} = ?"; args += subId.toString() }
        val cols = arrayOf(
            Telephony.Sms._ID, Telephony.Sms.ADDRESS, Telephony.Sms.BODY, Telephony.Sms.DATE,
            Telephony.Sms.READ, Telephony.Sms.SUBSCRIPTION_ID,
        )
        val out = mutableListOf<Sms>()
        resolver.query(
            Uri.parse("content://sms/inbox"), cols,
            filtros.joinToString(" AND ").ifEmpty { null }, args.toTypedArray().ifEmpty { null },
            "${Telephony.Sms.DATE} DESC",
        )?.use { c ->
            val iSub = c.getColumnIndex(Telephony.Sms.SUBSCRIPTION_ID)
            while (c.moveToNext() && out.size < limite.coerceIn(1, 500)) {
                val sub = if (iSub >= 0) c.getInt(iSub) else -1
                out += Sms(
                    id = c.getLong(0).toString(),
                    slot = telefonia.slotDeSubId(sub),
                    de = c.getString(1).orEmpty(),
                    texto = c.getString(2).orEmpty(),
                    fechaMs = c.getLong(3),
                    leido = c.getInt(4) == 1,
                )
            }
        }
        return out
    }
}
