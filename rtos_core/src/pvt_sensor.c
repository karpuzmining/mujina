/*
 * Per-chip PVT (process/voltage/temperature) sensor reading, via the
 * TVSENW (select/arm) and TVSENR (result) registers.
 *
 * The sequence is Canaan's a3197s_ops.c (canaan_sdk_for_linux, MIT):
 * one global mode, the four TVSENW writes broadcast to every chip, then
 * the one chip is selected and TVSENR is only re-read. Re-arming on
 * every retry restarts the conversion before a slow sensor finishes, so
 * a hashing chip comes back as the -273 sentinel.
 */

#include <stdint.h>
#include <string.h>
#include <unistd.h>
#include "util.h"
#include "asic_engine.h"

#define PVT_MODE_VOLTAGE 0
#define PVT_MODE_TEMP    1

/* Both temperature and voltage sampling use PVT channel selector 2. */
#define PVT_CHANNEL_SEL 2

/* One mode for the chain, matching the SDK's g_pvt_mode. Per-chip copies
 * drift apart when the mode write is broadcast. */
static uint8_t g_pvt_mode;
static uint8_t g_pvt_sel;
static uint16_t raw_temp[HBOARD_COUNT][MAX_ASIC_ON_SINGLE_HASH];
static uint16_t raw_volt[HBOARD_COUNT][MAX_ASIC_ON_SINGLE_HASH];

static void pvt_write_word(uint8_t miner_id, uint32_t word)
{
        a3197s_set_reg(miner_id, REG_PVT_SELECT, word);
}

/* Latch under the old mode, latch the new mode, pulse go, then start+go.
 * The caller has already selected broadcast or one chip. */
static void pvt_program_mode(uint8_t miner_id, uint8_t mode, uint8_t sel)
{
        uint32_t old_word = DEFAULT_PVT_SELECT | ((uint32_t)g_pvt_sel << 3) | g_pvt_mode;
        uint32_t new_word = DEFAULT_PVT_SELECT | ((uint32_t)sel << 3) | mode;

        pvt_write_word(miner_id, old_word);
        g_pvt_mode = mode;
        g_pvt_sel = sel;
        pvt_write_word(miner_id, new_word);
        usleep(1);
        pvt_write_word(miner_id, new_word | 2u);
        pvt_write_word(miner_id, new_word | 6u);
}

/* Raw TVSENR code -> degrees C. Canaan decode_pvt_temp()
 * (a3197s_pvt.c, MIT, canaan_sdk_for_linux 07fe73f). Out of −50..200 °C
 * is ASIC_TEMP_NONE; this tree reports that as −273 so existing callers
 * keep their PVT_TEMP_INVALID check. */
static double temp_decode(uint16_t raw)
{
        double c = (double)raw;
        double temp = (((2.28746e-10 * c + -2.84236e-06) * c + 1.31753e-02) * c + -26.70380) * c
                + 19725.61;

        if ((temp < -50) || (temp > 200))
                temp = -273;

        return temp;
}

/* Raw TVSENR code -> mV. Canaan decode_pvt_volt(): 0 below 14 mV.
 * The double is kept (the official helper rounds to uint16) so telemetry
 * is not quantized by an extra 0.5 mV. */
static double volt_decode(uint16_t raw)
{
        double mv = ((double)raw * 1000.0 + 19144.2) / 3434.4847;

        if (mv < 14.0)
                return 0;

        return mv;
}

void a3197s_pvt_init(uint8_t miner_id, uint16_t asic_id)
{
        g_pvt_mode = 0;
        g_pvt_sel = 0;
        a3197s_select_chip(miner_id, asic_id);

        pvt_write_word(miner_id, DEFAULT_PVT_SELECT);
        pvt_write_word(miner_id, DEFAULT_PVT_SELECT | 2u);
        pvt_write_word(miner_id, DEFAULT_PVT_SELECT | 2u);
        pvt_write_word(miner_id, DEFAULT_PVT_SELECT | 6u);

        memset(raw_temp[miner_id], 0, sizeof(raw_temp[miner_id]));
        memset(raw_volt[miner_id], 0, sizeof(raw_volt[miner_id]));
}

/* Ready bit in TVSENR. A read of a finished conversion clears it, so the
 * poll below only re-reads. It does not re-arm. */
#define PVT_READY_BIT 0x01000000u
#define PVT_READ_TRIES 8

static uint32_t pvt_read_ready(uint8_t miner_id)
{
        int i;
        uint32_t reg = 0;

        usleep(700);
        for (i = 0; i < PVT_READ_TRIES; i++) {
                reg = a3197s_get_reg(miner_id, REG_PVT_RESULT);
                if (reg & PVT_READY_BIT)
                        return reg;
                usleep(100);
        }
        return reg;
}

/* Broadcast the mode the way a3197s_pvt_select_mode() does, then select
 * this chip and only read the result. */
static uint32_t pvt_sample(uint8_t miner_id, uint16_t asic_id, uint8_t mode)
{
        a3197s_select_chip(miner_id, CHIP_ADDR_BROADCAST);
        pvt_program_mode(miner_id, mode, PVT_CHANNEL_SEL);
        a3197s_select_chip(miner_id, asic_id);
        return pvt_read_ready(miner_id);
}

void pvt_vcore_update(uint8_t miner_id, uint16_t asic_id)
{
        uint32_t reg = pvt_sample(miner_id, asic_id, PVT_MODE_VOLTAGE);

        raw_volt[miner_id][asic_id] = (reg & PVT_READY_BIT) ? ((reg >> 12) & 0xfff) : 0;
}

void pvt_tcore_update(uint8_t miner_id, uint16_t asic_id)
{
        uint32_t reg = pvt_sample(miner_id, asic_id, PVT_MODE_TEMP);

        raw_temp[miner_id][asic_id] = (reg & PVT_READY_BIT) ? (reg & 0xfff) : 0;
}

double pvt_vcore_get(uint8_t miner_id, uint16_t asic_id)
{
        return volt_decode(raw_volt[miner_id][asic_id]);
}

double pvt_tcore_get(uint8_t miner_id, uint16_t asic_id)
{
        return temp_decode(raw_temp[miner_id][asic_id]);
}

double pvt_tcore_avg_get(uint16_t asic_count, double *temp_avgs)
{
        uint16_t board, chip;
        uint16_t total_cnt = 0;
        double total_avg = 0;

        for (board = 0; board < HBOARD_COUNT; board++) {
                double board_avg = 0;
                uint16_t board_cnt = 0;

                for (chip = 0; chip < asic_count; chip++) {
                        double t;

                        if (raw_temp[board][chip] == 0)
                                continue;
                        t = temp_decode(raw_temp[board][chip]);
                        /* -273 is the invalid-read sentinel. It must not
                         * pull the average down. */
                        if (t <= -273.0)
                                continue;
                        board_avg += t;
                        board_cnt++;
                }

                total_avg += board_avg;
                total_cnt += board_cnt;
                temp_avgs[board] = board_cnt ? board_avg / board_cnt : -273.0;
        }

        if (!total_cnt)
                return -273.0;

        return total_avg / total_cnt;
}
