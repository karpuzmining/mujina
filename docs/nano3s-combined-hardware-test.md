# Nano 3S combined test checklist

Test build `v5.1.0-combined-test.1` on `nano3s-combined-test`. It is the multi-pool line plus per-chip clocks, the power sensor, and the dashboard popups.

SHA-256 of `nano-mujina-v5.1.0-combined-test.1-bundle.bin`:

`4821ea60448e90f5e08234821555c32be548b448592ad9bd6d2114dfdbb82705`

Expected build id on **Info** and `GET /api/v0/system`: `45ecfd1`.

`rtos_core.elf` SHA-256 (522320 bytes):

`e4e10be2b2e9c9141a312b42ee7f7cb9c573aee38e5344df3fec011bd4dd6415`

After the unit is running this image, `rtos_core.embedded_sha256` and `rtos_core.running_sha256` should both be that value. The bundle carries `mujina-minerd` only. The harness already on the unit is left as it is.

## Install

1. Boot into the normal dashboard, not ROM boot mode. Boot mode only accepts a `.kdimg`.
2. Open **Firmware** and upload `nano-mujina-v5.1.0-combined-test.1-bundle.bin`.
3. The bundle applies and the unit reboots. This image also replaces `rtos_core.elf`, so after that reboot it writes `/sharefs/reboot_request` and reboots one more time. Wait through both before judging hashrate or the core hash.
4. **Info** shows build `45ecfd1`.

## Menus

**Pool** is the only standalone page. **Dashboard, Power, Tuning, Fan, Fan Curve, LED, Firmware, Display, Info** open popups on `/dashboard`. `/power`, `/fan`, `/display`, and `/led` land on `/dashboard#<name>` and open that popup. `/dashboard#tuning` opens the Tuning popup. **Advanced tuning** closes it and opens `/tuning`.

## Overview

Hashrate, power, efficiency, hottest chip, outlet, fan or immersion, the pool row, one alert line, and the PSU-bypass / immersion badges stay here. Chain status, chip enumeration, per-chip volts, SmartSpeed pass/fail, nameplate versus measured hashrate, rail V/I, and error counters are on `/tuning#diagnostics`. The per-chip temperature grid stays on the overview. Latency and all-time best share render as `—` on this build.

## Wall meter

Read the plug-in meter and the overview power at the same time. They should agree within a few watts while the sensor is up. If the overview shows `~`, the INA226 did not answer: the figure is the labelled estimate, voltage and frequency must hold, and the 137 W trip must not be judged from that estimate. A scale, if you need one, is `factor=` in `/data/userconfig/power_cal.conf` (0.5–2.0). PSU bypass does not remove the 137 W trip.

## Smoke

On a machine that can reach the unit:

```sh
scripts/nano3s-hw-smoke.sh <ip>
```

That changes the display theme, reads it back, and restores it. It does not touch PSU bypass, immersion, the pool URL, or firmware unless you pass `--psu-bypass`, `--immersion`, `--pool`, or `--firmware`. `--reboot-check` sets a marker, reboots, and waits. Do not put a wallet or a worker name in the script.

`scripts/nano3s-ui-smoke.mjs` is the headless click-through against a mock. It does not talk to hardware. Chrome and `puppeteer-core` have to be on the machine that runs it.
