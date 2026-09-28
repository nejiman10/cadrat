# cadrat

**Unofficial Linux configuration software for the 3Dconnexion CadMouse Compact Wireless (C658) and its Universal Receiver (C652).** Not affiliated with or endorsed by 3Dconnexion ([NOTICE.md](NOTICE.md)).

`cadrat-tool` sets DPI, polling rate, wheel mode and button assignments over hidraw, with or without the Receiver, and reads, pairs and unpairs Receiver slots. Settings live in a TOML file, because the mouse cannot report its current settings.

CadMouse Compact Wireless（C658）と Universal Receiver（C652）を Linux で設定・利用するための非公式ソフトウェアです。3Dconnexion とは無関係です。

## Status

> **Phase 1, first release in preparation.** Settings and Receiver management are implemented, tested against a simulated device and checked with a real mouse and Receiver on Ubuntu 24.04. `hold-open`, which keeps a wired C658 working (see [Wired use](#wired-use-hold-open)), has been checked on the same hardware, including unplugging and reconnecting. Release packages support Ubuntu 22.04 and later; on older systems, build from source.

現在は Phase 1 で、最初のリリースを準備しています。設定の送信と Receiver 管理は、模擬デバイスでのテストと実機での確認（[docs/hardware-test.md](docs/hardware-test.md)）を終えました。有線の C658 を使い続けるための `hold-open` も、抜き差しを含めて実機で確認しました。リリース用の `.deb` は Ubuntu 22.04 以降が対象で、それより古い環境ではソースからビルドしてください。

## Components

| Name | Role（役割） | Status |
|---|---|---|
| `cadrat-tool` | Stand-alone CLI. 独立設定ツール。hidrawを直接操作し、設定はTOMLファイルだけが持つ | Implemented, hardware-verified |
| `cadratd` | Daemon. デーモン。hidrawを保持し、設定を管理し、D-Busで公開する（`hold-open` の役目も引き継ぐ） | Planned |
| `cadratctl` | Front end for `cadratd`. `cadratd` のフロントエンド。D-Bus経由でだけ操作する | Planned |
| cadrat Radial | GNOME Shell extension. GNOME Shell拡張。`cadratd` とD-Busでつなぐ | Planned |

これらは1つのCargo workspaceに置き、プロトコル・hidraw・設定のcrateを共有します。

## Usage

```sh
cadrat-tool list                       # connected mice and Receivers / 接続中のマウスとReceiver
cadrat-tool init                       # create the settings file / 設定ファイルを作る（値はすべてコメントアウト）
cadrat-tool set mouse.dpi=1600 buttons.radial=host:1
cadrat-tool apply                      # send the file as it is / 設定ファイルをそのまま送る
cadrat-tool receiver slots             # Receiver slots / Receiverのslot
cadrat-tool hold-open                  # keep a wired C658 working (usually run by the user service)
```

All output is in English. Commands, options and exit codes: [docs/spec/03-cli.md](docs/spec/03-cli.md)（コマンドの詳細）.

## Installation

### Release package (Ubuntu 22.04 or later)

Download the `.deb` from [GitHub Releases](https://github.com/nejiman10/cadrat/releases) once one is published, or build it on Ubuntu 22.04 from a fresh clone:

```sh
packaging/build-release.sh             # writes target/debian/cadrat-tool_<version>_amd64.deb
sudo apt install ./target/debian/cadrat-tool_<version>_amd64.deb
```

See [docs/packaging.md](docs/packaging.md) for a container recipe. リリース用の `.deb` は Ubuntu 22.04 上で作ります。22.04 より古い環境では下の「From source」の手順を使います（手順は [docs/packaging.md](docs/packaging.md)）。

### Test build package (Ubuntu)

```sh
packaging/build-deb.sh                 # writes target/debian/cadrat-tool_<version>~test<N>+g<commit>_amd64.deb
sudo apt install ./target/debian/cadrat-tool_*~test*_amd64.deb
```

The package installs the binary, the udev rule, the `hold-open` user service (disabled), manual pages and shell completions. It is a **test build**; see [docs/packaging.md](docs/packaging.md).

`.deb` はバイナリ、udevルール、`hold-open` の user service（無効のまま）、manページ、シェル補完を含みます。試験ビルドです（[docs/packaging.md](docs/packaging.md)）。

### From source

```sh
cargo install --path crates/cadrat-tool
sudo install -m 0644 udev/69-cadrat.rules /usr/lib/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger
```

For the `hold-open` user service, install the unit with the binary's path adjusted:

```sh
mkdir -p ~/.config/systemd/user
sed 's#/usr/bin/cadrat-tool#%h/.cargo/bin/cadrat-tool#' packaging/systemd/cadrat-hold-open.service \
    > ~/.config/systemd/user/cadrat-hold-open.service
```

## Wired use: hold-open

On the tested host, a **wired** C658 stops responding a few seconds after it is plugged in unless some process keeps its hidraw nodes open. The cause is unknown; keeping the nodes open is a workaround that worked there, and this service is what the author uses day to day. The package ships a systemd user service for this. It is **disabled by default**; if you use the mouse by cable, enable it once:

```sh
systemctl --user enable --now cadrat-hold-open.service
journalctl --user -u cadrat-hold-open.service     # shows "held /dev/hidrawN (MI_0x)" lines
```

It only opens the nodes and never sends anything to the mouse. It is not needed if you use the mouse only through the Receiver. If you installed the research project's `c658-hidraw-hold-open.service`, disable it: `systemctl --user disable --now c658-hidraw-hold-open.service`.

有線で使うと、hidraw をどのプロセスも開いていない場合に接続後数秒で入力が止まる事例がありました。`.deb` に入っている user service（既定で無効）を有効にすると、nodeを開いたままにして回避します。Receiver だけで使うなら不要です。

## Permissions

The udev rule [udev/69-cadrat.rules](udev/69-cadrat.rules) gives the logged-in user access to the C658 and C652 hidraw nodes, so no root is needed. Without it, `cadrat-tool` reports `PermissionDenied` with a hint.

一般ユーザーでhidrawにアクセスするため、[udev/69-cadrat.rules](udev/69-cadrat.rules) を使います（`.deb` なら自動で入ります）。

## Documentation

Most documents are in Japanese. 文書の多くは日本語です。

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
