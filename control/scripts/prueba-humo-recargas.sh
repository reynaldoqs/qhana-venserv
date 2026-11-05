#!/usr/bin/env bash
# Prueba de humo de montos con centavos, recargas dealer por operador, lotes desde chips elegidos, carriles,
# verificación por tipo de saldo y plantillas. SOLO contra agentes simulados: hace recargas (el script se niega
# a correr si hay un dispositivo que no es simulado).
#
#   T=$(mktemp -d); cp -R recetas "$T/recetas"
#   ./target/debug/qhana-control --escuchar 127.0.0.1:47940 --db "$T/c.db" --recetas "$T/recetas" --sin-adb --sin-modems &
#   ./target/debug/agente-simulado --url ws://127.0.0.1:47940/agente --id smk-1 --pin-dealer 4321 --saldo-dealer 50 \
#       --numero1 71110001 --numero2 71110002 --retardo-ms 50 &
#   ./target/debug/agente-simulado --url ws://127.0.0.1:47940/agente --id smk-2 --chip2 entel --entel-dealer \
#       --saldo-dealer-entel 20 --numero1 71110003 --numero-entel 71110004 --retardo-ms 50 &
#   A=http://127.0.0.1:47940/api scripts/prueba-humo-recargas.sh
set -u
A=${A:?"define A, p. ej. A=http://127.0.0.1:47940/api (nunca el control con teléfonos reales)"}
# D1: chip 1 Tigo dealer (con PIN) y chip 2 Tigo normal. D2: chip 2 (ranura 1) Entel dealer.
D1=${D1:-smk-1}; D2=${D2:-smk-2}
fallos=0
post() { curl -s -w '\n%{http_code}' -H 'content-type: application/json' -d "$2" "$A/$1"; }
put() { curl -s -w '\n%{http_code}' -X PUT -H 'content-type: application/json' -d "$2" "$A/$1"; }
# Como post, pero reintenta mientras el teléfono esté ocupado (p. ej. el control consultando el saldo de un chip nuevo).
post_libre() { local r; for _ in $(seq 1 30); do r=$(post "$1" "$2"); [ "$(codigo "$r")" != 409 ] && break; sleep 0.5; done; printf '%s' "$r"; }
patch() { curl -s -w '\n%{http_code}' -X PATCH -H 'content-type: application/json' -d "$2" "$A/$1"; }
cuerpo() { printf '%s' "$1" | sed '$d'; }
codigo() { printf '%s' "$1" | tail -n1; }
# `campo` evalúa expresiones escritas en este script (no datos de la API), igual que prueba-humo.sh.
campo() { python3 -c "import sys,json;d=json.load(sys.stdin);print(eval(sys.argv[1],{},{'d':d}))" "$1"; }
espera() {
  if [ "$2" == "$3" ]; then printf '  ok   %-50s %s\n' "$1" "$2"; else printf '  FALLA %-49s obtuve [%s] esperaba [%s]\n' "$1" "$2" "$3"; fallos=$((fallos+1)); fi; }
json() { python3 -c "import json,sys;print(json.dumps(sys.argv[1]))" "$1"; }

# Seguridad: todos los dispositivos deben ser simulados.
reales=$(curl -s "$A/dispositivos" | campo "len([x for x in d if x.get('fabricante') != 'SIMULADO'])")
[ "$reales" == "0" ] || { echo "hay $reales dispositivo(s) que no son simulados: no corro (hace recargas)"; exit 1; }
for d in $D1 $D2; do curl -s "$A/dispositivos" | grep -q "\"id\":\"$d\"" || { echo "falta el dispositivo $d"; exit 1; }; done

echo "== chips dealer"
espera "smk-1/0 → dealer" "$(cuerpo "$(patch "chips/$D1/0" '{"tipo":"dealer"}')" | campo "d['tipo']")" dealer
espera "smk-1/0 PIN" "$(cuerpo "$(put "chips/$D1/0/pin" '{"pin":"4321"}')" | campo "d['tiene_pin']")" True
# El nombre viejo `pda` se acepta y queda `dealer` (ver "Compatibilidad" en el protocolo).
espera "smk-2/1 (Entel) → pda (alias) = dealer" "$(cuerpo "$(patch "chips/$D2/1" '{"tipo":"pda"}')" | campo "d['tipo']")" dealer
espera "chips: nadie dice pda" "$(curl -s "$A/chips" | campo "sorted(set(c['tipo'] for c in d))")" "['dealer', 'normal']"

