# 未完了の作業

このファイルは未完了項目と達成条件の正本です。項目番号は再開時の参照に使うため、完了や並べ替えの後も再利用しません。

## 7. `.deb` パッケージを作る

目的: [仕様 04 §7](docs/spec/04-implementation.md#7-配布) に沿って `cadrat-tool` を配布できるようにする。

達成条件: 対象の Ubuntu LTS で `.deb` をインストール・削除でき、udev ルールが有効になり、一般ユーザーで `cadrat-tool list` が動く。

現状: 実機の Ubuntu 24.04.5 で、試験ビルドのインストール・削除（test4 を削除して test5 を入れた）、udev ルールの反映（実機確認 E1）、man ページ、一般ユーザーでの `list` を確認した（[docs/packaging.md](docs/packaging.md)）。達成条件は試験ビルドで満たした。残りは、リリース用のビルド（サポートする最も古い Ubuntu LTS 上）と GitHub Releases への公開で、対象 LTS の決定と項目 9 の後に行う。

## 8. 実機の HID descriptor をベクタに加える

目的: 合成 descriptor だけでなく、実機の descriptor でも `cadrat-proto` の長さ解析が SDK と一致することを確かめる。

前提: 調査リポジトリの TODO 12（実機 descriptor の収録）。

達成条件: 調査側で収録された descriptor を [vectors/README.md](vectors/README.md) の手順で取り込み、出所の commit とハッシュを記録する。個体識別子が含まれないことを確認し、ベクタテストが通る。

## 9. Receiver への初回送信の方針を見直す（Q7）

目的: 再ペアリング直後の最初の送信が効かず、送り直すと効いた事例（[実機確認 実施 2](docs/hardware-test.md) の R7・R8）を受けて、Q7 の方針を決め直す。

達成条件: 調査リポジトリでの原因調査（[Issue #2](https://github.com/nejiman10/3dx-hid-research/issues/2)）の状況を踏まえ、`cadrat-tool` の扱い（pair 後の案内、送信後の注記、再送の要否など）を仕様 02 §7・05 §3 と Q7 に決め、実装とテストを揃える。

