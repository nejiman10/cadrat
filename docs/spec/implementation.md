# 実装方針・検証・未決事項

## 1. crate構成

```
Cargo.toml                 # workspace
crates/
  cadrat-proto/           # I/Oなし、no_stdにできる形で書く
  cadrat-hidraw/          # Linuxのみ
  cadrat-config/          # TOMLスキーマと書き戻し
  cadrat-command/         # コマンドの手順と結果、出力の整形（Phase 2a）
  cadrat-dbus/            # D-Busの名前、引数、エラーの対応（Phase 2a）
  cadrat-tool/            # バイナリ（独立設定ツール）
  cadrat-hold-open/       # バイナリ（hold-openのシステムサービス、Phase 2a）
  cadratd/                # バイナリ（デーモン、Phase 2a）
  cadratctl/              # バイナリ（デーモンのフロントエンド、Phase 2a）
vectors/                   # 調査リポジトリから版を固定して取り込む
udev/69-cadrat.rules
udev/69-cadrat-hold-open.rules   # hold-openのサービスを起動する（Phase 2a）
packaging/                 # systemd unit、D-Busのactivationファイル、maintainer script
docs/spec/
```

| crate | 責務 | 後のデーモンで再利用するか |
|---|---|---|
| `cadrat-proto` | `Report10Config`、actionのenum、blob・wireの生成と `inspect`、HID descriptorの長さ解析、Report `0x03` のparser、Receiver管理packet（`41 02 …` / `41 04 …`）の生成、slot応答とIDプローブ応答（GET `0x08`）の解析 | する |
| `cadrat-hidraw` | 列挙、ioctl、候補の判定・選択、送信、Receiver管理（管理nodeの選択、slotのpoll、pair/unpairの手順）、hold-open、`/dev` の見張り（Phase 2a）。I/Oと時計はtraitの裏に隠し、テストではfakeに差し替える | する |
| `cadrat-config` | schema 1の型、検証、`toml_edit` での部分更新、原子的な保存、lock | する |
| `cadrat-command` | 各コマンドの手順（[tool/cli §4](tool/cli.md#4-set-の処理順序) など）。結果は `--json` のオブジェクト（`serde_json` の値）で返し、人間向けの表示はそのオブジェクトだけから作る（`render`、[tool/cli §5](tool/cli.md#5-出力)）。実行中に利用者へ出すもの（警告、pairの案内、unpairの確認）はフロントエンドのtraitを通す。`cadratd` との排他のロック（[daemon §4](daemon/daemon.md#4-デバイスへの書き込みの排他q9)）。Phase 1で `cadrat-tool` にあった `cmd` と `render` をここへ移す | する。`cadrat-tool` と `cadratd` が手順を、`cadratctl` が表示を使う（P11） |
| `cadrat-dbus` | bus名、path、interface、引数のkey、エラー名と終了コードの対応（[daemon/dbus.md](daemon/dbus.md)） | `cadratd` と `cadratctl` が使う |
| `cadrat-tool` | clap、終了コード | 独立ツールとして残す（デーモンのフロントエンド `cadratctl` とは別） |
| `cadrat-hold-open` | 引数の解析、シグナル、1つのnodeを開いて確かめ、消えるまで待つ（[hold-open/cli §3](hold-open/cli.md#3-コマンド)） | — |
| `cadratd` | D-Busのobject、要求の直列化、変化の検出、起動と終了 | — |
| `cadratctl` | clap、D-Busの呼び出し、Receiverの対話、終了コード | — |

依存は必要最小限にする。候補は `clap`、`toml_edit`、`serde`、`serde_json`、`thiserror`、`rustix`（ioctl、flock、inotify）、`sha2`。Phase 2aで `zbus` を加える（pure Rustで、`libdbus` や `libsystemd` にリンクしない）。非同期ランタイム（`tokio` など）は入れず、`zbus` のblocking APIとスレッドで組む。`cadrat-tool` と `cadrat-hold-open` は `zbus` に依存しない。`hidapi` は使わない。descriptorの取得とGET/SET Featureを直接制御したいため。

## 2. 型の方針

- 検証済みの値だけを型で表す（例: `Dpi` は50..8200の50刻みしか作れない）。blob生成関数は失敗しない（`Result` を返さない）ようにする。
- `Action` は `Direct(DirectAction)`、`HostRouted(HostIndex)`、`Raw(RawWire)` とする。`HostIndex` は0..215、`RawWire` は `0x10`..`0x27` しか作れない。どのwire値も書き方が1つに決まる。`DirectAction` に code 6 を `Unknown6` として含める。
- `cadrat-proto` は `core` だけに依存し、allocも使わない。
- descriptorの解析は調査SDKの `hid_descriptor.py` と同じ規則に従う。ただし、HIDの仕様上1 byteに収まらないReport ID（256以上）と、16段を超えるPUSHの入れ子はエラーにする。調査SDKはどちらも受け付ける。
- TOMLの文字列表現とJSONの表現は、どちらも同じ `Display` / `FromStr` を使う。

## 3. テストベクタ

調査リポジトリのPython SDKには、固定入力からJSONを書き出す `threedx_report10.test_vectors` がある（出力 `format_version` 1）。Rust側はその出力を `vectors/` に版を固定して取り込み、`crates/cadrat-proto/tests/vectors.rs` で照合する。出所の commit、再生成の手順、ハッシュは [vectors/README.md](../../vectors/README.md) に記録する。

| 組 | 入力 | 内容 |
|---|---|---|
| `research` | 調査側が公開した入力（`sdk/python/vectors/`）をそのまま複製 | 各種類の代表例 |
| `boundary` | 本リポジトリが用意した合成入力を、調査側のexporterで変換 | 境界値。dpi 50 / 8200、lift 0 / 31 / 255、全polling rate、全direct action、host 0 / 1 / 7 / 8 / 214 / 215、offset 18..24それぞれへの `host:1`、descriptorのPUSH/POP・long item・複数item合算・Report IDなし、Report `0x03` の遷移と上位bitの切り捨て、全slotのunpair packet |
| `real` | 調査側が収録した実機のdescriptor（`sdk/python/tests/data/real_hid_descriptors.json`）の `descriptor_hex` だけを取り出し、調査側のexporterで変換 | C652のMI_00・MI_02と、有線C658のMI_00・MI_01。descriptorだけでwire・Report `0x03`・Receiverは空 |

照合する項目: wire値と `inspect` の結果、descriptorのFeature / Input wire長、Report `0x03` のbitmapと押下・解放mask、Receiver packet。descriptorの `top_level_usages` は `cadrat-proto` の責務に含めないので照合しない。

- ベクタは調査SDKの挙動を示すもので、プロトコルの新しい根拠ではない。
- `real` のdescriptorは `cadrat-hidraw` のfakeにも使い、nodeの分類（device §4、receiver §1）と選択を確かめる（`crates/cadrat-hidraw/tests/fake.rs`）。
- exporterは設定から作ったwireしか検査しないため、次のものはベクタで表せない。Rust側だけのテストで確かめる。
  - `raw:` のボタン（`0x10`..`0x27`）と、actionに対応しないwire値（`0x00`..`0x09`）
  - 予約byteが0でないもの、offset 26が `0x1e` でないもの、`inspect` のエラー（長さ、Report ID、未知のwheel・polling）
  - slot応答とIDプローブ応答の解析
  - 調査SDKと意図して異なる挙動（DPIの拒否、256以上のReport ID、深すぎるPUSH）
- DPIについて: Python SDKはclampと切り捨てをするが、本CLIは範囲外や端数をエラーにする（[config §4.1](config.md#41-dpi)）。ベクタは50の倍数で範囲内のものに限る。
- 実機のdescriptorは、調査側TODO 12で収録されてから取り込む。調査の証拠に載っているSHA-256と一致し、個体識別子が含まれないことを調査側で確認したものに限る。
- ベクタを取り込み直すときは、どちらの組にも個体識別子とローカル環境の情報（パス、ユーザー名、hidraw node、シリアル）が無いことを確かめる。

## 4. テスト階層

| 階層 | 対象 | 実機 |
|---|---|---|
| 単体 | proto（ベクタ）、configの検証、書き戻しでコメントが保たれること、actionの解析 | 不要 |
| 結合（fake） | `cadrat-hidraw` を、fakeのsysfsとdescriptorとioctl応答で動かす。0件・1件・複数件、probeの不一致、EACCES、short write、ENODEV、マウスとReceiverの組み立て、機器IDによる2経路の統合、有効な経路の判定（有線nodeの有無）、機器IDが取れない場合の退避key、機器IDとslot識別子の照合（一致・不一致・interface番号の食い違い）、selectorと `--route` の解決、送信直前の宛先確認で機器IDが変わっていた場合（終了コード19） | 不要 |
| Receiver（fake） | fakeのslot列と仮想時計で、pair成功、timeout、中断（SIGINT）でも停止packetが送られること、停止の失敗、複数slotの新規占有、unpairのEPIPE後の空化・未空化、確認から実行までのslot変化、確認の拒否、端末でない場合、1台のReceiverに管理node候補が2つある場合、Receiverが2台ある場合 | 不要 |
| CLI | 一時ディレクトリで `set` の各分岐（dry-run、no-save、送信失敗時にTOMLが変わらないこと、H0不一致で終了コード9になること、lock競合） | 不要（fake transport） |
| コマンド層（Phase 2a） | `cadrat-command` の各コマンドを、fakeのtransportで動かす。JSONから作った人間向けの表示が、直接作った表示と一致すること。案内に出すコマンド名（`cadrat-tool` / `cadratctl`）の差し替え | 不要 |
| 排他（Phase 2a） | `cadratd.lock` を別のプロセスが持っているときに、`cadrat-tool` の送信系コマンドが20で終わり、TOMLもデバイスも変わらないこと。対象外のコマンドは動くこと。`cadrat-tool` がロックを持つ間、`cadratd` が起動を待つこと。fakeのnodeを別のプロセスが `LOCK_EX` で持っているときに、`set`、`apply`、`receiver pair`、`receiver unpair` が22で終わり、何も送らずTOMLも変わらないこと。Receiverの管理nodeだけがロックされているときに、Receiver経由の `set` が22になること。`list` と `receiver slots` はロックがあっても動くこと。unpairで、確認の間はロックを持たないこと。管理nodeの開き直しでロックを取り直せなくても、pairの停止packetが送られること | 不要 |
| D-Bus（Phase 2a） | テストごとに起動した `dbus-daemon --session` の上で、fakeのtransportを渡した `cadratd` と `cadratctl` を動かす。同じ場面で `cadrat-tool` と `cadratctl` の標準出力と終了コードが一致すること（案内のコマンド名を除く）、`Busy`、`Cancel` と呼び出し側の切断でpairの停止packetが送られること、`PairingStarted`、unpairの `expected` の不一致で16、相対パスの `config` で `InvalidArgs`、`/dev` の変化による `Devices` の更新と `PropertiesChanged`、起動の準備中の `Starting`、書き込み中の `List` が `Busy` にならないこと、`-v` と `--json` を組み合わせた出力、`Version` が違う `cadratd` への警告と、そのときの `UnknownMethod` で21になること | 不要 |
| 実機 | §5の達成条件 | 必要 |

CLIのテストでは、`cadrat-tool` のライブラリの入口 `run(args, env, io)` に、fakeのtransport・時計・シグナルと標準入出力を渡して、プログラム全体を動かす。バイナリ（`main.rs`）は実物を渡すだけで、fakeを含まない。fakeは `cadrat-hidraw` の `fake` featureで、開発時の依存からだけ使う。そのため、環境変数や隠しオプションによる切り替えは用意しない。`cadratd`、`cadratctl`、`cadrat-hold-open` も同じ形で、ライブラリの入口にfakeを渡してテストする。

## 5. 達成条件

### 5.1 Phase 1

v0.1.0で満たした（[実機確認](../hardware-test.md)）。

1. ベクタテストがすべて通る。つまり、同じ設定からPython SDKと同じ32 byteが作られる。
2. 有線C658で、`list` がマウスを1台に特定する。`set mouse.dpi=…`、`set buttons.radial=host:1`、`apply` について、変更がマウスの挙動に表れることを確かめる。
3. Receiver経由でも2と同じことを確かめる。
3a. Receiverへの各送信について、1回目の送信で効果が現れたかどうかを記録する（Q7）。効果が現れなかった場合は、送信先node、宛先確認の結果、送信後の操作を記録し、調査リポジトリへ報告する。
4. 有線とReceiverを両方つないだとき、`list` に1台のマウス（2経路）として表示され、有線モードとReceiverモードを切り替えると有効な経路が入れ替わる。`--mouse` なしで、有効な経路へ送られる。
4a. 抜き差しや再ペアリングの後も、マウスの識別キーが変わらない。
4b. 再ペアリング後に、古いhidraw pathを `--hidraw` で指定すると、送信直前の宛先確認で止まる（終了コード7または19）。
5. udevルールを外した状態で、`PermissionDenied` とヒントが表示される。
6. 送信の途中でマウスを抜くと、終了コード8になり、TOMLが変わっていない。
7. `set` を実行する間にエディタでTOMLを書き換えると、終了コード9になり、エディタでの変更が失われない。
8. Receiverで `receiver slots` → `receiver unpair`（確認表示 → 実行）→ `receiver pair` → `list` → `apply` の1サイクルを行う。slotの空化と再占有、その後の入力と設定反映を確かめ、調査リポジトリの結合監査と同じ結果になることを確認する。
9. `receiver pair` の待機中にCtrl-Cを押しても、停止packetが送られて終了コード12になる。
10. unpairの確認プロンプトで `n` を押すと、何もせず終了コード18になる。
11. 実機試験の手順と結果を `docs/hardware-test.md` に記録する。記録の形式は調査リポジトリの `HARDWARE_TEST.md` に倣う。
12. 有線C658で、調査側のuser serviceを止めて `cadrat-hold-open.service` を有効にすると、USBの抜き差しの後も入力が止まらず、journalに保持と解放の行が残る。serviceを止めると保持中のfdが閉じられる（[device §9](device.md#9-hold-open)）。

### 5.2 Phase 2a

1. §4のテストがすべて通る。特に、同じ場面で `cadrat-tool` と `cadratctl` の出力と終了コードが一致する。
2. 有線C658で、`cadratctl` の `list`、`set mouse.dpi=…`、`set buttons.radial=host:1`、`apply` について、変更がマウスの挙動に表れる（Phase 1の条件2と同じ確かめ方）。
3. Receiver経由でも2と同じことを確かめる。
4. `cadratctl` で `receiver slots` → `receiver unpair`（確認表示 → 実行）→ `receiver pair` → `list` → `apply` の1サイクルを行い、Phase 1の条件8と同じ結果になる。`receiver pair` の待機中のCtrl-Cで停止packetが送られて12になり、unpairの確認で `n` を押すと18になる。
5. `cadratd` が動いている間、`cadrat-tool set` が20で終わり、TOMLとマウスの設定が変わらない。`cadrat-tool list` は動く。`cadratd` を止めると、`cadrat-tool set` が動く。
6. ケーブルの抜き差し、有線とReceiverのモード切り替え、再ペアリングの後に、`cadratd` を再起動しなくても `cadratctl list` と `Devices` が新しい状態を示す。
7. `cadrat-hold-open` のシステムサービスだけで（user unitと調査側のserviceは無効）、有線C658の入力が次の場面で止まらない: 起動直後のログイン画面、ログイン後、ログアウトした後のログイン画面、ケーブルの抜き差しの後。journalに保持と解放の行が残る（Q20）。抜き差しのたびに `cadrat-hold-open@hidrawN.service` が起動・停止し、`systemctl mask cadrat-hold-open@.service` の後は起動しない。
8. `cadratd` を再起動しても、有線C658の入力が止まらない。
9. Ubuntu 22.04と24.04で、3つの `.deb` をインストール・削除できる。v0.1.0の `cadrat-tool` を入れた状態から更新でき、udevルールの持ち主が `cadrat-common` に移る。ログインすると `cadratd` が動き、止めていても `cadratctl` の呼び出しで起動する。
10. 実機試験の手順と結果を `docs/hardware-test.md` に記録する。

## 6. 未決事項

調査の結果は調査リポジトリ commit `6b151ae` による。

| # | 論点 | 状態 | 扱い |
|---|---|---|---|
| Q1 | 有線とReceiverが両方つながっているときの扱い | 解決 | 機器IDで1台のマウスにまとめ、有効な経路（有線nodeがあれば有線）にだけ送る（[device §2.3](device.md#23-有効な経路)） |
| Q2 | 有線C658で、Feature `0x10` 32 byteを宣言するinterfaceが1つか | 解決（試験個体） | MI_01だけだった。複数あれば `ambiguous-node` とする規則は残す |
| Q3 | 物理ボタン名とoffset 18..24の対応 | 解決（試験条件） | 7 entryすべて、両経路の成功監査で対応を確認（OBSERVED） |
| Q4 | 名称 | 決定 | プロジェクト名 cadrat。独立ツール `cadrat-tool`、デーモン `cadratd`、フロントエンド `cadratctl`（[README §0](README.md#0-cadrat-プロジェクトの構成)） |
| Q5 | ライセンス | 決定 | MIT（調査リポジトリと同じ） |
| Q6 | `--deny-warnings` を用意するか | 決定 | 用意しない |
| Q7 | Receiver経由の送信で、再送や適用確認をするか | 決定 | 1回だけ送り、送信直前に同じfdで宛先を確認する。自動では再送せず、Receiver経由の送信の成功時と `receiver pair` の成功時に、効いていなければ送り直すよう案内する（[device §7](device.md#7-送信) 手順7、[receiver §3](receiver.md#3-pair)）。実機確認（[実施 2](../hardware-test.md)）で、再ペアリング直後の最初の送信が約5分たっても効かず、送り直すと効く事例を再現したが、再現の条件が分からないため、原因の調査（調査側 [Issue #2](https://github.com/nejiman10/3dx-hid-research/issues/2)）を待たずに案内で対処する。原因が分かれば見直す |
| Q8 | MSRVと配布の形 | 決定 | Ubuntu LTS（最小22.04）を対象に、`.deb` をGitHub Releasesで配布する（§7）。MSRVは定めず、`rust-toolchain.toml` でツールチェーンを固定する |
| Q9 | デーモン導入時に、CLIとデーモンが同じTOMLへ同時に書かない方法 | 決定 | デバイスへの書き込みだけを、`cadratd` が動いている間ずっと排他にする。`cadratd` は `/run/user/<uid>/cadrat/cadratd.lock` を `LOCK_EX` で持ち、`cadrat-tool` の送信系コマンドは `LOCK_SH | LOCK_NB` が取れなければ終了コード20で止まる。起動そのものは排他にしない（[daemon §4](daemon/daemon.md#4-デバイスへの書き込みの排他q9)、[tool/cli §8](tool/cli.md#8-cadratd-との排他)）。D-Bus上の名前で検出する案は、`cadrat-tool` にD-Busの依存が要り、確かめてから送るまでの間の競合も防げないので採らない |
| Q10 | Receiverの管理nodeの選び方 | 決定（根拠は限定的） | interface番号が最小のもの。MI_02で効くことは観測済み、MI_00は状況証拠。純正の規則は不明。slotが変わると管理nodeが作り直されるので、pair / unpair の途中で消えたら同じ規則で選び直して開き直す（[receiver §6](receiver.md#6-管理nodeの開き直し)） |
| Q11 | slot byte1で占有を判定してよいか | 決定（根拠は限定的） | `0x00` ↔ 空き、`0x59` ↔ 占有を観測。任意の非0値を占有とする一般則は未検証なので、HYPOTHESISと明記して使う |
| Q12 | pair後に自動で `apply` するか | 決定 | しない |
| Q13 | USBシリアルはあるか | 解決 | 無い（試験個体）。マウスは機器ID、Receiverはポートパスで識別する |
| Q14 | 1台のReceiverに複数のマウスを結合したときの対応付け | 調査中 | 機器IDとslot識別子の照合で対応付ける。複数台での検証は調査側TODO 10 |
| Q15 | 有線と無線を同じマウスとしてまとめるか | 解決 | まとめる。機器IDが3か所で一致した（OBSERVED） |
| Q16 | モード切り替え後に設定が変わって見える原因 | 調査側TODO 13 | 送信は有効な経路だけに行い、切り替え後の `apply` を案内する（[device §7.1](device.md#71-モード切り替えとの関係)） |
| Q17 | GET `0x08` bytes 2..7 の正式な意味と一意性 | 未検証 | 機器IDとして使う。複数機器での一意性は調査側TODO 10で確認する |
| Q18 | `cadratd` のD-Busの名前とAPIの形 | 決定 | bus名 `cc.nejiman10.Cadrat1`（所有者のドメインによる）。object・interfaceは1つで、1つのメソッドが1つのコマンドに当たる。引数は `a{sv}`、結果は `--json` と同じJSONの文字列（[daemon/dbus.md](daemon/dbus.md)）。Phase 2aでは同じ版の `cadratctl` だけが使う内部のAPIとし、安定させるのはPhase 3で外部のクライアントと一緒に設計し直してからにする（[dbus §1](daemon/dbus.md#1-名前)）。それまでは、失敗の多くをD-Busのエラーではなく `"ok": false` のJSONで返す（Phase 2の方針の「エラー名は終了コード名と1:1」は、安定させるときに改めて決める） |
| Q19 | hold-openの実行形態 | 決定 | root・capabilityなし・sandboxのシステムサービスにする。user unitではログイン画面とログアウトの後に入力が止まるため。udevがC658のhidraw nodeごとにtemplate unit（`cadrat-hold-open@<node>.service`）を起動し、nodeが消えればsystemdが止める。周期的なsysfsの列挙はしない。既定で動き、`systemctl mask` で止められる。`cadratd` はhold-openを行わない（[hold-open/cli.md](hold-open/cli.md)） |
| Q20 | `cadrat-hold-open` が開くまでの間に入力が止まった場合に、開けば入力が戻るか（起動直後と、`Restart=on-failure` での再起動の後） | 未検証 | udevの `SYSTEMD_WANTS` で、nodeが現れた直後に起動する（Q19）。Phase 2aの実機確認（§5.2 の7）で確かめ、戻らなければ `DefaultDependencies=no` でさらに早める。新しい挙動が見つかれば調査リポジトリへ報告する |
| Q21 | ユーザーを切り替えたとき、別のユーザーの `cadratd` と同時に書き込まないか | 決定 | 書き込むプロセスは、書き込むhidraw nodeに `flock(LOCK_EX \| LOCK_NB)` をかけ、取れなければ22で止まる（[device §7.2](device.md#72-書き込みのロック)）。ロックはnodeのinodeにかかるので、ユーザーとプロセスをまたいで効く。加えて、`cadratd` はhidrawのfdを1つの操作の間だけ開く（[daemon §5](daemon/daemon.md#5-要求の処理)）ので、`uaccess` を失ったユーザーの `cadratd` は新しく開けない。v0.1.0の `cadrat-tool` はロックをかけないので、それとの重なりは防げない |

## 7. 配布

- 形式: `.deb`。`cargo-deb` で作り、GitHub Releasesに置く。
- 対象: Ubuntu LTS（amd64）。**最小サポートはUbuntu 22.04**（glibc 2.35）。標準サポート中のLTSで、ビルド環境を再現しやすく、CADソフトの対応OSとも釣り合う範囲にした。それより古い環境では `.deb` を提供せず、利用者がソースからビルドする（下の `cargo install`）。
- **リリース用ビルド。** glibcの互換性のため、Ubuntu 22.04上で `packaging/build-release.sh` を実行して作る。スクリプトは次を確かめ、満たさなければ止まる。
  - 実行環境がUbuntu 22.04であること、未コミットの変更が無いこと、`target/` が無いこと（別の環境でビルドした物を使い回さないため）
  - バイナリが要求するglibcのsymbol versionが2.35以下であること（`objdump -T`）
  - `.deb` のdataがxz圧縮であること。zstdに対応しない古いdpkgでも中身を確かめられるようにする
- リリース用ビルドの版は `Cargo.toml` の版そのままで、`--version` にも印を付けない。
- リリース用ビルドはGitHub Actions（`.github/workflows/release.yml`）の `ubuntu:22.04` コンテナで行う。pull requestではビルドと、同じコンテナでのインストール・実行・削除までを行う（Phase 2aからは3つのパッケージと、v0.1.0からの更新も）。`v<版>` のtagをpushすると、tagと版の一致を確かめたうえで、build provenanceのattestationを付け、3つの `.deb` と `SHA256SUMS` を載せた**下書き**のGitHub Releaseを作る。公開は所有者が下書きを確かめてから行う。
- 書式・lint・テスト（`cargo fmt --check`、`cargo clippy -D warnings`、`cargo test`）は `.github/workflows/ci.yml` がpull requestと、mainとdevelopへのpushで実行する。
- **パッケージの分割（Phase 2a）。** `cadrat-tool` だけを使う利用者が、デーモンを入れずに済むように分ける。どれも同じ版で作り、1つのtagで3つとも出す。

| パッケージ | 含むもの | 依存 |
|---|---|---|
| `cadrat-common` | `/usr/lib/udev/rules.d/69-cadrat.rules`（hidrawの `uaccess`）、`/usr/lib/udev/rules.d/69-cadrat-hold-open.rules`、`/usr/bin/cadrat-hold-open`、`/usr/lib/systemd/system/cadrat-hold-open@.service`（[hold-open/cli.md](hold-open/cli.md)）、manページ | `udev`、`systemd` |
| `cadrat-tool` | `/usr/bin/cadrat-tool`、manページ、シェル補完 | `cadrat-common`（同じ版） |
| `cadratd` | `/usr/bin/cadratd`、`/usr/bin/cadratctl`、`/usr/lib/systemd/user/cadratd.service`、`/usr/share/dbus-1/services/cc.nejiman10.Cadrat1.service`、manページ、`cadratctl` のシェル補完 | `cadrat-common`（同じ版）、`default-dbus-session-bus \| dbus-session-bus` |

- `cadrat-common`: インストール後に `udevadm control --reload` と `udevadm trigger` を実行する。hold-openのサービスは `enable` せず、udevルールが起動する。すでにつながっているC658には、そのnodeだけに `add` のイベントを起こし直す（[hold-open/cli §4.3](hold-open/cli.md#43-有効無効と更新)）。更新ではhold-openのサービスを止めも再起動もしない。削除では動いているinstanceを止め、有線C658の入力が止まることを表示する。hold-openのサービスは `enable` しないので、cargo-debの `systemd-units` の有効化は使わず、これらはmaintainer scriptに書く。
- `cadrat-common` は `Replaces: cadrat-tool (<< 0.2.0)` と `Breaks: cadrat-tool (<< 0.2.0)` を持つ。v0.1.0では `cadrat-tool` がudevルールを持っていたためである。
- `cadratd`: **初めてインストールしたときだけ**、全ユーザーについて有効にする。`postinst configure` で前の版が無いときに `deb-systemd-helper --user enable cadratd.service` を実行する。更新では有効・無効に触れない。管理者が無効にしたものを、更新で有効に戻さないためである（Debian Policyと `deb-systemd-helper` の考え方）。
  - Ubuntu 22.04のinit-system-helpersが `--user` に対応しているかは確かめていない（TODO 18）。対応していなければ、初回だけ `systemctl --global enable cadratd.service` を実行する。
  - すでに動いているユーザーの `cadratd` は、更新でも再起動しない。新しい版は次のログインか、`systemctl --user restart cadratd.service` から使われる。その間の版のずれは `cadratctl` が扱う（[ctl/cli §4.1](ctl/cli.md#41-cadratd-との版のずれ)）。
  - 削除では `systemctl --global disable` を行い、`postrm purge` で有効化の記録を消す。
  - デーモンを使わず `cadrat-tool` だけで運用する方法を、READMEに書く。`cadratd` のパッケージを入れないか、`systemctl --user disable --now cadratd.service` で止める。止めた後も、`cadratctl` を呼べばD-Busのactivationで起動する。
- Phase 1のuser unit（`/usr/lib/systemd/user/cadrat-hold-open.service`）は配らない。有効にしていた利用者には、リリースノートと `cadrat-common` のインストール時の表示で `systemctl --user disable cadrat-hold-open.service` を案内する。
- 後のPhaseでは、GNOME Shell拡張（`/usr/share/gnome-shell/extensions/`）を別のパッケージで追加する。
- manページとシェル補完は、CLIの定義から `cargo run -p xtask -- dist` で生成する。
- **試験ビルド。** 各リリースの実機確認（§5）を終えるまでに作る `.deb` は試験ビルドとし、リリースしない。Phase 2aでは3つのパッケージすべてが対象で、3つとも同じ版と同じ印を付ける。版を `<版>~test<番号>+g<commit>` とし、`--version` にも `(test build)` と表示する。作り方と確認の記録は [docs/packaging.md](../packaging.md) に置く。
- flatpakとAppImageは採用しない。
  - flatpak: サンドボックスの中からudevルール、systemd unit、GNOME Shell拡張をホストに入れられない。hidrawへのアクセスにも `--device=all` が要る。CLIの起動も `flatpak run …` になる。
  - AppImage: udevルールとunitを別の手順で入れる必要がある。最近のUbuntuでは、AppImageの実行にlibfuse2の追加インストールが要る。
- 開発者と、22.04より古い環境の利用者には `cargo install --path crates/cadrat-tool` を案内する（`cadrat-hold-open`、`cadratd`、`cadratctl` も同じ形）。この場合、udevルール、systemd unit、D-Busのactivationファイルは手動で入れる。

