package bo.qhana.agente.servicio

import bo.qhana.agente.protocolo.Mensaje
import kotlinx.coroutines.CompletableDeferred

/**
 * Las últimas [maximo] respuestas por `req`. Si el control repite una petición (p. ej. la reenvía tras reconectar),
 * se contesta con la respuesta guardada, o con la de la ejecución en curso, en vez de ejecutarla otra vez: un
 * `ussd_responder` repetido podría enviar dos veces una opción que mueve dinero. Vive en el servicio, no en la
 * conexión, para sobrevivir a una reconexión.
 */
class MemoriaRespuestas(private val maximo: Int = 20) {
    private val mapa = LinkedHashMap<String, CompletableDeferred<Mensaje>>()

    /** Registra [req]. Devuelve su respuesta (futura) y si es nueva: solo una petición nueva se ejecuta. */
    @Synchronized
    fun registrar(req: String): Pair<CompletableDeferred<Mensaje>, Boolean> {
        mapa[req]?.let { return it to false }
        val d = CompletableDeferred<Mensaje>()
        mapa[req] = d
        val it = mapa.entries.iterator()
        while (mapa.size > maximo && it.hasNext()) {
            it.next()
            it.remove()
        }
        return d to true
    }
}
