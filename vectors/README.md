# テストベクタ

調査リポジトリ [nejiman10/3dx-hid-research](https://github.com/nejiman10/3dx-hid-research) の Python SDK が書き出したベクタです。`cadrat-proto` の出力が SDK と一致することを、`crates/cadrat-proto/tests/vectors.rs` で確かめます。方針は [仕様 04 §3](../docs/spec/04-implementation.md#3-テストベクタ) を参照してください。

## 出所

- 調査リポジトリ commit: `6b151ae574da99981f3777f1f86cdb57d23ace4d`
- 生成ツール: `sdk/python/src/threedx_report10/test_vectors.py`（出力 `format_version` 1）

| ファイル | 出所 | SHA-256 |
|---|---|---|
| `research/input.json` | 調査側 `sdk/python/vectors/input.json` の複製 | `acc4bfbb0dd7a54e5fbc1908317ef8517fcf3892262efa0a021a6acdc11b98c2` |
| `research/output.json` | 調査側 `sdk/python/vectors/output.json` の複製 | `edc7925636b4eb49899522583313076dc2a9758ae1291a24df8341e60ca5f693` |
| `boundary/input.json` | 本リポジトリで用意した合成入力 | `d77d6902ee28029730cb6329aaf63fe4014e12e48f38326705a3234c372f63d7` |
| `boundary/output.json` | `boundary/input.json` を上記 commit の生成ツールで変換 | `05201837399a906bbfee69fa11cf2b497d0e8c125f8e643c9114278a8858373b` |

入力はすべて合成値です。実機から取得した descriptor や応答は含みません。

## 再生成

調査リポジトリを上記 commit で checkout し、そのルートで実行します（Python 3.10 以降）。`<cadrat>` は本リポジトリのパスです。

```sh
cp sdk/python/vectors/input.json sdk/python/vectors/output.json <cadrat>/vectors/research/
PYTHONPATH=sdk/python/src python3 -m threedx_report10.test_vectors \
    --input <cadrat>/vectors/boundary/input.json \
    --output <cadrat>/vectors/boundary/output.json
```

その後、本リポジトリで `sha256sum vectors/*/*.json` を実行して上の表を更新し、`cargo test -p cadrat-proto --test vectors` を実行します。

## 取り込むときの確認

- どのファイルにも、個体識別子（GET `0x08` や slot 応答の bytes 2..7 の実値）と、ローカル環境の情報（ホームディレクトリのパス、ユーザー名、hidraw node、シリアル）が含まれないこと。
- `research/` の2ファイルが、調査リポジトリの同じ commit のファイルと完全に一致すること。
- 調査リポジトリの commit を変えるときは、`docs/spec/README.md` の参照 commit と、仕様が引用している主張も見直すこと。
