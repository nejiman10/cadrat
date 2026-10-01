# cadrat-hold-open

`cadrat-hold-open` は、有線C658のhidraw nodeを1つ開いたまま保持するプログラムである。udevが、C658のhidraw nodeが現れるたびに、そのnode用のsystemdのサービス（`cadrat-hold-open@<node>.service`）として起動する。保持の規則は [device §9](../device.md#9-hold-open) にある。

## 1. システムサービスにする理由

- hold-openが無いと、有線C658は接続から数秒で入力が止まる。保持しているプロセスが閉じても止まる（[device §9](../device.md#9-hold-open)、調査側 [Issue #4](https://github.com/nejiman10/3dx-hid-research/issues/4)）。
- Phase 1のsystemd user unitは、ユーザーがログインしている間しか動かない。そのため、ログイン画面ではマウスが使えず、ログアウトするとその場で入力が止まる。
- hold-openは設定を持たず、reportの送受信もしない。どのユーザーのものでもないので、システムのサービスにする。
- `cadratd` はhold-openを行わない。`cadratd` の再起動や停止で、入力が止まらないようにするためである。

## 2. udevから起動する理由

- ホットプラグへの反応は、udevとsystemdのdevice unitに任せる。プログラムがsysfsを周期的に見張る必要がなくなる。
- nodeが現れた直後に開ける。起動時は、udevが既存のデバイスを処理した直後に始まり、`multi-user.target` を待たない。抜き差しでも、周期の待ち（Phase 1の既定で最長1秒）が無い。開くまでの間に入力が止まる場面（Q20）を減らすためである。
- 1つのサービスが1つのnodeだけを扱うので、sandboxで開けるデバイスをそのnode 1つに絞れる（§4）。
- nodeが消えれば、systemdがそのサービスを止める（`BindsTo=`）。保持中のfdを片付ける処理をプログラムが持たずに済む。

## 3. コマンド

```
cadrat-hold-open <hidraw node のパス>
```

1. 引数のnode（例: `/dev/hidraw5`）を `O_RDWR | O_CLOEXEC | O_NONBLOCK` で開く。開けなければ、errnoに応じた終了コードで終わる（§3.1）。
2. 開いたfdに `HIDIOCGRAWINFO` を発行し、bus USB、VID `256f`、PID `c658` であることを確かめる。違えば閉じて `DeviceInvalid`（7）で終わる。udevの条件と同じものを、プログラムの側でも確かめる。
3. 標準出力に `held      /dev/hidraw5 (MI_01)` の1行を出す。interface番号はsysfsから取り、取れなければ括弧ごと省く。
4. `poll()` を、要求するイベントを0にして待つ。reportは読まないので、入力reportでは起きない。`POLLHUP` か `POLLERR`（nodeが消えた）で起きたら、手順5へ進む。
5. fdを閉じ、標準出力に `released  /dev/hidraw5` の1行を出して、終了コード0で終わる。SIGINTかSIGTERMを受けたときも同じである。

- 設定ファイルもD-Busも使わない。reportの送受信もしない。
- `--version` と `--help` を持つ。引数が1つでなければUsage（2）とする。
- 出力の文言は英語にする（[tool/cli §5](../tool/cli.md#5-出力)）。journalに残すため、保持と解放の行は `cadrat-tool hold-open` と同じ形にする。

### 3.1 終了コード

| コード | 名前 | いつ |
|---:|---|---|
| 0 | Success | シグナルで止めた、またはnodeが消えた |
| 1 | Internal | 想定外の内部エラー |
| 2 | Usage | 引数の誤り |
| 4 | NoDevice | nodeが無い（`ENOENT`、`ENODEV`） |
| 6 | PermissionDenied | nodeを開く権限が無い |
| 7 | DeviceInvalid | nodeが有線C658のものでない |

番号と名前は [tool/cli §6](../tool/cli.md#6-終了コード) の表と同じものを使う。

## 4. systemd unit

### 4.1 udevルール

`/usr/lib/udev/rules.d/69-cadrat-hold-open.rules`。

```
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="256f", ATTRS{idProduct}=="c658", TAG+="systemd", ENV{SYSTEMD_WANTS}+="cadrat-hold-open@%k.service"
```

- 条件は `69-cadrat.rules` のC658の行と同じである。PID `c658` は有線のときだけ現れ、Receiver経由のマウスは `c652` になるので、Receiver経由のnodeは対象にならない。
- `69-cadrat.rules`（`uaccess` を付けるルール）とは別のファイルにする。権限のルールを調査リポジトリのルールと同じ内容に保つためと、管理者がhold-openだけを止められるようにするためである（§4.3）。
- `TAG+="systemd"` で、そのnodeのdevice unit（`dev-hidraw5.device`）ができる。`SYSTEMD_WANTS` で、systemdがdevice unitの現れたときにサービスを起動する。

### 4.2 サービス

`/usr/lib/systemd/system/cadrat-hold-open@.service`（template unit）。instance名はnodeのカーネル名（`hidraw5`）である。

- `BindsTo=dev-%i.device`、`After=dev-%i.device`。nodeが消えれば止まる。
- `ExecStart=/usr/bin/cadrat-hold-open /dev/%I`、`Type=exec`。
- `Restart=on-failure`、`RestartPreventExitStatus=7`。対象でないnodeは開き直しても同じ結果になるので、再起動しない。
- `[Install]` は持たない。`systemctl enable` は使わず、udevルールが起動を決める。
- **rootで動かし、権限を最小にする。** hidrawのnodeはrootが所有者なので、capabilityが無くても開ける。
  - `CapabilityBoundingSet=`（空）、`AmbientCapabilities=`（空）、`NoNewPrivileges=yes`
  - `DevicePolicy=closed`、`DeviceAllow=/dev/%I rw`。開けるデバイスは、このinstanceのnode 1つだけになる。`PrivateDevices` は使わない（hidrawが見えなくなる）
  - `ProtectSystem=strict`、`ProtectHome=yes`、`PrivateTmp=yes`、`PrivateNetwork=yes`
  - `ProtectKernelTunables=yes`、`ProtectKernelModules=yes`、`ProtectKernelLogs=yes`、`ProtectControlGroups=yes`、`ProtectClock=yes`、`ProtectHostname=yes`
  - `RestrictAddressFamilies=AF_UNIX`、`RestrictNamespaces=yes`、`RestrictRealtime=yes`、`RestrictSUIDSGID=yes`、`LockPersonality=yes`、`MemoryDenyWriteExecute=yes`、`SystemCallArchitectures=native`
  - sysfsは読み取りだけで足りる。
- `DefaultDependencies` は既定のままにする。起動時のサービスは `basic.target` の後に始まり、ログイン画面より前になる。それでも入力が止まる場合は、`DefaultDependencies=no` で早める（Q20）。
- `Restart=on-failure` で再起動した場合、閉じてから開き直すまでの間に入力が止まる。開き直せば入力が戻るかは確かめていない（Q20）。

### 4.3 有効・無効と更新

- **既定で動く。** パッケージを入れればudevルールが働く。C658がつながっていなければ何も起動しない。
- 使わない管理者は `systemctl mask cadrat-hold-open@.service` で止める。udevルールは残るが、サービスは起動しない。
- **インストール時**: udevルールを読み直し（`udevadm control --reload`）、すでにつながっているC658のnodeについてだけ `add` のイベントを起こし直す（`udevadm trigger --action=add`）。対象をC658のnodeに絞る方法は、Ubuntu 22.04で確かめてから決める（TODO 16）。ほかのデバイスに `add` を起こし直さないためである。
- **パッケージの更新では止めも再起動もしない。** 止めると、閉じた瞬間に入力が止まるためである。新しい版は、次に抜き差ししたときか次の起動から使われる。maintainer scriptに `systemctl restart` は置かない。
- **削除**: 動いているinstanceをすべて止め、有線C658の入力が止まることを表示する。

## 5. 他の保持との共存

- hidrawは複数のプロセスが同時に開ける。`cadrat-tool hold-open`、Phase 1のuser unit、調査側のuser serviceと同時に動いても害はない。
- Phase 1のuser unit（`cadrat-hold-open.service`、user）は配布から外す。有効にしていた利用者には、`systemctl --user disable cadrat-hold-open.service` で無効にするよう案内する（[implementation §7](../implementation.md#7-配布)）。名前が同じでもuser unitとsystem unitは別物で、system側は `cadrat-hold-open@.service` なので衝突しない。
- `cadrat-tool hold-open`（sysfsを周期的に見るもの、[tool/cli §3](../tool/cli.md#hold-open---poll-interval秒)）は、試験と調査、systemdの無い環境のために残す。
- `cadrat-hold-open` はnodeにロックをかけない（[device §7](../device.md#7-送信) の書き込みのロックとは無関係）。
