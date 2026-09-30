# 設定ファイル

## 1. 場所

- 既定のパス: `$XDG_CONFIG_HOME/cadrat/default.toml`。`XDG_CONFIG_HOME` が未設定なら `~/.config/cadrat/default.toml`。XDG Base Directory仕様に従い、空や相対パスの `XDG_CONFIG_HOME` は未設定とみなす。
- `--config=<path>` で別のファイルを指定できる。開発中の試験用設定や、用途別の設定を使い分けるときに使う。
- 自動では作らない。作るのは `init` だけ（[tool/cli.md](tool/cli.md)）。

### 1.1 複数の設定ファイル

- Phase 1では、「プロファイル」の概念も一覧コマンド（`config list` など）も持たない。
  - 理由: CLIは状態を持たないので、「どのファイルが今マウスに入っているか」を記録できない。記録できない以上、一覧を出しても `ls ~/.config/cadrat/` と同じ情報にしかならない。
  - 運用: 使い分けたい設定は同じディレクトリに `<名前>.toml` として置き、`apply --config=...` で送る。マウスの中身は、最後に送ったファイルの内容になる。
- 1つの設定ファイルは、`--mouse` で選んだマウスに送られる。マウスごとに設定を紐付ける仕組みは持たない。
- `cadratd` が動いているときの扱い: `cadrat-tool` は送信系のコマンド（`set`、`apply`、`receiver pair` / `unpair`）を拒否し、`cadratctl` を使うよう案内する（終了コード20）。設定の正本がデーモンとTOMLの2つに分かれるのを防ぐため（Q9）。検出にはロックファイルを使う（[daemon §4](daemon/daemon.md#4-デバイスへの書き込みの排他q9)、[tool/cli §8](tool/cli.md#8-cadratd-との排他)）。
- プロファイルの切り替え、現在有効なプロファイルの記録、マウスの識別キー（[device §2.2](device.md#22-識別キー)）と設定の紐付けは、Phase 2bで `cadratd` が扱う。紐付けはデーモン用の別のファイルに置き、プロファイルのTOMLには機器IDを入れない。ファイル名を `default.toml` にしたのは、同じディレクトリにプロファイルが並ぶ将来の構成と矛盾しないようにするためである。

## 2. スキーマ（schema 1）

```toml
schema = 1

[mouse]
dpi = 1600              # 50..8200, in steps of 50
polling_rate = 1000     # 125 | 250 | 500 | 1000
wheel = "normal"        # "normal" | "inertial"

[mouse.lift]            # experimental on C658 (§4.4)
enabled = false
threshold = 31          # 0..255, used only when enabled = true

[buttons]               # all 7 entries are required
left    = "mouse:left"
right   = "mouse:right"
middle  = "mouse:middle"
wheel   = "mouse:middle"
forward = "mouse:forward"
back    = "mouse:backward"
radial  = "host:1"
```

### 2.1 共通規則

- **全キー必須。** 欠けていたら `ConfigIncomplete` エラーにし、欠けたキーをすべて列挙する。
- **未知のキーはエラー。** 打ち間違いを黙って無視しない（`ConfigInvalid`）。
- **欠けたキーと他の誤りが両方あれば `ConfigInvalid` とし、すべてを列挙する。** `[mouse]` などのテーブルが無ければ、その中のキーはすべて欠けているものとする。テーブルであるべき場所に別の値があれば、その誤りだけを示す。
- **`schema` が無い、または `1` 以外ならエラー。** `schema` がCLIの対応より新しい場合は、その旨をメッセージで示す。この場合、他のキーは別のschemaに従っている可能性があるので検証しない。
- **整数は10進と16進（`0x..`）の両方を受け付ける。** TOMLの仕様どおり。
- テーブルは `[mouse]` の形のほか、dotted key（`mouse.dpi = 1600`）とinline table（`mouse.lift = { … }`）でも書ける。TOMLの仕様どおり。
- 文字列の値は小文字のみ受け付け、大文字小文字を区別する。

## 3. フィールドとblobの対応

blob offsetの対応は、調査SPEC「Report 0x10」の [CONFIRMED] 項目に従う。

| TOMLキー | blob offset | 変換 |
|---|---:|---|
| （なし） | 0 | `0x00` |
| `mouse.dpi` | 1 | `dpi / 50` |
| `mouse.lift` | 2 | `enabled = false` なら `0x1f`、`true` なら `threshold` |
| `mouse.wheel` | 3..6 | `normal` → `01 ff 00 00`、`inertial` → `00 00 00 01` |
| （なし） | 7..17 | `0x00` |
| `buttons.left` … `buttons.radial` | 18..24 | §5のaction変換 |
| （なし） | 25 | `0x00` |
| （なし） | 26 | 固定値 `0x1e` |
| （なし） | 27..29 | `0x00` |
| `mouse.polling_rate` | 30 | 1000→`1`、500→`2`、250→`4`、125→`8` |

wire reportは `0x10` とblobを連結した32 byteになる。

## 4. 値の検証

### 4.1 dpi

- 50以上8200以下で、50の倍数でなければならない。それ以外は `ConfigInvalid` とする。
- 静的解析で得たgeneratorは範囲外をclampし、端数を切り捨てる。本CLIはどちらも**しない**。黙って値が変わるのを防ぐためである。
- 範囲内の値が実際にマウスでどう効くかは未検証（調査側 `UNKNOWN`）。

### 4.2 polling_rate

- `125` / `250` / `500` / `1000` のいずれかとする。

### 4.3 wheel

- `"normal"` / `"inertial"` のいずれかとする。

### 4.4 lift

- `threshold` は0..255。
- `enabled = true` で `threshold = 31`（`0x1f`）にすると、disabledと同じbyteになる。この場合は警告 `W-LIFT-AMBIGUOUS` を出す。
- `enabled = true` の設定を送るときは、毎回警告 `W-LIFT-EXPERIMENTAL` を出す。純正UIはC658でLift Detectionを表示しない（調査側 静的解析）。

## 5. action

各ボタンの値は `<種別>:<引数>` 形式の文字列とする。

| 値 | wire | 根拠 | 送信時の扱い |
|---|---|---|---|
| `mouse:left` | `0x0a` | CONFIRMED | — |
| `mouse:right` | `0x0b` | CONFIRMED | — |
| `mouse:middle` | `0x0c` | CONFIRMED | — |
| `mouse:backward` | `0x0d` | CONFIRMED | — |
| `mouse:forward` | `0x0e` | CONFIRMED | — |
| `unknown:6` | `0x0f` | wireはCONFIRMED、意味はUNKNOWN | 警告 `W-UNKNOWN-6` |
| `host:<N>`（N = 0..215） | `0x28 + N` | CONFIRMED | Nが1..7以外なら警告 `W-HOST-UNOBSERVABLE` |
| `raw:<0xNN>`（NN = `0x10`..`0x27`） | NN | 通常の経路からは生成されない | 警告 `W-RAW` |

- `host:N` の押下は、Report `0x03` のbitmapとしてホストに届く。1..7はbit 0..6に対応し、有線とReceiverの両方で観測済み（OBSERVED）。0と8以上はbitmapに現れる位置が無い。
- `unknown:6` は、正式名が見つかるまでこの名前を使う。`radial` などの名前は付けない（調査SPEC「Direct Action」）。
- `raw:` は、他の書き方で表せない `0x10`..`0x27` だけを受け付ける。`0x0a`..`0x0f` と `0x28` 以上は、名前付きの書き方を使うよう求めるエラーにする。

## 6. 物理ボタン名

| キー | blob offset | 物理ボタン |
|---|---:|---|
| `left` | 18 | 左 |
| `right` | 19 | 右 |
| `middle` | 20 | 中 |
| `wheel` | 21 | ホイールクリック |
| `forward` | 22 | 進む |
| `back` | 23 | 戻る |
| `radial` | 24 | radial |

根拠: 有線とReceiverの両経路の成功監査で、offset 18..24を1つずつ `host:1`（wire `0x29`）に変え、対応する物理ボタンの押下でReport `0x03` bitmap `0x01` が出ることを7 entryすべてで確認した（OBSERVED、調査SPEC「Report 0x10」）。7ボタン×全indexの組み合わせは試験していない。

## 7. 書き戻し

- **`toml_edit` で書き換える。** 変更したキーの値だけを置き換え、コメント、キーの順序、空行、他の値の表記（10進か16進か）は保つ。
  - 置き換える値の前後の空白と行末コメントは残す。元の値が16進（`0x..`）で書かれていれば、新しい値も16進で書く。
  - 指定された値が今の値と同じキーは書き換えない（`1_600` などの表記も残る）。
- **原子的に書き込む。** 同じディレクトリの一時ファイルに書いて `fsync` し、`rename` で置き換え、ディレクトリも `fsync` する。元ファイルのpermissionは引き継ぐ。
- **symbolic linkはたどる。** 設定ファイルがlinkなら、link先のファイルを置き換える（linkは残す）。lockファイルもlink先の隣に置く。
- **並行実行を防ぐ。** 同じディレクトリの `<設定ファイル名>.lock`（例: `default.toml.lock`）に `flock(LOCK_EX)` をかけ、読み込みから保存までの間を保護する。ロックが取れなければ5秒待ち、それでも取れなければ `ConfigLocked` エラーにする。
- **エディタでの同時編集を検出する。** 保存の直前にファイルを読み直し、読み込み時とSHA-256が違えば上書きしない（[tool/cli §4](tool/cli.md#4-set-の処理順序)）。

## 8. `init` が作るテンプレート

- 既定では、すべての値をコメントアウトしたテンプレートを作る。そのままでは `ConfigIncomplete` になるので、ユーザーが値を決めて書き込む必要がある。
- `--preset=research-baseline` を付けたときだけ、調査SDKの `latest_software_baseline()` と同じ値で埋める。ファイル冒頭には、実機から取得した値でも工場出荷値でもない旨のコメントを入れる。
  - 値: dpi 1400、lift無効、wheel normal、polling 1000、ボタンは left / right / middle / middle / forward / backward / middle。
  - 調査SDKの順序では第5 entryがforward、第6 entryがbackwardになる。本仕様の物理名対応では、`forward = "mouse:forward"`、`back = "mouse:backward"` になる。
- テンプレートのコメントは英語で書く（[tool/cli §5](tool/cli.md#5-出力)）。
- 既にファイルがあれば上書きせずにエラーにする（`--force` で上書き）。
