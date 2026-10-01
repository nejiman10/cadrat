# 未完了の作業

このファイルは未完了項目と達成条件の正本です。項目番号は再開時の参照に使うため、完了や並べ替えの後も再利用しません。

Phase 1 は [v0.1.0](https://github.com/nejiman10/cadrat/releases/tag/v0.1.0) の公開で完了しました。Phase 2a の仕様（v0.24）は所有者が承認し、develop にマージしました（TODO 13 完了）。コマンド層は `cadrat-command` に移しました（TODO 14 完了、仕様 v0.25 で `--json` のフィールドを追記）。以下は Phase 2a（[仕様 README §2.2](docs/spec/README.md#22-phase-2a)）の実装の項目です。実装はおおむね番号の順に進めます。

## 15. `cadrat-tool` の `cadratd` との排他

目的: [tool/cli §8](docs/spec/tool/cli.md#8-cadratd-との排他) の排他を入れる（終了コード 20）。

達成条件: [implementation §4](docs/spec/implementation.md#4-テスト階層) の「排他」のテストが通る。

## 16. `cadrat-hold-open` とシステムサービス

目的: udev が node ごとに起動する hold-open のシステムサービスを作る（[hold-open/cli.md](docs/spec/hold-open/cli.md)）。

達成条件: `cadrat-hold-open` のバイナリ、template unit（`cadrat-hold-open@.service`）、udev ルール（`69-cadrat-hold-open.rules`）があり、`systemd-analyze verify` と `systemd-analyze security` で unit を確かめた結果を記録している。Ubuntu 22.04 で、`systemctl mask cadrat-hold-open@.service` で起動が止まることと、インストール時に `add` を起こし直す対象を C658 の node に絞る方法を確かめ、hold-open/cli §4.3 に書いている。実機の確認は 19 で行う。

## 17. `cadratd` と `cadratctl`

目的: [daemon/daemon.md](docs/spec/daemon/daemon.md)、[daemon/dbus.md](docs/spec/daemon/dbus.md)、[ctl/cli.md](docs/spec/ctl/cli.md) を実装する。

達成条件: [implementation §4](docs/spec/implementation.md#4-テスト階層) の「D-Bus」のテストが CI で通る。

## 18. パッケージの分割

目的: `cadrat-common`、`cadrat-tool`、`cadratd` の 3 つの `.deb` を作る（[implementation §7](docs/spec/implementation.md#7-配布)）。README の導入手順と [docs/packaging.md](docs/packaging.md) も合わせる。

達成条件: release workflow が 3 つのパッケージを作り、Ubuntu 22.04 のコンテナでインストール・実行・削除と、v0.1.0 からの更新を確かめる。`cadratd` を無効にした後で更新しても、有効に戻らないことを確かめる（[implementation §7](docs/spec/implementation.md#7-配布)）。

## 19. Phase 2a の実機確認

目的: [implementation §5.2](docs/spec/implementation.md#52-phase-2a) の達成条件を実機で確かめる。

達成条件: 手順と結果を [docs/hardware-test.md](docs/hardware-test.md) に記録し、§5.2 の条件をすべて満たす。デバイスへ書き込む手順は、所有者の指示、元に戻す値の記録、書面の手順がそろってから行う。新しい挙動は調査リポジトリへ報告する。
