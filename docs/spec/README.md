# cadrat 仕様

状態: **草案 v0.28**（2026-10-01）。Phase 1は実装済み（v0.1.0）。Phase 2aは実装前の合意用。

## 0. cadrat プロジェクトの構成

| 名前 | 役割 | 状態 |
|---|---|---|
| `cadrat-tool` | 独立設定ツール。hidrawを直接操作し、状態はTOMLだけが持つ | Phase 1（v0.1.0） |
| `cadrat-hold-open` | 有線C658のhidrawを開いたまま保持するシステムサービス | Phase 2a |
| `cadratd` | ユーザーごとのデーモン。設定の送信と保存、Receiver管理をD-Busで提供する | Phase 2a |
| `cadratctl` | `cadratd` のフロントエンド。D-Bus経由でだけ操作し、hidrawには触らない | Phase 2a |
| cadrat Radial | GNOME Shell拡張。D-Busで `cadratd` につなぐ | Phase 3 |

- `〜ctl` はLinuxの慣習（`systemctl`、`bluetoothctl`、`ratbagctl`）に合わせ、デーモンのフロントエンドに使う。独立ツールには使わない。
- それぞれ別の実行ファイルとする。`cadrat-proto`、`cadrat-hidraw`、`cadrat-config` のcrateと、設定スキーマを共有する。
- `cadrat-tool` はデーモンができた後も、デバッグとデーモンを使わない環境のために残す。
- 本プロジェクトは3Dconnexionとは無関係の非公式プロジェクトである。READMEにその旨を明記する。

## 1. 目的

CadMouse Compact Wireless（C658）とUniversal Receiver（C652）を、Linux上で設定できるようにする。Phaseごとの狙いは次のとおり。

| Phase | 狙い |
|---|---|
| 1 | デーモンを介さない独立CLI（`cadrat-tool`）を作り、後のデーモンとGNOME拡張が再利用する部品を成熟させる。部品は、プロトコルコア（Report `0x10` とReceiver管理packetの生成・検査、応答の解析、I/Oなし）、Linux transport（hidrawの列挙、送信先の検出・検証、送信）、設定ファイル（TOMLスキーマとコメントを保つ書き戻し）、Receiver管理（slotの読み取り、pair、unpairと成否の判定） |
| 2a | 同じ操作をD-Bus経由で行うデーモン（`cadratd`）とフロントエンド（`cadratctl`）を作る。hold-openをシステムサービス（`cadrat-hold-open`）にし、パッケージを分ける |
| 2b | 接続時とモード切り替え時の自動 `apply`、マウスとプロファイルの紐付け |
| 3 | Report `0x03` / `0x17` の監視と、GNOME Shell拡張（cadrat Radial） |

## 2. 範囲

### 2.1 Phase 1

| 含む | 含まない（後のPhase） |
|---|---|
| 有線C658とReceiver（C652）経由C658へのReport `0x10` 送信 | デーモン、D-Bus、設定の常駐管理 |
| TOMLの読み込み・検証・部分更新・書き戻し | 複数プロファイル、アプリ別設定 |
| マウス単位の列挙、識別キー、自動選択と明示指定 | Report `0x03` / `0x17` の監視（`monitor`） |
| TOMLから作ったwire値の表示 | 送信履歴などのCLI側状態保存 |
| Receiverのslot読み取り、pair、unpair | プロファイルの切り替え、マウスごとの設定の紐付け |
| `--json` 出力 | |
| 有線C658のhold-open（`hold-open` と、既定で無効のsystemd user unit） | |

