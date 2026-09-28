# 作業引き継ぎ

次に着手する項目: [TODO.md の 7](TODO.md#7-deb-パッケージを作る) の残り。PR #4（hold-open）を取り込んだ後、公開前の下書きと `v0.1.0` の tag を消し、新しい main に付け直して、下書きを確かめて公開する。hold-open は実施 3 で実機確認済み。H5 で、接続中に保持をやめても入力が止まることが分かり、調査リポジトリに [Issue #4](https://github.com/nejiman10/3dx-hid-research/issues/4) として報告した。Receiver 経由の送信が効かない件（Q7）は、送り直しの案内で対処済み。原因は調査リポジトリの [Issue #2](https://github.com/nejiman10/3dx-hid-research/issues/2) で調べており、結果によって仕様 02 §7 の手順7を見直す。

再開時の注意: 仕様が参照する調査リポジトリの commit は `docs/spec/README.md` に、ベクタの出所は `vectors/README.md` に記載している。調査側が更新されていたら、先に仕様への影響を確認する。実機確認で報告した調査リポジトリの Issue は #1〜#3（[docs/hardware-test.md](docs/hardware-test.md) の末尾）で、仕様 05 §6（管理 node の開き直し）と 02 §7.1（経路の扱い）は、その結果によって見直す。所有者の PC には試験ビルド test6 が入っていて、`cadrat-hold-open.service` を使っている（調査側の hold-open service は無効）。リリース版を入れれば上書きされる。
