# cadratd

`cadratd` は、ログインしたユーザーごとに動くデーモンである。`cadrat-tool` と同じ操作（列挙、設定の送信と保存、Receiver管理）を、D-Busのsession busで提供する。操作の手順と判定は `cadrat-tool` と同じで、共有crateの同じ実装を使う（[README §3](../README.md#3-基本原則)）。D-BusのAPIは [dbus.md](dbus.md)、フロントエンドは [ctl/cli.md](../ctl/cli.md) にある。

## 1. 範囲（Phase 2a）

| 含む | 含まない |
|---|---|
| `cadrat-tool` の `list`、`init`、`get`、`check`、`set`、`apply`、`receiver slots` / `pair` / `unpair` と同じ操作 | hold-open（`cadrat-hold-open` の役目、[hold-open/cli.md](../hold-open/cli.md)） |
| hidrawの変化の検出と、接続中の機器の公開 | 接続時やモード切り替え時の自動 `apply`（Phase 2b） |
| 動作中の、デバイスへの書き込みの排他（§4） | マウスとプロファイルの紐付け（Phase 2b） |
| | Report `0x03` / `0x17` の監視（Phase 3） |

Phase 2aの `cadratd` は、TOML以外に設定の状態を持たない。マウスに何を送ったかも記録しない。

## 2. 実行形態

- systemdの **user service**（`cadratd.service`）として動き、**session bus**（ユーザーのbus）に名前 `cc.nejiman10.Cadrat1` を取る。
  - 理由: hidrawの権限はudevの `uaccess` でログイン中のユーザーに与えられている（[device §1](../device.md#1-権限)）。設定ファイルはユーザーの `$XDG_CONFIG_HOME` にある。後のGNOME Shell拡張もsession busを使う。
- rootでは動かさない。system busは使わない。
- unitは `Type=dbus`、`BusName=cc.nejiman10.Cadrat1` とし、`default.target` に入れる。パッケージは全ユーザーについて有効にする（[implementation §7](../implementation.md#7-配布)）。
- **システムユーザーでは動かさない。** unitに `ConditionUser=!@system` を付ける。ログイン画面（gdm）などのシステムユーザーのセッションでも、全ユーザーについて有効にしたuser unitが起動するためである。Phase 2bの自動 `apply` をログイン画面で行わないためでもある。
- D-Busのactivationファイル（`cc.nejiman10.Cadrat1.service`、`SystemdService=cadratd.service`）も配る。有効にする前からログインしていたセッションでも、`cadratctl` の最初の呼び出しで起動する。
- 同じユーザーの `cadratd` は1つだけ動く。2つ目はbus名を取れずに終わる（§3）。
- **sandbox**: `NoNewPrivileges=yes`、`LockPersonality=yes`、`MemoryDenyWriteExecute=yes`、`RestrictRealtime=yes`、`RestrictSUIDSGID=yes`、`RestrictNamespaces=yes`、`SystemCallArchitectures=native`、`RestrictAddressFamilies=AF_UNIX` にとどめる。ファイルシステムを制限する設定（`ProtectSystem`、`ProtectHome`、`PrivateTmp`）は使わない。設定ファイルは、`cadrat-tool` と同じく、ユーザーが書ける任意の絶対パスに置けるためである。

## 3. 起動と終了

起動の順序:

```
1. 書き込みロックのファイル（§4）を開く                  開けない → 終了（1）
2. 変化の検出（§6）を始める
3. bus名 cc.nejiman10.Cadrat1 を取る（キューに並ばない）  取れない → 終了（1）
4. 書き込みロック（§4）を取る。最長90秒待つ              取れない → 終了（1）
5. 最初の列挙を行い、Devices を公開する（§6）
6. すべての要求を受け付ける
```

- **手順3から手順5が終わるまで（準備中）に届いた要求**:
  - デバイスに触れる要求（[dbus §6](dbus.md#6-メソッド) の表で「デバイス」の列が「読む」「書く」のもの）は、キューに入れずに、すぐエラー `Starting` を返す（[dbus §5](dbus.md#5-エラー)）。メッセージで、`cadrat-tool` の終わりを待っていることを伝える。
  - キューに入れないのは、呼び出し側がtimeoutで諦めた後に実行して、利用者の知らない送信をしないためである。
  - デバイスに触れない要求（`Get`、`Check`、`Init`、dry-run）と、プロパティの読み取りは、準備中でも処理する。準備中の `Devices` は空（マウスもReceiverも無し）である。
- 手順4で待つのは、`cadrat-tool` の送信系コマンドが動いている間である。90秒は、`receiver pair` の既定のtimeout（60秒）と停止の再試行（最長10秒、[receiver §6](../receiver.md#6-管理nodeの開き直し)）より長くしてある。
  - `--timeout` を長くした `cadrat-tool receiver pair` などで90秒を超えたら、`cadratd` は終わり、systemdが再起動する（`Restart=on-failure`）。
  - `flock` には、待っている書き手を優先する仕組みが無い。`cadrat-tool` の送信系コマンドが途切れずに続くと、`cadratd` は起動に失敗して再起動を繰り返す。人が手で打つ使い方では起きないので、対策はしない。

終了:

- SIGTERMまたはSIGINTで終わる。実行中の操作があれば、`cadrat-tool` がSIGINTを受けた場合と同じように止める。pairなら停止packetを必ず送る（[receiver §3](../receiver.md#3-pair)）。その操作の要求には結果を返してから終わる。
- 終了コードは、シグナルで止めた場合0、起動の失敗は1。
- `Restart=on-failure` とする。

## 4. デバイスへの書き込みの排他（Q9）

`cadratd` が動いている間、デバイスへの書き込みは `cadratd` だけが行う。設定の正本がデーモンとTOMLの2つに分かれるのを防ぎ、Phase 2bの自動 `apply` が `cadrat-tool` の送った設定を黙って上書きすることを防ぐためである。

- **ロックファイル**: `/run/user/<uid>/cadrat/cadratd.lock`。`<uid>` はプロセスの実uid（`getuid()`）である。
  - `XDG_RUNTIME_DIR` は使わない。ssh、`sudo -E`、`env -i` などで値がずれると、`cadratd` と `cadrat-tool` が別のファイルを見て、排他が黙って外れるためである。
  - `/run/user/<uid>` はlogindが作るもので、`cadrat` のプログラムは作らない。その下の `cadrat/` が無ければ作る（mode 0700）。
  - `/run/user/<uid>` が無ければ、`cadratd` は起動の手順1で終わる。
- **`cadratd`**: 起動の手順4で `flock(LOCK_EX)` を取り、終了まで持ち続ける。プロセスが終われば、カーネルが解放する。
- **`cadrat-tool`**: 送信系のコマンドは、デバイスに触れる前に `flock(LOCK_SH | LOCK_NB)` を取り、コマンドの終わりまで持つ（[tool/cli §8](../tool/cli.md#8-cadratd-との排他)）。
  - 取れなければ、`cadratd` が動いているので `DaemonRunning`（20）で止まる。
  - 持っている間は `cadratd` が起動の手順4で待つ。そのため、確かめてから送るまでの間に `cadratd` が書き込み始めることはない。
  - rootで実行した場合は、すべてのユーザーのロックファイルを確かめる（[tool/cli §8](../tool/cli.md#8-cadratd-との排他)）。
- 排他するのはデバイスへの書き込みだけである。`cadrat-tool` の `list`、`get`、`check`、`init`、`--dry-run`、`receiver slots`、`hold-open` は、`cadratd` が動いていても使える。
- 同じTOMLへの同時の書き込みは、これとは別に、TOMLのロック（[config §7](../config.md#7-書き戻し)）で防ぐ。
- ユーザーをまたぐ排他は、このロックではなく §5 のfdの規則に頼る（Q21）。

## 5. 要求の処理

- 要求は、[dbus.md](dbus.md) のメソッド1つが1つのコマンドに当たる。処理の順序、判定、結果は `cadrat-tool` の同じコマンドと同じにする（[tool/cli §3](../tool/cli.md#3-コマンド)、[§4](../tool/cli.md#4-set-の処理順序)、[receiver](../receiver.md)）。結果の形は [tool/cli §5](../tool/cli.md#5-出力) の `--json` と同じである。
- **デバイスへ書き込む操作は1つずつ行う。** 対象は、`Set` / `Apply`（dry-runでないもの）、`ReceiverPair`、`ReceiverUnpair` である。
  - 実行中に別の書き込む操作の要求が来たら、待たせずに `Busy` のエラーを返す（[dbus §5](dbus.md#5-エラー)）。pairは1分近くかかることがあり、その間、後の要求を黙って待たせないためである。
  - デバイスを読むだけの操作（`List`、`ReceiverSlots`）と、§6 の列挙し直しは、書き込む操作の実行中でも行う。`cadrat-tool list` が、別の `cadrat-tool` の送信やpairと同時に動けるのと同じである。
  - デバイスに触れない操作（`Get`、`Check`、`Init`、dry-run、プロパティの読み取り）は、いつでも行う。
  - したがって `Busy` は、利用者の書き込む要求どうしが重なったときだけ返る。§6 の列挙し直しが原因で返ることはない。
- **hidrawのfdは、1つの操作の間だけ開く。** 操作が終わったら閉じる。操作ごとに列挙し直し、前の結果（`Devices`）を送信先の決定に使わない。
  - 理由: `uaccess` の権限はアクティブなユーザーに付き、ユーザーを切り替えると外れる。開いたままのfdは権限が外れても使えてしまうので、別のユーザーのデーモンと同時に書き込むことになる。また、slotが変わるとC652のnodeが作り直される（[receiver §6](../receiver.md#6-管理nodeの開き直し)）。
  - 1つの操作の間（pairの待機など）にユーザーが切り替わると、その操作が終わるまでは、開いているfdを使い続ける。この短い間は、ユーザーをまたいで書き込みが重なり得る（Q21）。
- **選択の規則は同じにする。** マウスのselector（番号、key、接頭辞）とReceiverのkeyは、`cadratd` がその操作の列挙結果に対して、[device §6](../device.md#6-マウスの選択) と [receiver §1](../receiver.md#1-管理nodeの検出) の規則で解決する。番号は、その列挙での `list` の番号である。
- **設定ファイル**: 要求で渡された絶対パスを使う（[dbus §3](dbus.md#3-共通の引数)）。保存の手順（ロック、H0の照合、原子的な保存）は `cadrat-tool` と同じである（[config §7](../config.md#7-書き戻し)）。
- **unpairの確認**: `cadratd` は対話しない。確認は呼び出し側が行い、そのとき見せたslotの生の値を要求に付ける。`cadratd` は実行の直前にslotを読み直し、一致しなければ何もせずに `SlotChanged`（16）を返す（[receiver §4](../receiver.md#4-unpair) の手順4）。
- **pairの中断**: `Cancel` の要求か、pairを要求した接続がbusから消えたときに、SIGINTと同じ扱いで待機をやめ、停止packetを送る。結果は `PairTimeout`（12）になる（停止に失敗すれば13）。
- **unpairの待機中に呼び出し側が消えたとき**: 待機（[receiver §4](../receiver.md#4-unpair) の手順6）をやめる。解除要求はすでに送っており、取り消す手段は無い。`cadrat-tool` がこの待機中にシグナルで終わった場合と同じ扱いである（[tool/cli §3](../tool/cli.md#receiver-unpair-slot---receiverkey---yes---timeout秒---poll-interval秒)）。`Cancel` はunpairには効かない。
- `--hidraw` に当たる指定は受け付けない。nodeを直接指定する開発者向けの操作は `cadrat-tool` で行う。

## 6. 接続中の機器の公開

- `/dev` をinotifyで見張り、名前が `hidraw` で始まるファイルの作成（`IN_CREATE`）、削除（`IN_DELETE`）、属性の変更（`IN_ATTRIB`）があれば、250 ms待ってから列挙し直す。待つ間の通知はまとめる。
  - 作成と削除で、抜き差し、モード切り替え、再ペアリングによるnodeの変化が分かる。属性の変更で、udevが `uaccess` のACLを付けたことと、ユーザーの切り替えでACLが変わったことが分かる。
  - udevのnetlinkの通知は使わない。udevのグループの通知の形式は、systemdの内部の形式で、公開されていないためである。カーネルのグループの通知はACLが付く前に届くので、それだけでは開けないことがある。
  - 通知のキューがあふれたら（`IN_Q_OVERFLOW`）、列挙し直す。
- 列挙の手順と、読み取り要求（IDプローブ、slotの読み取り）は `list` と同じである（[device §3](../device.md#3-列挙の手順)）。書き込みはしない。
- 結果は `Devices` プロパティ（`list --json` と同じ中身）に置き、変わったら `PropertiesChanged` を出す（[dbus §4](dbus.md#4-プロパティとシグナル)）。
- `Devices` は表示のためのものである。送信先は、送信の要求ごとに列挙し直して決める（§5）。

## 7. ログ

- 標準エラーに1行ずつ出し、journalに残す。
- 残すもの: 起動と終了、ロックの取得、操作ごとの要求と結果（コマンド、終了コード名、送信した経路、保存したか）、警告。
- 機器IDとslotの識別子は、常に `--redact` と同じ形（`id-N`）で伏せる（[tool/cli §3](../tool/cli.md#list---nodes---redact)）。journalは不具合の報告に貼られることが多いためである。
  - 番号は `cadratd` のプロセス全体で1つの対応表から振る。同じプロセスのログの中では、同じ個体は同じ `id-N` になり、行どうしで照合できる。`cadratd` を再起動すると振り直す。
  - D-Busの応答では伏せない（`redact` を指定した場合を除く）。応答の `id-N` は、`cadrat-tool --redact` と同じく、その応答の中だけで振る。
- wire reportのhexはログに出してよい。個体を識別する情報を含まない。
- §6 の列挙し直しで出た警告（`W-INACCESSIBLE` など）は、前の列挙から内容が変わったときだけログに残す。アクティブでないユーザーの `cadratd` が、切り替えのたびに同じ警告を積み上げないためである。
