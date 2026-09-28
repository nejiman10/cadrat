# cadrat-tool 仕様（Phase 1: 独立設定ツール）

状態: **草案 v0.17**（2026-09-28）。実装前の合意用。

## 0. cadrat プロジェクトの構成

| 名前 | 役割 | 状態 |
|---|---|---|
| `cadrat-tool` | 独立設定ツール。hidrawを直接操作し、状態はTOMLだけが持つ | **本仕様（Phase 1）** |
| `cadratd` | デーモン。hidrawを保持し、設定を管理し、D-Busで公開する | Phase 2以降 |
| `cadratctl` | デーモンのフロントエンド。D-Bus経由でだけ操作し、hidrawには触らない | Phase 2以降 |
| cadrat Radial | GNOME Shell拡張。D-Busで `cadratd` につなぐ | Phase 2以降 |

- `〜ctl` はLinuxの慣習（`systemctl`、`bluetoothctl`、`ratbagctl`）に合わせ、デーモンのフロントエンドに使う。独立ツールには使わない。
- 3つは別々の実行ファイルとする。`cadrat-proto`、`cadrat-hidraw`、`cadrat-config` のcrateと、設定スキーマを共有する。
- `cadrat-tool` はデーモンができた後も、デバッグとデーモンを使わない環境のために残す。
- 本プロジェクトは3Dconnexionとは無関係の非公式プロジェクトである。READMEにその旨を明記する。

## 1. 目的

CadMouse Compact Wireless（C658）の設定を、Linux上でデーモンを介さずに変更する独立CLIを作る。このPhaseの狙いは機能の多さではなく、後のデーモン（`3dxd`）とGNOME拡張がそのまま再利用できる次の3つを成熟させることにある。

1. **プロトコルコア**: Report `0x10` の生成・検査、Receiver管理packetの生成、slot応答の解析（I/Oなし）
2. **Linux transport**: hidrawの列挙、送信先の自動検出・検証、送信
3. **設定ファイル**: TOMLスキーマと、コメントを保つ安全な書き戻し
4. **Receiver管理**: slotの読み取り、pair、unpairと、その成否の判定

## 2. 範囲

| 含む | 含まない（後のPhase） |
|---|---|
| 有線C658とReceiver（C652）経由C658へのReport `0x10` 送信 | デーモン、D-Bus、常駐、hold-open |
| TOMLの読み込み・検証・部分更新・書き戻し | 複数プロファイル、アプリ別設定 |
| マウス単位の列挙、識別キー、自動選択と明示指定 | Report `0x03` / `0x17` の監視（`monitor`） |
| TOMLから作ったwire値の表示 | 送信履歴などのCLI側状態保存 |
| Receiverのslot読み取り、pair、unpair | プロファイルの切り替え、マウスごとの設定の紐付け |
| `--json` 出力 | |

この段階のhold-openは、調査リポジトリの既存user service（`c658-hidraw-hold-open.service`）に任せる。CLIはhidrawを一時的に開くだけなので共存できる。

## 3. 基本原則

- **P1 TOMLが唯一の設定正本。** デバイスから現在設定を読み戻す手段はない（Windows版にも実装は見つかっていない）。CLIプロセスは状態を持たず、状態はTOMLファイルだけが持つ。
- **P2 常に完全snapshotを送る。** 部分変更コマンドも、TOML全体と合成した31-byte blobを送る。
- **P3 推測で埋めない。** TOMLが無い、または欠けたフィールドがある場合は送信しない。実験用baselineを暗黙の既定値として使わない。
- **P4 推測で選ばない。** 対象のマウスが複数あればエラーで止める。
- **P5 送信成功後に保存する。** `set` は送信が成功してからTOMLを保存する。送信に失敗したらTOMLは変えない。
- **P6 送信成功はホスト側の完了を意味する。** 送信成功はioctlの完了であり、マウスへの適用の証明ではない。表示や出力でもこの区別を保つ。
- **P7 結合状態の変化は結果で判定する。** pair/unpairの成否はSETの戻り値では決めず、前後のslot snapshotの変化で判定する。
- **P8 取り消しにくい操作は確認してから行う。** unpairは、対象を表示して確認を取り（`--yes` で省略可）、確認から実行までに対象slotが変わっていないことをCLIが照合してから実行する。
- **P9 利用者はマウス単位で扱う。** hidraw nodeは実装詳細とし、マウスには機器IDから作った安定した識別キーを付ける。このモデルと識別キーは、後のデーモンでもそのまま使う。
- **P10 送る直前に宛先を確かめる。** 送信は1回だけ行い、その直前に同じfdで機器IDを読み直して、選んだマウスと一致することを確かめる。

## 4. 文書構成

| 文書 | 内容 |
|---|---|
| [01-config.md](01-config.md) | TOMLスキーマ、値の検証、wireへの対応 |
| [02-device.md](02-device.md) | デバイスモデル（マウス・Receiver・識別キー）、検出、選択、送信 |
| [03-cli.md](03-cli.md) | コマンド体系、`set` の処理順序、出力、終了コード |
| [05-receiver.md](05-receiver.md) | Receiver管理nodeの検出、slotの読み取り、pair/unpairの手順と判定 |
| [04-implementation.md](04-implementation.md) | crate構成、テスト方針、達成条件、未決事項 |

## 5. 調査リポジトリとの関係

プロトコルの事実は [nejiman10/3dx-hid-research](https://github.com/nejiman10/3dx-hid-research) の `SPEC.md` を正本とし、本仕様はそれを引用するだけとする。参照基準は commit `6b151ae`（2026-09-27）。

本仕様内のプロトコル記述には、調査側の根拠ラベル（`CONFIRMED` / `OBSERVED` / `HYPOTHESIS` / `UNKNOWN`）を付ける。調査側のSPECが更新されたら、影響箇所を洗い出して本仕様を改訂する。

コードは共有しない。調査リポジトリのPython SDKは参照実装として残し、両者はテストベクタ（[04-implementation.md](04-implementation.md#3-テストベクタ)）でつなぐ。

## 6. 用語

| 用語 | 意味 |
|---|---|
| blob | Report `0x10` の31-byte payload |
| wire report | Report ID `0x10` とblobを合わせた32 byte |
| 物理ボタン名 | blob offset 18..24 の7 entryに付ける名前（`left` … `radial`） |
| action | 各物理ボタンに割り当てる1 byteのwire値の意味（direct / host-routed / raw） |
| マウス | 利用者が扱う単位。機器IDで識別し、1つまたは2つの経路（route）を持つ（[02 §2](02-device.md#2-デバイスモデル)） |
| route | `wired`（C658直結）または `receiver`（C652経由）。有効な経路は1つで、もう一方は `standby` |
| 機器ID | GET `0x08` 応答の bytes 2..7。有線・Receiverの両経路で同じ値になる |
| 識別キー | マウスやReceiverを安定して指す文字列（[02 §2.3](02-device.md#23-識別キー)） |
| 管理node | C652のうち、slot報告（Feature `0x43..0x47`）とpairing制御（Feature `0x41`）を宣言するhidraw node。Report `0x10` の送信先とは別 |
| slot | Receiverの結合枠（0..4）。GET `0x43 + slot` で状態を読む |
