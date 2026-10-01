# cadratctl

`cadratctl` は `cadratd` のフロントエンドである。D-Bus（[daemon/dbus.md](../daemon/dbus.md)）だけを使い、hidrawには触れない。コマンド、引数、出力、終了コードは、なるべく `cadrat-tool` と同じにする。スクリプトでは、`cadrat-tool` を `cadratctl` に置き換えるだけで同じ結果が得られるようにする。

## 1. コマンド

| コマンド | 呼ぶメソッド | `cadrat-tool` との違い |
|---|---|---|
| `list [--nodes] [--redact]` | `List` | なし |
| `init [--preset=research-baseline] [--force]` | `Init` | なし |
| `get [<key>...] [--wire] [-n]` | `Get` | なし |
| `check` | `Check` | なし |
| `set <key>=<value>... [--dry-run \| --no-save]` | `Set` | なし |
| `apply [--dry-run]` | `Apply` | なし |
| `receiver slots [--receiver=<key>] [--redact]` | `ReceiverSlots` | なし |
| `receiver pair [--receiver=<key>] [--timeout=<秒>] [--poll-interval=<秒>]` | `ReceiverPair` | §2 |
| `receiver unpair <slot> [--receiver=<key>] [--yes] [--timeout=<秒>] [--poll-interval=<秒>]` | `ReceiverSlots`、`ReceiverUnpair` | §2 |

- 共通オプションは `cadrat-tool` と同じ（[tool/cli §1](../tool/cli.md#1-共通オプション)）。ただし `--hidraw` は無い。
- `hold-open` は無い。hold-openは `cadrat-hold-open` のシステムサービスが行う（[hold-open/cli.md](../hold-open/cli.md)）。
- `--config` は、`cadratctl` の環境で [config §1](../config.md#1-場所) の規則でパスを決め、絶対パスにしてから渡す。指定が無いときも、既定のパスを絶対パスにして渡す。`cadratd` と `cadratctl` の環境変数が違っても、`cadrat-tool` と同じファイルを使うためである。
- 引数の解析と検証（`set` の `key=value`、同じキーの重複など）は、`cadrat-tool` と同じ規則で `cadratctl` が先に行う。誤りがあればD-Busを呼ばずにUsage（2）で終える。

## 2. Receiverの対話

**pair**
1. シグナル `PairingStarted` を受け取る準備をしてから、`ReceiverPair` を呼ぶ。
2. `PairingStarted` を受けたら、`cadrat-tool` と同じ案内（"put the mouse in pairing mode"）を出す。
3. 待機中にSIGINTかSIGTERMを受けたら `Cancel` を呼び、`ReceiverPair` の応答を待つ。`cadratd` が停止packetを送り、結果（12または13）を返す。
   - `cadratctl` が応答を待たずに終わっても、`cadratd` は接続が消えたことで待機をやめ、停止packetを送る（[daemon §5](../daemon/daemon.md#5-要求の処理)）。

**unpair**
1. `ReceiverSlots` で対象slotを読む。空きなら `SlotChanged`（16）で終える。
2. `cadrat-tool` と同じ内容で確認を取る（[receiver §4](../receiver.md#4-unpair) の手順3）。`--yes`、端末でない場合、`--json` の扱いも同じである。
3. 確認で見せたslotの生の値を `expected` に、手順1の結果にあったReceiverのkeyを `receiver` に入れて、`ReceiverUnpair` を呼ぶ。
   - 確認してから実行するまでにslotが変わっていないかは、`cadratd` が照合する。
4. 手順3の応答を待つ間（slotが空になるのを待つ間）にSIGINTかSIGTERMを受けたら、`cadrat-tool` と同じく、シグナルの既定の動作で終わる。`cadratd` は接続が消えたことで待機をやめる。解除要求はすでに送っているので、結果は `receiver slots` で確かめる（[tool/cli §3](../tool/cli.md#receiver-unpair-slot---receiverkey---yes---timeout秒---poll-interval秒)）。手順3より前に終われば、何も送られない。

## 3. 出力

- `cadratd` が返したJSONから、`cadrat-tool` と同じ人間向けの表示を作る（[tool/cli §5](../tool/cli.md#5-出力)）。`--json` のときは、返ったJSONをそのまま出す。
- 案内の文言に出てくるコマンド名は `cadratctl` にする（例: "run `cadratctl apply` after switching modes"）。
- `--json` のときも、D-Busのエラー（§4の21、22）は外枠のJSONで出す。`error.code` は終了コードの名前とする。
- **`-v`**: `verbose` を付けて呼び、返ったJSONの `nodes` を、`cadrat-tool -v` と同じ形でstderrに出す。stdoutに出すJSONからは `nodes` を除く。ただし `list --nodes` のときは、`cadrat-tool` と同じく `nodes` を残す。これで、`-v` と `--json` を組み合わせても、stdoutとstderrが `cadrat-tool` と同じになる。

## 4. 終了コード

終了コードの表は1つにまとめ、[tool/cli §6](../tool/cli.md#6-終了コード) に置く。`cadratctl` は、`cadratd` が返したJSONの `exit_code` で終える。それに加えて、`cadratctl` だけが次を使う。

| コード | 名前 | 意味 |
|---:|---|---|
| 21 | DaemonUnavailable | `cadratd` に届かない（session busが無い、起動できない、応答が無い、呼び出しを拒否された）。呼び出しのtimeoutの場合は、要求が実行されたかどうか分からないと表示する |
| 22 | Busy | `cadratd` が別の書き込む操作を実行中だった（`Busy`）、または起動の準備中だった（`Starting`）。何もしていない。メッセージに理由（実行中のコマンド名、または `cadrat-tool` の終わりを待っていること）を出す |

- 20（`DaemonRunning`）は `cadrat-tool` だけが使う（[tool/cli §8](../tool/cli.md#8-cadratd-との排他)）。
- D-Busの `InvalidArgs` はUsage（2）、`Internal` はInternal（1）とする。
- シグナルで終わった場合は、シグナルの既定の動作に従う（§2）。

### 4.1 `cadratd` との版のずれ

パッケージを更新しても、動いている `cadratd` は再起動されない（[implementation §7](../implementation.md#7-配布)）。そのため、新しい `cadratctl` が古い `cadratd` を呼ぶことがある。D-Bus APIはPhase 2aでは安定していない（[dbus §1](../daemon/dbus.md#1-名前)）ので、`cadratctl` が次のように扱う。

1. 最初の呼び出しの前に `Version` プロパティを読む。自分の版と違えば、stderrに警告を出して続ける（"cadratd <版> is running, but cadratctl is <版>; run `systemctl --user restart cadratd.service`"）。`--json` のときも、警告はstderrだけに出す。
2. 版が違うときに、呼び出しが `org.freedesktop.DBus.Error.UnknownMethod` か `InvalidArgs` で失敗したら、Usage（2）ではなく `DaemonUnavailable`（21）で終え、1と同じ再起動の案内を出す。版が同じときの `InvalidArgs` は、これまでどおりUsage（2）とする。

## 5. 呼び出しのtimeout

- `ReceiverPair` と `ReceiverUnpair`: 要求したtimeoutに30秒を足した時間。
- その他: 60秒。
