# 未完了の作業

このファイルは未完了項目と達成条件の正本です。項目番号は再開時の参照に使うため、完了や並べ替えの後も再利用しません。

## 1. 調査リポジトリのテストベクタを取り込む

目的: `cadrat-proto` の出力を Python SDK と突き合わせる基準を用意する。

前提: 調査リポジトリの TODO 11（テストベクタ書き出しツール）と TODO 12（descriptor の収録）。

達成条件: 調査リポジトリの特定commitで生成したベクタを `vectors/` に置き、出所のcommitと再生成手順を記録する。ベクタに個体識別子が含まれないことを確認する。

## 2. Cargo workspace と `cadrat-proto` を作る

目的: I/O を持たない Report `0x10` 生成・検査、descriptor 解析、Report `0x03` 解析、Receiver packet と slot 応答の処理を実装する。

達成条件: [仕様 04 §1〜§3](docs/spec/04-implementation.md) の crate 構成と型の方針に沿い、項目1のベクタテストがすべて通る。仕様 01 の値検証（DPI の範囲外・端数の拒否など）を Rust 側のテストで確認する。

## 3. `cadrat-config` を作る

目的: schema 1 の読み込み・検証・部分更新・原子的な保存・lock を実装する。

達成条件: [仕様 01](docs/spec/01-config.md) の規則をすべてテストで確認する。書き戻しでコメント・順序・表記が保たれることを含む。

## 4. `cadrat-hidraw` を作る

目的: 列挙、役割判定、機器IDによるマウスの組み立て、選択、送信直前の宛先確認、送信、Receiver 管理を実装する。

達成条件: [仕様 02・05](docs/spec/02-device.md) の各分岐を、fake の sysfs・descriptor・ioctl で確認する（[仕様 04 §4](docs/spec/04-implementation.md#4-テスト階層)）。

## 5. `cadrat-tool` を作る

目的: [仕様 03](docs/spec/03-cli.md) のコマンド、出力、終了コードを実装する。

達成条件: fake transport を使った CLI テストで、`set` の処理順序の各分岐と終了コードを確認する。

## 6. Phase 1 の実機確認

目的: [仕様 04 §5](docs/spec/04-implementation.md#5-phase-1-の達成条件) の達成条件を実機で確認する。

達成条件: 手順と結果を `docs/hardware-test.md` に記録する。識別子の実値は記録しない。調査リポジトリの知見と食い違う挙動があれば、調査リポジトリへ報告する。

## 7. `.deb` パッケージを作る

目的: [仕様 04 §7](docs/spec/04-implementation.md#7-配布) に沿って `cadrat-tool` を配布できるようにする。

達成条件: 対象の Ubuntu LTS で `.deb` をインストール・削除でき、udev ルールが有効になり、一般ユーザーで `cadrat-tool list` が動く。
