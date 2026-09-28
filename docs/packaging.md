# `.deb` パッケージ

[仕様 04 §7](spec/04-implementation.md#7-配布) に沿って、`cadrat-tool` を `.deb` にする手順です。

## 現状

Phase 1 の実機確認（[hardware-test.md](hardware-test.md)）は終わった。リリース用ビルドの手順（下の「リリース用ビルド」）を用意し、Ubuntu 22.04 で確認した。**GitHub Releases にはまだ何も置いていない。** これまでに所有者の PC へ入れた `.deb` はすべて試験ビルドである。

## 試験ビルド

開発中の確認に使う `.deb` です。リリースしません。試験ビルドは次の3か所で区別できます。

| 場所 | 表記 |
|---|---|
| パッケージの版 | `0.1.0~test1+gabc1234`（`~test<番号>` と、ビルドしたcommit） |
| `cadrat-tool --version` | `cadrat-tool 0.1.0~test1+gabc1234 (test build)` |
| ビルドスクリプトの最後の行 | `TEST BUILD: cadrat-tool … (not hardware-verified, not a release)` |

Debianの版の比較では `~` は何よりも前に並ぶので、`0.1.0~test1+…` は正式版 `0.1.0` より古いとみなされる。正式版を入れれば、試験ビルドはそのまま上書きされる。

### 作り方

```sh
cargo install cargo-deb --locked       # 初回だけ
packaging/build-deb.sh [試験番号]       # 既定は 1
```

- `cargo run -p xtask -- dist` で、manページ（コマンドとサブコマンドごと、gzip圧縮）とシェル補完（bash / zsh / fish）を `target/dist/` に生成する。
- `cargo deb` で `target/debian/` に `.deb` を作る。メタデータは `crates/cadrat-tool/Cargo.toml` の `[package.metadata.deb]` にある。
- 同じ試験番号・同じcommitなら、同じ版になる。作り直すときは試験番号を上げる。
- 未コミットの変更があるツリーでビルドすると、版の末尾に `.dirty` が付く。記録に残す試験ビルドは、コミット済みのツリーから作る。
- 試験ビルドはビルドした環境の glibc を要求する（24.04 でビルドすると `libc6 (>= 2.34)`）。古い Ubuntu に入れるものはリリース用ビルドで作る。

## リリース用ビルド

最小サポートの Ubuntu 22.04 上で、新しく clone したツリーから作ります（[仕様 04 §7](spec/04-implementation.md#7-配布)）。22.04 より古い Ubuntu 向けの `.deb` は作りません。その環境の利用者はソースからビルドします（README の「From source」）。

```sh
packaging/build-release.sh
```

スクリプトは Ubuntu 22.04 以外、`target/` がある、未コミットの変更がある、のいずれかなら何もせずに止まります。ビルド後は、バイナリが要求する glibc が 2.35 以下であることと、`.deb` が xz 圧縮であること（zstd に対応しない古い dpkg でも中身を確かめられるように）を確かめます。最後に版、commit、要求する glibc と、`.deb` の SHA-256 を表示します。版は `Cargo.toml` の版そのままで、`--version` にも印は付きません。

### GitHub Actions（通常の方法）

リリースは [`.github/workflows/release.yml`](../.github/workflows/release.yml) で作ります。

1. main の、リリースする commit に注釈付き tag を付けて push する（`git tag -a v0.1.0 -m "cadrat-tool 0.1.0"`、`git push origin v0.1.0`）。tag は `Cargo.toml` の版に `v` を付けたものにする。一致しなければ workflow が止まる。
2. workflow が `ubuntu:22.04` コンテナで `packaging/build-release.sh` を実行し、同じコンテナでインストール・実行・削除を試す。
3. `.deb` に build provenance の attestation を付け、`.deb` と `SHA256SUMS` を載せた**下書き**の Release を作る。
4. 所有者が下書きを確かめて公開する。

pull request でも同じビルドと試験が走ります（Release は作らない）。できた `.deb` は workflow の artifact `deb` から取り出せます。利用者は `sha256sum -c SHA256SUMS` か `gh attestation verify <file> --repo nejiman10/cadrat` で確かめられます。

### 22.04 の環境の用意（手元で作る場合）

22.04 の PC が無ければコンテナを使います。Docker の例（Podman でも同じ）:

```sh
docker run --rm -it ubuntu:22.04 bash
# ここからコンテナの中
apt-get update
apt-get install -y build-essential ca-certificates curl git xz-utils binutils
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
. "$HOME/.cargo/env"
cargo install cargo-deb --locked --version 3.8.0
git clone --branch <tag> https://github.com/nejiman10/cadrat.git
cd cadrat
packaging/build-release.sh              # rust-toolchain.toml の版を rustup が入れる
```

できた `.deb` は `docker cp` などでコンテナの外へ出します。

## 中身

| パス | 内容 |
|---|---|
| `/usr/bin/cadrat-tool` | 本体 |
| `/usr/lib/udev/rules.d/69-cadrat.rules` | hidrawの `uaccess`（[udev/69-cadrat.rules](../udev/69-cadrat.rules)） |
| `/usr/lib/systemd/user/cadrat-hold-open.service` | 有線C658の hold-open（[packaging/systemd/](../packaging/systemd/cadrat-hold-open.service)、仕様 02 §9）。**有効にしない**。利用者が `systemctl --user enable --now cadrat-hold-open.service` で有効にする |
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

### 削除と test5 の導入（実機、2026-09-28）

- `apt remove cadrat-tool` で test4 を削除し、`/usr/bin/cadrat-tool` と `/usr/lib/udev/rules.d/69-cadrat.rules` が消えたことを確認した。`dpkg -s` は `deinstall ok config-files`（削除後の通常の状態）。
- test5（commit `2a45ea6`）を `apt install` で入れ、`--version`、udevルール、`man -w cadrat-tool` を確認した。
- ビルド時に `cargo deb` が、`target/dist/` の資産について「Cargo の target ディレクトリとして扱わない」という警告を出す。資産は `xtask` が先に作るので、パッケージには入る。

### リリース用ビルドの確認（Ubuntu 22.04、2026-09-28）

- 開発用のクラウド環境で、`debootstrap` で作った Ubuntu 22.04（jammy、glibc 2.35、dpkg 1.21.1）の chroot に、commit 済みのツリーを clone して `packaging/build-release.sh` を実行した。Rust は `rust-toolchain.toml` の 1.94.1、`cargo-deb` は 3.8.0 を chroot の中でビルドしたもの。
- 結果は `cadrat-tool_0.1.0_amd64.deb`。バイナリが要求する glibc は 2.34 以上で、依存は `libc6 (>= 2.34), udev`。`control.tar.xz` と `data.tar.xz`。
- 同じ chroot で `dpkg -i` して、man-db がmanページを登録した（`man -w cadrat-tool-apply` で見つかる）。一般ユーザーで `cadrat-tool --version` が `cadrat-tool 0.1.0`（印なし）を表示し、`cadrat-tool list` は終了コード0（`no mice found`）だった。`dpkg -r` でバイナリとudevルールが消えた。
- chroot では udev が動いていないので、`postinst` / `postrm` は `udevadm` を呼ばない分岐を通った。udev ルールの反映と実機での動作は、24.04 の試験ビルドで確認済み（上の test3〜test5）。22.04 の実機では確認していない。
- この確認で作った `.deb` は公開していない。Docker Hub からイメージを取得できない環境だったため、コンテナではなく chroot を使った。
- 経緯: 最初は最小サポートを 18.04 として同じ確認を行い通過したが、ビルド環境の再現しやすさとCADソフトの対応OSを考えて 22.04 に引き上げた。

## リリースまでに残ること

- 公開する commit に tag を付け、そこからリリース用ビルドを作る。
- GitHub Releases に `.deb` と SHA-256 を置く（所有者の指示を待つ）。
