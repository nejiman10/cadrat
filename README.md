# cadrat

CadMouse Compact Wireless（C658）と Universal Receiver（C652）を Linux で設定・利用するための非公式ソフトウェアです。3Dconnexion とは無関係です（[NOTICE.md](NOTICE.md)）。

現在は実装前の仕様策定段階です。

## 構成

| 名前 | 役割 | 状態 |
|---|---|---|
| `cadrat-tool` | 独立設定ツール。hidrawを直接操作し、設定はTOMLファイルだけが持つ | 仕様策定中（Phase 1） |
| `cadratd` | デーモン。hidrawを保持し、設定を管理し、D-Busで公開する | 予定 |
| `cadratctl` | `cadratd` のフロントエンド。D-Bus経由でだけ操作する | 予定 |
| cadrat Radial | GNOME Shell拡張。`cadratd` とD-Busでつなぐ | 予定 |

これらは1つのCargo workspaceに置き、プロトコル・hidraw・設定のcrateを共有します。

## 文書

- [docs/spec/](docs/spec/README.md): Phase 1（`cadrat-tool`）の仕様。実装の正本
- [TODO.md](TODO.md): 未完了の作業と達成条件
- [HANDOFF.md](HANDOFF.md): 次に着手する項目と再開時の注意
- [AGENTS.md](AGENTS.md): リポジトリで作業するときの規則

## プロトコルの根拠

デバイスのプロトコルに関する事実は、調査リポジトリ [nejiman10/3dx-hid-research](https://github.com/nejiman10/3dx-hid-research) の `SPEC.md` を正本とします。本リポジトリの仕様は、それを参照する調査側のcommitを明記したうえで引用します。

## 権限

一般ユーザーでhidrawにアクセスするため、[udev/69-cadrat.rules](udev/69-cadrat.rules) を使います。

```sh
sudo install -m 0644 udev/69-cadrat.rules /usr/lib/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger
```

## ライセンス

[MIT](LICENSE)
