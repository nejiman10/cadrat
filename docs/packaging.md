# `.deb` パッケージ

[仕様 04 §7](spec/04-implementation.md#7-配布) に沿って、`cadrat-tool` を `.deb` にする手順です。

## 現状: 試験ビルドのみ

**これまでに作った `.deb` はすべて試験ビルドで、リリースではありません。** Phase 1 の実機確認（[hardware-test.md](hardware-test.md)）を終えるまで、GitHub Releases には置きません。

試験ビルドは次の3か所で区別できます。

| 場所 | 表記 |
|---|---|
| パッケージの版 | `0.1.0~test1+gabc1234`（`~test<番号>` と、ビルドしたcommit） |
| `cadrat-tool --version` | `cadrat-tool 0.1.0~test1+gabc1234 (test build)` |
| ビルドスクリプトの最後の行 | `TEST BUILD: cadrat-tool … (not hardware-verified, not a release)` |

Debianの版の比較では `~` は何よりも前に並ぶので、`0.1.0~test1+…` は正式版 `0.1.0` より古いとみなされる。正式版を入れれば、試験ビルドはそのまま上書きされる。

## 作り方

```sh
cargo install cargo-deb --locked       # 初回だけ
packaging/build-deb.sh [試験番号]       # 既定は 1
```

- `cargo run -p xtask -- dist` で、manページ（コマンドとサブコマンドごと、gzip圧縮）とシェル補完（bash / zsh / fish）を `target/dist/` に生成する。
- `cargo deb` で `target/debian/` に `.deb` を作る。メタデータは `crates/cadrat-tool/Cargo.toml` の `[package.metadata.deb]` にある。
- 同じ試験番号・同じcommitなら、同じ版になる。作り直すときは試験番号を上げる。
- 未コミットの変更があるツリーでビルドすると、版の末尾に `.dirty` が付く。記録に残す試験ビルドは、コミット済みのツリーから作る。

## 中身

| パス | 内容 |
|---|---|
| `/usr/bin/cadrat-tool` | 本体 |
| `/usr/lib/udev/rules.d/69-cadrat.rules` | hidrawの `uaccess`（[udev/69-cadrat.rules](../udev/69-cadrat.rules)） |
| `/usr/share/man/man1/cadrat-tool*.1.gz` | manページ |
| `/usr/share/bash-completion/completions/cadrat-tool` | bash補完 |
| `/usr/share/zsh/vendor-completions/_cadrat-tool` | zsh補完 |
| `/usr/share/fish/vendor_completions.d/cadrat-tool.fish` | fish補完 |
| `/usr/share/doc/cadrat-tool/` | README、NOTICE、copyright |

インストール後（`postinst`）と削除後（`postrm`）に `udevadm control --reload` と `udevadm trigger --subsystem-match=hidraw --action=change` を実行し、接続中のデバイスにもルールを反映する。udevが動いていない環境（コンテナなど）では何もしない。依存は `libc6` と `udev`。

## 確認の記録

### 試験ビルド test1（commit `a751a96`、2026-09-27）

- ビルド環境: Ubuntu 24.04 LTS（amd64）のコンテナ。依存は `libc6 (>= 2.34)` になった。
- `dpkg-deb -c` で、上の表のファイルがすべて入っていることを確認した。
- `dpkg -i` → 依存（`udev`）の解決 → `install ok installed` まで進んだ。コンテナではudevが動いていないため、`postinst` の `udevadm` は実行されない分岐を通った。
- インストール後、一般ユーザー（`nobody`）で `cadrat-tool list` が終了コード0で動いた（デバイスが無いので `no mice found`）。
- `dpkg -r` でバイナリとudevルールが消えた。
- 実機のUbuntuでのインストール、udevルールの反映、実機を使った `list` は未確認（[TODO.md の 7](../TODO.md#7-deb-パッケージを作る)）。

## リリースまでに残ること

- 実機のUbuntu LTSで、インストール・削除とudevルールの反映を確認する。
- リリースのビルドは、サポートする最も古いUbuntu LTS上で行う（glibcの互換性のため）。対象とするLTSの版は、リリース時点で決める。24.04でビルドした試験ビルドは、22.04では依存を満たさない。
- 版から `~test…` を外し、GitHub Releasesに置く。
