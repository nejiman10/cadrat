# 実機確認の手順と記録（Phase 1）

[仕様 04 §5](spec/04-implementation.md#5-phase-1-の達成条件) の達成条件を、実機で確かめる手順です。記録の形式は調査リポジトリの `HARDWARE_TEST.md` に倣います。

**状態: 実施 1・2 を完了（2026-09-28）。** 実施には、所有者の明示的な指示、復元値の記録、この手順書の3つが要る（[AGENTS.md](../AGENTS.md)「Safety」）。3つとも揃った（下の「記録」）。

## 0. 実施の前提

### 0.1 決まりごと

- **書き込みは所有者の設定を上書きする。** マウスから現在の設定を読み戻す手段は無い（仕様 02 §8）。始める前に、所有者が指定した32-byteの復元値を控える（§0.3）。
- **pair / unpair は結合状態を変える。** unpairの後、マウスはReceiverでは動かなくなる。有線ケーブル（またはBluetooth）で操作を続けられることを、始める前に確かめる。
- **識別子を記録しない。** 機器ID（GET `0x08` の bytes 2..7）とslotの識別子は、この文書にもコミットにも書かない。貼り付ける出力は `--redact` 付きで取るか、伏せてから貼る。同じかどうかは「一致」「不一致」とだけ書く。
- **ログは非公開の場所に置く。** `--json` の出力、ローカルパス、ユーザー名を含むログは、リポジトリの外（下の `$T/logs/`）に置く。ここには結果の要約だけを書く。
- **既定の設定ファイルを使わない。** すべてのコマンドに `--config` を付け、試験用のファイルだけを使う。
- **調査リポジトリのhold-open serviceはそのままでよい。** `cadrat-tool` はhidrawを一時的に開くだけなので共存できる（仕様 README §2）。
- 各段階で、予定と違う挙動が出たら**そこで止めて**、§G.1の手順で復元してから記録する。先に進まない。
- 調査リポジトリの知見と食い違う挙動は、調査リポジトリへ報告する（[AGENTS.md](../AGENTS.md)「Authority」）。

### 0.2 準備

```sh
# 作業用ディレクトリ（リポジトリの外）
T=~/cadrat-hwtest
mkdir -p "$T/logs"

# 試験に使う cadrat-tool（どちらか一方）
#   a) 試験ビルドの .deb（docs/packaging.md）
sudo apt install ./target/debian/cadrat-tool_*~test*_amd64.deb
#   b) ソースから入れ、udevルールを手で入れる
cargo install --path crates/cadrat-tool
sudo install -m 0644 udev/69-cadrat.rules /usr/lib/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger

# 環境を記録する
cadrat-tool --version
git -C <cadratのclone> rev-parse --short HEAD
lsb_release -ds; uname -r
```

以下では、試験用の設定ファイルを使う短縮形を使う。

```sh
ct() { cadrat-tool --config "$T/test.toml" "$@"; }
```

### 0.3 復元値

1. 所有者が指定した32-byteの復元値（`10 …`）を `$T/restore.hex` に保存する。実機から取った値か、所有者が意図して指定した値かを記録する。
2. 復元値を `cadrat-tool` の設定ファイル `$T/restore.toml` に書き直す。
   - 値の意味は、調査リポジトリのSDKで読める。`inspect_wire_report(bytes.fromhex(...))` を使う。
   - `cadrat-tool init --config "$T/restore.toml" --preset research-baseline` で作ったファイルを、読んだ値に合わせて編集する。
3. 書き直しが正しいことを、byte単位で確かめる。

   ```sh
   cadrat-tool --config "$T/restore.toml" get --wire --json \
     | python3 -c 'import json,sys; print(json.load(sys.stdin)["wire_hex"])'
   tr -d ' \n' < "$T/restore.hex"; echo
   ```

   2行が完全に一致しなければ、`restore.toml` は復元に使えない。予約byteが0でないなど、`cadrat-tool` の設定で表せない値のときは、復元には調査リポジトリの `restore-report10` を使う（§G.1）。
4. 試験用の設定ファイルを復元値から作る: `cp "$T/restore.toml" "$T/test.toml"`

### 0.4 試験値

復元値と違う値を選び、ここに記録してから始める。

| 項目 | 復元値 | 試験値 | 選び方 |
|---|---|---|---|
| `mouse.dpi` | （記録） | （記録） | 体感で区別できるよう、復元値から大きく離す（例: 復元値が1600なら400） |
| `buttons.radial` | （記録） | `host:1` | 復元値が `host:1` なら `host:2` |

### 0.5 入力の観察

`host:N` の効果は、Input Report `0x03` のbitmapで確かめる（`host:1` → `03 01`）。

1. 入力nodeを特定する必要はない。次のスクリプトは、指定したnodeをすべて同時に見て、Report `0x03` を受け取ったnodeと内容を表示する。有線ではC658の全nodeを、ReceiverではC652の全nodeを渡す（`ct list --nodes` に出るpath）。
2. 次のスクリプトを `$T/watch03.py` として置く。

```python
# usage: python3 watch03.py SECONDS /dev/hidrawA [/dev/hidrawB ...]
import os, select, sys, time
seconds, paths = float(sys.argv[1]), sys.argv[2:]
fds = {}
for path in paths:
    try:
        fds[os.open(path, os.O_RDONLY | os.O_NONBLOCK)] = path
    except OSError as e:
        print(f"{path}: {e}", file=sys.stderr)
counts = {}
end = time.monotonic() + seconds
while (left := end - time.monotonic()) > 0:
    for fd in select.select(list(fds), [], [], left)[0]:
        try:
            data = os.read(fd, 64)
        except BlockingIOError:
            continue
        if data[:1] == b"\x03":
            key = data[:2].hex(" ")
            counts[key] = counts.get(key, 0) + 1
            print(time.strftime("%H:%M:%S"), fds[fd], data.hex(" "), flush=True)
print("summary:", ", ".join(f"{k} x{n}" for k, n in sorted(counts.items())) or "no Report 0x03")
```

DPIの効果は、カーソルの速さの違いとして試験者が判断する。その旨を記録する。

## A. 準備の確認（書き込みなし）

| # | 手順 | 期待 | 対応する条件 |
|---|---|---|---|
| A1 | cadratのcloneで `cargo test` | 全テスト成功（ベクタテストを含む） | 1 |
| A2 | 有線で接続し、`ct list --redact` | マウスが1台、`ACTIVE` が `wired` | 2 |
| A3 | `ct list --nodes --redact` | 有線の設定nodeが `candidate (wired setting, …)` | 2 |
| A4 | `ct check` | `ok`（復元値の設定に警告があれば記録） | — |
| A5 | `ct apply --dry-run` | 表示されたwireが `$T/restore.hex` と一致 | — |

## B. 有線での送信（条件 2）

有線で接続したまま行う。

| # | 手順 | 期待 |
|---|---|---|
| B1 | `ct set mouse.dpi=<試験値>` | 終了コード0。`sent` と `saved` が出る |
| B2 | カーソルを動かす | 速さが変わる（試験者の判断を記録） |
| B3 | `ct set buttons.radial=host:1` | 終了コード0 |
| B4 | `python3 $T/watch03.py 30 <C658の全node>` を動かし、radialを10回押す | `03 01` の押下と `03 00` の解放が10組 |
| B5 | `cadrat-tool --config "$T/restore.toml" apply` | 終了コード0。DPIとradialが元に戻る（B2、B4と同じ方法で確かめる） |
| B6 | `ct get` | B1、B3の値が保存されている。コメントと並びは変わっていない（`diff "$T/restore.toml" "$T/test.toml"` で、変わったのが2行だけであること） |

## C. Receiver経由での送信（条件 3、3a）

マウスをReceiverモードにし、有線ケーブルを外す。

1. C0: `ct list --redact` で、`ACTIVE` が `receiver` のマウスが1台と、slot番号が出ることを確かめる。
2. B1〜B5を同じように行う（入力nodeはReceiver側に読み替える）。
3. **条件 3a**: 各送信（C-B1、C-B3、C-B5）について、1回目の送信で効果が現れたかを記録する。現れなかった場合は、次の3点を記録して調査リポジトリに報告する。
   - 送信先のnodeとinterface（`sent_via`）
   - 宛先確認の結果（終了コード0なら一致）
   - 送信後の操作（マウスを動かしたか、何秒後に確かめたか）

## D. 有線とReceiverの同時接続（条件 4、4a）

| # | 手順 | 期待 | 条件 |
|---|---|---|---|
| D1 | Receiverを挿したまま、マウスを有線モードでケーブル接続し、`ct list --redact` | マウスは1台（2経路）。`ACTIVE` は `wired`、receiverは `standby` | 4 |
| D2 | `ct apply` | `via wired`。`note … receiver route is on standby …` が出る | 4 |
| D3 | マウスをReceiverモードに切り替え、ケーブルを抜いて `ct list --redact` | 同じマウス（keyが一致）。`ACTIVE` は `receiver` | 4、4a |
| D4 | `ct apply`（`--mouse` なし） | `via receiver` | 4 |
| D5 | 有線に戻し、ケーブルを一度抜き差しして `ct list` | keyが最初と一致（一致・不一致だけを記録） | 4a |
| D6 | `ct apply --route=receiver` | `W-INACTIVE-ROUTE` が出る。効果の有無を記録する（未検証の事項、仕様 02 §6） | — |

経路を切り替えた後に設定が変わって見えたら、その様子を記録する（調査側TODO 13、Q16）。

## E. 失敗時の振る舞い（条件 5、6、7）

### E1. udevルールが無い場合（条件 5）

```sh
sudo mv /usr/lib/udev/rules.d/69-cadrat.rules "$T/"
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=hidraw --action=change
# マウスとReceiverを抜き差しする
ct list; echo "exit=$?"               # W-INACCESSIBLE とudevルールのヒント
ct apply; echo "exit=$?"              # 終了コード 6（PermissionDenied）とヒント
sudo mv "$T/69-cadrat.rules" /usr/lib/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=hidraw --action=change
ct list --redact                      # 元どおり見えること
```

`.deb` で入れた場合は、ルールのパスが同じなのでこの手順のままでよい。終わったら `dpkg -V cadrat-tool` で、ファイルが元どおりであることを確かめる。

### E2. 送信中にマウスを抜く（条件 6）

送信は数ミリ秒で終わるので、ちょうど送信の最中に抜くのは手では狙えない。そこで、送信を繰り返している間に有線ケーブルを抜き、次の2点を確かめる。

- どの失敗でもTOMLが変わらないこと
- 終了コード8が出るか

```sh
for i in $(seq 1 300); do
  dpi=$([ $((i % 2)) -eq 0 ] && echo <試験値> || echo <復元値のdpi>)
  before=$(sha256sum "$T/test.toml" | cut -c1-64)
  ct -q set mouse.dpi=$dpi; code=$?
  after=$(sha256sum "$T/test.toml" | cut -c1-64)
  echo "$i code=$code toml_changed=$([ "$before" = "$after" ] && echo no || echo yes)"
  [ $code -ne 0 ] && break
done | tee "$T/logs/unplug.log"
```

ループが動いている間にケーブルを抜く。

- **PASS の条件**: 0以外の終了コードの行で、`toml_changed=no` であること。
- **終了コード**: 8（送信中または送信直前の確認で切断）、4（列挙時点で見つからない）、19のどれが出たかを記録する。
- **8 が出なかった場合**: 最大3回まで繰り返す。それでも8が出なければ「8は再現できず」と記録する。

### E3. `set` の最中にエディタでTOMLを書き換える（条件 7）

`set` の読み込みから保存までは数ミリ秒なので、手で書き換えを挟むことはできない。この条件はデバイスに依存しない。そこで、試験機で自動テストを実行して確かめる。

```sh
cargo test -p cadrat-tool --test cli concurrent_edit_is_not_overwritten
```

このテストでは、送信の最中（fake transportが送信を受け取った瞬間）にファイルを書き換えます。そのうえで、次の3点を確かめます。
- 終了コードが9になること
- エディタでの変更が残ること
- 送ったwireと、反映すべき変更が表示されること

**合意済み（2026-09-28）:** 条件6と条件7は、仕様どおりの実機操作では再現できない。この手順（条件6は繰り返し送信中に抜く、条件7は自動テスト）で達成とみなすことを、所有者が承認した。

## F. Receiverの管理（条件 8、9、10、4a、4b）

**始める前に:** 有線ケーブルでマウスを操作に戻せることを確かめておく。unpairの後、マウスはReceiverでは動かない。

| # | 手順 | 期待 | 条件 |
|---|---|---|---|
| F1 | Receiverモードで `ct receiver slots --redact` | マウスのslotが `occupied  type 0x59 … → mouse 1 …` | 8 |
| F2 | `ct list --redact` を記録し、`ct list --nodes` でReceiver経路のhidraw pathを控える（非公開） | — | 4b |
| F3 | `ct receiver unpair <slot>` と入力し、確認で `n` | 終了コード18。`ct receiver slots` が変わらない。マウスは動く | 10 |
| F4 | `ct receiver pair` を実行し、マウスを操作せずに5秒後にCtrl-C | 終了コード12、`interrupted; pairing mode was stopped` | 9 |
| F5 | F4の直後に、マウスを30秒ペアリング操作する | slotが変わらない（pairing modeが止まっていることの補助確認） | 9 |
| F6 | `ct receiver unpair <slot>` と入力し、確認の表示を見て `y` | 終了コード0、`unpaired slot N`、slotが `empty`。マウスがReceiverで動かない。`W-UNPAIR-EPIPE` が出たら記録 | 8 |
| F7 | `ct receiver pair` を実行し、表示に従ってマウスをペアリング操作する | 終了コード0、`paired  slot M`。以前と同じslotか違うslotかを記録 | 8 |
| F8 | `ct list --redact` | マウスが再び見え、keyがF2と一致（一致・不一致を記録）。設定nodeのinterfaceはslot Mと同じ番号 | 8、4a |
| F9 | `cadrat-tool --config "$T/test.toml" apply --hidraw <F2で控えたpath>` | 終了コード7または19。何も送られない（F2と違うnodeに移った場合）。同じnodeのままなら、そのことを記録する | 4b |
| F10 | `ct apply` を実行し、入力（B4と同じ方法）を確かめる | 終了コード0、設定の効果が現れる | 8 |

F6〜F10の結果（slotの空き→再占有、入力と設定の反映）を、調査リポジトリの結合監査（`evidence/receiver-repair-2026-09`）と比べる。

## G. 終了と記録（条件 11）

### G.1 復元

1. `cadrat-tool --config "$T/restore.toml" apply` で復元する。有線で行うのが確実。
2. B2、B4と同じ方法で、元の動作に戻ったことを確かめる。
3. `restore.toml` で表せない値のとき、または `cadrat-tool` が使えないときは、調査リポジトリの `restore-report10` で `$T/restore.hex` を送る（調査リポジトリ `HARDWARE_TEST.md` の「復元と緊急時」）。
4. 利用者の既定の設定ファイル（`~/.config/cadrat/default.toml`）を使っているなら、その内容がマウスに入っている状態に戻したいときは `cadrat-tool apply` を実行する。

### G.2 片付け

- `.deb` を入れた場合、試験後も使い続けるかを所有者と決める。外すときは `sudo apt remove cadrat-tool` を実行する。
- `$T/logs/` は非公開のまま保管する。

## 記録

実施したら、この節に追記する。実施日ごとに節を分ける。識別子の実値、ローカルパス、ユーザー名は書かない。

```markdown
### 実施 1（YYYY-MM-DD）

- 実施の指示: （所有者の指示を受けた日時と範囲）
- cadrat: commit `xxxxxxx`、`cadrat-tool --version` の出力
- 環境: Ubuntu の版、カーネル、接続したポート（USB 2 / 3 程度の区別）
- 復元値: 実機から取得 / 所有者の指定（値は書かない）。restore.toml との一致: 一致
- 試験値: dpi …、radial …

| # | 結果 | 観察 |
|---|---|---|
| A1 | PASS / FAIL / INCONCLUSIVE | … |
| … | | |

- 条件 3a（Receiverへの初回送信の効果）: …
- 調査リポジトリへの報告: なし / あり（内容）
- 復元の確認: …
```

結果は PASS / FAIL / INCONCLUSIVE のいずれかにする。判定できない場合は INCONCLUSIVE とし、条件と限界を書く（調査リポジトリと同じ扱い）。

### 実施 1（2026-09-28〜）

- 実施の指示: 2026-09-28、所有者から実施可能との指示を受けた。範囲はこの手順書の全段階（A〜G）。条件6・7の代替手順（§E2、§E3）も承認を受けた。
- 復元値: 所有者の指定（実機から読んだ値ではない）。内容は `init --preset research-baseline` の設定と同じで、`get --wire` の出力と byte 単位で一致することを確認した。
- 試験値: dpi 400、radial `host:1`（復元値は dpi 1400、radial `mouse:middle`）

| # | 結果 | 観察 |
|---|---|---|
| A1 | PASS | 全テスト成功（計158件）。`cadrat-tool 0.1.0~test3+g4174787 (test build)`（`.deb` 試験ビルド）、Ubuntu 24.04.5 LTS、カーネル 7.0.0-34-generic |
| A2 | PASS | マウス1台。有効な経路は wired。Receiver経路（slot 4、MI_04）も standby として同じマウスにまとまった |
| A3 | PASS | 有線: MI_01が設定node、MI_00は `no-feature-0x10`（調査側Q2と一致）。Receiver: MI_00〜MI_04すべてが管理候補、MI_04がslot 4の設定node（MI_N ↔ slot N と一致）。管理nodeにはMI_00を選んだ |
| A4 | PASS | `ok` |
| A5 | PASS | dry-runのwireが復元値と完全に一致 |
| B1 | PASS | 終了コード0、`via wired (MI_01)`、`saved`。standbyのreceiver経路について `note` が出た |
| B2 | PASS | 試験者の判断: 明らかに遅くなった |
| B3 | PASS | 終了コード0。wire byte 25 が `29` |
| B4 | PASS | 30秒で `03 01` ×10、`03 00` ×10 |
| B5 | PASS | 復元値の送信は終了コード0。その後15秒の radial 押下で Report `0x03` なし（中クリックに戻った）。DPIも元の速さに戻った |
| B6 | PASS | `get` は 400 と `host:1`。`restore.toml` との差分は dpi と radial の2行だけ。置き換えた値の前後の空白と行末コメントは残った |
| C0 | PASS | Receiverモードでケーブルを外すと、マウス1台で `ACTIVE` は receiver（slot 4、MI_04）。wired経路は表示されない |
| C-B1 | PASS | `apply`（dpi 400、radial `host:1`）は終了コード0、`via receiver (MI_04)`。1回目の送信で遅くなった（送信の約1秒後に確認） |
| C-B4 | PASS | マウスを動かしてから約10秒後に開始。30秒で `03 01` ×10、`03 00` ×10（MI_04のnodeで受信） |
| C-B5 | PASS | 復元値の送信は終了コード0。送信直後にマウスを動かさずradialを押すと、1回目だけ `03 01` / `03 00`（古い割り当て）が出て、その後は中クリックに戻った。DPIも戻った |

- 条件 3a（Receiverへの初回送信の効果）: 3回の送信（C-B1、C-B4の前、C-B5）はすべて1回目の送信で効果が現れ、送り直しは要らなかった。ただしC-B5では、送信直後の最初の押下1回だけが古い割り当てで処理された。調査SPECの HYPOTHESIS（Receiverは書き込み後の転送機会まで新しい割り当てを反映しないことがある）と、index切り替え監査の OBSERVED（切り替え直後に旧bitmapが一時的に出た）に沿う。押下そのものが転送機会になった可能性があるが、この試験だけでは区別できない。調査リポジトリへ報告する。
| D1 | PASS | Receiverを挿したまま有線接続: マウス1台（2経路）、ACTIVE wired、receiver standby |
| D2 | PASS | `apply` は `via wired (MI_01)`、standbyのreceiver経路について `note` |
| D3 | PASS | Receiverモードでケーブルを外すと ACTIVE receiver。keyの指紋（SHA-256の先頭8桁）がD1と一致 |
| D4 | PASS | `--mouse` なしの `apply` は `via receiver (MI_04)` |
| D5 | PASS | 有線に戻してケーブルを抜き差し: ACTIVE wired、keyの指紋がD1と一致 |
| D6 | 観察 | 有線モードのまま `apply --route=receiver`（dpi 400、radial `host:1`）: `W-INACTIVE-ROUTE`、送信は終了コード0。有線で使っているマウスに効果は無かった（カーソルは遅くならず、radialでReport `0x03` なし） |
| D7 | PASS | 両経路に復元値を送った（receiverは `W-INACTIVE-ROUTE` 付き）。いずれも終了コード0 |

- 条件 4a: モード切り替えとケーブルの抜き差しの前後で、マウスのkeyは変わらなかった（指紋の一致で確認）。
- D6の観察: standbyのReceiver経路への送信は、ホスト側では成功しても、有線モードで使用中のマウスには反映されなかった。その送信がReceiverモードへ切り替えた後に効くかは、D7で両経路を復元してから切り替えたため判定していない（仕様 02 §6 の未検証事項、調査側 TODO 13 / Q16 の材料）。調査リポジトリへ報告する。
- モード切り替え直後に設定が変わって見える現象（Q16）は、この段階では報告されなかった。
| E1 | PASS | udevルール（cadratと調査リポジトリの2つ）を退避して抜き差し: `list` は7 nodeを `W-INACCESSIBLE` とヒントで示し終了コード0、`apply` は終了コード6（PermissionDenied）とヒント。ルールを戻すと元どおり見え、`dpkg -V cadrat-tool` も異常なし |
| E2 | PASS | 1回目: 4回目の `set` の送信中に切断され、終了コード8（`errno: EPIPE`）、`toml_changed=no`。2回目: 切断途中で有線nodeのIDプローブが失敗し、その経路が退避key（`c658-port:…/if1`）の別マウスとして数えられ、終了コード5（AmbiguousTarget）で何も送らず `toml_changed=no`。3回目: ループが止まらなかった（ケーブルを抜いた後もReceiver経路で送信が続いたと考えられるが、`-q` のため経路の表示は無く判定不能） |
| E3 | PASS | `concurrent_edit_is_not_overwritten` を試験機で実行し成功 |

- 条件 6: 0以外の終了コードで終わった試行（8と5）では、どちらもTOMLが変わらなかった。切断時のerrnoは `ENODEV` ではなく `EPIPE` だったため、「device disconnected」の表示は付かなかった（仕様 02 §7 の「区別して表示する」の対象外）。
- E2 2回目の観察: 切断途中にプローブが失敗した有線nodeは退避keyのマウスとして扱われ、同じマウスのReceiver経路とは別の1台として数えられた（仕様 02 §2.2 のとおり）。そのため、推測で選ばずに止まった。
| F0 | 観察 | 有線で復元値（dpi 1400）を送った後にReceiverモードへ切り替えると、dpiは400のままだった（E2の3回目でReceiver経由の送信が続いた結果と考えられる）。送信せずにUSBを抜き差しすると、1400に変わる場合と変わらない場合があった。Bluetoothで接続していた可能性もあり、判定不能。Receiver経由でも復元値を送って続行した |
| F1 | PASS | slot 4 が `occupied type 0x59 … → mouse 1` |
| F2 | PASS | Receiver経路の設定nodeは MI_04。keyの指紋はD1と一致 |
| F3 | PASS | 確認で `n` → 終了コード18、`not confirmed; nothing was done`。slotは変わらず、マウスも動作した |
| F4 | PASS | pair開始の約5秒後にCtrl-C → 終了コード12、`interrupted; pairing mode was stopped` |
| F5 | PASS | その後マウスを30秒ペアリング操作しても、slotは変わらなかった |
| F6 | FAIL | 確認で `y`。解除要求は `EPIPE`（`W-UNPAIR-EPIPE`）を返し、直後の slot 4 の読み取りも `EPIPE` で失敗したため、cadratは待機をやめて終了コード17を返した。実際には解除されていた（マウスはReceiverで動かなくなり、F8では slot 4 が空き）。解除直後のGETの一時的な失敗で打ち切る実装の不具合 |
| F7 | PASS | pairの案内に従って操作し、終了コード0、`paired  slot 0`（以前の slot 4 とは違うslot） |
| F8 | PASS | マウスが再び見え、keyの指紋はD1と一致。設定nodeは MI_00（slot 0 と同じ番号）。管理nodeも同じ MI_00 |
| F9 | PASS | 古いパス（以前の MI_04）を `--hidraw` で指定 → 終了コード7、`not a setting node`。何も送られない |
| F10 | PASS | `set`（dpi 400、radial `host:1`）は `via receiver (MI_00)`、終了コード0。直後30秒の観察では Report `0x03` なし、カーソルも遅くならなかったが、送信の約10秒後に遅くなった。送り直さずに再観察すると `03 01` ×10、`03 00` ×10 |

- 条件 8: slotの空化と再占有、その後の入力と設定反映は確認できた。ただしF6でunpairの成否判定を誤ったため、修正後に再試験する。
- 条件 4a: 再ペアリング（slot 4 → slot 0）の後もkeyは変わらなかった。
- 条件 4b: 古いhidraw pathは終了コード7で止まった。
- F10の観察: 再ペアリング直後のReceiver経由の送信は、約10秒遅れて効いた（条件 3a と同様、送り直しは不要）。
- F0とD6の観察: 設定が経路ごとに保持されているように見える（調査側 TODO 13 / Q16）。調査リポジトリへ報告する。

F6の修正（commit `2657786`、試験ビルド test4）の後、unpair → pair の1サイクルを再試験した。マウスは slot 0（設定nodeは MI_00）にいた。

| # | 結果 | 観察 |
|---|---|---|
| F6′ | FAIL | `receiver unpair 0` に `y`。管理nodeには MI_00 が選ばれた（unpair対象の slot 0 の設定nodeと同じinterface）。解除後、そのfdへのslotのGETは timeoutまでの31回すべて `ENODEV` で、終了コード15（`did not become empty before the timeout`）。実際には解除されていた（マウスはReceiverで動かなくなり、F8′で slot 0 は空き） |
| F7′ | FAIL | `receiver pair`。待機中のGETは55回すべて `ENODEV` で、新しいslotを検出できないまま timeout。停止のSETも `ENODEV` で失敗し、終了コード13とヒントを表示した。マウスは slot 1 に結合され、動くようになったが、Receiverのpairing表示はReceiver自身のtimeoutまで続いた |
| F8′ | PASS | マウスは slot 1（設定nodeは MI_01）で見え、keyの指紋はD1と一致 |
| F10′ | PASS | `apply`（dpi 400、radial `host:1`）。マウスを動かしたりクリックしたりしても、効くまで約30秒かかった。その後は遅くなり、Report `0x03` も確認できた |
| G1 | PASS | Receiver経路に復元値を送り、すぐにDPIとradial（中クリック）が元に戻った |
| G2 | PASS | 有線経路にも復元値を送り、元どおりであることを確認した |

- 条件 8: 実施 1 では未達。slotの空化・再占有と、その後の入力・設定反映は2サイクルとも起きたが、cadratの成否判定が2サイクルとも実際と食い違った（F6: 17、F6′: 15、F7′: 13）。
- F6′とF7′の観察: slotが変わった後、開いていた管理nodeのfdが `ENODEV` を返し続けた。F6（slot 4 の解除、管理nodeは MI_00）では `EPIPE` が1回だけだったので、解除したslotと管理nodeのinterfaceが同じだったことが関係している可能性がある。F7′では、新しいslot（1）と管理node（MI_00）のinterfaceが違ってもfdが使えなくなった。hidraw nodeの再作成か、Receiver全体の再列挙かは、この記録からは区別できない。調査リポジトリへ報告する。
- 遅延の観察: 再ペアリング後のReceiver経由の送信は、効くまでに約10秒（F10）と約30秒（F10′）かかった。いずれも送り直しは要らなかった。
- 後片付け: 両経路を復元した。`.deb`（試験ビルド test4）は所有者の希望で入れたまま使い続ける。

### 実施 1 のまとめ

| 条件 | 結果 |
|---|---|
| 1 ベクタテスト | PASS（A1） |
| 2 有線での送信 | PASS（B） |
| 3 Receiver経由の送信 | PASS（C） |
| 3a 初回送信の効果 | 記録済み（C、F10、F10′。いずれも送り直し不要、効果の遅れあり） |
| 4 同時接続と有効な経路 | PASS（D） |
| 4a 識別キーの安定 | PASS（D、F8、F8′） |
| 4b 古いhidraw pathの拒否 | PASS（F9） |
| 5 udevルールなし | PASS（E1） |
| 6 送信中の切断 | PASS（E2、承認済みの代替手順） |
| 7 同時編集 | PASS（E3、承認済みの代替手順） |
| 8 unpair → pair の1サイクル | **未達**（管理nodeのfdがslotの変化の後に使えなくなる。修正と再試験が必要） |
| 9 pair待機中のCtrl-C | PASS（F4） |
| 10 unpair確認の拒否 | PASS（F3） |
| 11 記録 | この文書 |

### 実施 2（2026-09-28、条件 8 の再試験）

- 実施の指示: 2026-09-28、所有者から「開き直して続行する修正を、確認まで行う」との指示を受けた。範囲は条件 8 の再試験と復元。復元値は実施 1 と同じ。
- cadrat: commit `2a45ea6`、`cadrat-tool 0.1.0~test5+g2a45ea6 (test build)`（仕様 05 §6 の開き直しを含む）
- 開始時の状態: マウスは slot 1。管理nodeは MI_00

| # | 結果 | 観察 |
|---|---|---|
| R1 | PASS | `receiver unpair 1` に `y`: 終了コード0、`unpaired slot 1`。`W-UNPAIR-EPIPE`、`W-SLOT-READ-RETRY`（3回、最後は `ENODEV`）、`W-MANAGEMENT-REOPENED`（1回。途中の試みは `no matching device is connected`）。解除後の全slotは空き |
| R2 | PASS | `receiver pair`: 終了コード0、`paired  slot 2`。見出しの管理nodeは一時的に別の番号（MI_00 のまま、hidraw番号だけ変化）。`W-SLOT-READ-RETRY`（2回、`ENODEV`）、`W-MANAGEMENT-REOPENED`（1回。途中の試みは、新しい5つのnodeを開けない `EACCES`）。pairing表示は成立時に点滅から点灯に変わった |
| R3 | PASS | マウスは slot 2（MI_02）で見え、keyの指紋はD1と一致 |
| R4 | PASS | `receiver unpair 2` に `y`: 終了コード0。`W-SLOT-READ-RETRY`（3回、`ENODEV`）、`W-MANAGEMENT-REOPENED`（1回） |
| R5 | PASS | `receiver pair`: 終了コード0、`paired  slot 3`。`W-SLOT-READ-RETRY`（2回）、`W-MANAGEMENT-REOPENED`（1回）。停止も成功 |
| R6 | PASS | マウスは slot 3（MI_03）で見え、keyの指紋はD1と一致 |
| R7 | 観察 | `apply`（dpi 400、radial `host:1`）は `via receiver (MI_03)`、終了コード0。約5分マウスを使いながら待ったが効果が現れなかった（30秒の観察4回とも Report `0x03` なし） |
| R8 | PASS | もう一度 `apply` を送ると、直後から効いた（radialで `03 01` / `03 00` ×3） |
| R9 | PASS | Receiver経路、続いて有線経路に復元値を送った。いずれも終了コード0 |

- 条件 8: PASS。2サイクルとも、slotの空化と再占有をcadratが正しく判定した。解除のたびに管理nodeのfdが `ENODEV` になったが、開き直して続行できた（仕様 05 §6）。
- 開き直しの途中の失敗理由（`no matching device is connected`、新しいnodeの `EACCES`）から、slotが変わるとReceiverの全interfaceのhidraw nodeが作り直され、udevの権限付与が一瞬遅れると考えられる。調査リポジトリへ報告する。
- slotの割り当て: 実施 1・2 を通じて 4 → 0 → 1 → 2 → 3 と、毎回ひとつ先のslotに結合された。調査リポジトリへ報告する。
- 条件 3a / Q7: 再ペアリング直後の最初の送信（R7）は効果が現れず、送り直し（R8）で効いた。調査で観察されていた「Receiverへの初回送信で効果が見えない」事例の再現である。仕様 04 のQ7に従い、方針を見直す。

### 実施 2 の後の条件

実施 1 と 2 で、仕様 04 §5 の条件 1〜11 をすべて満たした（条件 6・7 は承認済みの代替手順）。ただし条件 3a の記録から、Q7（Receiverへの初回送信）の方針の見直しが要る。

