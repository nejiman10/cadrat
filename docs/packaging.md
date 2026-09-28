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

### 試験ビルド test2（`0.1.0~test2+gf84162a`、2026-09-27）

- コミット済みのツリー（commit `f84162a`）から `packaging/build-deb.sh 2` で作った。
- ビルド環境は Ubuntu 24.04 LTS（amd64）のコンテナ。依存は `libc6 (>= 2.34), udev` になった。
- `dpkg-deb -c` で、上の表のファイルがすべて入っていることを確認した（manページ11枚を含む）。
- `dpkg -i` で `install ok installed` になった。コンテナではudevが動いていないので、`postinst` は `udevadm` を呼ばない分岐を通った。
- インストール後の確認:
  - 一般ユーザー（`nobody`）で `cadrat-tool --version` が `0.1.0~test2+gf84162a (test build)` を表示した。
  - `cadrat-tool list` は終了コード0だった（デバイスが無いので `no mice found`）。
  - zshの補完ファイルが所定の場所に入った。
  - manページは入らなかった。このコンテナは最小構成のUbuntuで、dpkgの設定（`path-exclude=/usr/share/man/*`）がmanページをすべて除外するためで、パッケージ側の問題ではない。
- `dpkg -r` でバイナリとudevルールが消えた。
- 未確認のこと（[TODO.md の 7](../TODO.md#7-deb-パッケージを作る)）:
  - 実機のUbuntuでのインストールと、そこでmanページが入ること
  - udevルールの反映
  - 実機を使った `list`

これより前の test1（commit `a751a96` に未コミットの変更を加えたツリーから作ったもの）は、版の表記が中身と一致しないので記録から外した。

### 試験ビルド test3 / test4（実機の Ubuntu 24.04.5、2026-09-28）

- 所有者の PC（Ubuntu 24.04.5 LTS、カーネル 7.0.0-34-generic）で `packaging/build-deb.sh` を実行してビルドし、`apt install` で入れた（test3: commit `4174787`、test4: commit `2657786`）。
- `cadrat-tool --version` が試験ビルドの版を表示した。
- udev ルールが有効に働き、一般ユーザーで `list` と送信ができた。ルールを外すと開けなくなり、戻すと元どおり使えた（実機確認 E1）。`dpkg -V cadrat-tool` も異常なし。
- 実機での削除は、所有者が使い続けるため未確認。

## リリースまでに残ること

- 実機のUbuntu LTSで、削除を確認する（インストールとudevルールの反映は test3 / test4 で確認済み）。
- リリースのビルドは、サポートする最も古いUbuntu LTS上で行う（glibcの互換性のため）。対象とするLTSの版は、リリース時点で決める。24.04でビルドした試験ビルドは、22.04では依存を満たさない。
- 版から `~test…` を外し、GitHub Releasesに置く。