echo "== montos: formato base y valores calculados"
r=$(post_libre recetas/entel-dealer-recargar/ejecutar "{\"dispositivo\":\"$D2\",\"slot\":1,\"simular\":true,\"parametros\":{\"numero\":\"71234567\",\"monto\":\"0,5\"}}")
espera "entel-dealer 0,5 simulado" "$(cuerpo "$r" | campo "d['estado']")" simulado
espera "marca *133*N*050#" "$(cuerpo "$r" | campo "d['pasos'][0]['enviado']")" "*133*71234567*050#"
# Por el nombre anterior de la receta (`nombres_anteriores`): corre la renombrada.
r=$(post_libre recetas/entel-pda-recargar/ejecutar "{\"dispositivo\":\"$D2\",\"slot\":1,\"simular\":true,\"parametros\":{\"numero\":\"71234567\",\"monto\":\"50\"}}")
espera "monto 50 (no es opción) → error sin marcar" "$(cuerpo "$r" | campo "(d['estado'], len(d['pasos']))")" "('error', 0)"
espera "nombre anterior → receta renombrada" "$(cuerpo "$r" | campo "d['receta']")" entel-dealer-recargar
espera "versiones con el historial del nombre anterior" "$(curl -s "$A/recetas/tigo-dealer-recargar/versiones" | campo "any(v['id'].endswith('_tigo-pda-recargar.yaml') for v in d)")" True
Y=$(json "$(cat "${RECETAS:-recetas}/entel-dealer-recargar.yaml" 2>/dev/null || curl -s "$A/recetas/entel-dealer-recargar" | campo "d['yaml']")")
r=$(post recetas/probar-paso "{\"yaml\":$Y,\"paso\":1,\"texto\":\"Esta seguro de recargar Bs. 0.50 al telefono 71234567\",\"parametros\":{\"numero\":\"71234567\",\"monto\":\".50\"}}")
espera "probar-paso enviaria" "$(cuerpo "$r" | campo "d['enviaria']")" "*133*71234567*050#"
r=$(post recetas/validar "{\"yaml\":$(json "nombre: x
parametros:
  - { nombre: monto }
  - { nombre: monto_cent }
pasos:
  - marcar: '*1*{monto_cent3}#'
")}")
espera "monto_cent declarado → error reservado" "$(cuerpo "$r" | campo "any('reservado' in e['mensaje'] for e in d['errores'])")" True
espera "sin aviso 'no declarado' por monto_cent3" "$(cuerpo "$r" | campo "any('monto_cent3' in a['mensaje'] for a in d['avisos'])")" False

echo "== plantillas"
p=$(curl -s "$A/lotes/plantilla.csv?tipo=recargas&separador=;")
espera "plantilla recargas con ;" "$(printf '%s' "$p" | sed -n 2p | tr -d '\r')" "71234567;0.50;ENTEL;recargar;Cliente 1"
espera "plantilla chips encabezado" "$(curl -s "$A/lotes/plantilla.csv?tipo=chips" | head -1 | tr -d '\r' | sed 's/^\xef\xbb\xbf//')" "numero,monto,operador,referencia"
espera "plantilla desconocida → 400" "$(curl -s -o /dev/null -w '%{http_code}' "$A/lotes/plantilla.csv?tipo=otra")" 400

