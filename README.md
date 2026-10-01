# cadrat

**Unofficial Linux configuration software for the 3Dconnexion CadMouse Compact Wireless (C658) and its Universal Receiver (C652).** Not affiliated with or endorsed by 3Dconnexion ([NOTICE.md](NOTICE.md)).

`cadrat-tool` sets DPI, polling rate, wheel mode and button assignments over hidraw, with or without the Receiver, and reads, pairs and unpairs Receiver slots. Settings live in a TOML file, because the mouse cannot report its current settings.

CadMouse Compact Wireless（C658）と Universal Receiver（C652）を Linux で設定・利用するための非公式ソフトウェアです。3Dconnexion とは無関係です。

## Status

> **Phase 1: [v0.1.0](https://github.com/nejiman10/cadrat/releases/tag/v0.1.0) released.** Settings and Receiver management are implemented, tested against a simulated device and checked with a real mouse and Receiver on Ubuntu 24.04. `hold-open`, which keeps a wired C658 working (see [Wired use](#wired-use-hold-open)), has been checked on the same hardware, including unplugging and reconnecting. Release packages support Ubuntu 22.04 and later; on older systems, build from source.

現在は Phase 1 で、最初のリリース [v0.1.0](https://github.com/nejiman10/cadrat/releases/tag/v0.1.0) を公開しました。設定の送信と Receiver 管理は、模擬デバイスでのテストと実機での確認（[docs/hardware-test.md](docs/hardware-test.md)）を終えました。有線の C658 を使い続けるための `hold-open` も、抜き差しを含めて実機で確認しました。リリース用の `.deb` は Ubuntu 22.04 以降が対象で、それより古い環境ではソースからビルドしてください。

## Components

| Name | Role（役割） | Status |
|---|---|---|
| `cadrat-tool` | Stand-alone CLI. 独立設定ツール。hidrawを直接操作し、設定はTOMLファイルだけが持つ | Implemented, hardware-verified |
| `cadratd` | Daemon. デーモン。設定を管理し、D-Busで公開する。動いている間はデバイスへ書き込む唯一のプロセス | Implemented, not yet hardware-verified |
| `cadratctl` | Front end for `cadratd`. `cadratd` のフロントエンド。D-Bus経由でだけ操作する | Implemented, not yet hardware-verified |
| `cadrat-hold-open` | System service for a wired C658. 有線C658のnodeを開いたままにするシステムサービス | Implemented, not yet hardware-verified |
| cadrat Radial | GNOME Shell extension. GNOME Shell拡張。`cadratd` とD-Busでつなぐ | Planned |

これらは1つのCargo workspaceに置き、プロトコル・hidraw・設定のcrateを共有します。

## Usage

```sh
cadrat-tool list                       # connected mice and Receivers / 接続中のマウスとReceiver
cadrat-tool init                       # create the settings file / 設定ファイルを作る（値はすべてコメントアウト）
cadrat-tool set mouse.dpi=1600 buttons.radial=host:1
cadrat-tool apply                      # send the file as it is / 設定ファイルをそのまま送る
cadrat-tool receiver slots             # Receiver slots / Receiverのslot
cadrat-tool hold-open                  # keep a wired C658 working in the foreground (for testing)
```

All output is in English. Commands, options and exit codes: [docs/spec/tool/cli.md](docs/spec/tool/cli.md)（コマンドの詳細）.

## Installation

From v0.2.0 the software comes as three packages of the same version. v0.1.0 had only `cadrat-tool`; installing the v0.2.0 packages over it upgrades it in place.

v0.2.0 からは同じ版の3つのパッケージに分かれます。v0.1.0 の `cadrat-tool` の上にそのまま入れれば更新されます。

| Package | Contents（中身） |
|---|---|
| `cadrat-common` | udev rules, the `cadrat-hold-open` system service for a wired C658（udevルールと有線C658用のシステムサービス） |
| `cadrat-tool` | `cadrat-tool`, stand-alone（単独で動くCLI） |
| `cadratd` | `cadratd` and `cadratctl`; the daemon is enabled for every user on first installation（デーモンとフロントエンド。初回のインストールで全ユーザーについて有効になる） |

### Release package (Ubuntu 22.04 or later)

Download the `.deb` files and `SHA256SUMS` from the [latest release](https://github.com/nejiman10/cadrat/releases/latest), check and install them:

```sh
sha256sum -c SHA256SUMS
sudo apt install ./cadrat-common_<version>_amd64.deb ./cadrat-tool_<version>_amd64.deb ./cadratd_<version>_amd64.deb
```

`cadrat-tool` and `cadratd` each need only `cadrat-common`; leave out the one you do not want. If you enabled the `cadrat-hold-open.service` user unit of v0.1.0, disable it: `systemctl --user disable cadrat-hold-open.service` (the system service replaces it).

Or build them yourself on Ubuntu 22.04 from a fresh clone:

```sh
packaging/build-release.sh             # writes target/debian/{cadrat-common,cadrat-tool,cadratd}_<version>_amd64.deb
sudo apt install ./target/debian/*_<version>_amd64.deb
```

See [docs/packaging.md](docs/packaging.md) for a container recipe. リリース用の `.deb` は Ubuntu 22.04 上で作ります。22.04 より古い環境では下の「From source」の手順を使います（手順は [docs/packaging.md](docs/packaging.md)）。

### Using cadrat-tool without the daemon

While `cadratd` runs, it is the only writer to the devices and `cadrat-tool` refuses commands that write (exit code 20); use `cadratctl` instead, which takes the same commands. To use `cadrat-tool` alone:

- Do not install the `cadratd` package, or
- mask the daemon for your user: `systemctl --user mask --now cadratd.service`. This also stops `cadratctl` from starting it through D-Bus. Undo with `systemctl --user unmask cadratd.service`.

`systemctl --user disable --now cadratd.service` only stops starting it at login: the next `cadratctl` call starts it again through D-Bus, and it runs until you log out. Use `disable` if you want the daemon only when you call `cadratctl`.

`cadratd` が動いている間は `cadratd` だけがデバイスへ書き込み、`cadrat-tool` の送信系コマンドは終了コード20で止まります（同じコマンドは `cadratctl` で使えます）。`cadrat-tool` だけで使うなら、`cadratd` のパッケージを入れないか、`systemctl --user mask --now cadratd.service` で止めます。`mask` は D-Bus からの起動も止めます。`disable` はログイン時の起動をやめるだけで、`cadratctl` を呼ぶと起動し、ログアウトまで動き続けます。

### Test build package (Ubuntu)

```sh
packaging/build-deb.sh                 # writes target/debian/*_<version>~test<N>+g<commit>_amd64.deb
sudo apt install ./target/debian/*~test*_amd64.deb
```

These are **test builds** of the same three packages; see [docs/packaging.md](docs/packaging.md). 3つのパッケージの試験ビルドです。

### From source

```sh
cargo install --path crates/cadrat-tool
sudo install -m 0644 udev/69-cadrat.rules /usr/lib/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger
```

`cargo install --path crates/cadrat-hold-open` (and `crates/cadratd`, `crates/cadratctl`) works the same way. Their udev rule, systemd units and D-Bus activation file ([udev/](udev/), [packaging/systemd/](packaging/systemd/), [packaging/dbus/](packaging/dbus/)) are installed by hand; the units expect the binaries in `/usr/libexec/cadrat/` and `/usr/bin/`, so copy them there or edit `ExecStart=` and `Exec=`.

## Wired use: hold-open

On the tested host, a **wired** C658 stops responding a few seconds after it is plugged in unless some process keeps its hidraw nodes open. The cause is unknown; keeping the nodes open is a workaround that worked there. The `cadrat-common` package does this with the `cadrat-hold-open@.service` system service, which udev starts for each wired C658 node; there is nothing to enable.

```sh
systemctl list-units 'cadrat-hold-open@*'          # one instance per wired C658 node
journalctl -u 'cadrat-hold-open@*'
```

It only opens the nodes and never sends anything to the mouse. Upgrades leave it running; stopping it made the mouse stop responding on the tested host, even without unplugging. To turn it off: `sudo systemctl mask cadrat-hold-open@.service`. If you installed the research project's `c658-hidraw-hold-open.service`, disable it: `systemctl --user disable --now c658-hidraw-hold-open.service`.

有線で使うと、hidraw をどのプロセスも開いていない場合に接続後数秒で入力が止まる事例がありました。`cadrat-common` のシステムサービス `cadrat-hold-open@.service` を udev が node ごとに起動し、nodeを開いたままにして回避します。有効にする操作は要りません。止めるときは `sudo systemctl mask cadrat-hold-open@.service` です。

## Permissions

The udev rule [udev/69-cadrat.rules](udev/69-cadrat.rules) gives the logged-in user access to the C658 and C652 hidraw nodes, so no root is needed. Without it, `cadrat-tool` reports `PermissionDenied` with a hint.

一般ユーザーでhidrawにアクセスするため、[udev/69-cadrat.rules](udev/69-cadrat.rules) を使います（`cadrat-common` のパッケージで入ります）。

## Documentation

Most documents are in Japanese. 文書の多くは日本語です。

- [docs/](docs/README.md): index of documents / 文書の索引
- [docs/spec/](docs/spec/README.md): Phase 1 specification, the reference for the implementation / Phase 1（`cadrat-tool`）の仕様。実装の正本
- [docs/hardware-test.md](docs/hardware-test.md): hardware test procedure / 実機確認の手順と記録
- [docs/packaging.md](docs/packaging.md): building the `.deb` / `.deb` の作り方
- [TODO.md](TODO.md): open work and completion criteria / 未完了の作業と達成条件
- [HANDOFF.md](HANDOFF.md): next item and restart notes / 次に着手する項目と再開時の注意
- [AGENTS.md](AGENTS.md): rules for working in this repository / リポジトリで作業するときの規則

## Protocol sources

Device protocol facts come from the research repository [nejiman10/3dx-hid-research](https://github.com/nejiman10/3dx-hid-research) (`SPEC.md`), cited at a fixed commit with its evidence labels.

デバイスのプロトコルに関する事実は、調査リポジトリの `SPEC.md` を正本とします。本リポジトリの仕様は、参照する調査側のcommitを明記したうえで引用します。

## Development

The toolchain is pinned in [rust-toolchain.toml](rust-toolchain.toml).

```sh
cargo test
cargo clippy --all-targets
cargo run -p xtask -- dist             # manual pages and shell completions into target/dist
```

## License

[MIT](LICENSE)
