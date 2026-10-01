# 未完了の作業

このファイルは未完了項目と達成条件の正本です。項目番号は再開時の参照に使うため、完了や並べ替えの後も再利用しません。

Phase 1 は [v0.1.0](https://github.com/nejiman10/cadrat/releases/tag/v0.1.0) の公開で完了しました。Phase 2a の仕様（v0.24）は所有者が承認し、develop にマージしました（TODO 13 完了）。コマンド層は `cadrat-command` に移しました（TODO 14 完了、仕様 v0.25 で `--json` のフィールドを追記）。`cadrat-tool` の `cadratd` との排他と、node ごとの書き込みのロックを入れました（TODO 15 完了、仕様 v0.26）。`cadrat-hold-open` のバイナリ、template unit、udev ルールを作りました（TODO 16 完了、仕様 v0.27）。`cadratd` と `cadratctl` を作りました（TODO 17 完了、仕様 v0.28）。Linux の慣習と責任の境界を見直し、`cadrat-cli` を分け、`cadrat-hold-open` の置き場所と man の章、journal の重要度、`cadrat-tool` だけで使う場合の `mask` を決めました（仕様 v0.29）。以下は Phase 2a（[仕様 README §2.2](docs/spec/README.md#22-phase-2a)）の実装の項目です。実装はおおむね番号の順に進めます。

## 18. パッケージの分割

目的: `cadrat-common`、`cadrat-tool`、`cadratd` の 3 つの `.deb` を作る（[implementation §7](docs/spec/implementation.md#7-配布)）。`packaging/systemd/` と `packaging/dbus/` の unit と activation ファイル、`cadrat-hold-open`・`cadratd`・`cadratctl` の man ページとシェル補完（`xtask dist`）も含める。`cadrat-hold-open` は `/usr/libexec/cadrat/` に置き、`cadrat-hold-open(8)` と `cadratd(8)` は 8 章で作る（implementation §7 の「置き場所とmanの章」）。README には `cadrat-tool` だけで使う方法として `systemctl --user mask --now cadratd.service` を書く。README の導入手順と [docs/packaging.md](docs/packaging.md) も合わせる。

達成条件: release workflow が 3 つのパッケージを作り、Ubuntu 22.04 のコンテナでインストール・実行・削除と、v0.1.0 からの更新を確かめる。`cadratd` を無効にした後で更新しても、有効に戻らないことを確かめる（[implementation §7](docs/spec/implementation.md#7-配布)）。

## 19. Phase 2a の実機確認

目的: [implementation §5.2](docs/spec/implementation.md#52-phase-2a) の達成条件を実機で確かめる。

達成条件: 手順と結果を [docs/hardware-test.md](docs/hardware-test.md) に記録し、§5.2 の条件をすべて満たす。デバイスへ書き込む手順は、所有者の指示、元に戻す値の記録、書面の手順がそろってから行う。新しい挙動は調査リポジトリへ報告する。
