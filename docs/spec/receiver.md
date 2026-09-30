# Receiver管理（slot / pair / unpair）

C652 Universal Receiverの結合状態を読んだり変えたりする。この機能はTOMLに一切触れない（設定正本とは独立）。

根拠は、調査SPEC「Receiver管理とLinux実装」と `evidence/receiver-repair-2026-09`（slot 2の解除 → slot 3への再ペアリング、1サイクル、OBSERVED）。

## 1. 管理nodeの検出

1. [device §3](device.md#3-列挙の手順) の手順で列挙し、親USBデバイスごとにまとめる。1つの物理Receiverが1つのグループになる。
2. 次の条件をすべて満たすnodeを、管理nodeの候補にする。
   - bus USB、VID `256f`、PID `c652`
   - Feature `0x43`..`0x47` のwire長がすべて8
   - Feature `0x41` のwire長が5（`slots` だけを実行する場合、この条件は要らない）
3. グループ内で候補が複数あるのは正常である。試験したC652では、MI_00..MI_04の5つすべてがFeature `0x41`（5 byte）とslot報告を宣言していた（OBSERVED）。
4. **グループ内では、`bInterfaceNumber` が最小の候補を使う。** 選んだnodeとinterface番号は必ず出力に載せる。
   - 根拠: MI_02からunpair・pair開始・停止を送り、slotの変化と入力の復旧を確認した（OBSERVED）。先行の結合監査で使った管理nodeはMI_00だった可能性が高いが、書き込み時点のinterface番号は記録されていない（状況証拠）。
   - 純正ソフトが「最初に見つかったもの」を使うという規則は、静的解析資料からは確認できなかった（調査側 UNKNOWN）。したがってこの選び方は、実機で効いた範囲に基づく決め打ちである（Q10）。
5. グループ内に条件を満たす候補が無ければ、そのReceiverには `DeviceInvalid`（7）を返す。
6. Receiverの選択:
   - 1台ならそれを選ぶ。
   - 2台以上なら `AmbiguousTarget` にし、`--receiver=<key>` で指定してもらう（keyは[device §2.2](device.md#22-識別キー)）。keyの一意な接頭辞も受け付ける。`list` はReceiverに番号を付けないので、番号では指定できない。
   - 開発者向けの `--hidraw=<path>` で管理nodeを直接指定することもできる。その場合も条件2で判定する。

## 2. slotの読み取り

- slot 0..4について、GET Feature `0x43 + slot` を長さ8で要求する。
- 列挙のときは、slot報告を宣言する管理nodeの候補のうち、interface番号が最小のものから読む（`0x41` の条件は問わない）。
- 応答の長さが8で、かつ `resp[0] == 0x43 + slot` でなければ `ReceiverProtocolError`（17）にする。GET自体がerrnoで失敗した場合も17とする。
- 解析:

| byte | 解釈 | 根拠 |
|---|---|---|
| 0 | Report ID | CONFIRMED |
| 1 | 機種種別の候補。`0x00` なら空き、それ以外なら占有 | HYPOTHESIS（再確認報告 §7）。C658の結合で `0x59` を観測 |
| 2..7 | 個体識別子の候補（6 byte） | HYPOTHESIS |

- 「占有」の判定は `byte1 != 0` だけで行う。本CLIではこの解釈をHYPOTHESISと明記したうえで使う。調査側で否定された場合は仕様を改訂する。
- 識別子の候補は、Receiver経由マウスの識別キーに使う（[device §2.2](device.md#22-識別キー)）。既定では表示し、`--redact` を付けたときだけ伏せる。

```
$ cadrat-tool receiver slots
receiver recv:port-3-2  (/dev/hidraw7, MI_00)
slot 0  empty
slot 1  empty
slot 2  occupied  type 0x59  id 0a1b2c3d4e5f   → mouse 1 (c658:0a1b2c3d4e5f)
slot 3  empty
slot 4  empty
```

slotの識別子とマウスの機器IDが一致した場合は（[device §5](device.md#5-receiver経由の経路とslotの対応付け)）、`list` の番号とkeyを併記する。

## 3. pair

```
cadrat-tool receiver pair [--receiver=<key>] [--timeout=<秒>] [--poll-interval=<秒>]
```

既定値は、timeout 60秒、poll間隔 1.0秒とする（調査CLIと同じ）。

処理順序:

```
1. 管理nodeを選び、そのfdを開いたまま使う（ただし §6 の開き直しを行う）
2. slot snapshot S0 を読む
3. SIGINT / SIGTERM のハンドラを設定する（以降の中断は必ず手順6に進む）
4. SET 41 02 02 00 00（pairing開始）                 失敗 → 6へ進み、ReceiverCommandFailed
5. "put the mouse in pairing mode" と表示し、
   poll間隔ごとにslotを読み、S0で空きだったslotが占有になるまで待つ
     見つかった → 6へ
     timeoutまたは中断 → 6へ
6. SET 41 02 00 00 00（pairing停止）を必ず送る       失敗 → PairStopFailed
7. 結果を判定する
```

判定:

| 状況 | 結果 | 終了コード |
|---|---|---:|
| 新たに占有されたslotがあり、停止も成功 | 成功。新しいslot番号を表示 | 0 |
| timeoutまたは中断で、停止は成功 | `PairTimeout` | 12 |
| 開始のSETが失敗 | `ReceiverCommandFailed` | 14 |
| 停止のSETが失敗（§6 の開き直しの後も失敗した場合。他の結果より優先） | `PairStopFailed`。pairing modeが続いている可能性と、Receiverを抜き差しして解除する方法を表示 | 13 |

- 新たに占有されたslotが2つ以上あれば、すべて表示する。成功として扱うが、警告 `W-PAIR-MULTIPLE` を出す。
- 手順2のslot読み取りに失敗したら、何も送らずに `ReceiverProtocolError`（17）で終える。
- 手順4が失敗した場合は、手順5の案内を表示しない。
- 手順5の待機中に、slotのGETがerrnoで失敗した場合は、timeoutまで読み続ける（警告 `W-SLOT-READ-RETRY`）。応答の長さやReport IDが違う場合だけ、待機をやめて手順6に進み、`ReceiverProtocolError`（17）とする。停止の失敗はこれより優先する。
- 成功は「slotが占有された」ことまでしか意味しない。入力が来るかどうかは確かめない（調査のpair CLIと同じ境界）。成功時には次の手順を案内する。
  - `cadrat-tool list` で新しいマウスが現れたかを確認する。設定nodeは、新しいslot番号と同じinterface（MI_N）に現れると見込まれる（OBSERVED）。
  - `cadrat-tool apply --mouse=<番号またはkey>` で設定を送る。
  - pair後の最初の送信は失われることがあるので、変化がなければ送り直す（Q7、[device §7](device.md#7-送信) 手順7）。
- 再ペアリング後、設定nodeはslotに合わせて別のinterfaceへ移る（調査では MI_03 → MI_04）。以前のhidraw pathを使い回さない。

## 4. unpair

```
cadrat-tool receiver unpair <slot> [--receiver=<key>] [--yes]
                           [--timeout=<秒>] [--poll-interval=<秒>]
```

既定値は、timeout 15秒、poll間隔 0.5秒とする（調査CLIと同じ）。

処理順序:

```
1. 管理nodeを選び、fdを開いたまま使う（ただし §6 の開き直しを行う）
2. 対象slotを読み、その生の値を S として控える     空き → SlotChanged（16）
3. 確認を取る
     対象slot、機種種別、識別子、対応するマウスの番号とkeyを表示する。
     このマウスがReceiver経由でしか操作できない場合、解除すると操作できなくなること、
     有線やBluetoothで戻す方法を表示する。
     --yes あり              → 4へ
     端末で "Unpair slot 2? [y/N]" に y → 4へ
     y 以外                  → Aborted（18、何もしない）
     端末でなく --yes も無い  → Usage（2、何もしない）
4. 対象slotを読み直し、S と完全に一致するか確かめる  違う → SlotChanged（16、何もしない）
5. SET 41 04 <slot> 00 00
     成功（戻り値5）       → 6へ
     EPIPE                → 警告 W-UNPAIR-EPIPE を出して 6へ
     その他のerrno、short → ReceiverCommandFailed（14）
6. poll間隔ごとに対象slotを読み、byte1 == 0 になるまで待つ
     空きになった → 成功（0）
     timeout      → UnpairNotConfirmed（15）
```

- **確認と照合の分担。** 利用者は、画面の内容を見て `y` を押すだけでよい。確認を表示してから実行するまでにslotが変わっていないかは、CLIが手順4で自動的に照合する。調査の手順（dry-runで出た生の値を人が照合して、実行コマンドに渡す）と同じ安全性を、手入力なしで保つためである。
- **EPIPEだけで成功とも失敗とも判定しない。** 調査の実装はEPIPEの後もslotを見続けており、HARDWARE_TESTでも「EPIPEだけでunpair成功扱いしない」とされている。判定は手順6のslotの空化だけで行う。
- 空化を確かめた後、全slotのsnapshotも表示する。このsnapshotの読み取りに失敗しても、結果は成功のままとする（空化は手順6で確認済みのため）。
- 手順2と4のslot読み取りに失敗したら、何も送らずに `ReceiverProtocolError`（17）で終える。
- 手順6で、slotのGETがerrnoで失敗した場合は、timeoutまで読み続ける（警告 `W-SLOT-READ-RETRY`）。timeoutまでに空きを確認できなければ `UnpairNotConfirmed`（15）とする。応答の長さやReport IDが違う場合は、17とする。
  - 経緯: 実機確認（実施 1、F6）で、解除要求が `EPIPE` を返した直後の slot の GET が一度 `EPIPE` で失敗し、次の読み取りでは空いていた。読み取りの失敗1回で打ち切ると、解除できているのに失敗と判定してしまう。
- 検証を飛ばす手段（`--no-verify` など）は用意しない。

## 5. 共通事項

- `--mouse` と `--route` は使わない。指定されたら使い方の誤り（終了コード2）にする。
- `--json` は[tool/cli §5](tool/cli.md#5-出力)の共通の外枠に従い、`receiver`、`slots_before`、`slots_after`、`new_slots`、`stop_sent` などを載せる。`--json` のときの `unpair` は対話しないので、`--yes` が必須になる。
- 自動再試行はしない。
- 管理nodeへのSETはすべて、Feature Reportの宣言長（`0x41` は5 byte）どおりに送る。宣言長と違う長さでは送らない。

## 6. 管理nodeの開き直し

slotが変わった後、開いていた管理nodeのfdが使えなくなることがある。実機確認（実施 1、F6′・F7′）では、unpairやpairの後にそのfdへのGETとSETが `ENODEV` を返し続けた。一方、同じReceiverの管理nodeは列挙し直せば再び見つかった。hidraw nodeの再作成かReceiver全体の再列挙かは区別できていない（調査リポジトリへ報告する）。

そこで、pair（§3）とunpair（§4）では、次の規則で管理nodeを開き直して続行する。

- **対象**: 手順5（pairの待機）、pairの手順6（停止）、unpairの手順6（待機）で、デバイスが消えたことを示すerrno（`ENODEV`、`ENXIO`、`ESHUTDOWN`、`ENOENT`）が返った場合。
- **開き直し方**: 列挙をやり直し、同じReceiver（同じ識別キー、つまり同じUSBポート）の管理nodeを §1 の規則（interface番号が最小の候補）で選び直して開く。選び直したnodeとinterfaceは、以前と違ってよい。
- **待機中**: 開き直せたら、次のpollからそのfdで読む。開き直せなければ、次のpollでまた試す。timeoutの扱いは変えない。
- **停止**: 停止のSETがこれらのerrnoで失敗したら、開き直して停止を送り直す。最長10秒まで繰り返す。それでも送れなければ `PairStopFailed`（13）とする。停止packetを必ず送るという §3 の原則を保つためである。
- **送らないもの**: 開き直した後に、pair開始（`41 02 02 …`）やunpair（`41 04 …`）を送り直すことはしない。すでに送った要求の結果は、slotの変化だけで判定する（P7）。
- **表示**: 開き直した場合は警告 `W-MANAGEMENT-REOPENED` を出す（回数と、最後に失敗した試みがあればその理由）。
- **対象外**: slotの読み取り（`receiver slots`）と、要求を送る前の読み取り（pairの手順2、unpairの手順2と4）では開き直さない。失敗したら何も送らずに17で終える。

