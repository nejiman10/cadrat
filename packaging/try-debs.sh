#!/bin/sh
# Installs, runs, upgrades and removes the three packages in a throwaway
# Ubuntu container (spec implementation §7). The Release workflow runs it
# after packaging/build-release.sh; it changes the system, so it refuses to
# run outside a container.
#
# Usage: packaging/try-debs.sh DIR   (DIR holds the three .deb files)
set -eu

dir=$(CDPATH= cd -- "$1" && pwd)
[ -f /.dockerenv ] || [ -f /run/.containerenv ] \
    || { echo "try-debs: run this only in a throwaway container" >&2; exit 1; }

export DEBIAN_FRONTEND=noninteractive
step() { echo "== $*"; }
fail() { echo "try-debs: $*" >&2; exit 1; }
wants=/etc/systemd/user/default.target.wants/cadratd.service

common=$(ls "$dir"/cadrat-common_*.deb)
tool=$(ls "$dir"/cadrat-tool_*.deb)
daemon=$(ls "$dir"/cadratd_*.deb)
version=$(dpkg-deb -f "$tool" Version)

id tester >/dev/null 2>&1 || useradd --create-home tester
as_tester() { su tester -c "$1"; }

step "v0.1.0, as released"
apt-get install -y --no-install-recommends curl ca-certificates >/dev/null
old=$(mktemp -d)
(cd "$old" \
    && curl -fsSLO https://github.com/nejiman10/cadrat/releases/download/v0.1.0/cadrat-tool_0.1.0_amd64.deb \
    && curl -fsSLO https://github.com/nejiman10/cadrat/releases/download/v0.1.0/SHA256SUMS \
    && sha256sum -c SHA256SUMS)
apt-get install -y "$old/cadrat-tool_0.1.0_amd64.deb"
test -f /usr/lib/systemd/user/cadrat-hold-open.service

step "upgrade to $version"
apt-get install -y "$common" "$tool" "$daemon"
[ "$(dpkg-query -W -f '${Version}' cadrat-tool)" = "$version" ] || fail "cadrat-tool was not upgraded"
dpkg -S /usr/lib/udev/rules.d/69-cadrat.rules | grep -q '^cadrat-common:' \
    || fail "69-cadrat.rules did not move to cadrat-common"
test ! -e /usr/lib/systemd/user/cadrat-hold-open.service || fail "the Phase 1 user unit is still installed"
test -f /usr/lib/udev/rules.d/69-cadrat-hold-open.rules
test -f /usr/lib/systemd/system/cadrat-hold-open@.service
test -x /usr/libexec/cadrat/cadrat-hold-open
test ! -e /usr/bin/cadrat-hold-open
test -f /usr/lib/systemd/user/cadratd.service
test -f /usr/share/dbus-1/services/cc.nejiman10.Cadrat1.service
test -L "$wants" || fail "cadratd.service was not enabled on first installation"

step "run"
as_tester 'cadrat-tool --version'
as_tester 'cadratctl --version'
as_tester 'cadratd --version'
as_tester '/usr/libexec/cadrat/cadrat-hold-open --version'
# No devices here: list succeeds and finds nothing.
as_tester 'cadrat-tool list'
# Without a session bus cadratctl cannot reach cadratd (exit 21).
status=0
as_tester 'env -u DBUS_SESSION_BUS_ADDRESS cadratctl list' || status=$?
[ "$status" = 21 ] || fail "cadratctl without a bus exited with $status, not 21"
# With one, the bus starts cadratd from the activation file. cadratd needs
# the runtime directory that a login would create.
runtime=/run/user/$(id -u tester)
install -d -m 700 -o tester -g tester "$runtime"
as_tester "XDG_RUNTIME_DIR=$runtime dbus-run-session -- cadratctl list"

step "disabled stays disabled across an upgrade"
systemctl --global disable cadratd.service
test ! -e "$wants"
apt-get install -y --reinstall "$daemon"
test ! -e "$wants" || fail "the upgrade enabled cadratd.service again"
systemctl --global enable cadratd.service

step "remove"
apt-get remove -y cadratd cadrat-tool cadrat-common
for path in /usr/bin/cadrat-tool /usr/bin/cadratd /usr/bin/cadratctl \
    /usr/libexec/cadrat/cadrat-hold-open /usr/lib/udev/rules.d/69-cadrat.rules \
    /usr/lib/systemd/user/cadratd.service; do
    test ! -e "$path" || fail "$path is still there"
done
apt-get purge -y cadratd
test ! -e "$wants" || fail "purge left $wants"
test ! -e /etc/systemd/user/cadratd.service || fail "purge left the mask"

echo "try-debs: all steps passed for $version"
