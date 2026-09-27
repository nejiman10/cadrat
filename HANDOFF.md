# 作業引き継ぎ

次に着手する項目: [TODO.md の 6](TODO.md#6-phase-1-の実機確認)（実機確認）。手順書は [docs/hardware-test.md](docs/hardware-test.md)。所有者の実施指示と復元値を受け取り、手順書 §E の条件6・7の扱いを合意してから始める。実機確認の中で、[TODO.md の 7](TODO.md#7-deb-パッケージを作る) の残り（実機の Ubuntu でのインストールと udev ルールの反映）も確かめられる。[TODO.md の 8](TODO.md#8-実機の-hid-descriptor-をベクタに加える) は調査リポジトリの TODO 12 を待っている。

再開時の注意: 仕様が参照する調査リポジトリの commit は `docs/spec/README.md` に、ベクタの出所は `vectors/README.md` に記載している。調査側が更新されていたら、先に仕様への影響を確認する。`.deb` は試験ビルドしか作っていない（`docs/packaging.md`）。
