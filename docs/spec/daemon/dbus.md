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
- **引数は `a{sv}`（オプションの辞書）で渡す。** keyは §3 と §6 の表に従う。後でkeyを足しても、古い呼び出し側は壊れない。
  - 知らないkey、型の違う値、必須のkeyが無いことは、D-Busのエラー `org.freedesktop.DBus.Error.InvalidArgs` にする。
  - 型は合っているが値が誤っているもの（slot番号が0..4の外、`assignments` の書き方の誤り、`route` が `wired` / `receiver` 以外、`dry_run` と `no_save` の両方がtrueなど）は、`cadrat-tool` が引数の誤りとして扱うものと同じなので、コマンドの結果としてUsage（2）のJSONで返す。
- **結果は、`cadrat-tool --json` と同じJSONの文字列（`s`）で返す。** 外枠（`format`、`command`、`ok`、`exit_code`、`warnings`、`error`）と中身は [tool/cli §5](../tool/cli.md#5-出力) と同じである。
  - 理由: コマンドの失敗（`SentNotSaved` での送ったwireと未保存の変更、unpairの前後のslotなど）にも、表示すべき中身がある。D-Busのエラーはメッセージの文字列しか運べない。
  - コマンドとしての失敗（[tool/cli §6](../tool/cli.md#6-終了コード) の終了コード1〜19）は、通常の応答として `"ok": false` のJSONで返す。想定外の内部エラー（1）も、結果のJSONを作れる場合はJSONで返す。
  - D-Busのエラーは、要求をコマンドとして処理できなかったときだけに使う（§5）。`Internal` は、JSONを作ることもできなかった場合に限る。
  - JSONには、人間向けの表示に要る情報をすべて載せる。`cadratctl` はJSONから `cadrat-tool` と同じ表示を作る（[ctl/cli §3](../ctl/cli.md#3-出力)）。
- 呼び出し側は、同じユーザーでなければならない。session busの既定の方針に加えて、`cadratd` は呼び出し元のuid（`GetConnectionUnixUser`）が自分と同じかを確かめ、違えば `org.freedesktop.DBus.Error.AccessDenied` にする。

## 3. 共通の引数

| key | 型 | 意味 | 対応する `cadrat-tool` のオプション |
|---|---|---|---|
| `config` | `s` | 設定ファイルの**絶対パス**。省略すると、`cadratd` の環境で [config §1](../config.md#1-場所) の既定のパスを使う | `--config` |
| `mouse` | `s` | マウスのselector（番号、key、接頭辞）。省略は「指定なし」 | `--mouse` |
| `route` | `s` | `wired` / `receiver` | `--route` |
| `receiver` | `s` | Receiverのkeyまたは接頭辞 | `--receiver` |
| `verbose` | `b` | nodeごとの判定結果を、JSONの `nodes` に載せる。`cadrat-tool -v` がstderrに出すものと同じ内容である。`cadratctl` はそれをstderrに出し、stdoutのJSONからは除く（[ctl/cli §3](../ctl/cli.md#3-出力)） | `-v` |
| `redact` | `b` | 機器IDとslotの識別子を伏せる | `--redact` |

- 相対パスの `config` は `InvalidArgs` にする。`cadratd` の作業ディレクトリは呼び出し側と違うためである。`cadratctl` は自分の環境でパスを決め、絶対パスにして渡す。
- `cadrat-tool` の `--hidraw` に当たるkeyは無い（[daemon §5](daemon.md#5-要求の処理)）。

## 4. プロパティとシグナル

| 名前 | 型 | 内容 |
|---|---|---|
| `Version` | `s`（読み取り専用） | `cadratd` の版（例: `0.2.0`） |
| `Devices` | `s`（読み取り専用） | 最後の列挙結果。`list --json` の `mice` と `receivers` を持つJSON。機器IDは伏せない |
| `Busy` | `s`（読み取り専用） | 実行中の、デバイスへ書き込む操作のコマンド名（例: `receiver pair`）。無ければ空文字列 |
| `Ready` | `b`（読み取り専用） | 起動の準備（[daemon §3](daemon.md#3-起動と終了)）が終わり、デバイスに触れる要求を受け付けるか |

- `Devices`、`Busy`、`Ready` が変わったら、`org.freedesktop.DBus.Properties.PropertiesChanged` を値付きで出す。
- シグナル `PairingStarted(s receiver)`: `ReceiverPair` がpairingの開始（[receiver §3](../receiver.md#3-pair) の手順4）に成功したときに出す。引数はReceiverのkey。開始に失敗したときは出さない。
  - broadcastにせず、その `ReceiverPair` を呼んだ接続を宛先（destination）にして出す。ほかのクライアントが受けて、自分の要求の案内と取り違えないためである。ほかのクライアントは、`Busy` プロパティでpairの実行中を知る。
  - 呼び出し側は、これを受けてから「マウスをpairing modeにする」よう案内する。

## 5. エラー

| エラー名 | いつ | `cadratctl` の終了コード |
|---|---|---:|
| `cc.nejiman10.Cadrat1.Error.Busy` | デバイスへ書き込む操作の実行中に、別の書き込む操作を要求した。メッセージに実行中のコマンド名を入れる。何もしていない | 22 |
| `cc.nejiman10.Cadrat1.Error.Starting` | 起動の準備中に、デバイスに触れる操作を要求した（[daemon §3](daemon.md#3-起動と終了)）。メッセージで、`cadrat-tool` の終わりを待っていることを伝える。何もしていない | 22 |
| `org.freedesktop.DBus.Error.InvalidArgs` | 引数の辞書のkey、型、必須のkeyが §3 と §6 に合わない（§2） | 2 |
| `org.freedesktop.DBus.Error.AccessDenied` | 呼び出し元のuidが違う | 21 |
| `cc.nejiman10.Cadrat1.Error.Internal` | 想定外の内部エラーで、結果のJSONも作れなかった | 1 |

- 上の表以外の失敗は、コマンドの結果としてJSONで返す（§2）。

## 6. メソッド

すべて `a{sv}` を1つ受け取り、JSONの `s` を1つ返す。表の「key」は §3 の共通の引数に加えて受け付けるものである。「デバイス」の列は、書き込む操作の直列化（`Busy`）と起動の準備中（`Starting`）の扱いを決める（[daemon §3](daemon.md#3-起動と終了)、[§5](daemon.md#5-要求の処理)）。

| メソッド | コマンド | デバイス | 共通の引数 | key |
|---|---|---|---|---|
| `List` | `list` | 読む | `verbose`、`redact` | `nodes`（`b`） |
| `Init` | `init` | なし | `config` | `preset`（`s`、`research-baseline`）、`force`（`b`） |
| `Get` | `get` | なし | `config` | `keys`（`as`）、`wire`（`b`） |
| `Check` | `check` | なし | `config` | — |
| `Set` | `set` | 書く（`dry_run` ならなし） | `config`、`mouse`、`route`、`verbose` | `assignments`（`as`、`key=value` の並び。必須）、`dry_run`（`b`）、`no_save`（`b`） |
| `Apply` | `apply` | 書く（`dry_run` ならなし） | `config`、`mouse`、`route`、`verbose` | `dry_run`（`b`） |
| `ReceiverSlots` | `receiver slots` | 読む | `receiver`、`redact`、`verbose` | — |
| `ReceiverPair` | `receiver pair` | 書く | `receiver`、`verbose` | `timeout_ms`（`u`）、`poll_interval_ms`（`u`） |
| `ReceiverUnpair` | `receiver unpair` | 書く | `receiver`、`verbose` | `slot`（`y`、必須）、`expected`（`ay`、必須）、`timeout_ms`（`u`）、`poll_interval_ms`（`u`） |
| `Cancel` | — | なし | — | — |

- `get -n` は表示だけの違いなので、`cadratctl` の側で行う。
- `Set` の `assignments` は、`cadrat-tool set` の引数と同じ書き方と検証である（[tool/cli §2](../tool/cli.md#2-設定キーと値の書き方)）。
- `timeout_ms` と `poll_interval_ms` を省略すると、`cadrat-tool` と同じ既定値を使う（[receiver §3](../receiver.md#3-pair)、[§4](../receiver.md#4-unpair)）。
- `ReceiverUnpair` の `expected` は、確認のときに見せた対象slotの生の応答8 byteである。`receiver` には、確認のときの `ReceiverSlots` の結果にあったkeyを、接頭辞ではなくそのまま渡す。`cadratd` は確認をしない（[daemon §5](daemon.md#5-要求の処理)）。
  - そのため、`ReceiverSlots` のJSONは、各slotの生の応答（`raw_hex`）を載せる。`redact` を指定したときは載せない。
- `Cancel` は、実行中の `ReceiverPair` の待機をやめさせる。結果は `ReceiverPair` の応答（12または13）で返る。pair以外の実行中の操作（unpairを含む）と、何も実行していないときは何もしない。
- 所要時間: `ReceiverPair` と `ReceiverUnpair` は、timeoutまで応答しないことがある。呼び出し側は、D-Busの呼び出しのtimeoutを、要求したtimeoutより30秒以上長くする。
