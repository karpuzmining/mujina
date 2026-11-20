#ifndef PLL_CPM_TABLE_H
#define PLL_CPM_TABLE_H

#include <stdint.h>

/* SPDX-License-Identifier: MIT
 *
 * Frequency (MHz) -> PLL L1 word. Official Canaan A3197S table,
 * 349 rows, 15–600 MHz. See pll_cpm_table.c and NOTICE.
 */
#define PLL_TABLE_ROWS 349

extern const uint32_t cpm_table[PLL_TABLE_ROWS][2];

#endif
