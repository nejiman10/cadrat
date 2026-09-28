# 作業引き継ぎ

次に着手する項目: [TODO.md の 10](TODO.md#10-hold-open-を実機で確かめる)（hold-open の実機確認、所有者が実施）。その後に [TODO.md の 7](TODO.md#7-deb-パッケージを作る) の残り（公開前の下書きと `v0.1.0` の tag を消し、hold-open を含む main に付け直して、下書きを確かめて公開）。Receiver 経由の送信が効かない件（Q7）は、送り直しの案内で対処済み。原因は調査リポジトリの [Issue #2](https://github.com/nejiman10/3dx-hid-research/issues/2) で調べており、結果によって仕様 02 §7 の手順7を見直す。

再開時の注意: 仕様が参照する調査リポジトリの commit は `docs/spec/README.md` に、ベクタの出所は `vectors/README.md` に記載している。調査側が更新されていたら、先に仕様への影響を確認する。実機確認で報告した調査リポジトリの Issue は #1〜#3（[docs/hardware-test.md](docs/hardware-test.md) の末尾）で、仕様 05 §6（管理 node の開き直し）と 02 §7.1（経路の扱い）は、その結果によって見直す。所有者の PC には試験ビルド test5 が入っている（リリース版を入れれば上書きされる）。
