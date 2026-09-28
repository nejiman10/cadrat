# 作業引き継ぎ

次に着手する項目: [TODO.md の 6](TODO.md#6-phase-1-の実機確認) の条件 8。実施 1 で、unpair / pair の後に管理nodeのfdが `ENODEV` を返し続け、成否判定を誤った（[docs/hardware-test.md](docs/hardware-test.md) の F6′、F7′）。管理nodeの扱い（選び方と、デバイスが消えた場合の開き直し）を仕様 05 から見直し、実装してから、所有者の実施指示を得て条件 8 を再試験する。並行して、実施 1 で見た新しいデバイスの挙動（C-B5、D6、F0、F6、F6′、F7′、F10 の遅延）を調査リポジトリへ報告する。

再開時の注意: 仕様が参照する調査リポジトリの commit は `docs/spec/README.md` に、ベクタの出所は `vectors/README.md` に記載している。調査側が更新されていたら、先に仕様への影響を確認する。所有者の PC には試験ビルド test4 が入っている。
