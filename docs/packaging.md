# `.deb` パッケージ

[仕様 implementation §7](spec/implementation.md#7-配布) に沿って、`cadrat-common`、`cadrat-tool`、`cadratd` の3つの `.deb` を作る手順です。3つはいつも同じ版で、1回のビルドでそろって作ります。

## 現状

Phase 1 の実機確認（[hardware-test.md](hardware-test.md)）を終え、最初のリリース [v0.1.0](https://github.com/nejiman10/cadrat/releases/tag/v0.1.0) を 2026-09-29 に公開した（下の「v0.1.0 の公開」）。所有者の PC では試験ビルドをアンインストールし、公開した v0.1.0 を入れている。v0.1.0 は `cadrat-tool` の1つのパッケージだった。Phase 2a（v0.2.0）から3つに分かれる（下の「中身」）。

## 試験ビルド

開発中の確認に使う `.deb` です。リリースしません。試験ビルドは次の3か所で区別できます。

| 場所 | 表記 |
|---|---|
| パッケージの版 | `0.2.0~test1+gabc1234`（`~test<番号>` と、ビルドしたcommit）。3つとも同じ |
| `--version` | `cadrat-tool 0.2.0~test1+gabc1234 (test build)`（`cadratd`、`cadratctl`、`cadrat-hold-open` も同じ印） |
| ビルドスクリプトの最後の行 | `TEST BUILD: cadrat … (not hardware-verified, not a release)` |

Debianの版の比較では `~` は何よりも前に並ぶので、`0.2.0~test1+…` は正式版 `0.2.0` より古いとみなされる。正式版を入れれば、試験ビルドはそのまま上書きされる。

### 作り方

```sh
cargo install cargo-deb --locked       # 初回だけ
packaging/build-deb.sh [試験番号]       # 既定は 1
```

- `cargo run -p xtask -- dist` で、manページとシェル補完（bash / zsh / fish）を `target/dist/` に生成する。`cadrat-tool` と `cadratctl` はコマンドとサブコマンドごとに1章（`man/man1/`）、`cadratd` と `cadrat-hold-open` は1枚ずつ8章（`man/man8/`）。どれもgzip圧縮。
- 4つのバイナリを `cargo build --release` でまとめてビルドし、`cargo deb --no-build` で3つの `.deb` を `target/debian/` に作る。共通の手順は `packaging/debs.sh` にある。
- メタデータの置き場所は、パッケージごとに次の crate の `[package.metadata.deb]`。`cadratctl` は `cadratd` のパッケージに入る。

| パッケージ | crate | maintainer script |
|---|---|---|
| `cadrat-common` | `crates/cadrat-hold-open` | `packaging/debian/cadrat-common/` |
| `cadrat-tool` | `crates/cadrat-tool` | なし |
| `cadratd` | `crates/cadratd` | `packaging/debian/cadratd/` |

- `cadrat-tool` と `cadratd` の依存 `cadrat-common (= <版>)` は、Cargo.toml では `(= @VERSION@)` と書いておく。cargo-debには版を差し込む変数が無いので、`packaging/debs.sh` が `.deb` を作った後に `dpkg-deb -R` で開いて版を書き込み、`dpkg-deb --root-owner-group -Zxz -b` で作り直す。`cargo deb` を直接実行して作ったものは依存が壊れているので使わない。
- 同じ試験番号・同じcommitなら、同じ版になる。作り直すときは試験番号を上げる。
- 未コミットの変更があるツリーでビルドすると、版の末尾に `.dirty` が付く。記録に残す試験ビルドは、コミット済みのツリーから作る。
- 試験ビルドはビルドした環境の glibc を要求する（24.04 でビルドすると `libc6 (>= 2.34)`）。古い Ubuntu に入れるものはリリース用ビルドで作る。

## リリース用ビルド

最小サポートの Ubuntu 22.04 上で、新しく clone したツリーから作ります（[仕様 implementation §7](spec/implementation.md#7-配布)）。22.04 より古い Ubuntu 向けの `.deb` は作りません。その環境の利用者はソースからビルドします（README の「From source」）。

```sh
packaging/build-release.sh
```

スクリプトは Ubuntu 22.04 以外、`target/` がある、未コミットの変更がある、のいずれかなら何もせずに止まります。ビルド後は、4つのバイナリが要求する glibc が 2.35 以下であることと、3つの `.deb` が xz 圧縮であること（zstd に対応しない古い dpkg でも中身を確かめられるように）を確かめます。最後に版、commit、要求する glibc と、`.deb` ごとの SHA-256 を表示します。版は `Cargo.toml` の版そのままで、`--version` にも印は付きません。

### GitHub Actions（通常の方法）

リリースは [`.github/workflows/release.yml`](../.github/workflows/release.yml) で作ります。

1. main の、リリースする commit に注釈付き tag を付けて push する（`git tag -a v0.1.0 -m "cadrat-tool 0.1.0"`、`git push origin v0.1.0`）。tag は `Cargo.toml` の版に `v` を付けたものにする。一致しなければ workflow が止まる。
2. workflow が `ubuntu:22.04` コンテナで `packaging/build-release.sh` を実行し、同じコンテナで `packaging/try-debs.sh` を実行する（下の「コンテナでの試験」）。
3. 3つの `.deb` に build provenance の attestation を付け、`.deb` と `SHA256SUMS` を載せた**下書き**の Release を作る。
4. 所有者が下書きを確かめて公開する。

pull request でも同じビルドと試験が走ります（Release は作らない）。

### コンテナでの試験

`packaging/try-debs.sh <dir>` は、捨ててよいコンテナの中で次を順に確かめます。システムを書き換えるので、コンテナの外（`/.dockerenv` も `/run/.containerenv` も無い環境）では止まります。

1. 公開した v0.1.0 の `cadrat-tool` を Release から取得し（`SHA256SUMS` で確かめる）、入れる。
2. 3つを入れて更新する。`cadrat-tool` が新しい版になり、`69-cadrat.rules` が `cadrat-common` のものになり、Phase 1 の user unit が消え、hold-open が `/usr/libexec/cadrat/` に入り、`cadratd.service` が全ユーザーについて有効になる（`/etc/systemd/user/default.target.wants/`）。
3. 一般ユーザーで4つのコマンドの `--version` と `cadrat-tool list` を実行する。session bus が無ければ `cadratctl list` が21で終わること、`dbus-run-session` の中では activation ファイルから `cadratd` が起動して `cadratctl list` が通ることを確かめる。
4. `systemctl --global disable cadratd.service` の後で入れ直しても、有効に戻らない。
5. 3つを削除してファイルが消え、`cadratd` の purge で有効化の記録と mask が消える。できた `.deb` は workflow の artifact `deb` から取り出せます。利用者は `sha256sum -c SHA256SUMS` か `gh attestation verify <file> --repo nejiman10/cadrat` で確かめられます。

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

v0.2.0 からの3つのパッケージの中身です（[仕様 implementation §7](spec/implementation.md#7-配布)）。どれも `/usr/share/doc/<パッケージ>/` に README、NOTICE、copyright を持ちます。

### `cadrat-common`

依存は `libc6`、`udev`、`systemd`。`Breaks:` と `Replaces:` に `cadrat-tool (<< 0.2.0)` を持つ（v0.1.0 の `cadrat-tool` が udev ルールを持っていたため）。

| パス | 内容 |
|---|---|
| `/usr/lib/udev/rules.d/69-cadrat.rules` | hidrawの `uaccess`（[udev/69-cadrat.rules](../udev/69-cadrat.rules)） |
| `/usr/lib/udev/rules.d/69-cadrat-hold-open.rules` | 有線C658のnodeごとに hold-open を起動する（[udev/69-cadrat-hold-open.rules](../udev/69-cadrat-hold-open.rules)） |
| `/usr/libexec/cadrat/cadrat-hold-open` | hold-open の本体（[仕様 hold-open/cli](spec/hold-open/cli.md)） |
| `/usr/lib/systemd/system/cadrat-hold-open@.service` | template unit（[packaging/systemd/](../packaging/systemd/cadrat-hold-open@.service)）。`enable` しない。udev ルールが起動する |
| `/usr/share/man/man8/cadrat-hold-open.8.gz` | manページ |

maintainer script（`packaging/debian/cadrat-common/`）:

- `postinst configure`: `systemctl daemon-reload`、`udevadm control --reload`、hidraw への `udevadm trigger --action=change`（`uaccess` のため）。すでにつながっている有線C658のnodeにだけ `udevadm trigger --action=add` を起こし直し、hold-open を起動する。初回のインストールでは、Phase 1 の user unit を無効にする案内を表示する。
- 更新では hold-open を止めも再起動もしない。
- `prerm remove`: 動いている `cadrat-hold-open@*.service` を止め、入力が止まることがあると表示する。
- `postrm remove` / `purge`: `systemctl daemon-reload` と udev ルールの読み直し。
- systemd や udev が動いていない環境（コンテナなど）では、それぞれの手順を飛ばす。

### `cadrat-tool`

依存は `libc6` と `cadrat-common`（同じ版）。maintainer script は無い。

| パス | 内容 |
|---|---|
| `/usr/bin/cadrat-tool` | 本体 |
| `/usr/share/man/man1/cadrat-tool*.1.gz` | manページ（サブコマンドごと） |
| `/usr/share/bash-completion/completions/cadrat-tool`、`/usr/share/zsh/vendor-completions/_cadrat-tool`、`/usr/share/fish/vendor_completions.d/cadrat-tool.fish` | シェル補完 |

### `cadratd`

依存は `libc6`、`cadrat-common`（同じ版）、`default-dbus-session-bus | dbus-session-bus`。

| パス | 内容 |
|---|---|
| `/usr/bin/cadratd`、`/usr/bin/cadratctl` | デーモンとフロントエンド |
| `/usr/lib/systemd/user/cadratd.service` | user unit（[packaging/systemd/](../packaging/systemd/cadratd.service)） |
| `/usr/share/dbus-1/services/cc.nejiman10.Cadrat1.service` | D-Bus の activation ファイル（[packaging/dbus/](../packaging/dbus/cc.nejiman10.Cadrat1.service)） |
| `/usr/share/man/man8/cadratd.8.gz`、`/usr/share/man/man1/cadratctl*.1.gz` | manページ |
| `/usr/share/bash-completion/completions/cadratctl`、`/usr/share/zsh/vendor-completions/_cadratctl`、`/usr/share/fish/vendor_completions.d/cadratctl.fish` | `cadratctl` のシェル補完 |

maintainer script（`packaging/debian/cadratd/`）は、`dh_installsystemduser` が生成するものと同じ手順を書いたもの:

- `postinst configure`: `deb-systemd-helper --user was-enabled` が真なら `enable`、偽なら `update-state`。`deb-systemd-helper` は作ったリンクを `/var/lib/systemd/deb-systemd-user-helper-enabled/` に記録する。記録が無い初回は真になって全ユーザーについて有効になり、管理者が `systemctl --global disable cadratd.service` でリンクを消した後は偽になって、更新でも有効に戻らない。動いている `cadratd` は再起動しない。
- `postrm remove`: `deb-systemd-helper --user mask`。記録は残すので、入れ直すと削除前の状態に戻る。
- `postrm purge`: `deb-systemd-helper --user purge` と `unmask` で、リンクと記録を消す。
- Ubuntu 22.04 の init-system-helpers（1.62）が `--user` に対応していることは、上の試験で確かめている。

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
- 未確認のこと（当時の TODO 7。v0.1.0 の公開とともに完了して削除した）:
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

### 試験ビルド test6（実機、2026-09-28）

- hold-open を含む commit `36f0ba7` から作り、所有者の PC に入れた。`/usr/lib/systemd/user/cadrat-hold-open.service` が入り、既定で無効であることを確認した（実機確認 実施 3 の H0）。
- 所有者はその後 `cadrat-hold-open.service` を有効にし、普段使いにしている。

### リリース用ビルドの確認（Ubuntu 22.04、2026-09-28）

- 開発用のクラウド環境で、`debootstrap` で作った Ubuntu 22.04（jammy、glibc 2.35、dpkg 1.21.1）の chroot に、commit 済みのツリーを clone して `packaging/build-release.sh` を実行した。Rust は `rust-toolchain.toml` の 1.94.1、`cargo-deb` は 3.8.0 を chroot の中でビルドしたもの。
- 結果は `cadrat-tool_0.1.0_amd64.deb`。バイナリが要求する glibc は 2.34 以上で、依存は `libc6 (>= 2.34), udev`。`control.tar.xz` と `data.tar.xz`。
- 同じ chroot で `dpkg -i` して、man-db がmanページを登録した（`man -w cadrat-tool-apply` で見つかる）。一般ユーザーで `cadrat-tool --version` が `cadrat-tool 0.1.0`（印なし）を表示し、`cadrat-tool list` は終了コード0（`no mice found`）だった。`dpkg -r` でバイナリとudevルールが消えた。
- chroot では udev が動いていないので、`postinst` / `postrm` は `udevadm` を呼ばない分岐を通った。udev ルールの反映と実機での動作は、24.04 の試験ビルドで確認済み（上の test3〜test5）。22.04 の実機では確認していない。
- この確認で作った `.deb` は公開していない。Docker Hub からイメージを取得できない環境だったため、コンテナではなく chroot を使った。
- 経緯: 最初は最小サポートを 18.04 として同じ確認を行い通過したが、ビルド環境の再現しやすさとCADソフトの対応OSを考えて 22.04 に引き上げた。

### v0.1.0 の公開（2026-09-29）

- `v0.1.0` の tag は、hold-open を含む main の commit `e141c99`（PR #4 の取り込み）に付けた。最初は `c8cfd0d` に付けて下書きまで作ったが、hold-open を入れるため、公開前に下書きと tag を消して付け直した。
- Release workflow が `ubuntu:22.04` のコンテナでビルドし、同じコンテナでインストール・実行・削除を試したうえで、build provenance の attestation と下書きを作った。所有者が本文を書き、通常の release（pre-release ではない）として公開した。
- 添付は `cadrat-tool_0.1.0_amd64.deb` と `SHA256SUMS`。
- 所有者は PC（Ubuntu 24.04.5）から試験ビルド test6 をアンインストールし、公開した v0.1.0 の `.deb` を入れた。

### 3つのパッケージの試験（Ubuntu 22.04 コンテナ、2026-10-01）

- TODO 18 の作業中に、開発用のクラウド環境の Docker で `ubuntu:22.04` のコンテナを使い、コミット済みのツリーを clone して `packaging/build-release.sh` を実行した。Rust は `rust-toolchain.toml` の 1.94.1、`cargo-deb` は 3.8.0 をコンテナの中でビルドしたもの。
- 結果は `cadrat-common_0.2.0_amd64.deb`、`cadrat-tool_0.2.0_amd64.deb`、`cadratd_0.2.0_amd64.deb`。4つのバイナリが要求する glibc は 2.34 以上。`cadrat-tool` と `cadratd` の依存は `cadrat-common (= 0.2.0)` になった。
- 同じ版のパッケージで `packaging/try-debs.sh` の手順（上の「コンテナでの試験」）がすべて通った。v0.1.0 からの更新では `cadrat-common` の初回の案内が表示され、`cadratd.service` の有効化のリンクが `/etc/systemd/user/default.target.wants/` にできた。`dbus-run-session` の中で、activation ファイルから `cadratd` が起動し、`cadratctl list` が `no mice found` で通った。
- コンテナでは systemd と udev が動いていないので、`daemon-reload`、udev ルールの読み直し、有線C658への `add` の起こし直し、削除時の hold-open の停止は、それぞれの分岐を飛ばした。これらは Phase 2a の実機確認（TODO 19）で見る。
- この確認で作った `.deb` は公開していない。

## 次のリリースの手順

1. main の、リリースする commit で `Cargo.toml` の版を上げておく（v0.2.0 は develop で上げ済み。`cadrat-common` の `Breaks: cadrat-tool (<< 0.2.0)` があるので、0.2.0 より前の版では3つを入れられない）。
2. 注釈付き tag `v<版>` を push する（このリポジトリを操作するクラウド環境からは tag を push できないので、所有者が行う）。
3. できた下書きの本文を書き、確かめて公開する。
