/*
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * A3197S PLL programming.
 *
 * The write order in a3197s_set_pll_seq() is Canaan's a3197s_set_pll_seq
 * (canaan_sdk_for_linux v1.1.2-alpha, commit 07fe73f,
 * apps/miner/hal/asic/a3197s/a3197s_pll_seq.c, MIT). A wrong order or chip
 * select can hang or damage the chips. See NOTICE.
 *
 * The ramp, per-chip offsets, and rolltime refresh around that sequence
 * are this project's.
 *
 * Frequency words come from get_cpm_pll() (ceiling to the next official
 * table row). Canaan's correct_freq() additionally snaps some integers
 * down by 1 MHz (230 -> 229, 100 -> 99). That snap is not applied: it
 * would move clocks already saved by AutoTune and the manual API.
 */

#include <stdint.h>
#include <stdio.h>
#include "util.h"
#include "pll_cpm_table.h"
#include "asic_engine.h"
#include "asic_internal.h"

a3197s_pll_state_t g_pll_state;

static uint32_t get_cpm_pll(uint32_t freq)
{
	uint32_t i;
	for (i = 0; i < PLL_TABLE_ROWS; i++)
	{
		if (freq <= cpm_table[i][0])
			return cpm_table[i][1];
	}

	return 0;
}

uint32_t get_cpm_freq(uint32_t pll)
{
	uint32_t i, freq = 0;

	pll = (pll & 0x9fffffff) | 1;
	for (i = 0; i < PLL_TABLE_ROWS; i++)
	{
		if (pll == cpm_table[i][1])
		{
			return cpm_table[i][0];
		}
	}

	return 0;
}

/* Per-chip MHz offsets added to every PLL domain of that chip. All zero
 * (the default) keeps a3197s_pll_apply() on its original broadcast path. */
int16_t g_pll_chip_offset[HBOARD_COUNT][PLL_CHIP_OFFSET_MAX];

static int pll_offsets_active(uint8_t miner_id)
{
	int i;

	for (i = 0; i < PLL_CHIP_OFFSET_MAX; i++)
		if (g_pll_chip_offset[miner_id][i] != 0)
			return 1;
	return 0;
}

static uint32_t pll_clamp_freq(int32_t freq)
{
	if (freq < PLL_CHIP_MIN_MHZ)
		return PLL_CHIP_MIN_MHZ;
	if (freq > PLL_CHIP_MAX_MHZ)
		return PLL_CHIP_MAX_MHZ;
	return (uint32_t)freq;
}

static uint32_t pll_offset_freq(uint32_t base, int16_t offset)
{
	return pll_clamp_freq((int32_t)base + offset);
}

/* SS_CTRL + SPD_SYNC, after selecting the chip this step is for. */
static void pll_ss_write(uint8_t miner_id, uint16_t asic_id, uint32_t ss_ctrl)
{
	a3197s_select_chip(miner_id, asic_id);
	a3197s_smartspeed_set_ctrl(miner_id, ss_ctrl);
}

/* Official lock check: log only. A mismatch does not abort the sequence. */
static void pll_lock_check(uint8_t miner_id, uint16_t asic_id, const char *pll, uint32_t want)
{
	uint32_t cnt;

	delay_ms(5);
	if (asic_id == CHIP_ADDR_BROADCAST)
		a3197s_select_chip(miner_id, 0);
	cnt = a3197s_get_reg(miner_id, REG_SS_CORE_DIST);
	if (cnt != want)
		fprintf(stderr,
			"board %u asic %u: %s did not lock (lock count 0x%08x, expected 0x%08x)\n",
			miner_id, asic_id, pll, cnt, want);
}

/* Mux divider field: <400 -> 4, <600 -> 6, <800 -> 8, else 10. */
static uint32_t pll_mux_word(uint32_t freq)
{
	uint32_t mode = 10;

	if (freq < 400)
		mode = 4;
	else if (freq < 600)
		mode = 6;
	else if (freq < 800)
		mode = 8;
	return (DEFAULT_PLL_COMMIT & ~(0x0Fu << 21)) | (mode << 21);
}

/* Sets g_pll_state.set_pll[miner_id][0][0..3] and applies it via
 * a3197s_pll_apply(), which broadcasts to the whole chain (or addresses
 * each chip in turn while per-chip offsets are set). */
void asic_apply_pll_freq(uint8_t miner_id, const uint32_t pll_freq[4])
{
	int k;

	for (k = 0; k < 4; k++)
		g_pll_state.set_pll[miner_id][0][k] = (uint16_t)pll_freq[k];

	a3197s_pll_apply(miner_id, 0);
}

/* Blocking PLL ramp: steps PLL0 from init_freq to target_freq in
 * PLL_RAMP_STEP_MHZ increments, once per second. Each step, PLL1-3 are
 * set to PLL0 + domain_interval*{1,2,3}. */
#define PLL_RAMP_STEP_MHZ 10

