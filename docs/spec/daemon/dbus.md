# cadratd の D-Bus API

## 1. 名前

| 項目 | 値 |
|---|---|
| bus | session bus（ユーザーのbus） |
| bus名 | `cc.nejiman10.Cadrat1` |
| object path | `/cc/nejiman10/Cadrat1` |
| interface | `cc.nejiman10.Cadrat1.Manager` |
| エラー名の接頭辞 | `cc.nejiman10.Cadrat1.Error.` |

- 名前は、所有者のドメイン `nejiman10.cc` を逆順にしたものである。
- 末尾の `1` はAPIの大きな版である。互換性を壊す変更をするときは `Cadrat2` として並べて置く。メソッド、引数のkey、JSONのフィールドを足すだけなら上げない。
- Phase 2aでは、objectとinterfaceは1つだけとする。マウスごとのobjectは作らない。マウスのkeyは `:` を含み、object pathにするには変換が要ること、keyの集合は列挙のたびに変わることによる。

## 2. 方針

- **1つのメソッドが、`cadrat-tool` の1つのコマンドに当たる。** 手順、判定、終了コード、警告は同じである（[daemon §5](daemon.md#5-要求の処理)）。
- **引数は `a{sv}`（オプションの辞書）で渡す。** keyは §3 と §6 の表に従う。知らないkeyや、型の違う値は `org.freedesktop.DBus.Error.InvalidArgs` にする。後でkeyを足しても、古い呼び出し側は壊れない。
- **結果は、`cadrat-tool --json` と同じJSONの文字列（`s`）で返す。** 外枠（`format`、`command`、`ok`、`exit_code`、`warnings`、`error`）と中身は [tool/cli §5](../tool/cli.md#5-出力) と同じである。
  - 理由: コマンドの失敗（`SentNotSaved` での送ったwireと未保存の変更、unpairの前後のslotなど）にも、表示すべき中身がある。D-Busのエラーはメッセージの文字列しか運べない。
  - コマンドとしての失敗（[tool/cli §6](../tool/cli.md#6-終了コード) の終了コード2〜19）は、通常の応答として `"ok": false` のJSONで返す。D-Busのエラーは、要求をコマンドとして処理できなかったときだけに使う（§5）。
  - JSONには、人間向けの表示に要る情報をすべて載せる。`cadratctl` はJSONから `cadrat-tool` と同じ表示を作る（[ctl/cli §3](../ctl/cli.md#3-出力)）。
- 呼び出し側は、同じユーザーでなければならない。session busの既定の方針に加えて、`cadratd` は呼び出し元のuid（`GetConnectionUnixUser`）が自分と同じかを確かめ、違えば `org.freedesktop.DBus.Error.AccessDenied` にする。

## 3. 共通の引数

| key | 型 | 意味 | 対応する `cadrat-tool` のオプション |
|---|---|---|---|
| `config` | `s` | 設定ファイルの**絶対パス**。省略すると、`cadratd` の環境で [config §1](../config.md#1-場所) の既定のパスを使う | `--config` |
| `mouse` | `s` | マウスのselector（番号、key、接頭辞）。省略は「指定なし」 | `--mouse` |
| `route` | `s` | `wired` / `receiver` | `--route` |
| `receiver` | `s` | Receiverのkeyまたは接頭辞 | `--receiver` |
| `verbose` | `b` | nodeごとの判定結果（`nodes`）もJSONに載せる | `-v` |
| `redact` | `b` | 機器IDとslotの識別子を伏せる | `--redact` |

- 相対パスの `config` は `InvalidArgs` にする。`cadratd` の作業ディレクトリは呼び出し側と違うためである。`cadratctl` は自分の環境でパスを決め、絶対パスにして渡す。
- `cadrat-tool` の `--hidraw` に当たるkeyは無い（[daemon §5](daemon.md#5-要求の処理)）。

## 4. プロパティとシグナル

| 名前 | 型 | 内容 |
|---|---|---|
| `Version` | `s`（読み取り専用） | `cadratd` の版（例: `0.2.0`） |
| `Devices` | `s`（読み取り専用） | 最後の列挙結果。`list --json` の `mice` と `receivers` を持つJSON。機器IDは伏せない |
| `Busy` | `s`（読み取り専用） | 実行中のデバイス操作のコマンド名（例: `receiver pair`）。無ければ空文字列 |

- `Devices` と `Busy` が変わったら、`org.freedesktop.DBus.Properties.PropertiesChanged` を値付きで出す。
- シグナル `PairingStarted(s receiver)`: `ReceiverPair` がpairingの開始（[receiver §3](../receiver.md#3-pair) の手順4）に成功したときに出す。引数はReceiverのkey。呼び出し側は、これを受けてから「マウスをpairing modeにする」よう案内する。開始に失敗したときは出さない。

## 5. エラー

| エラー名 | いつ | `cadratctl` の終了コード |
|---|---|---:|
| `cc.nejiman10.Cadrat1.Error.Busy` | デバイスに触れる操作の実行中に、別のデバイス操作を要求した。メッセージに実行中のコマンド名を入れる | 22 |
| `org.freedesktop.DBus.Error.InvalidArgs` | 引数の辞書が §3 と §6 に合わない | 2 |
| `org.freedesktop.DBus.Error.AccessDenied` | 呼び出し元のuidが違う | 21 |
| `cc.nejiman10.Cadrat1.Error.Internal` | 想定外の内部エラー | 1 |

- 上の表以外の失敗は、コマンドの結果としてJSONで返す（§2）。

## 6. メソッド

すべて `a{sv}` を1つ受け取り、JSONの `s` を1つ返す。表の「key」は §3 の共通の引数に加えて受け付けるものである。

| メソッド | コマンド | 共通の引数 | key |
|---|---|---|---|
| `List` | `list` | `verbose`、`redact` | `nodes`（`b`） |
| `Init` | `init` | `config` | `preset`（`s`、`research-baseline`）、`force`（`b`） |
| `Get` | `get` | `config` | `keys`（`as`）、`wire`（`b`） |
| `Check` | `check` | `config` | — |
| `Set` | `set` | `config`、`mouse`、`route`、`verbose` | `assignments`（`as`、`key=value` の並び。必須）、`dry_run`（`b`）、`no_save`（`b`） |
| `Apply` | `apply` | `config`、`mouse`、`route`、`verbose` | `dry_run`（`b`） |
| `ReceiverSlots` | `receiver slots` | `receiver`、`redact`、`verbose` | — |
| `ReceiverPair` | `receiver pair` | `receiver`、`verbose` | `timeout_ms`（`u`）、`poll_interval_ms`（`u`） |
| `ReceiverUnpair` | `receiver unpair` | `receiver`、`verbose` | `slot`（`y`、必須）、`expected`（`ay`、必須）、`timeout_ms`（`u`）、`poll_interval_ms`（`u`） |
| `Cancel` | — | — | — |

- `get -n` は表示だけの違いなので、`cadratctl` の側で行う。
- `Set` の `assignments` は、`cadrat-tool set` の引数と同じ書き方と検証である（[tool/cli §2](../tool/cli.md#2-設定キーと値の書き方)）。
- `timeout_ms` と `poll_interval_ms` を省略すると、`cadrat-tool` と同じ既定値を使う（[receiver §3](../receiver.md#3-pair)、[§4](../receiver.md#4-unpair)）。
- `ReceiverUnpair` の `expected` は、確認のときに見せた対象slotの生の応答8 byteである。`receiver` には、確認のときの `ReceiverSlots` の結果にあったkeyを、接頭辞ではなくそのまま渡す。`cadratd` は確認をしない（[daemon §5](daemon.md#5-要求の処理)）。
  - そのため、`ReceiverSlots` のJSONは、各slotの生の応答（`raw_hex`）を載せる。`redact` を指定したときは載せない。
- `Cancel` は、実行中の `ReceiverPair` の待機をやめさせる。結果は `ReceiverPair` の応答（12または13）で返る。pair以外の実行中の操作と、何も実行していないときは何もしない。
- 所要時間: `ReceiverPair` と `ReceiverUnpair` は、timeoutまで応答しないことがある。呼び出し側は、D-Busの呼び出しのtimeoutを、要求したtimeoutより30秒以上長くする。
