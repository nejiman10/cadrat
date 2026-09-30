# cadrat-hold-open

`cadrat-hold-open` は、有線C658のhidraw nodeを開いたまま保持するシステムサービスである。保持の規則は [device §9](../device.md#9-hold-open) にあり、`cadrat-tool hold-open` と同じ実装（`cadrat-hidraw` の hold-open）を使う。

## 1. システムサービスにする理由

- hold-openが無いと、有線C658は接続から数秒で入力が止まる。保持しているプロセスが閉じても止まる（[device §9](../device.md#9-hold-open)、調査側 [Issue #4](https://github.com/nejiman10/3dx-hid-research/issues/4)）。
- Phase 1のsystemd user unitは、ユーザーがログインしている間しか動かない。そのため、ログイン画面ではマウスが使えず、ログアウトするとその場で入力が止まる。
- hold-openは設定を持たず、reportの送受信もしない。どのユーザーのものでもないので、システムで1つ動かす。
- `cadratd` はhold-openを行わない。`cadratd` の再起動や停止で、入力が止まらないようにするためである。

## 2. コマンド

```
cadrat-hold-open [--poll-interval=<秒>]
```

- 動作、出力、警告、終了コードは `cadrat-tool hold-open` と同じ（[tool/cli §3](../tool/cli.md#hold-open---poll-interval秒)）。
- 設定ファイルもD-Busも使わない。
- `--version` と `--help` を持つ。

## 3. systemd unit

`/usr/lib/systemd/system/cadrat-hold-open.service`。

- **既定で有効にする。** C658がつながっていなければ何も開かないので、有効にしても害はない。
- `WantedBy=multi-user.target`、`Restart=on-failure`。
- **rootで動かし、権限を最小にする。** hidrawのnodeはrootが所有者なので、capabilityが無くても開ける。udevルールは変えない。
  - `CapabilityBoundingSet=`（空）、`AmbientCapabilities=`（空）、`NoNewPrivileges=yes`
  - `DevicePolicy=closed`、`DeviceAllow=char-hidraw rw`。`PrivateDevices` は使わない（hidrawが見えなくなる）
  - `ProtectSystem=strict`、`ProtectHome=yes`、`PrivateTmp=yes`、`PrivateNetwork=yes`
  - `ProtectKernelTunables=yes`、`ProtectKernelModules=yes`、`ProtectKernelLogs=yes`、`ProtectControlGroups=yes`、`ProtectClock=yes`、`ProtectHostname=yes`
  - `RestrictAddressFamilies=AF_UNIX`、`RestrictNamespaces=yes`、`RestrictRealtime=yes`、`RestrictSUIDSGID=yes`、`LockPersonality=yes`、`MemoryDenyWriteExecute=yes`、`SystemCallArchitectures=native`
  - sysfsは読み取りだけで足りる。
- **パッケージの更新では再起動しない。** 再起動すると、閉じた瞬間に入力が止まるためである。新しい版は次に起動したときから使われる。
  - cargo-debのsystemdの扱いで、`restart-after-upgrade = false` と `stop-on-upgrade = false` を指定して担保する（[implementation §7](../implementation.md#7-配布)）。maintainer scriptに手で書いた `systemctl restart` は置かない。
- `Restart=on-failure` でサービスが再起動した場合、閉じてから開き直すまでの間に入力が止まる。開き直せば入力が戻るかは確かめていない（Q20）。
- パッケージを削除すると止まり、有線C658の入力が止まる。削除のときにその旨を表示する。

## 4. 他の保持との共存

- hidrawは複数のプロセスが同時に開ける。`cadrat-tool hold-open`、Phase 1のuser unit、調査側のuser serviceと同時に動いても害はない。
- Phase 1のuser unit（`cadrat-hold-open.service`、user）は配布から外す。有効にしていた利用者には、`systemctl --user disable cadrat-hold-open.service` で無効にするよう案内する（[implementation §7](../implementation.md#7-配布)）。
- 起動直後、このサービスが始まる前に入力が止まった場合に、開けば入力が戻るかは確かめていない（Q20）。実機確認で確かめ、戻らなければunitの起動を早める。§3 の `Restart=on-failure` の後も同じ問題である。
