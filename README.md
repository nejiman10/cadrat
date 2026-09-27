# cadrat

**Unofficial Linux configuration software for the 3Dconnexion CadMouse Compact Wireless (C658) and its Universal Receiver (C652).** Not affiliated with or endorsed by 3Dconnexion ([NOTICE.md](NOTICE.md)).

`cadrat-tool` sets DPI, polling rate, wheel mode and button assignments over hidraw, with or without the Receiver, and reads, pairs and unpairs Receiver slots. Settings live in a TOML file, because the mouse cannot report its current settings.

CadMouse Compact Wireless（C658）と Universal Receiver（C652）を Linux で設定・利用するための非公式ソフトウェアです。3Dconnexion とは無関係です。

## Status

> **Phase 1, not yet verified on hardware.** Every command in the specification is implemented and tested against a simulated device, but none has been run against a real mouse yet. Packages built so far are **test builds** (version `…~test…`), not releases.

現在は Phase 1 の実装段階です。`cadrat-tool` は仕様のコマンドをすべて実装し、模擬デバイスでテストしていますが、実機での確認はまだです（手順: [docs/hardware-test.md](docs/hardware-test.md)）。これまでに作った `.deb` はすべて試験ビルドで、リリースではありません。

## Components

| Name | Role（役割） | Status |
|---|---|---|
| `cadrat-tool` | Stand-alone CLI. 独立設定ツール。hidrawを直接操作し、設定はTOMLファイルだけが持つ | Implemented, hardware check pending |
| `cadratd` | Daemon. デーモン。hidrawを保持し、設定を管理し、D-Busで公開する | Planned |
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
```

All output is in English. Commands, options and exit codes: [docs/spec/03-cli.md](docs/spec/03-cli.md)（コマンドの詳細）.

## Installation

### Test build package (Ubuntu)

```sh
packaging/build-deb.sh                 # writes target/debian/cadrat-tool_<version>~test<N>+g<commit>_amd64.deb
sudo apt install ./target/debian/cadrat-tool_*~test*_amd64.deb
```

The package installs the binary, the udev rule, manual pages and shell completions. It is a **test build**; see [docs/packaging.md](docs/packaging.md).

`.deb` はバイナリ、udevルール、manページ、シェル補完を含みます。試験ビルドです（[docs/packaging.md](docs/packaging.md)）。

### From source

```sh
cargo install --path crates/cadrat-tool
sudo install -m 0644 udev/69-cadrat.rules /usr/lib/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger
```

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
