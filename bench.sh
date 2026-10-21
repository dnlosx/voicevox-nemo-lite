#!/bin/bash
# usage: bench.sh <name> <image> <outdir> [extra docker args...]
set -u
NAME=$1; IMAGE=$2; OUT=$3; shift 3
docker rm -f $NAME >/dev/null 2>&1
T0=$(python3 -c 'import time;print(time.time())')
docker run -d --name $NAME --platform linux/amd64 -p 50999:50121 "$@" $IMAGE >/dev/null
q() { curl -s -f -X POST "http://127.0.0.1:50999/audio_query?speaker=10006" --data-urlencode "text=$1" -G -o "$2" -w '%{http_code}'; }
# poll until first full synthesis succeeds
until q "こんにちは" $OUT/q0.json >/dev/null 2>&1 && curl -s -f -X POST "http://127.0.0.1:50999/synthesis?speaker=10006" -H 'Content-Type: application/json' --data-binary @$OUT/q0.json -o $OUT/w0.wav; do sleep 0.2; done
T1=$(python3 -c 'import time;print(time.time())')
echo "cold_start_to_first_synthesis_s=$(python3 -c "print(round($T1-$T0,2))")"
sleep 5; echo "rss_after_first: $(docker stats --no-stream --format '{{.MemUsage}}' $NAME)"
i=1
while IFS= read -r line; do
  q "$line" $OUT/q$i.json >/dev/null
  S0=$(python3 -c 'import time;print(time.time())')
  curl -s -f -X POST "http://127.0.0.1:50999/synthesis?speaker=10006" -H 'Content-Type: application/json' --data-binary @$OUT/q$i.json -o $OUT/w$i.wav
  S1=$(python3 -c 'import time;print(time.time())')
  echo "s$i synth_s=$(python3 -c "print(round($S1-$S0,2))") bytes=$(wc -c < $OUT/w$i.wav)"
  i=$((i+1))
done <<'TXT'
食べる
今日はいい天気ですね。
日本語を勉強しています。
彼は図書館で本を読みましたか？
東京駅から新幹線に乗って京都へ行きました。
TXT
sleep 3; echo "rss_after_syntheses: $(docker stats --no-stream --format '{{.MemUsage}}' $NAME)"
docker top $NAME -o pid,rss,args | head -5
