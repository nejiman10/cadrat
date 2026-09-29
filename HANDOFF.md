# 作業引き継ぎ

現状: Phase 1 は完了し、[v0.1.0](https://github.com/nejiman10/cadrat/releases/tag/v0.1.0) を 2026-09-29 に公開した。[TODO.md](TODO.md) に未完了の項目はない。次は Phase 2（`cadratd`、仕様 README §0）の仕様作りから始める。着手の前に、所有者と範囲を決め、TODO.md に項目を立てる。

再開時の注意: 仕様が参照する調査リポジトリの commit は `docs/spec/README.md` に、ベクタの出所は `vectors/README.md` に記載している。調査側が更新されていたら、先に仕様への影響を確認する。実機確認で報告した調査リポジトリの Issue は #1〜#4（[docs/hardware-test.md](docs/hardware-test.md) の末尾）で、その結果によって次を見直す: 仕様 05 §6（管理 node の開き直し、#1）、02 §7 の手順7（Receiver への送信の案内、Q7、#2）、02 §7.1（経路の扱い、#3）、02 §9（hold-open、#4）。所有者の PC には試験ビルド test6 が入っていて、`cadrat-hold-open.service` を使っている（調査側の hold-open service は無効）。リリース版を入れれば上書きされる。次のリリースの手順は [docs/packaging.md](docs/packaging.md) の「次のリリースの手順」にある。
