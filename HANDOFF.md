# 作業引き継ぎ

現状: Phase 1 は完了し、[v0.1.0](https://github.com/nejiman10/cadrat/releases/tag/v0.1.0) を 2026-09-29 に公開した。[TODO.md](TODO.md) に未完了の項目はない。開発は `develop` で行う（AGENTS.md の Branches）。docs は共有文書と実行ファイルごとの文書に分けた（[docs/README.md](docs/README.md)）。次は Phase 2 の仕様作りで、仕様に書き起こし、TODO.md に項目（13 から）を立てる。

所有者と合意した Phase 2 の方針（仕様にはまだ書いていない）:
- 範囲: 2a は `cadratd` と `cadratctl`（`cadrat-tool` と同じ操作を D-Bus 経由で行う）と Q9 の排他。2b は接続時・モード切り替え時の自動 apply と、マウス key → プロファイルの紐付け（デーモン用の別 TOML に置き、プロファイルには機器IDを入れない）。3 は `monitor` と cadrat Radial。
- D-Bus: session bus、user service。bus 名 `cc.nejiman10.Cadrat1`、interface `cc.nejiman10.Cadrat1.Manager` / `.Mouse` / `.Receiver`、path `/cc/nejiman10/Cadrat1`。`zbus` を使う。エラー名は終了コード名と 1:1。
- 排他（Q9）: デバイスへの書き込みだけを、デーモンが動いている間ずっと排他にする。デーモンは `$XDG_RUNTIME_DIR/cadrat/cadratd.lock` を flock で持ち、`cadrat-tool` の送信系コマンドは終了コード 20（DaemonRunning）で拒否する。読むだけのコマンドと `init` は使える。仕様 config §1.1 の「D-Bus 上の名前で検出」を書き換える。
- hold-open: ログイン画面とログアウト後にも入力を保つため、システムサービスにする（root、capability なし、sandbox）。専用バイナリ `cadrat-hold-open` として `cadrat-common` に入れ、既定で有効。パッケージ更新時は再起動しない。`cadratd` は hold-open を引き継がない。user unit は配布から外し、無効にするよう案内する。
- 配布: `.deb` を `cadrat-common`（udev ルール、hold-open）、`cadrat-tool`、`cadratd`（`cadratctl` を含む、既定で有効、更新時に再起動しない）に分ける。`cadrat-common` に `Replaces:` / `Breaks: cadrat-tool (<< 0.2)` を付ける。これらは v0.2 にまとめる。

再開時の注意: 仕様が参照する調査リポジトリの commit は `docs/spec/README.md` に、ベクタの出所は `vectors/README.md` に記載している。調査側が更新されていたら、先に仕様への影響を確認する。実機確認で報告した調査リポジトリの Issue は #1〜#4（[docs/hardware-test.md](docs/hardware-test.md) の末尾）で、その結果によって次を見直す: 仕様 receiver §6（管理 node の開き直し、#1）、device §7 の手順7（Receiver への送信の案内、Q7、#2）、device §7.1（経路の扱い、#3）、device §9（hold-open、#4）。所有者の PC には公開した v0.1.0 が入っていて（試験ビルドはアンインストール済み）、`cadrat-hold-open.service` を使っている（調査側の hold-open service は無効）。次のリリースの手順は [docs/packaging.md](docs/packaging.md) の「次のリリースの手順」にある。
