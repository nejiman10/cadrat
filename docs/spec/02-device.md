# 02 デバイスモデル・検出・送信

## 1. 権限

- root不要で、一般ユーザーとして動くことを前提にする。
- 権限は調査リポジトリの udev ルール（`69-3dconnexion-c658.rules`、`TAG+="uaccess"`）で与える。本プロジェクトは同じ内容のルールを `69-cadrat.rules` として同梱する。
- permission不足で開けないnodeは、黙って無視しない。`inaccessible` として記録し、udevルールのヒントを表示する。

## 2. デバイスモデル

利用者が扱う単位は hidraw node ではなく**マウス**である。1台のマウスは、有線とReceiver経由の2つの**経路（route）**を持ち得る。hidraw nodeは内部の実装詳細とし、`list --nodes` でだけ表示する。

```
マウス（key = c658:<機器ID>）
 ├ route wired     … 有線C658の設定node（例: MI_01）
 └ route receiver  … C652の設定node（MI_N、N = slot番号）＋ slot N

Receiver（key = recv:port-<USBポートパス>）
 ├ 管理node（MI_00..MI_04 のいずれか）
 └ slot 0..4
```

### 2.1 機器ID

マウスは、GET Feature `0x08` の応答 bytes 2..7（6 byte）で識別する。これを**機器ID**と呼ぶ。

- 根拠: 有線C658とReceiverを同時接続した監査で、有線MI_01のGET `0x08`、C652 MI_03のGET `0x08`、占有slot 3の応答 bytes 2..7 の3つが一致した（OBSERVED、調査SPEC「Receiver管理とLinux実装」、`evidence/connection-identity-2026-09`）。
- 限界: このfieldの正式な意味と、複数の機器を並べたときの一意性は未検証（HYPOTHESIS扱い、Q17）。
- 機器IDはローカル表示には出す。`--redact` を付けたときだけ伏せる（[03](03-cli.md)）。

### 2.2 識別キー

| 対象 | key | 根拠・注意 |
|---|---|---|
| マウス | `c658:<機器ID 12桁hex>` | §2.1。有線でもReceiver経由でも同じkeyになる |
| マウス（機器IDが取れない場合） | `c658-port:<USBポートパス>/if<N>` | GET `0x08` が失敗した場合の退避。この形のkeyは経路をまたいでまとめない |
| Receiver | `recv:port-<USBポートパス>` | 試験したC652にはUSBシリアルが無かった（OBSERVED）。ポートを差し替えるとkeyが変わる |

- 試験したC658とC652には、USBシリアル（sysfs `serial`、udevの `ID_SERIAL*`）が無かった（OBSERVED）。そのためUSBシリアルは使わない。
- 識別キーは、後のデーモンが設定をマウスに紐付けるときのキーとして使う。

### 2.3 有効な経路

- 有線C658のnodeは、マウスが有線モードのときだけ列挙され、Receiverモードでは列挙されなかった（OBSERVED、`evidence/cross-route-management-2026-09`）。
- 一方、Receiver側の設定node（MI_N）は、マウスが有線モードのときも列挙され、GET `0x08` も応答した（OBSERVED）。
- そこで有効な経路を次のように決める。
  - そのマウスの有線nodeがあれば `wired`。
  - 無ければ `receiver`。
- 有効でない経路は `standby` と表示する。送信は、既定では有効な経路にだけ行う（§7）。

## 3. 列挙の手順

1. `/sys/class/hidraw/*` を列挙し、ueventの `HID_ID` からVID:PIDを取る。VIDが `256f` 以外と、`HID_ID` が読めないnodeは開かずに除外する。VIDが `256f` でも、interface番号か親USBデバイスのポートパスがsysfsから取れないnodeは開かずに `rejected`（`sysfs-incomplete`）にする。
2. 残ったnodeを `O_RDWR | O_CLOEXEC | O_NONBLOCK` で開く。`HIDIOCGRAWINFO` でbus type、VID、PIDを確かめる。
3. `HIDIOCGRDESCSIZE` / `HIDIOCGRDESC` でreport descriptorを読み、Report IDごとにFeature reportとInput reportのwire長を求める。解析規則は調査SDKの `hid_descriptor.py` と同じにする（PUSH/POPを扱い、long itemは飛ばし、同じIDのFeature itemは合算する）。
4. sysfsから `bInterfaceNumber`、親USBデバイスのパス、`HID_NAME` を集める。
5. §4で各nodeの役割を判定し、機器IDでまとめて§2のマウスとReceiverを組み立てる。

