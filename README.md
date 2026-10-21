# voicevox-nemo-lite

[VOICEVOX Nemo ENGINE](https://github.com/VOICEVOX/voicevox_nemo_engine) の
`POST /audio_query` と `POST /synthesis` だけを提供する、軽量な互換サーバーです。
Rust（axum）製で、公式の VOICEVOX Nemo コア（C API 0.15.0）を呼び出して音声を合成します。

「テキストを送って WAV を受け取る」用途で、公式エンジンの代わりにそのまま置けます。
公式イメージと同じ音声を出力しつつ、メモリ使用量とイメージサイズを小さくすることが目的です。

## 特長

- 公式エンジンと同じ形式の AudioQuery JSON と WAV（16bit PCM・モノラル・24kHz）を返します
- 起動時に読み込むのは指定した話者のモデルだけです（初期値は 10006 = 女声6）
- Python を使わないため、イメージは約 4 分の 1、起動後のメモリは約 25〜30% 小さくなります
- 計測は下の「計測結果」を参照してください

## エンドポイント

| メソッド | パス | 内容 |
|---|---|---|
| `POST` | `/audio_query?text=…&speaker=10006` | テキストから AudioQuery（JSON）を作成 |
| `POST` | `/synthesis?speaker=10006` | AudioQuery（リクエストボディ）から WAV を合成。`enable_interrogative_upspeak`（初期値 `true`）も指定できます |
| `GET` | `/version` | バージョン（ヘルスチェック用） |

公式エンジンのその他のエンドポイント（ユーザー辞書、プリセット、モーフィング、`/speakers` など）はありません。

## 使い方

```bash
docker build --platform linux/amd64 -t voicevox-nemo-lite .
docker run --rm --platform linux/amd64 -p 50121:50121 voicevox-nemo-lite

curl -s -X POST "http://localhost:50121/audio_query?speaker=10006" \
  --get --data-urlencode "text=こんにちは" -o query.json
curl -s -X POST "http://localhost:50121/synthesis?speaker=10006" \
  -H "Content-Type: application/json" --data-binary @query.json -o out.wav
```

### 環境変数

| 変数 | 初期値 | 内容 |
|---|---|---|
| `PORT` | `50121` | 待ち受けポート |
| `HOST` | `0.0.0.0` | 待ち受けアドレス |
| `SPEAKERS` | `10006` | 起動時に読み込む話者 ID（カンマ区切り） |
| `CPU_NUM_THREADS` | `1` | 推論スレッド数。`0` で自動。速度を優先する場合は `0` を推奨 |
| `OPEN_JTALK_DICT_DIR` | `/opt/open_jtalk_dic_utf_8-1.11` | OpenJTalk 辞書のディレクトリ |

### Railway へのデプロイ

`railway.json` で Dockerfile ビルドと `/version` へのヘルスチェックを設定済みです。
Railway でこのリポジトリから新しいサービスを作成し、必要なら `CPU_NUM_THREADS=0` を設定してください。
Railway が割り当てる `PORT` をそのまま使います。

## しくみ

Nemo の音声モデルは公開版の `voicevox_core`（0.16）向けには配布されておらず、
暗号化された状態で VOICEVOX Nemo コア（`libvoicevox_core.so`）にのみ同梱されています。
そのため本サーバーは Rust 版 `voicevox_core` クレートではなく、
Nemo コアの公開 C API（`voicevox_core.h`）を FFI で呼び出します。
公式エンジンも同じライブラリを Python から呼び出しています。

Nemo コア・ONNX Runtime・モデル・OpenJTalk 辞書は**このリポジトリには含まれていません**。
Docker ビルド時に、ダイジェストで固定した公式イメージ
`voicevox/voicevox_nemo_engine` からコピーします。
公式イメージが linux/amd64 のみのため、本イメージも linux/amd64 専用です。

## 計測結果

Apple Silicon の Mac 上で、Docker Desktop（linux/amd64 を Rosetta でエミュレーション）を使って計測しました。
絶対値は x86_64 のサーバーとは異なりますが、両イメージとも同じ条件です。
メモリはコンテナのメモリ使用量（`docker stats`）です。
計測スクリプトは `bench.sh` です。

| | 公式 `voicevox_nemo_engine` | voicevox-nemo-lite（1 スレッド） | voicevox-nemo-lite（`CPU_NUM_THREADS=0`） |
|---|---|---|---|
| イメージサイズ（展開後 / 圧縮） | 1.73 GB / 484 MB | **418 MB / 133 MB** | 同左 |
| 起動から最初の合成完了まで | 5.3 秒 | 6.1 秒 | **2.5 秒** |
| 最初の合成後のメモリ | 277 MiB | **191 MiB** | 194 MiB |
| さらに 5 文合成した後のメモリ | 359 MiB | **274 MiB** | 276 MiB |
| 合成時間（「食べる」/ 22 モーラの文） | 1.2 / 4.3 秒 | 3.2 / 8.1 秒 | **1.0 / 2.9 秒** |

### 出力の同一性

6 文で公式エンジンと比較しました。

- `audio_query`：読み・アクセント・ポーズはすべて一致しました。モーラの長さと音高の差は 2.4e-7 以下で、浮動小数点の丸め誤差の範囲です
- `synthesis`：WAV の形式とバイト長が一致しました。サンプル値の差は最大 10/32768（−70 dB）で、聞き分けられません

既知の違いとして、公式エンジンが読み込む既定のユーザー辞書（VOICEVOX のキャラクター名 79 語）は使いません。
コア 0.15 の C API にユーザー辞書の機能がないためです。
また、`pauseLength` と `pauseLengthScale` は受け付けますが、合成には反映しません。

## ライセンスと利用規約

- 本リポジトリのソースコード：MIT License（`LICENSE`）
- VOICEVOX Nemo コア：[VOICEVOX Nemo コアライブラリ利用規約](https://github.com/VOICEVOX/voicevox_nemo_core)
  （アプリケーションに組み込んでの再配布可。逆コンパイル・リバースエンジニアリングは禁止。
  イメージ内の `/opt/voicevox_core/TERMS.txt` に同梱されています）
- 音声ライブラリ：[VOICEVOX Nemo 利用規約](https://voicevox.hiroshiba.jp/nemo/term/)
- ONNX Runtime：MIT License / Open JTalk・辞書：修正 BSD ライセンス

**生成した音声を使用する際は「VOICEVOX Nemo」とクレジットを表記してください。**
機械学習への利用など、利用規約で禁止されている用途には使用できません。

本プロジェクトは非公式であり、VOICEVOX の開発者とは関係ありません。