void asic_ramp_pll_freq(uint8_t miner_id, uint16_t init_freq, uint16_t target_freq, uint16_t domain_interval)
{
	int span = (int)target_freq - (int)init_freq;
	int direction = (span >= 0) ? 1 : -1;
	unsigned abs_span = (unsigned)((span >= 0) ? span : -span);
	unsigned m = abs_span / PLL_RAMP_STEP_MHZ;
	unsigned n = abs_span % PLL_RAMP_STEP_MHZ;
	unsigned step;
	uint32_t pll_freq[4];

	if (target_freq == init_freq)
		return;

	for (step = 0; step <= m; step++) {
		uint16_t freq_step = (uint16_t)(init_freq + step * PLL_RAMP_STEP_MHZ * direction);

		pll_freq[0] = freq_step;
		pll_freq[1] = freq_step + domain_interval;
		pll_freq[2] = freq_step + domain_interval * 2;
		pll_freq[3] = freq_step + domain_interval * 3;
		asic_apply_pll_freq(miner_id, pll_freq);
		delay_ms(1000);
	}

	if (n != 0) {
		pll_freq[0] = target_freq;
		pll_freq[1] = target_freq + domain_interval;
		pll_freq[2] = target_freq + domain_interval * 2;
		pll_freq[3] = target_freq + domain_interval * 3;
		asic_apply_pll_freq(miner_id, pll_freq);
		delay_ms(1000);
	}
}

/*
 * Official a3197s_set_pll_seq with pll0_update=1 (every running clock
 * change retargets PLL0). pll0_update=0 parks PLL0 at init_freq and
 * leaves it there. Markers around the update-path writes let the Rust
 * order test see them.
 */
static void a3197s_set_pll_seq(uint8_t miner_id, uint16_t asic_id, uint8_t pll0_update,
			       uint32_t freq0, uint32_t freq1, uint32_t freq2, uint32_t freq3)
{
	uint32_t f0 = pll_clamp_freq((int32_t)freq0);
	uint32_t f1 = pll_clamp_freq((int32_t)freq1);
	uint32_t f2 = pll_clamp_freq((int32_t)freq2);
	uint32_t f3 = pll_clamp_freq((int32_t)freq3);
	uint32_t w0 = get_cpm_pll(f0);
	uint32_t w1 = get_cpm_pll(f1);
	uint32_t w2 = get_cpm_pll(f2);
	uint32_t w3 = get_cpm_pll(f3);
	uint16_t saved = asic_get_chip_select();

	if (w0 == 0 || w1 == 0 || w2 == 0 || w3 == 0) {
		fprintf(stderr,
			"board %u asic %u: refused PLL program, no table word for %u/%u/%u/%u\n",
			miner_id, asic_id, f0, f1, f2, f3);
		return;
	}

	if (!pll0_update) {
		uint32_t park = get_cpm_pll(pll_clamp_freq(PLL_CHIP_MIN_MHZ)) & 0xf7fffffeu;

		a3197s_select_chip(miner_id, asic_id);
		a3197s_set_reg(miner_id, REG_PLL0_FREQ, park);
		delay_ms(1);
	}

	/* SEQ_UPDATE_BEGIN */
	pll_ss_write(miner_id, asic_id, SS_CTRL_RAMP_PRE);
	pll_lock_check(miner_id, asic_id, "PLL0", 152);

	a3197s_select_chip(miner_id, asic_id);
	a3197s_set_reg(miner_id, REG_PLL1_FREQ, w1);
	a3197s_set_reg(miner_id, REG_PLL2_FREQ, w2);
	a3197s_set_reg(miner_id, REG_PLL3_FREQ, w3);
	a3197s_set_reg(miner_id, REG_ROLLTIME, ROLLTIME_TIMEOUT_CONST / f3);
	delay_ms(1);

	if (pll0_update) {
		pll_ss_write(miner_id, asic_id, SS_CTRL_RAMP_MID);
		pll_lock_check(miner_id, asic_id, "PLL1", 152u << 8);

		a3197s_select_chip(miner_id, asic_id);
		a3197s_set_reg(miner_id, REG_PLL_COMMIT, pll_mux_word(f1) | (1u << 4));
		a3197s_set_reg(miner_id, REG_PLL0_FREQ, w0);
		delay_ms(1);
		a3197s_set_reg(miner_id, REG_PLL_COMMIT, pll_mux_word(f0));
		delay_ms(1);
	}

	pll_ss_write(miner_id, asic_id, SS_CTRL_RAMP_COMMIT);
	/* SEQ_UPDATE_END */
	a3197s_select_chip(miner_id, saved);
}