列挙では書き込みを一切しない。読み取り要求は、GET `0x08`（§4）と、Receiverのslot読み取り（GET `0x43..0x47`）だけを行う。

## 4. nodeの役割判定

| 役割 | 条件 | 根拠 |
|---|---|---|
| 有線の設定node | bus USB（`0x03`）、VID `256f`、PID `c658`、Feature `0x10` のwire長が32、Feature `0x08` のwire長が8 | 有線C658 MI_01（OBSERVED）。宣言しているのは1 interfaceだけだった |
| Receiverの設定node | bus USB、VID `256f`、PID `c652`、Feature `0x10` のwire長が32、Feature `0x08` のwire長が8 | C652では、占有slotと同じ番号のMI_Nがこの宣言を持った（MI_02/slot 2、MI_03/slot 3、MI_04/slot 4。1台のReceiver・1台のマウス・単一占有の条件、OBSERVED。調査SPEC「Receiver管理とLinux実装」） |
| Receiverの管理node | [05 §1](05-receiver.md#1-管理nodeの検出) | |

設定nodeと判定したnodeには、GET Feature `0x08` を長さ8で送る（**IDプローブ**）。

- 成功の条件: 戻り値が8、`resp[0] == 0x08`、`resp[1] == 0x59`。成功したら、`resp[2..8]` を機器IDとする。
- `0x59` は、slot報告のbyte 1（機種種別の候補）でも観測されている。C658であることを示すと推定しているが、意味は確定していない。本CLIは一致判定にだけ使う。
- 有線の設定nodeでプローブが失敗した場合、そのnodeは使えるが機器IDは無いものとして、§2.2の退避keyを使う。警告 `W-NO-DEVICE-ID` を出す。
- Receiverの設定nodeでプローブが失敗した場合、そのnodeは `rejected` にする。

判定できなかったnodeは、`rejected` と理由（例: `no-feature-0x10`、`probe-mismatch: 08 00 …`、`probe-error: EPIPE`、`open-error: EIO`、`rawinfo-mismatch: …`、`unsupported-product: 256f:xxxx`、`descriptor-invalid: …`）を `list --nodes` に表示する。理由には機器IDのbyteを含めない。

Receiverの設定nodeでプローブが失敗しても、そのnodeが管理nodeの条件（[05 §1](05-receiver.md#1-管理nodeの検出)）を満たすなら、管理nodeの候補としては残す。

次の場合は、その経路を `ambiguous-node` とし、その経路への送信を拒否する。
- 1つのUSBデバイスに、有線の設定nodeが2つ以上ある。
- 1台のマウス（同じ識別キー）に、同じ種類の経路が2つ以上ある（例: 1台のReceiverで同じ機器IDの設定nodeが2つ）。

## 5. Receiver経由の経路とslotの対応付け

- Receiverの設定nodeの機器IDを、同じReceiverの各slotの応答 bytes 2..7 と照合する。一致したslotを、その経路のslotとする。
- 設定nodeのinterface番号がslot番号と一致しない場合は、警告 `W-SLOT-IF-MISMATCH` を出す。これまでの観測ではすべて一致しているため、一致しないことは想定外である。
- 一致するslotが無い場合は `slot = unknown` とする。
- 1台のReceiverに2台以上のマウスを結合した場合の検証は、調査側のTODO 10で行う（Q14）。

## 6. マウスの選択

送信系のコマンドは、`--mouse=<selector>` で対象を選ぶ。

| selector | 意味 |
|---|---|
| （省略） | マウスが1台ならそれを選ぶ。0台なら `NoDevice`、2台以上なら `AmbiguousTarget` |
| `1`, `2`, … | `list` の番号。並び順はkeyの辞書順で固定する。接続中のマウスが変われば番号も変わる |
| key、またはその一意な接頭辞 | 識別キーで指定する。スクリプトではこの形を使う |

- 有線とReceiverの両方で見えている同じマウスは、1台として数える。
- `--route=wired|receiver` を付けると、有効な経路の代わりに指定した経路へ送る。
  - 指定した経路が `standby` なら、警告 `W-INACTIVE-ROUTE` を出す。有効でない経路に送ったときの効果は未確認である（調査の同時接続監査では判定不能）。
  - 指定した経路が存在しなければ `NoDevice` にする。
- 開発者向けに `--hidraw=<path>` を用意する。マウスの選択を飛ばして特定のnodeを使うが、§4の判定とIDプローブは飛ばさない。判定に通らなければ `DeviceInvalid` にする。
- 対象の `status` が `inaccessible` なら `PermissionDenied`、`ambiguous-node` なら `DeviceInvalid` にする。
- 開けないnode（`inaccessible`）は、別のマウスである可能性がある（P4）。
  - selectorを省略した場合、マウスが2台以上なら `AmbiguousTarget` にする。そうでなく、`inaccessible` のnodeが1つでもあれば、マウスが1台見えていても `PermissionDenied` にする。
  - keyや接頭辞で指定し、どれにも一致せず、`inaccessible` のnodeがあれば `PermissionDenied` にする。
  - 番号が範囲外なら `NoDevice` にする。

## 7. 送信

1. 判定に使ったfdを**開いたまま**送信に使う。列挙し直さず、開き直さない。
2. **送信直前の宛先確認。** 同じfdで、もう一度IDプローブ（GET `0x08`）を行う。
   - `resp[1] == 0x59` であり、かつ機器IDが選んだマウスの機器IDと一致することを確かめる。
   - 一致しなければ送信せず、`TargetChanged`（終了コード19）にする。
   - 機器IDの無い退避keyのマウスでは、`resp[1] == 0x59` だけを確かめる。
   - 目的: 再ペアリングや再列挙でnodeが入れ替わった後に、別のnodeや空のslotへ送ってしまうのを防ぐ。調査リポジトリの試験手順（HARDWARE_TEST.md）も、書き込みと同じfdでGET `0x08` を照合する形になっている。
   - 経緯: 調査で観測された「Receiverへの初回送信で効果が見えない」2事例は、記録上は正しいnode（MI_03、MI_04）へ送っていた。ただし当時はGETとSETが別のfdで行われており、原因は判定不能のままである（調査側 cross-route-management-2026-09）。
3. 32-byteのwire reportを `HIDIOCSFEATURE(32)` で**1回だけ**送る。
4. 戻り値で判定する。

| 戻り値 | 判定 |
|---|---|
| `32` | 成功（`sent`） |
| 0以上で32以外 | `SendFailed`（`short-write: <n>`） |
| errno | `SendFailed`（`errno: <name>`）。`ENODEV` など、デバイスが抜けたことを示すerrnoは区別して表示する |

手順2のGET自体がerrnoで失敗した場合は、送信せずに `SendFailed`（8）とする。応答はあるが一致しない場合だけを `TargetChanged`（19）とする。デバイスが抜けた場合（達成条件6）を終了コード8にそろえるためである。

5. 自動再送はしない。同じ内容でも送信を抑止しない。
6. 成功はホスト側での送信完了を意味する（P6）。CLIは、マウスへの適用を待ったり確認したりしない。
7. **Receiver経由の送信の案内（Q7）。** `set` / `apply` がReceiver経路へ送って成功したら、効果が現れるまで30秒ほどかかることがあり、失われることもあるので、変化がなければ同じコマンドをもう一度実行するよう注記する（"a send through the Receiver can take about 30 s to show and is sometimes lost; if nothing changes, run the same command again"、`-q` のときは出さない）。有線経路の送信には出さない。
   - 根拠: 実機確認（[実施 2](../hardware-test.md)）で、再ペアリング直後の最初の送信が約5分たっても効かず、送り直すと効いた。それ以外のReceiver経由の送信も、効果が見えるまで10〜30秒かかることがあった。調査側でも同様の2事例がある（上の手順2の経緯）。再現の条件と原因は分かっていない（調査側 [Issue #2](https://github.com/nejiman10/3dx-hid-research/issues/2)）。
   - 自動では再送しない（手順5）。読み戻しができず（§8）効いたかどうかを判定できないので、再送が要るかは利用者が効果を見て決める。送り直す内容は同じなので、効いていた場合に送り直しても設定は変わらない。

### 7.1 モード切り替えとの関係

有線モードとReceiverモードを切り替えると、送った設定と違う挙動に戻る事例が観測された。設定が経路をまたいで伝わらないのか、切り替え時に何かが再適用されるのかは判定できていない（調査側 UNKNOWN、TODO 13、Q16）。

Phase 1では次のように扱う。
- 送信は有効な経路だけに行う（§6）。
- `set` / `apply` の成功時、そのマウスに `standby` の経路があれば、"the <route> route is on standby; run `cadrat-tool apply` after switching modes" と注記する（`<route>` は待機中の経路名）（`-q` のときは出さない）。

## 8. 読み戻し

GET `0x10` による読み戻しは行わない。調査で32-byteの現在設定を得られていないため（OBSERVED）。
