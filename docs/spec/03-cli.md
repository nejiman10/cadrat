# 03 CLI

## 1. 共通オプション

| オプション | 意味 |
|---|---|
| `--config=<path>` | 設定ファイルのパス（[01 §1](01-config.md#1-場所)） |
| `--mouse=<selector>` | 対象のマウス（[02 §6](02-device.md#6-マウスの選択)） |
| `--route=<wired\|receiver>` | 有効な経路の代わりに、指定した経路へ送る（[02 §6](02-device.md#6-マウスの選択)） |
| `--hidraw=<path>` | 開発者向け。送信先nodeを直接指定する（判定は飛ばさない） |
| `--json` | stdoutに機械可読なJSONを1つだけ出す |
| `-v`, `--verbose` | 詳細表示（検出の過程、Receiverの注記など） |
| `-q`, `--quiet` | 警告以外の情報を出さない |

- オプションは `--name=value` と `--name value` のどちらでも受け付ける。文書では `=` 形式で書く。
- 結果はstdoutに、警告とエラーはstderrに出す。`--json` のときもstderrは人間向けのままにする。
- 対話的な確認をするのは `receiver unpair` だけとする（[05 §4](05-receiver.md#4-unpair)）。それ以外のコマンドは、スクリプトからそのまま使える。

## 2. 設定キーと値の書き方

`get` / `set` のキーは、TOMLのパスをそのまま使う。形式はsysctlに倣い、`key=value` で書く。

| key | 値 |
|---|---|
| `mouse.dpi` | `50`..`8200`（50刻み） |
| `mouse.polling_rate` | `125` / `250` / `500` / `1000` |
| `mouse.wheel` | `normal` / `inertial` |
| `mouse.lift.enabled` | `true` / `false` |
| `mouse.lift.threshold` | `0`..`255`（`0x..` も可） |
| `buttons.left` / `.right` / `.middle` / `.wheel` / `.forward` / `.back` / `.radial` | actionの書き方（[01 §5](01-config.md#5-action)）。例: `mouse:left`、`host:1` |

- 引数は最初の `=` で分ける。キーの側に空白や `=` は含まない。
- 文字列の値は引用符なしで書ける（`mouse.wheel=inertial`）。
- 1回の呼び出しで同じキーを2回指定したら、使い方の誤り（終了コード2）にする。

## 3. コマンド

### `list [--nodes] [--redact]`

接続中のマウスとReceiverを列挙する。送信はしない。読み取り要求（probe、slot読み取り）は行う（[02 §3](02-device.md#3-列挙の手順)）。

```
$ cadrat-tool list
#  MOUSE              ACTIVE    ROUTES
1  c658:0a1b2c3d4e5f  wired     wired (/dev/hidraw5, MI_01)
                                receiver standby (recv:port-3-2 slot 3, /dev/hidraw9, MI_03)

RECEIVER        SLOTS  MANAGEMENT
recv:port-3-2   1/5    /dev/hidraw6 (MI_00)
```

1台のマウスが両方の経路で見えている場合も、1行にまとめて表示する。

- `--nodes`: hidraw nodeごとの判定結果（`candidate` / `rejected` と理由 / `inaccessible`）を表示する。開発と不具合調査に使う。
- `--redact`: 機器IDとslotの識別子を伏せて表示する（issueに貼るとき向け）。

### `init [--preset=research-baseline] [--force]`

設定ファイルのテンプレートを作る（[01 §8](01-config.md#8-init-が作るテンプレート)）。デバイスには触れない。

### `get [<key>...] [--wire] [-n]`

TOMLの値を `key=value` 形式で表示する。デバイスには触れない。

```
$ cadrat-tool get
mouse.dpi=1600
mouse.polling_rate=1000
mouse.wheel=normal
mouse.lift.enabled=false
mouse.lift.threshold=31
buttons.left=mouse:left
…
buttons.radial=host:1

$ cadrat-tool get mouse.dpi buttons.radial
mouse.dpi=1600
buttons.radial=host:1

$ cadrat-tool get -n mouse.dpi
1600
```

- キーを省略すると全キーを表示する。
- `-n`: 値だけを表示する（sysctl `-n` と同じ）。
- `--wire`: TOMLから作った32-byteのwire reportと、各フィールドとの対応も表示する。
- TOMLが不完全な場合、分かる範囲の値と欠けたキーを表示し、終了コード3で終える。

### `check`

TOMLを検証し、警告を表示する。デバイスには触れない。

### `set <key>=<value>... [--dry-run | --no-save]`

TOMLの値を部分的に変え、完全なsnapshotとして送信し、成功したら保存する。

```
$ cadrat-tool set mouse.dpi=1000 buttons.radial=host:1
```

- 変更はまとめて1回だけ送る。
- `--dry-run`: 送信も保存もしない。変更後のwire reportとフィールドの対応、変更箇所を表示する。**デバイスI/Oは一切しない。**
- `--no-save`: 送信だけして保存しない。成功したら警告 `W-NOT-SAVED` を出す（「マウスの設定とTOMLが食い違っています。`cadrat-tool apply` で戻せます」）。
- `--dry-run` と `--no-save` は同時に指定できない。

### `apply [--dry-run]`

TOMLの内容をそのまま送る。TOMLは変更しない。再接続後や `--no-save` の後に戻すとき、`--config` で別のファイルを送るときに使う。

### `receiver slots [--redact]`

Receiverのslot 0..4の状態を表示する。読み取りだけを行う（[05 §2](05-receiver.md#2-slotの読み取り)）。Receiverが2台以上あれば `--receiver=<key>` で選ぶ。

### `receiver pair [--receiver=<key>] [--timeout=<秒>] [--poll-interval=<秒>]`

pairing modeを開始し、新しいslotが占有されるまで待つ。終了時には必ず停止する（[05 §3](05-receiver.md#3-pair)）。

### `receiver unpair <slot> [--receiver=<key>] [--yes] [--timeout=<秒>] [--poll-interval=<秒>]`

対象slotの内容を表示して確認を取り、解除し、slotが空になったことで成否を判定する（[05 §4](05-receiver.md#4-unpair)）。

### 後のPhaseで追加するコマンド

`monitor`（Report `0x03` / `0x17`）。

### 削除したもの

- `decode <hex>`: CLIは状態を持たないので、wire reportはTOMLから作る以外に手に入らない。wire reportは `get --wire` と `set --dry-run` で表示する。任意のbyte列の解析は調査リポジトリの役目とする。
- `show`: `get` に統合した。
- `button`: `set buttons.<name>=...` と重複するため削除した。

## 4. `set` の処理順序

```
 1. 引数を解析して検証する                        失敗 → 2
 2. <config>.lock を取る                          失敗 → 11
 3. TOMLを読み、SHA-256 を H0 として控える         失敗 → 3
 4. schema と全キーを検証する                     失敗 → 3
 5. 変更を反映した新しい設定を作り、検証する       失敗 → 3
 6. 新しい設定からwire reportを作り、警告を表示する
    ── --dry-run ならここで結果を表示して終了（0）
 7. マウスを検出・選択する                        失敗 → 4 / 5 / 6 / 7
 8. 送信する                                      失敗 → 8（TOMLは変更しない）
    ── --no-save ならW-NOT-SAVEDを出して終了（0）
 9. TOMLを読み直し、SHA-256 が H0 と同じか確かめる  違う → 9
10. toml_edit で変更したキーだけを書き換え、原子的に保存する  失敗 → 9
11. ロックを解放し、結果を表示して終了（0）
```

終了コード9（送信済み・未保存）のときは、stderrに次を表示する。
- 送信したwire reportのhex
- TOMLに反映すべき変更（`key=value` の形）
- 対処方法: TOMLを手で直すか、`set` を再実行する

`apply` は、上の手順から5と9〜10を除いたものになる。

## 5. 出力

### 人間向け（`set` の成功例）

```
mouse   1  c658:0a1b2c3d4e5f  via wired (MI_01)
change  mouse.dpi=1600 → 1000
sent    10 00 14 1f 01 ff 00 00 00 … 1e 00 00 00 01
saved   ~/.config/cadrat/default.toml
note    receiver経路は待機中です。モードを切り替えたら `cadrat-tool apply` を実行してください
```

### `--json`

すべてのコマンドが、共通の外枠を持つオブジェクトを1つ出す。

```json
{
  "format": 1,
  "command": "set",
  "ok": true,
  "exit_code": 0,
  "mouse": {"key": "c658:0a1b2c3d4e5f", "active_route": "wired",
            "sent_via": {"route": "wired", "node": "/dev/hidraw5", "interface": 1},
            "routes": [{"route": "wired", "state": "active", "node": "/dev/hidraw5", "interface": 1},
                       {"route": "receiver", "state": "standby", "receiver": "recv:port-3-2",
                        "slot": 3, "node": "/dev/hidraw9", "interface": 3}]},
  "changes": [{"key": "mouse.dpi", "from": 1600, "to": 1000}],
  "wire_hex": "1000141f01ff…",
  "sent": true,
  "saved": true,
  "warnings": [{"code": "W-HOST-UNOBSERVABLE", "message": "…"}],
  "error": null
}
```

- エラーのときは `"ok": false`、`"error": {"code": "AmbiguousTarget", "message": "…", "details": {…}}` とする。
- `list --json` は、`mice`、`receivers`（`--nodes` 指定時は `nodes` も）を配列で出す。デーモンの前準備として、この構造を[02 §2](02-device.md#2-デバイスモデル)のモデルと一致させる。
- `format` はJSON出力の形式バージョン。フィールドを足すときは据え置き、互換性を壊すときだけ上げる。

## 6. 終了コード

| コード | 名前 | 意味 |
|---:|---|---|
| 0 | Success | 成功（警告があっても0） |
| 1 | Internal | 想定外の内部エラー |
| 2 | Usage | 引数の誤り。`receiver unpair` を端末以外から `--yes` なしで実行した場合も含む |
| 3 | ConfigError | TOMLが無い、構文エラー、`ConfigInvalid`、`ConfigIncomplete`、schemaの不一致 |
| 4 | NoDevice | 対象のマウスまたはReceiverが0台 |
| 5 | AmbiguousTarget | 対象が複数あり、1台に決まらない |
| 6 | PermissionDenied | 対象になり得るnodeを開けない |
| 7 | DeviceInvalid | `--hidraw` で指定したnodeが判定に通らない、または対象が `ambiguous-node` |
| 8 | SendFailed | 送信の失敗（TOMLは変更していない） |
| 9 | SentNotSaved | 送信は成功したが、TOMLを保存できなかった、またはTOMLが同時に変更されていた |
| 10 | IoError | 上記以外のファイルI/Oエラー（`init` の書き込み失敗など） |
| 11 | ConfigLocked | 設定ファイルのロックを取れなかった |
| 12 | PairTimeout | pairで、timeoutまでに新しいslotが占有されなかった（停止は成功） |
| 13 | PairStopFailed | pairing modeの停止に失敗した（pairing modeが続いている可能性がある） |
| 14 | ReceiverCommandFailed | pair開始またはunpairのSETが失敗した（unpairのEPIPEは除く） |
| 15 | UnpairNotConfirmed | unpairで、timeoutまでにslotが空にならなかった |
| 16 | SlotChanged | unpair対象のslotが空、または確認を表示してから実行するまでに内容が変わった（何もしていない） |
| 17 | ReceiverProtocolError | slot応答の長さやReport IDが想定と違う |
| 18 | Aborted | 確認プロンプトで拒否された |
| 19 | TargetChanged | 送信直前の宛先確認で、機器IDが一致しなかった（送信していない） |

## 7. 警告コード

| コード | 条件 |
|---|---|
| `W-LIFT-EXPERIMENTAL` | lift enabled を送る |
| `W-LIFT-AMBIGUOUS` | enabled = true かつ threshold = 31 |
| `W-UNKNOWN-6` | `unknown:6` を送る |
| `W-HOST-UNOBSERVABLE` | `host:N` のNが1..7以外 |
| `W-RAW` | `raw:` を送る |
| `W-NOT-SAVED` | `--no-save` で送信した |
| `W-PAIR-MULTIPLE` | pairで新たに占有されたslotが2つ以上 |
| `W-UNPAIR-EPIPE` | unpairのSETがEPIPEを返した（slotの確認は続ける） |
| `W-NO-DEVICE-ID` | 有線の設定nodeで機器IDが取れず、退避keyを使った |
| `W-SLOT-IF-MISMATCH` | Receiverの設定nodeのinterface番号が、機器IDで対応付けたslot番号と一致しない |
| `W-INACTIVE-ROUTE` | `--route` で待機中（standby）の経路へ送った |

警告は送信を止めない。止める設定（`--deny-warnings`）を用意するかは未決（[Q6](04-implementation.md#6-未決事項)）。
