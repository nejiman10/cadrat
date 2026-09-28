# 作業引き継ぎ

次に着手する項目: [TODO.md の 9](TODO.md#9-receiver-への初回送信の方針を見直す-q7)（Q7 の見直し）。調査リポジトリの [Issue #2](https://github.com/nejiman10/3dx-hid-research/issues/2) の状況を確認してから決める。[TODO.md の 7](TODO.md#7-deb-パッケージを作る) は、リリース用ビルドの対象 LTS を決めるところから。[TODO.md の 8](TODO.md#8-実機の-hid-descriptor-をベクタに加える) は調査リポジトリの TODO 12 を待っている。

再開時の注意: 仕様が参照する調査リポジトリの commit は `docs/spec/README.md` に、ベクタの出所は `vectors/README.md` に記載している。調査側が更新されていたら、先に仕様への影響を確認する。実機確認で報告した調査リポジトリの Issue は #1〜#3（[docs/hardware-test.md](docs/hardware-test.md) の末尾）で、仕様 05 §6（管理 node の開き直し）と 02 §7.1（経路の扱い）は、その結果によって見直す。所有者の PC には試験ビルド test5 が入っている。
