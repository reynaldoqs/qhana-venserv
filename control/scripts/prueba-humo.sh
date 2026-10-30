#!/usr/bin/env bash
# Prueba de humo de la API contra el agente simulado (NUNCA contra un teléfono real: hace transferencias).
#   ./target/debug/qhana-control --sin-adb &
#   ./target/debug/agente-simulado --id sim-1 --saldo 14 --retardo-ms 100 &
#   ./target/debug/agente-simulado --id sim-2 --chip2 entel --limite-diario 0 --retardo-ms 100 &
#   scripts/prueba-humo.sh
set -u
A=${A:-http://127.0.0.1:47800/api}
D1=${D1:-sim-1}; D2=${D2:-sim-2}
fallos=0
post() { curl -s -w '\n%{http_code}' -H 'content-type: application/json' -d "$2" "$A/$1"; }
cuerpo() { printf '%s' "$1" | sed '$d'; }
codigo() { printf '%s' "$1" | tail -n1; }
campo() { python3 -c "import sys,json;d=json.load(sys.stdin);print(eval(sys.argv[1],{},{'d':d}))" "$1"; }
espera() { # nombre, valor obtenido, valor esperado
  if [ "$2" == "$3" ]; then printf '  ok   %-45s %s\n' "$1" "$2"; else printf '  FALLA %-44s obtuve [%s] esperaba [%s]\n' "$1" "$2" "$3"; fallos=$((fallos+1)); fi; }

for d in $D1 $D2; do curl -s "$A/dispositivos" | grep -q "\"id\":\"$d\"" || { echo "falta el dispositivo $d: arranca el agente simulado"; exit 1; }; done
echo "== USSD manual"
r=$(post ussd/iniciar "{\"dispositivo\":\"$D1\",\"slot\":0,\"codigo\":\"*105#\"}")
espera "iniciar *105# → 200" "$(codigo "$r")" 200
espera "texto trae el saldo" "$(cuerpo "$r" | campo "'Saldo Principal' in d['texto']")" True
s=$(cuerpo "$r" | campo "d['sesion']")
espera "dispositivo queda ocupado" "$(curl -s $A/dispositivos | campo "[x['ocupado'] for x in d if x['id']=='$D1'][0]")" True
r=$(post ussd/iniciar "{\"dispositivo\":\"$D1\",\"slot\":1,\"codigo\":\"*105#\"}")
espera "segunda sesión → 409 ocupado" "$(codigo "$r")/$(cuerpo "$r" | campo "d['error']['codigo']")" 409/ocupado
espera "cancelar → ok" "$(cuerpo "$(post ussd/cancelar "{\"dispositivo\":\"$D1\",\"sesion\":\"$s\"}")" | campo "d['ok']")" True
r=$(post ussd/iniciar "{\"dispositivo\":\"$D1\",\"slot\":0,\"codigo\":\"*66#1#\"}"); s=$(cuerpo "$r" | campo "d['sesion']")
r=$(post ussd/responder "{\"dispositivo\":\"$D1\",\"sesion\":\"$s\",\"texto\":\"2\"}")
espera "menú → 2 = movimientos (cierra)" "$(cuerpo "$r" | campo "(d['texto'].split(chr(10))[0], d['abierta'])")" "('Ultimos movimientos', False)"
espera "responder sesión cerrada → 404" "$(codigo "$(post ussd/responder "{\"dispositivo\":\"$D1\",\"sesion\":\"$s\",\"texto\":\"1\"}")")" 404
espera "modo api → cerrada" "$(cuerpo "$(post ussd/iniciar "{\"dispositivo\":\"$D1\",\"codigo\":\"*105#\",\"modo\":\"api\"}")" | campo "d['abierta']")" False
espera "código inválido → 400" "$(codigo "$(post ussd/iniciar "{\"dispositivo\":\"$D1\",\"codigo\":\"*105#;x\"}")")" 400
espera "sin dispositivo → 404" "$(codigo "$(post ussd/iniciar '{"dispositivo":"nadie","codigo":"*105#"}')")" 404
r=$(post ussd/iniciar '{"x":1}')
espera "cuerpo malo → 400 parametro" "$(codigo "$r")/$(cuerpo "$r" | campo "d['error']['codigo']")" 400/parametro

echo "== Recetas"
espera "lista de recetas" "$(curl -s $A/recetas | campo "sorted(x['nombre'] for x in d)")" "['entel-transferencias', 'tigo-movimientos', 'tigo-saldo', 'tigo-transferir']"
r=$(post recetas/tigo-saldo/ejecutar "{\"dispositivo\":\"$D1\",\"slot\":0}")
espera "tigo-saldo → ok + salida" "$(cuerpo "$r" | campo "(d['estado'], d['salida'].get('linea'))")" "('ok', '78815232')"
antes=$(cuerpo "$(post recetas/tigo-saldo/ejecutar "{\"dispositivo\":\"$D1\",\"slot\":0}")" | campo "d['salida']['saldo_principal']")
r=$(post recetas/tigo-transferir/ejecutar "{\"dispositivo\":\"$D1\",\"slot\":0,\"parametros\":{\"numero\":\"69869869\",\"monto\":1},\"simular\":true}")
espera "transferir simulado → simulado" "$(cuerpo "$r" | campo "(d['estado'], d['pasos'][-1]['enviado'])")" "('simulado', '2')"
espera "simulado no cambia el saldo" "$(cuerpo "$(post recetas/tigo-saldo/ejecutar "{\"dispositivo\":\"$D1\",\"slot\":0}")" | campo "d['salida']['saldo_principal']")" "$antes"
r=$(post recetas/tigo-transferir/ejecutar "{\"dispositivo\":\"$D1\",\"slot\":0,\"parametros\":{\"numero\":\"69869869\",\"monto\":\"1\"}}")
espera "transferir real → ok, costo 0.20" "$(cuerpo "$r" | campo "(d['estado'], d['salida'].get('costo'))")" "('ok', '0.20')"
espera "saldo bajó 1.20" "$(cuerpo "$(post recetas/tigo-saldo/ejecutar "{\"dispositivo\":\"$D1\",\"slot\":0}")" | campo "round(float(d['salida']['saldo_principal']),2)")" "$(python3 -c "print(round($antes-1.2,2))")"
r=$(post recetas/tigo-transferir/ejecutar "{\"dispositivo\":\"$D2\",\"slot\":0,\"parametros\":{\"numero\":\"69869869\",\"monto\":\"1\"}}")
espera "límite diario → abortado" "$(cuerpo "$r" | campo "(d['estado'], 'maximo' in d['detalle'])")" "('abortado', True)"
r=$(post recetas/tigo-transferir/ejecutar "{\"dispositivo\":\"$D1\",\"slot\":0,\"parametros\":{\"numero\":\"123\",\"monto\":\"1\"}}")
espera "parámetro inválido → error" "$(cuerpo "$r" | campo "d['estado']")" error
r=$(post recetas/entel-transferencias/ejecutar "{\"dispositivo\":\"$D2\",\"slot\":1}")
espera "entel (formato '1 Opcion') → ok" "$(cuerpo "$r" | campo "(d['estado'], d['salida'].get('ultima'))")" "('ok', '59163819601 Bs 1.00 03/09/2026 22:32:12')"
espera "receta inexistente → 404" "$(codigo "$(post recetas/nada/ejecutar "{\"dispositivo\":\"$D1\"}")")" 404
espera "nada quedó ocupado" "$(curl -s $A/dispositivos | campo "any(x['ocupado'] for x in d)")" False

echo "== SMS e historial"
espera "SMS del agente (incluye el de la transferencia)" "$(curl -s "$A/dispositivos/$D1/sms?limite=5" | campo "any('Transferiste' in s['texto'] for s in d)")" True
espera "historial guarda eventos ussd" "$(curl -s "$A/historial?tipo=ussd&limite=5" | campo "len(d)>0 and d[0]['tipo']=='ussd'")" True
espera "ejecuciones guardadas" "$(curl -s "$A/ejecuciones?limite=1" | campo "d[0]['receta']")" entel-transferencias
echo "== Lotes (CSV)"
json_csv() { python3 -c "import sys,json;print(json.dumps(sys.stdin.read()))"; }
esperar_lote() { # id → estado final
  for _ in $(seq 1 300); do e=$(curl -s "$A/lotes/$1" | campo "d['estado']"); [ "$e" != en_curso ] && { echo "$e"; return; }; sleep 0.2; done; echo timeout; }
h=$(curl -s -D - -o /tmp/qhana-plantilla.csv "$A/lotes/plantilla.csv" | tr -d '\r')
espera "plantilla → adjunto csv" "$(echo "$h" | grep -ci 'content-disposition: attachment')" 1
espera "plantilla con BOM y encabezado" "$(head -c 50 /tmp/qhana-plantilla.csv | python3 -c "import sys;t=sys.stdin.buffer.read().decode();print(t.startswith('﻿numero,monto,operador'))")" True
CSV=$(printf '\xef\xbb\xbfTeléfono;Monto (Bs);Operadora;Ref\n+591 6986-9869;12,00;tigo;A\n69869869;1;TIGO;B\n71227468;5;ENTEL;C\n1234;5;TIGO;D\n78815232;12,50;TIGO;E\n' | json_csv)
r=$(post lotes/validar "{\"csv\":$CSV,\"consultar_saldos\":true}")
espera "validar → 200" "$(codigo "$r")" 200
espera "resumen (filas, válidas, avisos, inválidas)" "$(cuerpo "$r" | campo "(d['resumen']['filas'],d['resumen']['validas'],d['resumen']['con_avisos'],d['resumen']['invalidas'])")" "(5, 2, 2, 3)"
espera "12 Bs → envíos [10, 2] con plan" "$(cuerpo "$r" | campo "(d['filas'][0]['envios'], len(d['filas'][0]['plan']))")" "([10, 2], 2)"
espera "ENTEL sin receta de transferir" "$(cuerpo "$r" | campo "any('no hay receta de transferir para ENTEL' in e for e in d['filas'][2]['errores'])")" True
espera "centavos → error" "$(cuerpo "$r" | campo "any('centavos' in e for e in d['filas'][4]['errores'])")" True
espera "saldos consultados (chips TIGO con saldo)" "$(cuerpo "$r" | campo "all(c['saldo'] is not None for c in d['chips'] if c['operador']=='TIGO')")" True
espera "chips: cupo y enviados" "$(curl -s $A/chips | campo "all('cupo_hoy' in c and 'enviados_hoy' in c for c in d) and len(d)>=3")" True
espera "CSV sin columna operador → 400" "$(codigo "$(post lotes/validar '{"csv":"numero,monto\n69869869,5\n"}')")" 400
espera "crear sin filas válidas → 400" "$(codigo "$(post lotes '{"csv":"numero,monto,operador\n1,5,TIGO\n"}')")" 400

# Simulado: recorre todo y responde No.
saldo_d1=$(curl -s $A/chips | campo "[c['saldo'] for c in d if c['dispositivo']=='$D1' and c['slot']==0][0]")
r=$(post lotes "{\"nombre\":\"humo simulado\",\"csv\":$CSV,\"simular\":true}")
L=$(cuerpo "$r" | campo "d['id']")
espera "crear simulado → listo, 3 omitidas" "$(cuerpo "$r" | campo "(d['estado'], d['simular'], d['progreso']['omitidas'])")" "('listo', True, 3)"
espera "iniciar → en_curso" "$(cuerpo "$(post lotes/$L/iniciar '{}')" | campo "d['estado']")" en_curso
espera "simulado termina" "$(esperar_lote $L)" terminado
espera "filas simuladas" "$(curl -s $A/lotes/$L | campo "(d['progreso']['simulado'], d['progreso']['enviado_bs'])")" "(2, 0)"
espera "simulado no tocó el saldo conocido" "$(curl -s $A/chips | campo "[c['saldo'] for c in d if c['dispositivo']=='$D1' and c['slot']==0][0]")" "$saldo_d1"

# Real contra los simulados: el chip de $D2 tiene límite 0 → el operador lo bloquea y se reasigna.
REAL=$(printf 'numero,monto,operador,referencia\n69869869,3,TIGO,R1\n70000000,1,TIGO,R2\n69869869,2,TIGO,R3\n' | json_csv)
L2=$(cuerpo "$(post lotes "{\"nombre\":\"humo real\",\"csv\":$REAL}")" | campo "d['id']")
post lotes/$L2/iniciar '{}' >/dev/null
espera "otro iniciar mientras corre → 409" "$(codigo "$(post lotes/$L2/iniciar '{}')")" 409
espera "real termina" "$(esperar_lote $L2)" terminado
espera "3 filas ok, 6 Bs, 0.80 de comisión" "$(curl -s $A/lotes/$L2 | campo "(d['progreso']['ok'], d['progreso']['enviado_bs'], d['progreso']['comision_bs'])")" "(3, 6, 0.8)"
espera "chip con límite del operador → agotado" "$(curl -s $A/chips | campo "[(c['agotado_hoy'],c['cupo_hoy']) for c in d if c['dispositivo']=='$D2' and c['slot']==0][0]")" "(True, 0)"
espera "nada pendiente → 409" "$(cuerpo "$(post lotes/$L2/iniciar '{}')" | campo "d['error']['codigo']")" nada_pendiente
espera "pausar terminado → 409" "$(codigo "$(post lotes/$L2/pausar '{}')")" 409
csv=$(curl -s "$A/lotes/$L2/resultado.csv")
espera "resultado.csv: encabezado" "$(printf '%s' "$csv" | head -1 | tr -d '\r' | sed 's/^\xef\xbb\xbf//')" "fila,numero,operador,accion,monto,referencia,estado,enviado_bs,comision_bs,envios_ok,envios_total,chips,detalle,hora"
espera "resultado.csv: 3 filas ok" "$(printf '%s' "$csv" | grep -c ',ok,')" 3
espera "resultado.csv con ;" "$(curl -s "$A/lotes/$L2/resultado.csv?separador=;" | sed -n 2p | cut -d';' -f7)" ok
espera "lista de lotes sin filas" "$(curl -s $A/lotes | campo "(len(d)>=2, all('filas' not in l for l in d))")" "(True, True)"
L3=$(cuerpo "$(post lotes "{\"csv\":$REAL}")" | campo "d['id']")
espera "cancelar un lote listo" "$(cuerpo "$(post lotes/$L3/cancelar '{}')" | campo "d['estado']")" cancelado
espera "lote inexistente → 404" "$(codigo "$(curl -s -w '\n%{http_code}' $A/lotes/99999)")" 404
espera "eventos 'lote' en el historial" "$(curl -s "$A/historial?tipo=lote&limite=300" | campo "sorted(set(e['fase'] for e in d))")" "['creado', 'envio', 'fila', 'fin', 'inicio']"
espera "nada quedó ocupado" "$(curl -s $A/dispositivos | campo "any(x['ocupado'] for x in d)")" False

echo; [ $fallos -eq 0 ] && echo "TODO OK" || { echo "$fallos FALLAS"; exit 1; }