hold-openは当初、調査リポジトリのuser service（`c658-hidraw-hold-open.service`）に任せる予定だった。しかしそれがないと有線C658が接続後数秒で使えなくなる事例があり（[device §9](device.md#9-hold-open)）、利用者に調査用のPython SDKを入れてもらうのは現実的でないため、Phase 1に含めた。

### 2.2 Phase 2a

| 含む | 含まない（後のPhase） |
|---|---|
| `cadratd`: `cadrat-tool` の `list`、`init`、`get`、`check`、`set`、`apply`、`receiver` と同じ操作をD-Busで提供する（[daemon/daemon.md](daemon/daemon.md)、[daemon/dbus.md](daemon/dbus.md)） | 自動 `apply`（2b） |
| `cadratctl`: 同じコマンドをD-Bus経由で行う（[ctl/cli.md](ctl/cli.md)） | マウスとプロファイルの紐付け（2b） |
| `cadratd` の動作中に、`cadrat-tool` の送信系コマンドを拒否する（Q9） | `monitor` とGNOME Shell拡張（3） |
| hold-openのシステムサービス化（[hold-open/cli.md](hold-open/cli.md)） | |
| パッケージの分割（`cadrat-common`、`cadrat-tool`、`cadratd`、[implementation §7](implementation.md#7-配布)） | |

## 3. 基本原則

- **P1 TOMLが唯一の設定正本。** デバイスから現在設定を読み戻す手段はない（Windows版にも実装は見つかっていない）。`cadrat-tool` は状態を持たず、状態はTOMLファイルだけが持つ。`cadratd` もPhase 2aではTOML以外に設定を持たない。
- **P2 常に完全snapshotを送る。** 部分変更コマンドも、TOML全体と合成した31-byte blobを送る。
- **P3 推測で埋めない。** TOMLが無い、または欠けたフィールドがある場合は送信しない。実験用baselineを暗黙の既定値として使わない。
- **P4 推測で選ばない。** 対象のマウスが複数あればエラーで止める。
- **P5 送信成功後に保存する。** `set` は送信が成功してからTOMLを保存する。送信に失敗したらTOMLは変えない。
- **P6 送信成功はホスト側の完了を意味する。** 送信成功はioctlの完了であり、マウスへの適用の証明ではない。表示や出力でもこの区別を保つ。
- **P7 結合状態の変化は結果で判定する。** pair/unpairの成否はSETの戻り値では決めず、前後のslot snapshotの変化で判定する。
- **P8 取り消しにくい操作は確認してから行う。** unpairは、対象を表示して確認を取り（`--yes` で省略可）、確認から実行までに対象slotが変わっていないことを照合してから実行する。照合するのは、実行するプロセス（`cadrat-tool` または `cadratd`）である。
- **P9 利用者はマウス単位で扱う。** hidraw nodeは実装詳細とし、マウスには機器IDから作った安定した識別キーを付ける。このモデルと識別キーは、`cadratd` でもそのまま使う。
- **P10 送る直前に宛先を確かめる。** 送信は1回だけ行い、その直前に同じfdで機器IDを読み直して、選んだマウスと一致することを確かめる。
- **P11 同じ操作は同じ結果になる。** `cadratd` と `cadratctl` は、`cadrat-tool` の同じコマンドと同じ手順、判定、終了コード、出力になる。手順は共有crateに置き、実行ファイルごとに書き直さない。
- **P12 デバイスへ書き込むのは1か所。** `cadratd` が動いている間は、`cadratd` だけがデバイスへ書き込む（[daemon §4](daemon/daemon.md#4-デバイスへの書き込みの排他q9)）。どのプロセスも、書き込む間はそのhidraw nodeをロックし、ユーザーやプロセスをまたいで同じデバイスへの書き込みが重ならないようにする（[device §7.2](device.md#72-書き込みのロック)）。
- **P13 hold-openは設定と独立させる。** hold-openは、ログインの有無やデーモンの状態にかかわらず、システムサービスが行う。
- **P14 デーモンは呼び出し側に無い権限を与えない。** `cadratd` は呼び出し側と同じユーザーとして動き、そのユーザーが `uaccess` で持つ権限だけを使う。権限の境界をまたがないので、polkitもsystem busも使わない。

## 4. 文書構成

`docs/spec/` の直下には、実行ファイルが共有する文書を置く。1つの実行ファイルだけにかかわる文書は、その名前のディレクトリに置く。仕様の版と未決事項の表は、全体で1つにする。

| 文書 | 対象 | 内容 |
|---|---|---|
| [config.md](config.md) | 共通 | TOMLスキーマ、値の検証、wireへの対応 |
| [device.md](device.md) | 共通 | デバイスモデル（マウス・Receiver・識別キー）、検出、選択、送信、hold-openの規則 |
| [receiver.md](receiver.md) | 共通 | Receiver管理nodeの検出、slotの読み取り、pair/unpairの手順と判定 |
| [implementation.md](implementation.md) | 共通 | crate構成、テスト方針、達成条件、未決事項、配布 |
| [tool/cli.md](tool/cli.md) | `cadrat-tool` | コマンド体系、`set` の処理順序、出力、終了コード、`cadratd` との排他 |
| [hold-open/cli.md](hold-open/cli.md) | `cadrat-hold-open` | システムサービスにする理由、udevからの起動、コマンド、systemd unit |
| [daemon/daemon.md](daemon/daemon.md) | `cadratd` | 実行形態、起動と終了、書き込みの排他、要求の処理、機器の公開、ログ |
| [daemon/dbus.md](daemon/dbus.md) | `cadratd` | D-Bus API |
| [ctl/cli.md](ctl/cli.md) | `cadratctl` | コマンド、Receiverの対話、出力、終了コード |

文書をまたぐ参照は、ファイル名から `.md` を除いた名前と節番号で書く（例: `device §7`、`tool/cli §4`、`daemon §4`、`dbus §5`）。

## 5. 調査リポジトリとの関係

プロトコルの事実は [nejiman10/3dx-hid-research](https://github.com/nejiman10/3dx-hid-research) の `SPEC.md` を正本とし、本仕様はそれを引用するだけとする。参照基準は commit `6b151ae`（2026-09-27）。

本仕様内のプロトコル記述には、調査側の根拠ラベル（`CONFIRMED` / `OBSERVED` / `HYPOTHESIS` / `UNKNOWN`）を付ける。調査側のSPECが更新されたら、影響箇所を洗い出して本仕様を改訂する。

コードは共有しない。調査リポジトリのPython SDKは参照実装として残し、両者はテストベクタ（[implementation.md](implementation.md#3-テストベクタ)）でつなぐ。

## 6. 用語

| 用語 | 意味 |
|---|---|
| blob | Report `0x10` の31-byte payload |
| wire report | Report ID `0x10` とblobを合わせた32 byte |
| 物理ボタン名 | blob offset 18..24 の7 entryに付ける名前（`left` … `radial`） |
| action | 各物理ボタンに割り当てる1 byteのwire値の意味（direct / host-routed / raw） |
| マウス | 利用者が扱う単位。機器IDで識別し、1つまたは2つの経路（route）を持つ（[device §2](device.md#2-デバイスモデル)） |
| route | `wired`（C658直結）または `receiver`（C652経由）。有効な経路は1つで、もう一方は `standby` |
| 機器ID | GET `0x08` 応答の bytes 2..7。有線・Receiverの両経路で同じ値になる |
| 識別キー | マウスやReceiverを安定して指す文字列（[device §2.2](device.md#22-識別キー)） |
| 管理node | C652のうち、slot報告（Feature `0x43..0x47`）とpairing制御（Feature `0x41`）を宣言するhidraw node。Report `0x10` の送信先とは別 |
| slot | Receiverの結合枠（0..4）。GET `0x43 + slot` で状態を読む |