void a3197s_pll_apply(uint8_t miner_id, uint16_t asic_id)
{
	const uint16_t *base = g_pll_state.set_pll[miner_id][0];
	uint16_t chip_select_id;

	(void)asic_id;
	chip_select_id = asic_get_chip_select();

	if (!pll_offsets_active(miner_id)) {
		a3197s_set_pll_seq(miner_id, CHIP_ADDR_BROADCAST, 1,
			base[0], base[1], base[2], base[3]);
	} else {
		/* Each chip gets the same sequence the broadcast path sends,
		 * addressed to it alone, at its own clocks. */
		uint16_t chip;

		for (chip = 0; chip < PLL_CHIP_OFFSET_MAX; chip++) {
			int16_t off = g_pll_chip_offset[miner_id][chip];

			a3197s_set_pll_seq(miner_id, chip, 1,
				pll_offset_freq(base[0], off), pll_offset_freq(base[1], off),
				pll_offset_freq(base[2], off), pll_offset_freq(base[3], off));
		}
	}
	delay_ms(1);

	/* Read-only diagnostic: logs the LOCK/LOCK_IP bits inside the PLLxL1
	 * registers just written, read back from chip 0 as a representative
	 * sample. Not used to gate anything. */
	if (g_lock_diag_fp && g_lock_diag_count < PLL_LOCK_DIAG_MAX_LINES) {
		uint32_t pll0_raw, pll1_raw, pll2_raw, pll3_raw;

		a3197s_select_chip(miner_id, 0);
		pll0_raw = a3197s_get_reg(miner_id, REG_PLL0_FREQ);
		pll1_raw = a3197s_get_reg(miner_id, REG_PLL1_FREQ);
		pll2_raw = a3197s_get_reg(miner_id, REG_PLL2_FREQ);
		pll3_raw = a3197s_get_reg(miner_id, REG_PLL3_FREQ);

		fprintf(g_lock_diag_fp,
			"PLL_LOCK_DIAG chip0 pll0=0x%08x(lock=%d,lock_ip=%d) "
			"pll1=0x%08x(lock=%d,lock_ip=%d) pll2=0x%08x(lock=%d,lock_ip=%d) "
			"pll3=0x%08x(lock=%d,lock_ip=%d)\n",
			pll0_raw, !!(pll0_raw & PLL_LOCK_MASK), !!(pll0_raw & PLL_LOCK_IP_MASK),
			pll1_raw, !!(pll1_raw & PLL_LOCK_MASK), !!(pll1_raw & PLL_LOCK_IP_MASK),
			pll2_raw, !!(pll2_raw & PLL_LOCK_MASK), !!(pll2_raw & PLL_LOCK_IP_MASK),
			pll3_raw, !!(pll3_raw & PLL_LOCK_MASK), !!(pll3_raw & PLL_LOCK_IP_MASK));
		fflush(g_lock_diag_fp);
		g_lock_diag_count++;
	}

	a3197s_select_chip(miner_id, chip_select_id);
}

/* Periodic REG_ROLLTIME refresh, computed from freq3. Deliberately
 * separate from a3197s_pll_apply() so it doesn't also rewrite the
 * PLL0-3 dividers. */
void asic_refresh_rolltime(uint8_t miner_id, uint16_t freq3)
{
	uint16_t chip_select_id = asic_get_chip_select();
	uint32_t tmp32 = ROLLTIME_TIMEOUT_CONST / freq3;

	if (!pll_offsets_active(miner_id)) {
		a3197s_select_chip(miner_id, CHIP_ADDR_BROADCAST);
		a3197s_set_reg(miner_id, REG_ROLLTIME, tmp32);
	} else {
		uint16_t chip;

		for (chip = 0; chip < PLL_CHIP_OFFSET_MAX; chip++) {
			a3197s_select_chip(miner_id, chip);
			a3197s_set_reg(miner_id, REG_ROLLTIME, ROLLTIME_TIMEOUT_CONST /
				pll_offset_freq(freq3, g_pll_chip_offset[miner_id][chip]));
		}
	}
	a3197s_select_chip(miner_id, chip_select_id);
}

/* Moves the per-chip offsets toward target, at most PLL_RAMP_STEP_MHZ per
 * chip per step, one second apart (same pacing as asic_ramp_pll_freq()). */
void asic_set_pll_chip_offsets(uint8_t miner_id, const int16_t target[PLL_CHIP_OFFSET_MAX])
{
	int moving = 1;

	while (moving) {
		int i;

		moving = 0;
		for (i = 0; i < PLL_CHIP_OFFSET_MAX; i++) {
			int16_t *cur = &g_pll_chip_offset[miner_id][i];
			int16_t d = (int16_t)(target[i] - *cur);

			if (d > PLL_RAMP_STEP_MHZ)
				d = PLL_RAMP_STEP_MHZ;
			else if (d < -PLL_RAMP_STEP_MHZ)
				d = -PLL_RAMP_STEP_MHZ;
			if (d != 0) {
				*cur = (int16_t)(*cur + d);
				moving = 1;
			}
		}
		if (moving) {
			/* Once every offset reaches zero this is the broadcast path. */
			a3197s_pll_apply(miner_id, 0);
			delay_ms(1000);
		}
	}
}