echo "== lote por operador con recetas solo dealer"
CSV=$(json "numero,monto,operador,accion,referencia
69869869,1.50,TIGO,recargar,A
71234567,0.50,ENTEL,recargar,B
71234568,1.50,ENTEL,recargar,C")
r=$(post lotes/validar "{\"csv\":$CSV}")
espera "todas válidas" "$(cuerpo "$r" | campo "d['resumen']['invalidas']")" 0
espera "envíos 1 + 1 + 3" "$(cuerpo "$r" | campo "[len(f['envios']) for f in d['filas']]")" "[1, 1, 3]"
espera "plan solo en chips dealer (ranura 0 y la Entel)" "$(cuerpo "$r" | campo "sorted(set((p['dispositivo'],p['slot']) for f in d['filas'] for p in f['plan']))")" "[('$D1', 0), ('$D2', 1)]"
r=$(post lotes "{\"nombre\":\"humo recargas\",\"csv\":$CSV,\"simular\":false,\"forzar\":true}")
L=$(cuerpo "$r" | campo "d['id']")
post "lotes/$L/iniciar" '{}' >/dev/null
enviando=0
for _ in $(seq 1 100); do
  c=$(curl -s "$A/lotes/$L/carriles")
  n=$(printf '%s' "$c" | campo "len([x for x in d['carriles'] if x['estado']=='enviando'])")
  [ "$n" -gt "$enviando" ] && enviando=$n
  [ "$(curl -s "$A/lotes/$L?filas=false" | campo "d['estado']")" != "en_curso" ] && break
  sleep 0.2
done
espera "carriles: dos teléfonos enviando a la vez" "$enviando" 2
espera "lote terminado y completo" "$(curl -s "$A/lotes/$L?filas=false" | campo "(d['estado'], d['revision']['completo'])")" "('terminado', True)"
c=$(curl -s "$A/lotes/$L/carriles")
espera "carril Entel: 4 ok, 2 Bs" "$(printf '%s' "$c" | campo "[(x['ok'], x['enviado_bs']) for x in d['carriles'] if x['dispositivo']=='$D2' and x['slot']==1][0]")" "(4, 2.0)"
espera "carril chip normal excluido" "$(printf '%s' "$c" | campo "[x['estado'] for x in d['carriles'] if x['dispositivo']=='$D1' and x['slot']==1][0]")" excluido
espera "carriles: claves del contrato" "$(printf '%s' "$c" | campo "sorted(d.keys())")" "['carriles', 'eta_ms', 'pendientes']"
v=$(cuerpo "$(post "lotes/$L/verificar" '{}')")
espera "verificar: saldo dealer" "$(printf '%s' "$v" | campo "d['saldo']")" dealer
espera "Tigo dealer cuadra" "$(printf '%s' "$v" | campo "[x['estado'] for x in d['chips'] if x['dispositivo']=='$D1'][0]")" cuadra
espera "Entel dealer sin_dato (sin receta saldo_dealer)" "$(printf '%s' "$v" | campo "[x['estado'] for x in d['chips'] if x['dispositivo']=='$D2'][0]")" sin_dato

echo "== lote desde chips elegidos"
O="{\"chips\":[{\"dispositivo\":\"$D1\",\"slot\":0},{\"dispositivo\":\"$D2\",\"slot\":1},{\"dispositivo\":\"$D1\",\"slot\":1}],\"accion\":\"recargar\"}"
CSV2=$(json "numero,monto,operador,referencia
69869869,0.20,TIGO,A
71234567,0.50,ENTEL,B")
r=$(post lotes/validar "{\"csv\":$CSV2,\"origen\":$O}")
espera "avisos_origen: el chip normal" "$(cuerpo "$r" | campo "len(d['avisos_origen'])")" 1
espera "origen.chips con receta" "$(cuerpo "$r" | campo "[c['receta'] for c in d['origen']['chips']]")" "['tigo-dealer-recargar', 'entel-dealer-recargar']"
r=$(post lotes "{\"nombre\":\"humo chips\",\"csv\":$CSV2,\"simular\":true,\"origen\":$O}")
L2=$(cuerpo "$r" | campo "d['id']")
espera "Lote.origen accion" "$(cuerpo "$r" | campo "d['origen']['accion']")" recargar
post "lotes/$L2/iniciar" '{}' >/dev/null
for _ in $(seq 1 100); do [ "$(curl -s "$A/lotes/$L2?filas=false" | campo "d['estado']")" != "en_curso" ] && break; sleep 0.2; done
espera "simulación completa" "$(curl -s "$A/lotes/$L2?filas=false" | campo "(d['estado'], d['progreso']['simulado'])")" "('terminado', 2)"
r=$(post lotes/validar "{\"csv\":$CSV2,\"origen\":{\"chips\":[{\"dispositivo\":\"$D1\",\"slot\":1}],\"accion\":\"recargar\"}}")
espera "ningún chip sirve → 400" "$(codigo "$r")" 400

echo
if [ $fallos -eq 0 ]; then echo "todo bien"; else echo "$fallos falla(s)"; exit 1; fi
