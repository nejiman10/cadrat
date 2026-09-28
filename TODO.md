# 未完了の作業

このファイルは未完了項目と達成条件の正本です。項目番号は再開時の参照に使うため、完了や並べ替えの後も再利用しません。

## 7. `.deb` パッケージを作る

目的: [仕様 04 §7](docs/spec/04-implementation.md#7-配布) に沿って `cadrat-tool` を配布できるようにする。

達成条件: 対象の Ubuntu LTS で `.deb` をインストール・削除でき、udev ルールが有効になり、一般ユーザーで `cadrat-tool list` が動く。

現状: 実機の Ubuntu 24.04.5 で、試験ビルドのインストール・削除（test4 を削除して test5 を入れた）、udev ルールの反映（実機確認 E1）、man ページ、一般ユーザーでの `list` を確認した。最小サポートを Ubuntu 22.04 とし、リリース用ビルドの手順（`packaging/build-release.sh`）を用意して、22.04 の chroot でビルド・インストール・削除を確認した（[docs/packaging.md](docs/packaging.md)）。リリースを作る GitHub Actions の workflow を用意した。`v0.1.0` の tag と下書きの Release はいったん作ったが、hold-open を含めるため作り直す。hold-open は実機確認 実施 3 で確かめた。残りは、PR #4 の取り込み後に下書きと tag を消して付け直し、下書きを確かめて公開すること。
