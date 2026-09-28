# 未完了の作業

このファイルは未完了項目と達成条件の正本です。項目番号は再開時の参照に使うため、完了や並べ替えの後も再利用しません。

## 6. Phase 1 の実機確認

目的: [仕様 04 §5](docs/spec/04-implementation.md#5-phase-1-の達成条件) の達成条件を実機で確認する。

達成条件: 手順と結果を `docs/hardware-test.md` に記録する。識別子の実値は記録しない。調査リポジトリの知見と食い違う挙動があれば、調査リポジトリへ報告する。

現状: 手順書と実施 1 の記録は [docs/hardware-test.md](docs/hardware-test.md)。実施 1（2026-09-28）で条件 8 以外は満たした。条件 8 は、unpair / pair でslotが変わった後に管理nodeのfdが `ENODEV` を返し続け、成否判定を誤ったため未達。管理nodeの扱いを直してから、条件 8 を再試験する（所有者の実施指示が要る）。実施 1 で見た新しいデバイスの挙動は、調査リポジトリへ報告する。

## 7. `.deb` パッケージを作る

目的: [仕様 04 §7](docs/spec/04-implementation.md#7-配布) に沿って `cadrat-tool` を配布できるようにする。

達成条件: 対象の Ubuntu LTS で `.deb` をインストール・削除でき、udev ルールが有効になり、一般ユーザーで `cadrat-tool list` が動く。

現状: 試験ビルドの仕組み（`packaging/build-deb.sh`）を用意した。コンテナで試験ビルド test2 のインストール・削除を、実機の Ubuntu 24.04.5 で test3 / test4 のインストールと udev ルールの反映（実機確認 E1）、実機を使った `list` を確認した（[docs/packaging.md](docs/packaging.md)）。実機での削除は、所有者が使い続けるため未確認。残りは、実機での削除の確認と、リリース用のビルド（最も古い対象 LTS 上）とリリースで、項目 6 の後に行う。

## 8. 実機の HID descriptor をベクタに加える

目的: 合成 descriptor だけでなく、実機の descriptor でも `cadrat-proto` の長さ解析が SDK と一致することを確かめる。

前提: 調査リポジトリの TODO 12（実機 descriptor の収録）。

達成条件: 調査側で収録された descriptor を [vectors/README.md](vectors/README.md) の手順で取り込み、出所の commit とハッシュを記録する。個体識別子が含まれないことを確認し、ベクタテストが通る。
