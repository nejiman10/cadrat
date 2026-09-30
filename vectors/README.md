# テストベクタ

調査リポジトリ [nejiman10/3dx-hid-research](https://github.com/nejiman10/3dx-hid-research) の Python SDK が書き出したベクタです。`cadrat-proto` の出力が SDK と一致することを、`crates/cadrat-proto/tests/vectors.rs` で確かめます。方針は [仕様 implementation §3](../docs/spec/implementation.md#3-テストベクタ) を参照してください。

## 出所

- 調査リポジトリ commit: `6b151ae574da99981f3777f1f86cdb57d23ace4d`
- 生成ツール: `sdk/python/src/threedx_report10/test_vectors.py`（出力 `format_version` 1）
- `real/` だけは調査リポジトリ commit `0a66eb5eb79e233c96c44a9c17d4d840aed38e0e` による。この commit でも生成ツールと `sdk/python/vectors/` は上の commit から変わっていない。

| ファイル | 出所 | SHA-256 |
|---|---|---|
| `research/input.json` | 調査側 `sdk/python/vectors/input.json` の複製 | `acc4bfbb0dd7a54e5fbc1908317ef8517fcf3892262efa0a021a6acdc11b98c2` |
| `research/output.json` | 調査側 `sdk/python/vectors/output.json` の複製 | `edc7925636b4eb49899522583313076dc2a9758ae1291a24df8341e60ca5f693` |
| `boundary/input.json` | 本リポジトリで用意した合成入力 | `d77d6902ee28029730cb6329aaf63fe4014e12e48f38326705a3234c372f63d7` |
| `boundary/output.json` | `boundary/input.json` を上記 commit の生成ツールで変換 | `05201837399a906bbfee69fa11cf2b497d0e8c125f8e643c9114278a8858373b` |
| `real/input.json` | 調査側 `sdk/python/tests/data/real_hid_descriptors.json` の `name` と `descriptor_hex` だけを、descriptor の組として並べたもの | `90541374ed1b7eb6202353328a28621b9e412dcba9d4ed4e124f637ab8e86bde` |
| `real/output.json` | `real/input.json` を生成ツールで変換 | `2c15ac0bc12ae57193fb3d2bc87b7b7ff2daf02414dd56d2a0481bdcd948ee6c` |

`research/` と `boundary/` の入力はすべて合成値です。`real/` だけが実機から取得した descriptor です。

### `real/` の確認（2026-09-28）

- 調査側の収録前確認（`sdk/python/tests/data/README.md`）を読み、4件の descriptor の SHA-256 が fixture の記録、および調査側の公開監査（`evidence/read-paths-2026-09/README.md`）の記録と一致することを確かめた。
- descriptor は report 宣言と Usage などの静的な item だけで、String item も GET 応答も含まない。取り込んだのは `name` と descriptor の byte 列だけで、取得日時・原本のハッシュなどの metadata は取り込んでいない。
- 調査側の SDK テストと `tools/run-tests.sh` がその commit で通ることを確かめた。
- 長さ解析の結果は調査側 SDK テストの期待値と一致する。C652 MI_00 の Feature `0x51` は 9 byte、MI_02 では 8 byte と、interface によって宣言が異なる（cadrat は `0x51` を使わない）。

## 再生成

調査リポジトリを上記 commit で checkout し、そのルートで実行します（Python 3.10 以降）。`<cadrat>` は本リポジトリのパスです。

```sh
cp sdk/python/vectors/input.json sdk/python/vectors/output.json <cadrat>/vectors/research/
PYTHONPATH=sdk/python/src python3 -m threedx_report10.test_vectors \
    --input <cadrat>/vectors/boundary/input.json \
    --output <cadrat>/vectors/boundary/output.json
```

`real/` は、`real/` の commit で checkout したうえで次を実行します。

```sh
python3 - <cadrat>/vectors/real/input.json <<'PY'
import json, sys
fixtures = json.load(open("sdk/python/tests/data/real_hid_descriptors.json"))["descriptors"]
descriptors = [{"name": d["name"], "hex": d["descriptor_hex"]} for d in fixtures]
out = {"wire": [], "descriptors": descriptors, "report03": [], "receiver": []}
open(sys.argv[1], "w").write(json.dumps(out, indent=2) + "\n")
PY
PYTHONPATH=sdk/python/src python3 -m threedx_report10.test_vectors \
    --input <cadrat>/vectors/real/input.json \
    --output <cadrat>/vectors/real/output.json
```

その後、本リポジトリで `sha256sum vectors/*/*.json` を実行して上の表を更新し、`cargo test -p cadrat-proto --test vectors` と `cargo test -p cadrat-hidraw --test fake` を実行します。

## 取り込むときの確認

- どのファイルにも、個体識別子（GET `0x08` や slot 応答の bytes 2..7 の実値）と、ローカル環境の情報（ホームディレクトリのパス、ユーザー名、hidraw node、シリアル）が含まれないこと。
- `research/` の2ファイルが、調査リポジトリの同じ commit のファイルと完全に一致すること。
- 調査リポジトリの commit を変えるときは、`docs/spec/README.md` の参照 commit と、仕様が引用している主張も見直すこと。
