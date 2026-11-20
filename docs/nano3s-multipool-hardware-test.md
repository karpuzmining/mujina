# Nano 3S multi-pool hardware checklist

Test build `v5.1.0-multipool-test.1`. It is v5.1.0 plus in-process multi-pool. The RT-Smart core is the v5.1.0 `rtos_core.elf`. Do not use this bundle to test the per-chip PLL changes; that is a different image.

## Install

1. Boot the unit into the normal dashboard, not ROM boot mode. Boot mode only accepts a `.kdimg`.
2. Open **Firmware** and upload `nano-mujina-v5.1.0-multipool-test.1-bundle.bin`.
3. Wait until the dashboard is back. Confirm the build string on **Info**.

## Pool page

1. The main menu **Pool** item opens `/pool`, not the old single-pool sheet.
2. Add a second pool. Set weights with the sliders. They must show 100 before Save is accepted.
3. Save. The page says the change was applied with no reboot. Mining must keep running; hashrate must not fall to 0 and stay there (that is the stale Core 1 IPC failure mode of a process restart).
4. Each pool row shows connected or down, difficulty, accepted, rejected, and effective hashrate for 10 min and 1 h.
5. Set one weight to 0. That pool stays in the list as a backup and should not take hash time while the others are up.
6. Remove a pool and save again. The removed pool drops off the live list without a reboot.
7. `GET /api/v0/pool` still returns pool 1. `GET /api/v0/pools` returns every pool and never includes a `password` field.

## Failover

1. Point one pool at a closed port and give it a non-zero weight.
2. Within about a minute its row shows down, and the live pools' effective hashrate rises to absorb its share.
3. Shares found just before a switch still show up as accepted or rejected on the pool that issued the job, not on the pool that received the next slot.

## Rollback

Upload the v5.1.0 OTA bundle from the `v5.1.0` release, or USB-flash `nano-mujina-v5.1.0-release.kdimg` if the dashboard will not boot. `pools.conf` is only read by this build. v5.1.0 reads `pool.conf`, which this build keeps in sync with pool 1.
