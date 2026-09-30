# 文書の索引

| 文書 | 内容 |
|---|---|
| [spec/README.md](spec/README.md) | 仕様の入口。プロジェクトの構成、原則、調査リポジトリとの関係、用語、仕様の版 |
| [spec/config.md](spec/config.md) | 共通: 設定ファイル |
| [spec/device.md](spec/device.md) | 共通: デバイスモデル、検出、選択、送信、hold-open |
| [spec/receiver.md](spec/receiver.md) | 共通: Receiver管理 |
| [spec/implementation.md](spec/implementation.md) | 共通: crate構成、テスト、達成条件、未決事項、配布 |
| [spec/tool/cli.md](spec/tool/cli.md) | `cadrat-tool` のコマンド |
| [spec/hold-open/cli.md](spec/hold-open/cli.md) | `cadrat-hold-open`（hold-openのシステムサービス） |
| [spec/daemon/daemon.md](spec/daemon/daemon.md) | `cadratd` の動作 |
| [spec/daemon/dbus.md](spec/daemon/dbus.md) | `cadratd` の D-Bus API |
| [spec/ctl/cli.md](spec/ctl/cli.md) | `cadratctl` のコマンド |
| [hardware-test.md](hardware-test.md) | 実機確認の手順と記録 |
| [packaging.md](packaging.md) | `.deb` の作り方とリリースの手順 |

仕様の正本は `docs/spec/` である。`docs/spec/` の直下には実行ファイルが共有する文書を、1つの実行ファイルだけにかかわる文書はその名前のディレクトリ（`tool/`、`hold-open/`、`daemon/`、`ctl/`）に置く。
