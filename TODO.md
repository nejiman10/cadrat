# 未完了の作業

このファイルは未完了項目と達成条件の正本です。項目番号は再開時の参照に使うため、完了や並べ替えの後も再利用しません。

## 6. Phase 1 の実機確認

目的: [仕様 04 §5](docs/spec/04-implementation.md#5-phase-1-の達成条件) の達成条件を実機で確認する。

達成条件: 手順と結果を `docs/hardware-test.md` に記録する。識別子の実値は記録しない。調査リポジトリの知見と食い違う挙動があれば、調査リポジトリへ報告する。

現状: 実施 1・2（2026-09-28）で条件 1〜11 をすべて満たし、[docs/hardware-test.md](docs/hardware-test.md) に記録した。残りは、実施で見た新しいデバイスの挙動を調査リポジトリへ Issue で報告すること。

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

達成条件: 調査リポジトリでの原因調査の状況を踏まえ、`cadrat-tool` の扱い（pair 後の案内、送信後の注記、再送の要否など）を仕様 02 §7・05 §3 と Q7 に決め、実装とテストを揃える。

