#![no_std]
#![no_main]

//! Première image V4 chargeable par U-Boot.
//!
//! Ce jalon prépare le chemin Saturn jusqu'au timing 1600x900. Le test courant
//! utilise une couche couleur pleine du compositeur afin d'isoler l'émission
//! DPU/DisplayPort de la lecture DDR par RDMA.

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{Ordering, fence};

const UART0_BASE: usize = 0xD401_7000;
const UART_THR: usize = 0x00;
const UART_IER: usize = 0x04;
const UART_FCR_IIR: usize = 0x08;
const UART_LCR: usize = 0x0C;
const UART_MCR: usize = 0x10;
const UART_LSR: usize = 0x14;
const UART_LSR_TDRQ: u32 = 1 << 5;

// U-Boot starts WDT0 before handing control to the bare-metal image. The
// firmware must take ownership of it, otherwise the 60 s U-Boot timeout
// resets the board while a display test is running.
const WDT0_BASE: usize = 0xD401_4000;
const WDT_TWFAR: usize = 0xB0;
const WDT_TWSAR: usize = 0xB4;
const WDT_TWER: usize = 0xB8;
const WDT_TWFAR_KEY: u32 = 0xBABA;
const WDT_TWSAR_KEY: u32 = 0xEB10;

const SPLASH_ADDR: usize = 0x1020_0000_0;
const SPLASH_SIZE: usize = 1600 * 900 * 4;
const RGB_HOLD_CYCLES: usize = 500_000_000;
// First isolate CMP1 from RDMA/DMMU with a solid blue layer. Once this is
// visible, the same scene can be reused for the framebuffer path.
const SATURN_SOLID_TEST: bool = false;
// Hold the first solid-blue frame while debugging the Saturn path. The RGB
// cycle remains available for a later visual test once a stable frame exists.
const SATURN_SOLID_CYCLE_TEST: bool = false;
// The HEE composer commits layer registers through its cmdlist. Direct MMIO
// readbacks alone do not make a scene active, so use the constructor's
// composer cmdlist for the visible solid-color test.
const SATURN_SOLID_DIRECT_TEST: bool = false;
// Diagnostic isolating the DMMU from the pixel path. The direct RDMA test is
// re-enabled only after the solid CMP1 path is visible.
const RDMA_PHYS_DIRECT_TEST: bool = true;
// The constructor's cmdlist row for POSTPIPE+0x34 is still read back as zero
// on this board. Write the same active dimensions directly before CFG_READY
// to distinguish a bad cmdlist row from an inactive/wrong POSTPIPE window.
// This is a temporary diagnostic and is disabled once the path is understood.
const POSTPIPE_DIRECT_TEST: bool = true;
// Set this to true only for the known-good blue reference. It bypasses Saturn
// entirely; false selects the normal Saturn/RDMA stream. The reference has
// already validated the physical DP path, so the bring-up image now tests
// the actual DPU pixel path again.
const DP_COLORBAR_TEST: bool = false;
// Diagnostic after the first visible gray frame: disable layer00 and make the
// TMG generate a pure red background. This isolates the output color path
// from Saturn's layer and command-list data.
const TMG_BACKGROUND_TEST: bool = false;
const CMP_BACKGROUND_TEST: bool = false;

// The Pico-ITX device tree routes the active external output through DP1/eDP1
// and DPU1. The initial probe is read-only. The controlled APMU step below
// only enables the documented DPU1/eDP1 gates and deasserts their resets.
const DPU1_BASE: usize = 0xC044_0000;
const DP1_BASE: usize = 0xCAC8_8000;
const APMU_BASE: usize = 0xD428_2800;

// Inno DisplayPort/eDP register fields from spacemit_inno_dp.h.  The helper
// below performs the same read-modify-write used by the Linux driver.
const DP_CORE_CONTROL: usize = 0x18;
const DP_SOFT_RESET: usize = 0x1C;
const DP_CLK_INV_MUX: usize = 0x28;
const DP_PHYIF_CTRL: usize = 0x100;
const DP_MPLL_CTRL0: usize = 0x180;
const DP_MPLL_CTRL1: usize = 0x184;
const DP_MPLL_CTRL2: usize = 0x188;
const DP_PREPLL_CTRL0: usize = 0x190;
const DP_PREPLL_CTRL1: usize = 0x194;
const DP_PREPLL_CTRL2: usize = 0x198;
const DP_PREPLL_SSC: usize = 0x19C;
const DP_ANA_TX_CLK_CTRL: usize = 0x1A0;
const DP_ANA_TX_ENABLE: usize = 0x1A4;
const DP_ANA_TX_CTRL1: usize = 0x1A8;
const DP_ANA_TX_CTRL2: usize = 0x1AC;
const DP_ANA_TX_CTRL3: usize = 0x1B0;
const DP_ANA_BG_CTRL: usize = 0x1BC;
const DP_ANA_RTCAL_CTRL: usize = 0x1C0;
const DP_ANA_TX_CTRL8: usize = 0x1C4;
const DP_ANA_TX_AUX_CTRL: usize = 0x1D0;
const DP_ANA_LANE_EMPH: usize = 0x1DC;
const DP_GENERAL_INTERRUPT: usize = 0x080;
const DP_GENERAL_INTERRUPT_MASK: usize = 0x084;
const DP_HPD_INTERRUPT_STATUS: usize = 0x088;
const DP_HPD_INTERRUPT_ENABLE: usize = 0x08C;
const DP_AUX_STATUS: usize = 0x400;
const DP_AUX_CONFIG: usize = 0x404;
const DP_AUX_START: usize = 0x408;
const DP_AUX_DATA4: usize = 0x40C;
const DP_AUX_DATA3: usize = 0x410;
const DP_AUX_DATA2: usize = 0x414;
const DP_AUX_DATA1: usize = 0x418;
const DP_VIDEO_VSAMPLE: usize = 0x200;
const DP_VINPUT_POLARITY: usize = 0x20C;
const DP_VIDEO_CONFIG2: usize = 0x210;
const DP_VIDEO_CONFIG1: usize = 0x214;
const DP_VIDEO_CONFIG5: usize = 0x218;
const DP_VIDEO_CONFIG3: usize = 0x21C;
const DP_VIDEO_CONFIG4: usize = 0x220;
const DP_VIDEO_MSA1: usize = 0x224;
const DP_VIDEO_MSA2: usize = 0x228;
const DP_VIDEO_MSA3: usize = 0x22C;
const DP_VIDEO_HBLANK_INTERVAL: usize = 0x230;
const DP_VIDEO_COLORBAR_CFG: usize = 0x238;

// Inno DP video reset covers the four stream state machines in bits 3:0.
// The Linux driver asserts the complete field during transmitter init.
const DP_VIDEO_RESET_HIGH: u32 = 3;
const DP_VIDEO_RESET_LOW: u32 = 0;
const DP_STREAM_ENC_EN: u32 = 18;

const DP_CORE_PLL_LOCK: u32 = 1 << 7;
const DP_PIXEL_PLL_LOCK: u32 = 1 << 7;
// Bianbu/Linux selects the minimum link that exactly carries 1600x900@60:
// RBR, 1.62 Gb/s, two lanes. This is also the mode accepted by the working
// USB-C/HDMI adapter on the reference system.
const DP_LINK_RATE_KHZ: u32 = 162_000;

// K3 APMU offsets from include/soc/spacemit/k3-syscon.h.  The four registers
// below contain the DPU1/eDP1 clock gates and reset bits. The firmware first
// logs them for comparison, then applies only the documented enable/reset
// mask before touching Saturn.
const APMU_LCD_EDP_CTRL: usize = 0x23C;
const APMU_LCD_CLK_RES_CTRL3: usize = 0x26C;
const APMU_LCD_CLK_RES_CTRL4: usize = 0x270;
const APMU_LCD_CLK_RES_CTRL5: usize = 0x274;

const APMU_EDP1_RESET: u32 = 1 << 16;
const APMU_EDP1_CLK_GATE: u32 = 1 << 17;
const APMU_EDP1_PIXCLK_SEL: u32 = 1 << 18;
const APMU_DSI4LN2_ESC_CLK_GATE: u32 = 1 << 2;
const APMU_DSI4LN2_ESC_RESET: u32 = 1 << 3;
const APMU_DSI4LN2_LCD_RESET: u32 = 1 << 4;
// CTRL4 contains the DSI4LN2 LCD pixel/memory/DSC clock selectors and gates.
// These fields were previously (incorrectly) ORed into CTRL3, whose low
// bits belong to the ESC clock.  That changed the ESC source and never
// enabled the DPU pixel clock explicitly.
const APMU_DSI4LN2_LCD_PXCLK_GATE: u32 = 1 << 16;
const APMU_DSI4LN2_LCD_DSC_GATE: u32 = 1 << 14;
const APMU_DSI4LN2_LCD_MCLK_GATE: u32 = 1;
const APMU_DSI4LN2_LCD_MCLK_RESET: u32 = 1 << 9;
const APMU_DSI4LN2_LCD_DSC_RESET: u32 = 1 << 15;
const APMU_DSI4LN2_DPU_ACLK_RESET: u32 = 1;
const APMU_DSI4LN2_DPU_ACLK_GATE: u32 = 1 << 1;

// Linux's active DPU1 clock tree on the K3 Pico-ITX is:
//   PXCLK: PLL2_DIV4 / 5  ~= 108 MHz
//   MCLK:  614.4 MHz / 2  = 307.2 MHz
//   ACLK:  409.6 MHz / 1  = 409.6 MHz
// Keep these field values explicit so bare-metal startup does not depend on
// whatever partial clock state U-Boot happened to leave behind.
const APMU_DSI4LN2_LCD_PXCLK_SEL: u32 = 5;
const APMU_DSI4LN2_LCD_PXCLK_DIV: u32 = 4;
const APMU_DSI4LN2_LCD_MCLK_SEL: u32 = 2;
const APMU_DSI4LN2_LCD_MCLK_DIV: u32 = 1;
const APMU_DSI4LN2_DPU_ACLK_SEL: u32 = 0;
const APMU_DSI4LN2_DPU_ACLK_DIV: u32 = 0;

const DPU_DMA_TOP_IMAGE_CTRL: usize = 0x1284;
const DPU_DMA_TOP_TIMEOUT: usize = 0x1288;
const DPU_MMU_TOP_CTRL: usize = 0x1200;
const DPU_MMU_TOP_DMAC0_OUTS: usize = 0x1208;
const DPU_MMU_TOP_DMAC1_OUTS: usize = 0x120C;
// Linux maps dev_id=1 to CMP1_BASE_ADDR = 0x30000.
const DPU1_COMPOSER1: usize = 0x30000;
// Linux configures the postpipe for every online commit, including the
// no-post-scaler case. Without these two composer-cmdlist rows, CMP1 can
// accept the layer values while the normal pixel path remains black.
const DPU1_POSTPIPE: usize = DPU1_COMPOSER1 + 0x300;
const POSTPIPE_CTRL: usize = DPU1_POSTPIPE;
const POSTPIPE_SIZE: usize = DPU1_POSTPIPE + 0x34;
// Linux maps pipeline 1 to TMG0_BASE_ADDR = 0x51200.  The output timing
// block is deliberately staged with fm_timing_en=0 until the RDMA/DMMU and
// DP link have been configured.
const DPU1_TMG0: usize = 0x51200;
const TMG_CTRL: usize = 0x00;
const TMG_USER: usize = 0x08;
const TMG_HFP_HBP: usize = 0x0C;
const TMG_HSYNC_HBP: usize = 0x10;
const TMG_VFP_VSYNC: usize = 0x14;
const TMG_VBP_HACTIVE: usize = 0x18;
const TMG_VACTIVE: usize = 0x1C;
const TMG_EOF_LINE_DELAY: usize = 0x2C;
const TMG_SOF_PRE_LINE: usize = 0x38;
const TMG_VSYNC_ALIGN: usize = 0x3C;
const TMG_BACKGROUND_RG: usize = 0x24;
const TMG_BACKGROUND_B: usize = 0x28;

const DPU1_RDMA0: usize = 0x1000;
const DPU1_INT: usize = 0x700;
const DPU_ONLINE_IRQ_MASK: usize = DPU1_INT + 0x0C;
const DPU_ONLINE_IRQ_STATUS: usize = DPU1_INT + 0x30;
const DPU_ONLINE_IRQ_ENTER_PENDING: usize = DPU1_INT + 0x34;
const DPU_ONLINE_IRQ_RAW: usize = DPU1_INT + 0x58;
const DPU_ONLINE_IRQ_RAW_ENTER_PENDING: usize = DPU1_INT + 0x5C;
const DPU_ONLINE_IRQ_RAW_PADDR: usize = DPU1_INT + 0x60;
// RDMA_PATH_REG index 43..52, from the constructor Saturn register map.
// These are read-only diagnostics and do not alter the active scanout.
const RDMA_DBG_IRQ_RAW: usize = DPU1_RDMA0 + 43 * 4;
const RDMA_DBG_IRQ_MASK: usize = DPU1_RDMA0 + 44 * 4;
const RDMA_DBG_IRQ_STATUS: usize = DPU1_RDMA0 + 45 * 4;
const RDMA_DBG_INFO0: usize = DPU1_RDMA0 + 47 * 4;
const RDMA_DBG_INFO1: usize = DPU1_RDMA0 + 48 * 4;
const RDMA_DBG_INFO2: usize = DPU1_RDMA0 + 49 * 4;
const RDMA_DBG_INFO3: usize = DPU1_RDMA0 + 50 * 4;
const RDMA_DBG_INFO4: usize = DPU1_RDMA0 + 51 * 4;
const DPU1_CTRL_TOP: usize = 0x3C0;
// cmdlist_cmps_other_en and cmdlist_cmps_top_en for dev_id=1. Linux enables
// both bits when it commits the composer cmdlist group.
// dpu_ctl_top_reg_10[dev_id] contains cmdlist_cmps_other_en and
// cmdlist_cmps_top_en.  For composer 1 this is register 10, hence +0x28.
const DPU1_CMDLIST_CMPS: usize = DPU1_CTRL_TOP + 0x28;
const DPU1_CMDLIST_CFG_READY: usize = DPU1_CTRL_TOP + 0x34;
const DPU1_CMDLIST_REUSE_ACTIVE: usize = DPU1_CTRL_TOP + 0x40;
const DPU1_CMDLIST_SW_EVENT_BASE: usize = DPU1_CTRL_TOP + 0x58;
const DPU1_CMDLIST_START_CMPS_Y_BASE: usize = DPU1_CTRL_TOP + 0x90;
const DPU1_SCENE1: usize = 0x340;
const DPU1_SCENE1_CMD_UPDATE: usize = DPU1_SCENE1 + 0x04;
const DPU1_SCENE1_VIDEO_MODE: usize = DPU1_SCENE1 + 0x0C;
// The Linux driver selects scene-control block 1 for DPU1 through
// DPU_SCENE_CTL_ADDR(dev_id): 0x300 + 1 * 0x40 = 0x340.  The previous
// version used the separately named scene-control block 2 at 0x380 for the
// ready/start handshake; those writes read back but do not start DPU1.
const DPU1_SCENE_CTL: usize = DPU1_SCENE1;
const DPU_SCENE_CTL_CONFIG_READY: usize = DPU1_SCENE_CTL + 0x08;
const DPU_SCENE_CTL_VIDEO_MODE: usize = DPU1_SCENE_CTL + 0x0C;
const DPU_SCENE_CTL_SW_START: usize = DPU1_SCENE_CTL + 0x10;
const RDMA_CTRL: usize = 0x00;
const RDMA_BASE_ADDR0_LOW: usize = 0x24;
const RDMA_BASE_ADDR0_HIGH: usize = 0x28;
const RDMA_STRIDE0: usize = 0x3C;
const RDMA_IMAGE_SIZE: usize = 0x40;
const RDMA_BBOX_START: usize = 0x44;
const RDMA_BBOX_END: usize = 0x48;
const RDMA_FORMAT: usize = 0x74;

// Linux uses TBU0 for RDMA0.  The table is placed in a separate DRAM range:
// it does not overlap the splash at 0x102000000 or the firmware at
// 0x140000000.
const DMMU_TABLE_ADDR: usize = 0x1010_0000_0;
const DMMU_PAGE_SIZE: usize = 4096;
const DMMU_GUARD_ENTRIES: usize = 60;
const DMMU_PAGE_COUNT: usize = (SPLASH_SIZE + DMMU_PAGE_SIZE - 1) / DMMU_PAGE_SIZE;
const DMMU_TABLE_ENTRIES: usize = DMMU_PAGE_COUNT + DMMU_GUARD_ENTRIES;
const DMMU_TBU0: usize = 0x1100;
const DMMU_TBU_CTRL: usize = 0x00;
const DMMU_TBU_BASE0_LOW: usize = 0x04;
const DMMU_TBU_BASE0_HIGH: usize = 0x08;
const DMMU_TBU_VA0: usize = 0x1C;
const DMMU_TBU_SIZE0: usize = 0x28;
const DMMU_TBU_VSYNC: usize = 0x34;
const DMMU_BASE_VA: u32 = 0x1000_0000;
const DMMU_TBU_CTRL_VALUE: u32 = 0x431; // enable, burst limit, active fetch, urgent QoS
const DMMU_TBU_CTRL_DIRECT_VALUE: u32 = 0;

// Linux keeps the plane list on the RDMA channel and the CRTC list on the
// composer channel. The two buffers are separate so the HEE parser cannot
// apply RDMA rows as if they belonged to CMP1.
const CMDLIST_ADDR: usize = 0x1011_0000_0;
const CMDLIST_CRTC_ADDR: usize = 0x1011_1000_0;
const CMDLIST_BASE: usize = 0x500;
// wb_cmdlist() uses channel hwdev->rdma_nums + 2 + crtc_id for the online
// composer list. Saturn HEE has four RDMA channels and DPU1 is CRTC 1, so
// the composer list is channel 7. Channel 0 belongs to RDMA0.
const CMDLIST_COMPOSER_CHANNEL: usize = 4 + 2 + 1;
const CMDLIST_PLANE_CHANNEL: usize = 0;
const CMDLIST_PLANE_START_ADDRL: usize = CMDLIST_BASE + CMDLIST_PLANE_CHANNEL * 4;
const CMDLIST_PLANE_START_ADDRH: usize = CMDLIST_BASE + 0x40 + CMDLIST_PLANE_CHANNEL * 4;
const CMDLIST_PLANE_Y_FIRST: usize = CMDLIST_BASE + 0xC0 + CMDLIST_PLANE_CHANNEL * 4;
const CMDLIST_COMPOSER_START_ADDRL: usize = CMDLIST_BASE + CMDLIST_COMPOSER_CHANNEL * 4;
const CMDLIST_COMPOSER_START_ADDRH: usize = CMDLIST_BASE + 0x40 + CMDLIST_COMPOSER_CHANNEL * 4;
const CMDLIST_COMPOSER_Y_FIRST: usize = CMDLIST_BASE + 0xC0 + CMDLIST_COMPOSER_CHANNEL * 4;
const CMDLIST_START_CMPS_Y: usize = DPU1_CTRL_TOP + 0x90;
const CMDLIST_DBG0: usize = CMDLIST_BASE + 0x1D8 + CMDLIST_COMPOSER_CHANNEL * 4;
const CMDLIST_DBG_STATUS: usize = CMDLIST_BASE + 0x1F8;
// saturn_get_cl_cfg_rdy_addr() returns DPU_CTRL_BASE_ADDR + 0x34.  This is
// dpu_ctl_top_reg_13, not a register in the cmdlist window: DPU1+0x534 is
// reserved in cmdlist_top.h.  The final cmdlist row must therefore target
// DPU1+0x3F4.
const CMDLIST_CFG_READY_ADDR: usize = DPU1_BASE + 0x3F4;
const CMDLIST_PLANE_CFG_BITS: u32 = 1 << CMDLIST_PLANE_CHANNEL;
const CMDLIST_COMPOSER_CFG_BITS: u32 = 1 << 21;
// The Saturn row stores a 12-bit strobe field at bits 19..30. Each register
// occupies one nibble in that field (0xF, 0xF0, 0xF00).
const CMDLIST_STROBE_ONE: u32 = 0xF;
const CMDLIST_STROBE_TWO: u32 = 0xFF;
const CMDLIST_STROBE_ALL: u32 = 0xFFF;
// Linux's Saturn cmdlist waits for RDMA reload (wait_event bit 4) and
// composer reload (wait_event bit 6). A solid-only diagnostic has no RDMA
// channel, so it must not wait for an event that can never occur. The header
// stores bits 0..4 in the high five bits of the first u64 and the remaining
// bits in the next u64.
const CMDLIST_WAIT_EVENT_LOW: u64 = if SATURN_SOLID_TEST || CMP_BACKGROUND_TEST {
    0
} else {
    1u64 << 63
};
const CMDLIST_WAIT_EVENT_HIGH: u64 = 1u64 << 1;

const CMP_DST_SIZE: usize = DPU1_COMPOSER1 + 0x04;
const CMP_BG_ALPHA: usize = DPU1_COMPOSER1 + 0x14;
const CMP_LAYER0_AREA_X: usize = DPU1_COMPOSER1 + 0x20;
const CMP_LAYER0_AREA_Y: usize = DPU1_COMPOSER1 + 0x24;
const CMP_BG_R: usize = DPU1_COMPOSER1 + 0x08;
const CMP_BG_G: usize = DPU1_COMPOSER1 + 0x0C;
const CMP_BG_B: usize = DPU1_COMPOSER1 + 0x10;
const CMP_LAYER0_SOLID_R: usize = DPU1_COMPOSER1 + 0x28;
const CMP_LAYER0_SOLID_G: usize = DPU1_COMPOSER1 + 0x2C;
const CMP_LAYER0_SOLID_B: usize = DPU1_COMPOSER1 + 0x30;
const CMP_LAYER0_SOLID_A: usize = DPU1_COMPOSER1 + 0x34;
const CMP_LAYER0_CTRL: usize = DPU1_COMPOSER1 + 0x38;

const SOLID_ALPHA: u32 = 0xFF;
const SOLID_RED: u32 = 0x3FF;
const SOLID_GREEN: u32 = 0x3FF;
const SOLID_BLUE: u32 = 0x3FF;
// The K3 HEE driver writes solid_color_a through the bitfield and leaves
// solid_area_mode at its reset value. Keep the bare-metal value identical to
// composer_x.h instead of forcing the undocumented bit 16.
const CMP_LAYER0_SOLID_A_VALUE: u32 = SOLID_ALPHA;
const CMP_LAYER0_SOLID_CTRL: u32 = 0x0000_FF09;
const CMP_LAYER0_TEST_CTRL: u32 = if TMG_BACKGROUND_TEST || CMP_BACKGROUND_TEST {
    0
} else {
    CMP_LAYER0_SOLID_CTRL
};

// HP Compaq LA2006x: 1600x900@60 DMT timing. The monitor accepts the
// resolution and refresh rate, but its native timing is the 108 MHz DMT
// mode rather than the 97.5 MHz CVT reduced-blanking variant.
const DPU1_MODE_WIDTH: u32 = 1600;
const DPU1_MODE_HEIGHT: u32 = 900;
const DPU1_MODE_HFP: u32 = 24;
const DPU1_MODE_HSYNC: u32 = 80;
const DPU1_MODE_HBP: u32 = 96;
const DPU1_MODE_VFP: u32 = 1;
const DPU1_MODE_VSYNC: u32 = 3;
const DPU1_MODE_VBP: u32 = 96;
const DPU1_PIXEL_CLOCK_KHZ: u32 = 108_000;

// saturn_init_tmg() programs these control values before enabling the frame
// timing state machine. The Linux driver stores vfp-7 and vfp-6 as 16-bit
// values; for the LA2006x mode with vfp=1 this intentionally wraps to FFFA
// and FFFB.
const TMG_EOF_LINE_DELAY_VALUE: u32 =
    (DPU1_MODE_VFP.wrapping_sub(7) & 0xFFFF) | ((DPU1_MODE_VFP.wrapping_sub(6) & 0xFFFF) << 16);
const TMG_SOF_PRE_LINE_VALUE: u32 = 0;

const TMG_CTRL_HSP_VSP: u32 = (1 << 7) | (1 << 8);
const TMG_OUTFMT_RGB888: u32 = 2 << 12;
// TMG0+0x0C contains split_overlap in bits 9:0 and HFP in bits 27:16.
// HBP is in TMG0+0x10, so it must not be packed into this register.
const TMG_HFP_REGISTER_VALUE: u32 = DPU1_MODE_HFP << 16;
// saturn_tmg_reg_0: bit 0 is split_en, while bit 2 is fm_timing_en. The
// previous value used bit 0 and left the frame-timing generator stopped.
const TMG_CTRL_ACTIVE: u32 = TMG_CTRL_HSP_VSP | (1 << 2);

global_asm!(
    r#"
        .section .text._start, "ax"
        .global _start
        .type _start, @function
    _start:
        la      sp, __stack_top
        call    rust_main
    1:
        wfi
        j       1b
    "#
);

struct Uart;

impl Uart {
    const fn new() -> Self {
        Self
    }

    fn register(offset: usize) -> *mut u32 {
        (UART0_BASE + offset) as *mut u32
    }

    fn write_register(&mut self, offset: usize, value: u32) {
        // SAFETY: UART0_BASE is the documented MMIO window for the DEBUG UART.
        unsafe { write_volatile(Self::register(offset), value) }
    }

    fn read_register(&self, offset: usize) -> u32 {
        // SAFETY: UART0_BASE is the documented MMIO window for the DEBUG UART.
        unsafe { read_volatile(Self::register(offset)) }
    }

    fn init(&mut self) {
        // Keep the divisor selected by U-Boot; it already produced 115200 8N1.
        self.write_register(UART_IER, 0);
        self.write_register(UART_LCR, 0x03); // 8 data bits, no parity, 1 stop.
        self.write_register(UART_FCR_IIR, 0x07); // Enable and clear both FIFOs.
        self.write_register(UART_MCR, 0);
        self.write_register(UART_IER, 1 << 6); // UART unit enable.
    }

    fn put_byte(&mut self, byte: u8) {
        while self.read_register(UART_LSR) & UART_LSR_TDRQ == 0 {
            core::hint::spin_loop();
        }
        self.write_register(UART_THR, u32::from(byte));
    }

    fn write_str(&mut self, text: &str) {
        for byte in text.bytes() {
            if byte == b'\n' {
                self.put_byte(b'\r');
            }
            self.put_byte(byte);
        }
    }

    fn write_hex_u32(&mut self, value: u32) {
        const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
        self.write_str("0x");
        for shift in (0..8).rev() {
            self.put_byte(DIGITS[((value >> (shift * 4)) & 0xF) as usize]);
        }
    }

    fn write_hex_u64(&mut self, value: u64) {
        const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
        self.write_str("0x");
        for shift in (0..16).rev() {
            self.put_byte(DIGITS[((value >> (shift * 4)) & 0xF) as usize]);
        }
    }

    fn write_mmio(&mut self, label: &str, address: usize) {
        // SAFETY: the addresses are the documented DPU1/DP1 MMIO windows;
        // this probe only performs a 32-bit volatile read.
        let value = unsafe { read_volatile(address as *const u32) };
        self.write_str(label);
        self.write_hex_u32(value);
        self.write_str("\n");
    }

    fn disable_inherited_watchdog(&mut self) -> bool {
        // WDT writes are individually protected by the two access keys. The
        // documented TWER value 0 stops the counter and clears its value.
        unsafe {
            write_volatile((WDT0_BASE + WDT_TWFAR) as *mut u32, WDT_TWFAR_KEY);
            write_volatile((WDT0_BASE + WDT_TWSAR) as *mut u32, WDT_TWSAR_KEY);
            write_volatile((WDT0_BASE + WDT_TWER) as *mut u32, 0);
        }
        let readback = unsafe { read_volatile((WDT0_BASE + WDT_TWER) as *const u32) };
        self.write_str("WDT0+0x0B8_RB=");
        self.write_hex_u32(readback);
        self.write_str("\n");
        readback & 1 == 0
    }

    fn or_mmio(&mut self, label: &str, address: usize, mask: u32) -> bool {
        // SAFETY: this is a documented APMU register. The mask contains only
        // clock-gate and reset-deassert bits identified by the K3 Linux
        // clock/reset drivers.
        let old = unsafe { read_volatile(address as *const u32) };
        let value = old | mask;
        unsafe { write_volatile(address as *mut u32, value) };
        let readback = unsafe { read_volatile(address as *const u32) };
        self.write_str(label);
        self.write_hex_u32(readback);
        self.write_str("\n");
        readback & mask == mask
    }

    fn write_mmio_value(&mut self, label: &str, address: usize, value: u32) -> bool {
        // SAFETY: the address is a documented Saturn register in DPU1. The
        // caller supplies the reset-time value from the Linux Saturn driver.
        unsafe { write_volatile(address as *mut u32, value) };
        let readback = unsafe { read_volatile(address as *const u32) };
        self.write_str(label);
        self.write_hex_u32(readback);
        self.write_str("\n");
        readback == value
    }

    fn write_mmio_command(&mut self, label: &str, address: usize, value: u32) -> bool {
        // Some Saturn control registers are write strobes: the command is
        // accepted when written, then the hardware clears it before the
        // readback. A zero readback is therefore a valid completion state.
        unsafe { write_volatile(address as *mut u32, value) };
        let readback = unsafe { read_volatile(address as *const u32) };
        self.write_str(label);
        self.write_hex_u32(readback);
        self.write_str("\n");
        readback == value || readback == 0
    }

    fn write_field(&mut self, address: usize, high: u32, low: u32, value: u32) -> bool {
        let width = high - low + 1;
        let field_mask = if width == 32 {
            u32::MAX
        } else {
            ((1u32 << width) - 1) << low
        };
        let old = unsafe { read_volatile(address as *const u32) };
        let updated = (old & !field_mask) | ((value << low) & field_mask);
        unsafe { write_volatile(address as *mut u32, updated) };
        let readback = unsafe { read_volatile(address as *const u32) };
        (readback & field_mask) == (updated & field_mask)
    }

    fn delay(&self, cycles: usize) {
        for _ in 0..cycles {
            core::hint::spin_loop();
        }
    }

    fn dp_aux_read(&mut self, address: u32, data: &mut [u8]) -> bool {
        if data.is_empty() || data.len() > 16 {
            return false;
        }

        // Clear the previous reply event, program a native AUX read, and
        // trigger it. The register layout follows spacemit_inno_dp.c.
        unsafe { write_volatile((DP1_BASE + DP_GENERAL_INTERRUPT) as *mut u32, 1 << 16) };
        // AUX_CONFIG and AUX_START are command registers. Use the complete
        // values, as U-Boot and the Linux driver do, instead of relying on a
        // read-modify-write plus a readback of the start strobe.
        let config = ((data.len() - 1) as u32) << 24 | (address << 4) | 0x9;
        unsafe {
            write_volatile((DP1_BASE + DP_AUX_CONFIG) as *mut u32, config);
            write_volatile((DP1_BASE + DP_AUX_START) as *mut u32, 0);
            write_volatile((DP1_BASE + DP_AUX_START) as *mut u32, 1 << 31);
        }

        let mut completed = false;
        for _ in 0..2_000_000 {
            if unsafe { read_volatile((DP1_BASE + DP_GENERAL_INTERRUPT) as *const u32) } & (1 << 16)
                != 0
            {
                completed = true;
                break;
            }
            core::hint::spin_loop();
        }
        if !completed
            || ((unsafe { read_volatile((DP1_BASE + DP_AUX_STATUS) as *const u32) } >> 24) & 0xF)
                != 0
        {
            return false;
        }

        let words = [
            unsafe { read_volatile((DP1_BASE + DP_AUX_DATA1) as *const u32) },
            unsafe { read_volatile((DP1_BASE + DP_AUX_DATA2) as *const u32) },
            unsafe { read_volatile((DP1_BASE + DP_AUX_DATA3) as *const u32) },
            unsafe { read_volatile((DP1_BASE + DP_AUX_DATA4) as *const u32) },
        ];
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = (words[index / 4] >> ((index % 4) * 8)) as u8;
        }
        true
    }

    fn dp_aux_write(&mut self, address: u32, data: &[u8]) -> bool {
        if data.is_empty() || data.len() > 16 {
            return false;
        }

        let mut words = [0u32; 4];
        for (index, byte) in data.iter().enumerate() {
            words[index / 4] |= u32::from(*byte) << ((index % 4) * 8);
        }
        unsafe { write_volatile((DP1_BASE + DP_AUX_DATA1) as *mut u32, words[0]) };
        unsafe { write_volatile((DP1_BASE + DP_AUX_DATA2) as *mut u32, words[1]) };
        unsafe { write_volatile((DP1_BASE + DP_AUX_DATA3) as *mut u32, words[2]) };
        unsafe { write_volatile((DP1_BASE + DP_AUX_DATA4) as *mut u32, words[3]) };
        unsafe { write_volatile((DP1_BASE + DP_GENERAL_INTERRUPT) as *mut u32, 1 << 16) };
        let config = ((data.len() - 1) as u32) << 24 | (address << 4) | 0x8;
        unsafe {
            write_volatile((DP1_BASE + DP_AUX_CONFIG) as *mut u32, config);
            write_volatile((DP1_BASE + DP_AUX_START) as *mut u32, 0);
            write_volatile((DP1_BASE + DP_AUX_START) as *mut u32, 1 << 31);
        }

        for _ in 0..20_000_000 {
            if unsafe { read_volatile((DP1_BASE + DP_GENERAL_INTERRUPT) as *const u32) } & (1 << 16)
                != 0
            {
                let status = (unsafe { read_volatile((DP1_BASE + DP_AUX_STATUS) as *const u32) }
                    >> 24)
                    & 0xF;
                return status == 0;
            }
            core::hint::spin_loop();
        }
        false
    }
}

fn fill_framebuffer(color: u32) {
    let mut address = SPLASH_ADDR as *mut u32;
    let end = SPLASH_ADDR + SPLASH_SIZE;
    while (address as usize) < end {
        unsafe { write_volatile(address, color) };
        address = unsafe { address.add(1) };
    }
    fence(Ordering::SeqCst);
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rust_main() -> ! {
    let mut uart = Uart::new();
    uart.init();
    uart.write_str("\nBonjour Jupiter 2 V4\n");
    uart.write_str("UART_OK\n");
    if uart.disable_inherited_watchdog() {
        uart.write_str("WDT_DISABLED\n");
    } else {
        uart.write_str("WDT_DISABLE_READBACK_FAIL\n");
    }

    // The splash is transferred separately by U-Boot. Reading both ends gives
    // a cheap DRAM visibility check before any DPU programming is attempted.
    // SAFETY: U-Boot places the validated 1600x900 BGRA8 test image here.
    let first = unsafe { read_volatile(SPLASH_ADDR as *const u32) };
    let last = unsafe { read_volatile((SPLASH_ADDR + SPLASH_SIZE - 4) as *const u32) };
    if first != 0 || last != 0 {
        uart.write_str("DRAM_OK\n");
        uart.write_str("SPLASH_RAM_READ\n");
    } else {
        uart.write_str("DRAM_READ_ZERO\n");
    }

    // These reads are the next bring-up checkpoint.  The offsets are kept
    // explicit so the UART capture can be compared directly with U-Boot's
    // `md.l` output and with the Saturn Linux register map.
    uart.write_str("MMIO_PROBE_BEGIN\n");
    uart.write_mmio("DPU1+0x000=", DPU1_BASE);
    uart.write_mmio("DPU1+0x340=", DPU1_BASE + 0x340);
    uart.write_mmio("DPU1+0x380=", DPU1_BASE + 0x380);
    uart.write_mmio("DPU1+0x3C0=", DPU1_BASE + 0x3C0);
    uart.write_mmio("DP1+0x000=", DP1_BASE);
    uart.write_mmio("DP1+0x00C=", DP1_BASE + 0x0C);

    // U-Boot has already initialized this transmitter enough to read DPCD.
    // Probe AUX before touching APMU, Saturn or the DP PLL so a later failure
    // can be attributed to one of those stages.
    uart.write_str("AUX_EARLY_BEGIN\n");
    let mut early_dpcd = [0u8; 4];
    let early_aux_ok = uart.dp_aux_read(0x0000, &mut early_dpcd);
    uart.write_str("AUX_EARLY_REV=");
    uart.write_hex_u32(u32::from(early_dpcd[0]));
    uart.write_str("\nAUX_EARLY_RATE=");
    uart.write_hex_u32(u32::from(early_dpcd[1]));
    uart.write_str("\nAUX_EARLY_LANES=");
    uart.write_hex_u32(u32::from(early_dpcd[2]));
    uart.write_str("\n");
    if early_aux_ok {
        uart.write_str("AUX_EARLY_OK\n");
    } else {
        uart.write_str("AUX_EARLY_FAIL\n");
    }

    uart.write_str("APMU_PROBE_BEGIN\n");
    uart.write_mmio("APMU+0x23C=", APMU_BASE + APMU_LCD_EDP_CTRL);
    uart.write_mmio("APMU+0x26C=", APMU_BASE + APMU_LCD_CLK_RES_CTRL3);
    uart.write_mmio("APMU+0x270=", APMU_BASE + APMU_LCD_CLK_RES_CTRL4);
    uart.write_mmio("APMU+0x274=", APMU_BASE + APMU_LCD_CLK_RES_CTRL5);
    uart.write_str("APMU_PROBE_OK\n");

    // Follow the Linux resource order.  The selectors/dividers are part of
    // the APMU clock tree, so setting only the gates is insufficient when the
    // firmware is started after a different U-Boot display path.
    uart.write_str("APMU_ENABLE_BEGIN\n");
    let ctrl3_ok = uart.or_mmio(
        "APMU+0x26C_RB=",
        APMU_BASE + APMU_LCD_CLK_RES_CTRL3,
        APMU_DSI4LN2_ESC_CLK_GATE | APMU_DSI4LN2_ESC_RESET | APMU_DSI4LN2_LCD_RESET,
    );
    let ctrl4_gate_reset_ok = uart.or_mmio(
        "APMU+0x270_RB=",
        APMU_BASE + APMU_LCD_CLK_RES_CTRL4,
        APMU_DSI4LN2_LCD_PXCLK_GATE
            | APMU_DSI4LN2_LCD_DSC_GATE
            | APMU_DSI4LN2_LCD_MCLK_GATE
            | APMU_DSI4LN2_LCD_MCLK_RESET
            | APMU_DSI4LN2_LCD_DSC_RESET,
    );
    let ctrl4_rate_ok = uart.write_field(
        APMU_BASE + APMU_LCD_CLK_RES_CTRL4,
        23,
        21,
        APMU_DSI4LN2_LCD_PXCLK_SEL,
    ) && uart.write_field(
        APMU_BASE + APMU_LCD_CLK_RES_CTRL4,
        19,
        17,
        APMU_DSI4LN2_LCD_PXCLK_DIV,
    ) && uart.write_field(
        APMU_BASE + APMU_LCD_CLK_RES_CTRL4,
        7,
        5,
        APMU_DSI4LN2_LCD_MCLK_SEL,
    ) && uart.write_field(
        APMU_BASE + APMU_LCD_CLK_RES_CTRL4,
        4,
        1,
        APMU_DSI4LN2_LCD_MCLK_DIV,
    );
    let ctrl5_rate_ok = uart.write_field(
        APMU_BASE + APMU_LCD_CLK_RES_CTRL5,
        7,
        5,
        APMU_DSI4LN2_DPU_ACLK_SEL,
    ) && uart.write_field(
        APMU_BASE + APMU_LCD_CLK_RES_CTRL5,
        4,
        2,
        APMU_DSI4LN2_DPU_ACLK_DIV,
    );
    let ctrl5_ok = uart.or_mmio(
        "APMU+0x274_RB=",
        APMU_BASE + APMU_LCD_CLK_RES_CTRL5,
        APMU_DSI4LN2_DPU_ACLK_RESET | APMU_DSI4LN2_DPU_ACLK_GATE,
    );
    let edp_ok = uart.or_mmio(
        "APMU+0x23C_RB=",
        APMU_BASE + APMU_LCD_EDP_CTRL,
        APMU_EDP1_RESET | APMU_EDP1_CLK_GATE | APMU_EDP1_PIXCLK_SEL,
    );
    uart.write_mmio("APMU+0x270_RATE_RB=", APMU_BASE + APMU_LCD_CLK_RES_CTRL4);
    uart.write_mmio("APMU+0x274_RATE_RB=", APMU_BASE + APMU_LCD_CLK_RES_CTRL5);
    if ctrl3_ok && ctrl4_gate_reset_ok && ctrl4_rate_ok && ctrl5_rate_ok && ctrl5_ok && edp_ok {
        uart.write_str("APMU_ENABLE_OK\n");
    } else {
        uart.write_str("APMU_ENABLE_READBACK_FAIL\n");
    }

    // Minimal Saturn resource initialization, matching saturn_hee_dpu_init()
    // before a mode or plane is committed. It does not start scanout.
    uart.write_str("DPU_REG_INIT_BEGIN\n");
    let dma_image_ok = uart.write_mmio_value(
        "DPU1+0x1284_RB=",
        DPU1_BASE + DPU_DMA_TOP_IMAGE_CTRL,
        0x0000_0810,
    );
    let dma_timeout_ok = uart.write_mmio_value(
        "DPU1+0x1288_RB=",
        DPU1_BASE + DPU_DMA_TOP_TIMEOUT,
        0xFFFF_FFFE,
    );
    let mmu_ctrl_ok =
        uart.write_mmio_value("DPU1+0x1200_RB=", DPU1_BASE + DPU_MMU_TOP_CTRL, 0xFFFF_0000);
    let mmu_dmac0_ok = uart.write_mmio_value(
        "DPU1+0x1208_RB=",
        DPU1_BASE + DPU_MMU_TOP_DMAC0_OUTS,
        0x0000_0008,
    );
    let mmu_dmac1_ok = uart.write_mmio_value(
        "DPU1+0x120C_RB=",
        DPU1_BASE + DPU_MMU_TOP_DMAC1_OUTS,
        0x0000_0008,
    );
    let composer_ok =
        uart.write_mmio_value("DPU1+0x30000_RB=", DPU1_BASE + DPU1_COMPOSER1, 0x0000_0001);
    let composer_bg_ok = if CMP_BACKGROUND_TEST {
        uart.write_mmio_value("CMP1+0x08_BG_R_RB=", DPU1_BASE + CMP_BG_R, 0x3FF)
            && uart.write_mmio_value("CMP1+0x0C_BG_G_RB=", DPU1_BASE + CMP_BG_G, 0)
            && uart.write_mmio_value("CMP1+0x10_BG_B_RB=", DPU1_BASE + CMP_BG_B, 0)
    } else {
        true
    };
    if dma_image_ok
        && dma_timeout_ok
        && mmu_ctrl_ok
        && mmu_dmac0_ok
        && mmu_dmac1_ok
        && composer_ok
        && composer_bg_ok
    {
        uart.write_str("DPU_REG_INIT_OK\n");
    } else {
        uart.write_str("DPU_REG_INIT_READBACK_FAIL\n");
    }

    // Stage the 1600x900 timing used by the DP sink.  This mirrors the
    // Linux saturn_init_tmg() layout: porch and sync widths are packed into
    // adjacent 32-bit registers.  The timing enable bit stays clear here, so
    // this checkpoint cannot start scanout on an unconfigured plane.
    uart.write_str("TIMING_STAGE_BEGIN\n");
    let timing_ctrl_ok = uart.write_mmio_value(
        "TMG0+0x00_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_CTRL,
        TMG_CTRL_HSP_VSP,
    );
    let timing_user_ok = uart.write_mmio_value(
        "TMG0+0x08_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_USER,
        TMG_OUTFMT_RGB888,
    );
    let timing_hfp_hbp_ok = uart.write_mmio_value(
        "TMG0+0x0C_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_HFP_HBP,
        TMG_HFP_REGISTER_VALUE,
    );
    let timing_hsync_hbp_ok = uart.write_mmio_value(
        "TMG0+0x10_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_HSYNC_HBP,
        DPU1_MODE_HSYNC | (DPU1_MODE_HBP << 16),
    );
    let timing_vfp_vsync_ok = uart.write_mmio_value(
        "TMG0+0x14_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_VFP_VSYNC,
        DPU1_MODE_VFP | (DPU1_MODE_VSYNC << 16),
    );
    let timing_vbp_hactive_ok = uart.write_mmio_value(
        "TMG0+0x18_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_VBP_HACTIVE,
        DPU1_MODE_VBP | (DPU1_MODE_WIDTH << 16),
    );
    let timing_vactive_ok = uart.write_mmio_value(
        "TMG0+0x1C_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_VACTIVE,
        DPU1_MODE_HEIGHT,
    );
    let timing_eof_delay_ok = uart.write_mmio_value(
        "TMG0+0x2C_EOF_DELAY_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_EOF_LINE_DELAY,
        TMG_EOF_LINE_DELAY_VALUE,
    );
    let timing_sof_pre_ok = uart.write_mmio_value(
        "TMG0+0x38_SOF_PRE_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_SOF_PRE_LINE,
        TMG_SOF_PRE_LINE_VALUE,
    );
    let timing_align_ok =
        uart.write_mmio_value("TMG0+0x3C_RB=", DPU1_BASE + DPU1_TMG0 + TMG_VSYNC_ALIGN, 1);
    let timing_background_ok = uart.write_mmio_value(
        "TMG0+0x24_BG_RG_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_BACKGROUND_RG,
        if TMG_BACKGROUND_TEST { 0x0000_0FFF } else { 0 },
    ) && uart.write_mmio_value(
        "TMG0+0x28_BG_B_RB=",
        DPU1_BASE + DPU1_TMG0 + TMG_BACKGROUND_B,
        0,
    );
    if timing_ctrl_ok
        && timing_user_ok
        && timing_hfp_hbp_ok
        && timing_hsync_hbp_ok
        && timing_vfp_vsync_ok
        && timing_vbp_hactive_ok
        && timing_vactive_ok
        && timing_eof_delay_ok
        && timing_sof_pre_ok
        && timing_align_ok
        && timing_background_ok
    {
        uart.write_str("TIMING_STAGE_OK\n");
    } else {
        uart.write_str("TIMING_STAGE_READBACK_FAIL\n");
    }

    // Stage RDMA0 for the linear BGRA8 framebuffer.  The physical address is
    // written as two words because Saturn accepts a 36-bit address.  RDMA0
    // and composer layer 0 stay disabled here; DMMU and cmdlist ownership are
    // required before the first memory fetch.
    uart.write_str("RDMA_STAGE_BEGIN\n");
    let rdma_ctrl_ok = uart.write_mmio_value(
        "RDMA0+0x00_RB=",
        DPU1_BASE + DPU1_RDMA0 + RDMA_CTRL,
        1 << 13,
    );
    let rdma_addr_low_ok = uart.write_mmio_value(
        "RDMA0+0x24_RB=",
        DPU1_BASE + DPU1_RDMA0 + RDMA_BASE_ADDR0_LOW,
        (SPLASH_ADDR as u32) & 0xFFFF_FFFF,
    );
    let rdma_addr_high_ok = uart.write_mmio_value(
        "RDMA0+0x28_RB=",
        DPU1_BASE + DPU1_RDMA0 + RDMA_BASE_ADDR0_HIGH,
        (SPLASH_ADDR >> 32) as u32,
    );
    let rdma_stride_ok = uart.write_mmio_value(
        "RDMA0+0x3C_RB=",
        DPU1_BASE + DPU1_RDMA0 + RDMA_STRIDE0,
        DPU1_MODE_WIDTH * 4,
    );
    let rdma_size_ok = uart.write_mmio_value(
        "RDMA0+0x40_RB=",
        DPU1_BASE + DPU1_RDMA0 + RDMA_IMAGE_SIZE,
        DPU1_MODE_WIDTH | (DPU1_MODE_HEIGHT << 16),
    );
    let rdma_bbox_start_ok = uart.write_mmio_value(
        "RDMA0+0x44_RB=",
        DPU1_BASE + DPU1_RDMA0 + RDMA_BBOX_START,
        0,
    );
    let rdma_bbox_end_ok = uart.write_mmio_value(
        "RDMA0+0x48_RB=",
        DPU1_BASE + DPU1_RDMA0 + RDMA_BBOX_END,
        (DPU1_MODE_WIDTH - 1) | ((DPU1_MODE_HEIGHT - 1) << 16),
    );
    let rdma_format_ok =
        uart.write_mmio_value("RDMA0+0x74_RB=", DPU1_BASE + DPU1_RDMA0 + RDMA_FORMAT, 7);
    let cmp_size_ok = uart.write_mmio_value(
        "CMP1+0x04_RB=",
        DPU1_BASE + CMP_DST_SIZE,
        DPU1_MODE_WIDTH | (DPU1_MODE_HEIGHT << 16),
    );
    let cmp_alpha_ok = uart.write_mmio_value("CMP1+0x14_RB=", DPU1_BASE + CMP_BG_ALPHA, 0xFF);
    let cmp_area_x_ok = uart.write_mmio_value(
        "CMP1+0x20_RB=",
        DPU1_BASE + CMP_LAYER0_AREA_X,
        DPU1_MODE_WIDTH - 1 << 16,
    );
    let cmp_area_y_ok = uart.write_mmio_value(
        "CMP1+0x24_RB=",
        DPU1_BASE + CMP_LAYER0_AREA_Y,
        DPU1_MODE_HEIGHT - 1 << 16,
    );
    let cmp_layer_disabled_ok =
        uart.write_mmio_value("CMP1+0x38_RB=", DPU1_BASE + CMP_LAYER0_CTRL, 0);
    if rdma_ctrl_ok
        && rdma_addr_low_ok
        && rdma_addr_high_ok
        && rdma_stride_ok
        && rdma_size_ok
        && rdma_bbox_start_ok
        && rdma_bbox_end_ok
        && rdma_format_ok
        && cmp_size_ok
        && cmp_alpha_ok
        && cmp_area_x_ok
        && cmp_area_y_ok
        && cmp_layer_disabled_ok
    {
        uart.write_str("RDMA_STAGE_OK\n");
    } else {
        uart.write_str("RDMA_STAGE_READBACK_FAIL\n");
    }

    // Build the minimal Linux-compatible TBU0 page table.  The BGRA splash
    // was transferred into contiguous DRAM, so each entry is simply the
    // physical page number; the extra 60 entries match HW_ALIGN_TTB_NUM.
    uart.write_str("DMMU_STAGE_BEGIN\n");
    let first_page = (SPLASH_ADDR / DMMU_PAGE_SIZE) as u32;
    for index in 0..DMMU_PAGE_COUNT {
        let page = first_page + index as u32;
        unsafe {
            write_volatile(
                (DMMU_TABLE_ADDR + index * core::mem::size_of::<u32>()) as *mut u32,
                page & 0x03FF_FFFF,
            );
        }
    }
    let last_page = first_page + (DMMU_PAGE_COUNT - 1) as u32;
    for index in DMMU_PAGE_COUNT..DMMU_TABLE_ENTRIES {
        unsafe {
            write_volatile(
                (DMMU_TABLE_ADDR + index * core::mem::size_of::<u32>()) as *mut u32,
                last_page & 0x03FF_FFFF,
            );
        }
    }
    unsafe { core::arch::asm!("fence rw, rw") };
    let table_first = unsafe { read_volatile(DMMU_TABLE_ADDR as *const u32) };
    let table_last =
        unsafe { read_volatile((DMMU_TABLE_ADDR + (DMMU_TABLE_ENTRIES - 1) * 4) as *const u32) };
    uart.write_str("DMMU_TABLE_FIRST=");
    uart.write_hex_u32(table_first);
    uart.write_str("\nDMMU_TABLE_LAST=");
    uart.write_hex_u32(table_last);
    uart.write_str("\n");

    let tbu_ctrl_ok = uart.write_mmio_value(
        "TBU0+0x00_RB=",
        DPU1_BASE + DMMU_TBU0 + DMMU_TBU_CTRL,
        DMMU_TBU_CTRL_VALUE,
    );
    let tbu_base_low_ok = uart.write_mmio_value(
        "TBU0+0x04_RB=",
        DPU1_BASE + DMMU_TBU0 + DMMU_TBU_BASE0_LOW,
        (DMMU_TABLE_ADDR as u32) & 0xFFFF_FFFF,
    );
    let tbu_base_high_ok = uart.write_mmio_value(
        "TBU0+0x08_RB=",
        DPU1_BASE + DMMU_TBU0 + DMMU_TBU_BASE0_HIGH,
        (DMMU_TABLE_ADDR >> 32) as u32,
    );
    let tbu_va_ok = uart.write_mmio_value(
        "TBU0+0x1C_RB=",
        DPU1_BASE + DMMU_TBU0 + DMMU_TBU_VA0,
        DMMU_BASE_VA >> 12,
    );
    let tbu_size_ok = uart.write_mmio_value(
        "TBU0+0x28_RB=",
        DPU1_BASE + DMMU_TBU0 + DMMU_TBU_SIZE0,
        DMMU_PAGE_COUNT as u32,
    );
    let tbu_vsync_ok = uart.write_mmio_value(
        "TBU0+0x34_RB=",
        DPU1_BASE + DMMU_TBU0 + DMMU_TBU_VSYNC,
        1 << 1,
    );
    // With DMMU translation enabled RDMA0 addresses the TBU virtual window.
    // Keep the channel and composer layer disabled until cmdlist validation.
    let rdma_va_low_ok = if RDMA_PHYS_DIRECT_TEST {
        uart.write_mmio_value(
            "RDMA0+0x24_PHYS_RB=",
            DPU1_BASE + DPU1_RDMA0 + RDMA_BASE_ADDR0_LOW,
            (SPLASH_ADDR as u32) & 0xFFFF_FFFF,
        )
    } else {
        uart.write_mmio_value(
            "RDMA0+0x24_VA_RB=",
            DPU1_BASE + DPU1_RDMA0 + RDMA_BASE_ADDR0_LOW,
            DMMU_BASE_VA,
        )
    };
    let rdma_va_high_ok = if RDMA_PHYS_DIRECT_TEST {
        uart.write_mmio_value(
            "RDMA0+0x28_PHYS_RB=",
            DPU1_BASE + DPU1_RDMA0 + RDMA_BASE_ADDR0_HIGH,
            (SPLASH_ADDR >> 32) as u32,
        )
    } else {
        uart.write_mmio_value(
            "RDMA0+0x28_VA_RB=",
            DPU1_BASE + DPU1_RDMA0 + RDMA_BASE_ADDR0_HIGH,
            0,
        )
    };
    let dmmu_mode_ok = if RDMA_PHYS_DIRECT_TEST {
        uart.write_str("DMMU_MODE=PHYSICAL_DIRECT\n");
        uart.write_mmio_value(
            "TBU0+0x00_DIRECT_RB=",
            DPU1_BASE + DMMU_TBU0 + DMMU_TBU_CTRL,
            DMMU_TBU_CTRL_DIRECT_VALUE,
        )
    } else {
        uart.write_str("DMMU_MODE=VIRTUAL\n");
        true
    };
    if table_first == first_page
        && table_last == last_page
        && (RDMA_PHYS_DIRECT_TEST && dmmu_mode_ok || !RDMA_PHYS_DIRECT_TEST && tbu_ctrl_ok)
        && tbu_base_low_ok
        && tbu_base_high_ok
        && tbu_va_ok
        && tbu_size_ok
        && tbu_vsync_ok
        && rdma_va_low_ok
        && rdma_va_high_ok
    {
        uart.write_str("DMMU_STAGE_OK\n");
    } else {
        uart.write_str("DMMU_STAGE_READBACK_FAIL\n");
    }

    // Build one Linux-compatible command list in DRAM.  It carries the RDMA,
    // TBU0 and composer shadow values plus the final configuration-ready row.
    // The channel remains disabled, so the list is only staged and cannot
    // trigger a fetch yet.
    uart.write_str("CMDLIST_STAGE_BEGIN\n");
    let mut row_index = 0usize;
    let header_words = (CMDLIST_ADDR as *mut u64, (CMDLIST_ADDR + 8) as *mut u64);
    let mut rows = [[0u32; 4]; 21];
    // saturn_hee_plane_update_hw_channel() always emits the RDMA path header
    // before the framebuffer registers.  In particular, layer_cmpsr_id=1
    // attaches RDMA0 to CMP1 (DPU1); without it the RDMA can fetch pixels
    // while the composer silently ignores that channel.  The second row
    // clears the offline/two-layer flags for the online single-layer path.
    rows[0] = [
        (DPU1_RDMA0 >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
        1 << 13, // layer_cmpsr_id=1, layer_mode=0
        0,
        0,
    ];
    rows[1] = [
        ((DPU1_RDMA0 + 0x04) >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
        0, // is_two_layers=0, is_offline=0
        0,
        0,
    ];
    rows[2] = [
        ((DPU1_RDMA0 + RDMA_BASE_ADDR0_LOW) >> 2) as u32 | (CMDLIST_STROBE_TWO << 19),
        if RDMA_PHYS_DIRECT_TEST {
            (SPLASH_ADDR as u32) & 0xFFFF_FFFF
        } else {
            DMMU_BASE_VA
        },
        if RDMA_PHYS_DIRECT_TEST {
            (SPLASH_ADDR >> 32) as u32
        } else {
            0
        },
        0,
    ];
    rows[3] = [
        ((DPU1_RDMA0 + RDMA_STRIDE0) >> 2) as u32 | (CMDLIST_STROBE_ALL << 19),
        DPU1_MODE_WIDTH * 4,
        DPU1_MODE_WIDTH | (DPU1_MODE_HEIGHT << 16),
        0,
    ];
    rows[4] = [
        ((DPU1_RDMA0 + RDMA_BBOX_END) >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
        (DPU1_MODE_WIDTH - 1) | ((DPU1_MODE_HEIGHT - 1) << 16),
        0,
        0,
    ];
    rows[5] = [
        ((DPU1_RDMA0 + RDMA_FORMAT) >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
        7,
        0,
        0,
    ];
    rows[6] = [
        (DMMU_TBU0 >> 2) as u32 | (CMDLIST_STROBE_ALL << 19),
        if RDMA_PHYS_DIRECT_TEST {
            DMMU_TBU_CTRL_DIRECT_VALUE
        } else {
            DMMU_TBU_CTRL_VALUE
        },
        if RDMA_PHYS_DIRECT_TEST {
            0
        } else {
            (DMMU_TABLE_ADDR as u32) & 0xFFFF_FFFF
        },
        if RDMA_PHYS_DIRECT_TEST {
            0
        } else {
            (DMMU_TABLE_ADDR >> 32) as u32
        },
    ];
    rows[7] = [
        ((DMMU_TBU0 + DMMU_TBU_VA0) >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
        if RDMA_PHYS_DIRECT_TEST {
            0
        } else {
            DMMU_BASE_VA >> 12
        },
        0,
        0,
    ];
    rows[8] = [
        ((DMMU_TBU0 + DMMU_TBU_SIZE0) >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
        if RDMA_PHYS_DIRECT_TEST {
            0
        } else {
            DMMU_PAGE_COUNT as u32
        },
        0,
        0,
    ];
    rows[9] = [
        ((DMMU_TBU0 + DMMU_TBU_VSYNC) >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
        if RDMA_PHYS_DIRECT_TEST { 0 } else { 1 << 1 },
        0,
        0,
    ];
    rows[10] = [
        (CMP_DST_SIZE >> 2) as u32 | (CMDLIST_STROBE_ALL << 19),
        DPU1_MODE_WIDTH | (DPU1_MODE_HEIGHT << 16),
        0,
        0,
    ];
    rows[11] = [
        ((CMP_BG_ALPHA - 4) >> 2) as u32 | (CMDLIST_STROBE_TWO << 19),
        0,
        0xFF,
        0,
    ];
    rows[12] = [
        (CMP_LAYER0_AREA_X >> 2) as u32 | (CMDLIST_STROBE_TWO << 19),
        (DPU1_MODE_WIDTH - 1) << 16,
        (DPU1_MODE_HEIGHT - 1) << 16,
        0,
    ];
    // saturn_hee_conf_dpuctrl() always submits the online postpipe in the
    // composer group, even when post scaling is disabled. The constructor
    // writes split_en=0 and the active composer dimensions here.
    rows[16] = [
        (POSTPIPE_CTRL >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
        0,
        0,
        0,
    ];
    rows[17] = [
        // m_inwidth and m_inheight share value32[13] at +0x34. The Linux
        // cmdlist writer marks only that one register slot, not a 3-register
        // row; using 0xFFF makes this POSTPIPE row invalid on the HEE parser.
        (POSTPIPE_SIZE >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
        DPU1_MODE_WIDTH | (DPU1_MODE_HEIGHT << 16),
        0,
        0,
    ];
    // Linux keeps the plane registers and the composer registers in separate
    // command-list groups. The RDMA/TBU list is channel 0; CMP1, POSTPIPE and
    // the composer CFG_RDY row are the CRTC list on channel 7. Keeping the
    // composer rows in the plane list makes the HEE parser consume them under
    // the wrong reload event, which leaves the output black even when every
    // register readback looks plausible.
    let composer_dst_row = rows[10];
    let composer_bg_row = rows[11];
    let composer_area_row = rows[12];
    let row_count;
    if SATURN_SOLID_TEST {
        // Linux submits RDMA/TBU rows on the plane channel and the CMP1 /
        // POSTPIPE rows on the CRTC composer channel. A solid layer has no
        // plane channel at all, so the composer list must contain only the
        // CMP1 group. Keeping the unrelated RDMA/TBU rows here made the
        // previous diagnostic differ from the driver's list topology.
        rows[0] = rows[10];
        rows[1] = rows[11];
        rows[2] = rows[12];
        rows[3] = [
            (CMP_LAYER0_SOLID_R >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            0,
            0,
            0,
        ];
        rows[4] = [
            (CMP_LAYER0_SOLID_G >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            0,
            0,
            0,
        ];
        rows[5] = [
            (CMP_LAYER0_SOLID_B >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            SOLID_BLUE,
            0,
            0,
        ];
        rows[6] = [
            (CMP_LAYER0_SOLID_A >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            CMP_LAYER0_SOLID_A_VALUE,
            0,
            0,
        ];
        rows[7] = [
            (CMP_LAYER0_CTRL >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            CMP_LAYER0_TEST_CTRL,
            0,
            0,
        ];
        rows[8] = [
            (POSTPIPE_CTRL >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            0,
            0,
            0,
        ];
        rows[9] = [
            (POSTPIPE_SIZE >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            DPU1_MODE_WIDTH | (DPU1_MODE_HEIGHT << 16),
            0,
            0,
        ];
        rows[10] = [
            ((CMDLIST_CFG_READY_ADDR - DPU1_BASE) >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            CMDLIST_COMPOSER_CFG_BITS,
            0,
            0,
        ];
        row_count = 11;
    } else {
        // layer00_en=1, alpha selection=pixel alpha, layer alpha=0xff,
        // solid color disabled, and RDMA0 selected.
        rows[13] = [
            (CMP_LAYER0_CTRL >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            if CMP_BACKGROUND_TEST { 0 } else { 0x0000_FF03 },
            0,
            0,
        ];
        // saturn_hee_cmdlist_fill_conf_row() adds this row to each plane
        // list before its final CFG_RDY row. It is required even when the
        // plane is not vertically sliced; the previous single-channel list
        // omitted it entirely.
        rows[14] = [
            ((DPU1_BASE + CMDLIST_START_CMPS_Y - DPU1_BASE) >> 2) as u32
                | (CMDLIST_STROBE_ONE << 19),
            0,
            0,
            0,
        ];
        rows[15] = [
            ((CMDLIST_CFG_READY_ADDR - DPU1_BASE) >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            CMDLIST_PLANE_CFG_BITS,
            0,
            0,
        ];
        // The plane list contains only RDMA/TBU rows, START_CMPS_Y and its
        // own CFG_RDY. Move the two rows that were staged after the composer
        // rows into the compact plane list before it is written below.
        rows[10] = rows[14];
        rows[11] = rows[15];
        row_count = 12;
    }
    // The HEE header is two u64 words.  The first word contains only the
    // next-list address, node length/type and the low five wait-event bits;
    // the remaining wait-event bits live in the second word.  Putting
    // CMDLIST_WAIT_EVENT_HIGH into header0 corrupts next_list_addr (the
    // previous trace exposed this as a spurious low value of 0x2).
    let header0 = ((row_count as u64 + 1) << 40) | (1u64 << 56) | CMDLIST_WAIT_EVENT_LOW;
    unsafe {
        write_volatile(header_words.0, header0);
        write_volatile(header_words.1, CMDLIST_WAIT_EVENT_HIGH);
        while row_index < row_count {
            let row_addr = (CMDLIST_ADDR + 16 + row_index * 16) as *mut u32;
            write_volatile(row_addr, rows[row_index][0]);
            write_volatile(row_addr.add(1), rows[row_index][1]);
            write_volatile(row_addr.add(2), rows[row_index][2]);
            write_volatile(
                row_addr,
                rows[row_index][0]
                    | if row_index + 1 == row_count {
                        1 << 31
                    } else {
                        0
                    },
            );
            write_volatile(row_addr.add(3), rows[row_index][3]);
            row_index += 1;
        }
        core::arch::asm!("fence rw, rw");
    }

    // The CRTC list carries only the online POSTPIPE and composer CFG_RDY
    // rows. Linux submits it on channel 7 after the plane list on channel 0;
    // keeping it separate is essential for the HEE channel-local reload
    // events.
    let mut crtc_rows = [[0u32; 4]; 16];
    let crtc_row_count;
    if SATURN_SOLID_TEST {
        // In solid mode rows[0..11] already contain the CMP1 layer, POSTPIPE
        // and composer CFG_RDY sequence. They belong on channel 7 as well.
        let mut index = 0usize;
        while index < row_count {
            crtc_rows[index] = rows[index];
            index += 1;
        }
        crtc_row_count = row_count;
    } else {
        // Recreate the composer rows emitted by
        // saturn_hee_plane_update_hw_channel(): destination, background,
        // layer area, layer blend state, then the online POSTPIPE rows.
        crtc_rows[0] = composer_dst_row;
        crtc_rows[1] = composer_bg_row;
        crtc_rows[2] = composer_area_row;
        crtc_rows[3] = [
            (CMP_LAYER0_SOLID_A >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            0,
            0,
            0,
        ];
        crtc_rows[4] = rows[13];
        crtc_rows[5] = [
            (POSTPIPE_CTRL >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            0,
            0,
            0,
        ];
        crtc_rows[6] = [
            (POSTPIPE_SIZE >> 2) as u32 | (CMDLIST_STROBE_ONE << 19),
            DPU1_MODE_WIDTH | (DPU1_MODE_HEIGHT << 16),
            0,
            0,
        ];
        crtc_rows[7] = [
            ((CMDLIST_CFG_READY_ADDR - DPU1_BASE) >> 2) as u32
                | (CMDLIST_STROBE_ONE << 19)
                | (1 << 31),
            CMDLIST_COMPOSER_CFG_BITS,
            0,
            0,
        ];
        crtc_row_count = 8;
    }
    let crtc_header0 = ((crtc_row_count as u64 + 1) << 40) | (1u64 << 56);
    unsafe {
        write_volatile(CMDLIST_CRTC_ADDR as *mut u64, crtc_header0);
        write_volatile((CMDLIST_CRTC_ADDR + 8) as *mut u64, CMDLIST_WAIT_EVENT_HIGH);
        for index in 0..crtc_row_count {
            let row = crtc_rows[index];
            let row_addr = (CMDLIST_CRTC_ADDR + 16 + index * 16) as *mut u32;
            write_volatile(row_addr, row[0]);
            write_volatile(row_addr.add(1), row[1]);
            write_volatile(row_addr.add(2), row[2]);
            write_volatile(row_addr.add(3), row[3]);
        }
        core::arch::asm!("fence rw, rw");
    }
    let cmd_header = unsafe { read_volatile(CMDLIST_ADDR as *const u64) };
    let cmd_last_row =
        unsafe { read_volatile((CMDLIST_ADDR + 16 + (row_count - 1) * 16) as *const u32) };
    let crtc_header = unsafe { read_volatile(CMDLIST_CRTC_ADDR as *const u64) };
    let crtc_last_row = unsafe {
        read_volatile((CMDLIST_CRTC_ADDR + 16 + (crtc_row_count - 1) * 16) as *const u32)
    };
    uart.write_str("CMDLIST_HEADER=");
    uart.write_hex_u64(cmd_header);
    uart.write_str("\nCMDLIST_LAST=");
    uart.write_hex_u32(cmd_last_row);
    uart.write_str("\n");
    let cmdlist_plane_start_low_ok = uart.write_mmio_value(
        "CMDLIST0+0x00_RB=",
        DPU1_BASE + CMDLIST_PLANE_START_ADDRL,
        ((CMDLIST_ADDR >> 4) as u32) & 0x0FFF_FFFF,
    );
    let cmdlist_plane_start_high_ok = uart.write_mmio_value(
        "CMDLIST0+0x40_RB=",
        DPU1_BASE + CMDLIST_PLANE_START_ADDRH,
        (CMDLIST_ADDR >> 32) as u32,
    );
    let cmdlist_plane_y_ok =
        uart.write_mmio_value("CMDLIST0+0xC0_RB=", DPU1_BASE + CMDLIST_PLANE_Y_FIRST, 0);
    let cmdlist_composer_start_low_ok = uart.write_mmio_value(
        "CMDLIST7+0x1C_RB=",
        DPU1_BASE + CMDLIST_COMPOSER_START_ADDRL,
        ((CMDLIST_CRTC_ADDR >> 4) as u32) & 0x0FFF_FFFF,
    );
    let cmdlist_composer_start_high_ok = uart.write_mmio_value(
        "CMDLIST7+0x5C_RB=",
        DPU1_BASE + CMDLIST_COMPOSER_START_ADDRH,
        (CMDLIST_CRTC_ADDR >> 32) as u32,
    );
    let cmdlist_composer_y_ok =
        uart.write_mmio_value("CMDLIST7+0xDC_RB=", DPU1_BASE + CMDLIST_COMPOSER_Y_FIRST, 0);
    let cmdlist_stage_ok = cmd_header == header0
        && cmd_last_row & (1 << 31) != 0
        && crtc_header == crtc_header0
        && crtc_last_row & (1 << 31) != 0
        && cmdlist_plane_start_low_ok
        && cmdlist_plane_start_high_ok
        && cmdlist_plane_y_ok
        && cmdlist_composer_start_low_ok
        && cmdlist_composer_start_high_ok
        && cmdlist_composer_y_ok;
    if cmdlist_stage_ok {
        uart.write_str("CMDLIST_STAGE_OK\n");
    } else {
        uart.write_str("CMDLIST_STAGE_READBACK_FAIL\n");
    }

    // Commit the staged normal path without starting the timing generator.
    // This lets the cmdlist fetch and the scene routing be checked while DP
    // remains quiescent.
    if cmdlist_stage_ok {
        uart.write_str("CMDLIST_ENABLE_BEGIN\n");
        // A solid composer layer has no RDMA channel, but its composer
        // cmdlist still has to be enabled so the staged composer rows are
        // consumed. The Linux driver enables both composer bits at reg 10.
        let cmdlist_channel_ok = uart.write_mmio_value(
            "DPU1+0x3C0_RB=",
            DPU1_BASE + DPU1_CTRL_TOP,
            if SATURN_SOLID_TEST || CMP_BACKGROUND_TEST {
                0
            } else {
                1
            },
        );
        let cmdlist_composer_ok = uart.write_mmio_value(
            "DPU1+0x3E8_RB=",
            DPU1_BASE + DPU1_CMDLIST_CMPS,
            if SATURN_SOLID_TEST && SATURN_SOLID_DIRECT_TEST {
                0
            } else {
                0x3
            },
        );
        // Match saturn_hee_conf_dpuctrl(): only the active RDMA mask,
        // nml_outctl_en and nml_frm_timing_en belong in this register.
        // The previous literal also enabled writeback and DSC bits, which
        // left the output black even though the timing generator was active.
        let scene_route_value = (if SATURN_SOLID_TEST || CMP_BACKGROUND_TEST {
            0
        } else {
            1
        }) | (1 << 18)
            | (2 << 23);
        let scene_route_ok =
            uart.write_mmio_value("DPU1+0x340_RB=", DPU1_BASE + DPU1_SCENE1, scene_route_value);
        let scene_update_ok =
            uart.write_mmio_value("DPU1+0x344_RB=", DPU1_BASE + DPU1_SCENE1_CMD_UPDATE, 1);
        let scene_video_ok =
            uart.write_mmio_value("DPU1+0x34C_RB=", DPU1_BASE + DPU1_SCENE1_VIDEO_MODE, 1);
        if cmdlist_channel_ok
            && cmdlist_composer_ok
            && scene_route_ok
            && scene_update_ok
            && scene_video_ok
        {
            uart.write_str("CMDLIST_ENABLE_OK\n");
        } else {
            uart.write_str("CMDLIST_ENABLE_READBACK_FAIL\n");
        }
    }

    // Arm the DPU1 normal path.  The command-list channel and scene routing
    // above are not sufficient on Saturn: Linux also enables the RDMA path,
    // output control and frame-timing state machine in scene control 2.
    uart.write_str("DPU_SCENE_STAGE_BEGIN\n");
    let scene_rch_ok = uart.write_field(
        DPU1_BASE + DPU1_SCENE_CTL,
        9,
        0,
        if SATURN_SOLID_TEST { 0 } else { 1 },
    );
    let scene_outctl_ok = uart.write_field(DPU1_BASE + DPU1_SCENE_CTL, 18, 18, 1);
    let scene_frm_timing_ok = uart.write_field(DPU1_BASE + DPU1_SCENE_CTL, 25, 23, 2);
    let scene_video_ok = uart.write_field(DPU1_BASE + DPU_SCENE_CTL_VIDEO_MODE, 0, 0, 1);
    // dpu_scene_ctl.h: video_mod bit 0, timing_inter0 bits 1:6 and
    // timing_inter1 bits 7:12. The former code shifted both fields by one
    // bit, yielding 0x821 instead of the Linux value 0x411.
    let scene_inter0_ok = uart.write_field(DPU1_BASE + DPU_SCENE_CTL_VIDEO_MODE, 6, 1, 8);
    let scene_inter1_ok = uart.write_field(DPU1_BASE + DPU_SCENE_CTL_VIDEO_MODE, 12, 7, 8);
    uart.write_mmio("DPU1+0x340_RB=", DPU1_BASE + DPU1_SCENE_CTL);
    uart.write_mmio("DPU1+0x34C_RB=", DPU1_BASE + DPU_SCENE_CTL_VIDEO_MODE);
    if scene_rch_ok
        && scene_outctl_ok
        && scene_frm_timing_ok
        && scene_video_ok
        && scene_inter0_ok
        && scene_inter1_ok
    {
        uart.write_str("DPU_SCENE_STAGE_OK\n");
    } else {
        uart.write_str("DPU_SCENE_STAGE_READBACK_FAIL\n");
    }

    // Prime layer00 directly as well as through the cmdlist. In the solid
    // test this makes the first visible color independent of a DDR fetch:
    // RGB are 10-bit composer values and alpha is 8-bit.
    let cmp_solid_ok = if SATURN_SOLID_TEST {
        uart.write_str("SATURN_SOLID_TEST_BEGIN\n");
        uart.write_str(if SATURN_SOLID_DIRECT_TEST {
            "SATURN_MODE=SOLID_DIRECT\n"
        } else {
            "SATURN_MODE=SOLID_CMDLIST\n"
        });
        uart.write_str("SATURN_RDMA_SOURCE=DISABLED\n");
        uart.write_str("SATURN_SOLID_RGB10=0,0,1023\n");
        uart.write_mmio("SATURN_SOLID_A_CFG=", DPU1_BASE + CMP_LAYER0_SOLID_A);
        let r_ok =
            uart.write_mmio_value("CMP1+0x28_SOLID_R_RB=", DPU1_BASE + CMP_LAYER0_SOLID_R, 0);
        let g_ok =
            uart.write_mmio_value("CMP1+0x2C_SOLID_G_RB=", DPU1_BASE + CMP_LAYER0_SOLID_G, 0);
        let b_ok = uart.write_mmio_value(
            "CMP1+0x30_SOLID_B_RB=",
            DPU1_BASE + CMP_LAYER0_SOLID_B,
            SOLID_BLUE,
        );
        let a_ok = uart.write_mmio_value(
            "CMP1+0x34_SOLID_A_RB=",
            DPU1_BASE + CMP_LAYER0_SOLID_A,
            CMP_LAYER0_SOLID_A_VALUE,
        );
        r_ok && g_ok && b_ok && a_ok
    } else {
        true
    };

    // The Linux plane commit enables composer layer 0 through its cmdlist.
    // Solid mode sets bit 3 and keeps the layer alpha at 0xff; framebuffer
    // mode selects RDMA0 and uses the historical 0x0000FF03 value.
    let cmp_layer_active_ok = uart.write_mmio_value(
        "CMP1+0x38_ACTIVE_RB=",
        DPU1_BASE + CMP_LAYER0_CTRL,
        if SATURN_SOLID_TEST {
            CMP_LAYER0_TEST_CTRL
        } else if CMP_BACKGROUND_TEST {
            0
        } else {
            0x0000_FF03
        },
    );
    if cmp_solid_ok && cmp_layer_active_ok {
        uart.write_str("DPU_LAYER_STAGE_OK\n");
    } else {
        uart.write_str("DPU_LAYER_STAGE_READBACK_FAIL\n");
    }

    uart.write_str("AUX_AFTER_DPU_BEGIN\n");
    let mut after_dpu_dpcd = [0u8; 4];
    let after_dpu_aux_ok = uart.dp_aux_read(0x0000, &mut after_dpu_dpcd);
    uart.write_mmio("AUX_AFTER_DPU_INT=", DP1_BASE + DP_GENERAL_INTERRUPT);
    uart.write_mmio("AUX_AFTER_DPU_STATUS=", DP1_BASE + DP_AUX_STATUS);
    if after_dpu_aux_ok {
        uart.write_str("AUX_AFTER_DPU_OK\n");
    } else {
        uart.write_str("AUX_AFTER_DPU_FAIL\n");
    }

    // Initialize the Inno DP/eDP transmitter with the K3 Linux values. The
    // stream and timing generator are still off; this checkpoint only proves
    // the reset, 24 MHz reference PLLs and two-lane HBR PHY.
    if cmdlist_stage_ok {
        uart.write_str("DP_PLL_STAGE_BEGIN\n");
        let mut dp_ok = true;
        // DP1 is the source-side DisplayPort engine (dp-id = 1 in the K3
        // device tree). The board exposes this path through USB-C and the
        // user's adapter converts DP Alt Mode to HDMI. Keep ENABLE_EDP clear
        // as U-Boot/Linux do for this external path.
        dp_ok &= uart.write_field(DP1_BASE + DP_CORE_CONTROL, 4, 4, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_CORE_CONTROL, 31, 31, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_CORE_CONTROL, 30, 30, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_CORE_CONTROL, 29, 29, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_CORE_CONTROL, 28, 28, 0);

        dp_ok &= uart.write_field(DP1_BASE + DP_PHYIF_CTRL, 20, 17, 0);
        // DP1 is fed by the DPU1 pixel clock. Linux selects this source in
        // APMU+0x23C bit 18 and uses the external-clock input of the DP
        // stream block (CLK_INV_MUX=0). Keep both selections explicit after
        // the DP reset so a stale U-Boot setting cannot hide this dependency.
        dp_ok &= uart.write_field(APMU_BASE + APMU_LCD_EDP_CTRL, 18, 18, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_CLK_INV_MUX, 0, 0, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL0, 0, 0, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL0, 0, 0, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_SOFT_RESET, 31, 31, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_SOFT_RESET, 30, 30, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_SOFT_RESET, 29, 29, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_SOFT_RESET, 27, 27, 1);
        dp_ok &= uart.write_field(
            DP1_BASE + DP_SOFT_RESET,
            DP_VIDEO_RESET_HIGH,
            DP_VIDEO_RESET_LOW,
            0xF,
        );
        uart.delay(100_000);
        dp_ok &= uart.write_field(DP1_BASE + DP_SOFT_RESET, 31, 31, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_SOFT_RESET, 30, 30, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_SOFT_RESET, 29, 29, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_SOFT_RESET, 27, 27, 0);
        dp_ok &= uart.write_field(
            DP1_BASE + DP_SOFT_RESET,
            DP_VIDEO_RESET_HIGH,
            DP_VIDEO_RESET_LOW,
            0,
        );
        uart.delay(50_000);

        // The DP core-control fields are reset by DP_SOFT_RESET on this
        // transmitter.  Reapply the Linux soc_dp_dev_init() settings after
        // releasing reset, otherwise enhanced framing can silently be lost
        // while AUX/link training still appears healthy.
        dp_ok &= uart.write_field(DP1_BASE + DP_CORE_CONTROL, 4, 4, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_CORE_CONTROL, 31, 31, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_CORE_CONTROL, 30, 30, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_CORE_CONTROL, 29, 29, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_CORE_CONTROL, 28, 28, 0);
        uart.write_mmio("DP1+0x018_CORE_RB=", DP1_BASE + DP_CORE_CONTROL);

        // Core PLL: Linux's RBR setup at 1.62 Gb/s.
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL0, 31, 24, 0x87);
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL0, 19, 16, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL0, 13, 8, 2);
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL0, 5, 5, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL0, 4, 4, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL1, 23, 0, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL2, 16, 16, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL2, 11, 11, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL2, 10, 8, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_ANA_TX_CLK_CTRL, 13, 8, 12);
        dp_ok &= uart.write_field(DP1_BASE + DP_MPLL_CTRL0, 0, 0, 0);

        // Linux's 108 MHz pixel PLL entry: prediv=1, fbdiv=117,
        // divaux=13, pclkdiv=1.
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL0, 31, 24, 117);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL0, 19, 16, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL0, 13, 8, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL0, 5, 5, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL0, 4, 4, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL0, 1, 1, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL1, 17, 16, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL1, 12, 8, 13);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL2, 31, 31, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL2, 28, 24, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL2, 17, 17, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL2, 9, 8, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_SSC, 23, 0, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_PREPLL_CTRL0, 0, 0, 0);
        uart.delay(100_000);

        // RBR, two lanes, transmitter enabled. Keep analog lane defaults from
        // the bootloader and only apply the documented DP controls.
        dp_ok &= uart.write_field(DP1_BASE + DP_PHYIF_CTRL, 31, 31, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_PHYIF_CTRL, 6, 5, 1);
        dp_ok &= uart.write_field(DP1_BASE + DP_PHYIF_CTRL, 1, 0, 0);
        dp_ok &= uart.write_field(DP1_BASE + DP_PHYIF_CTRL, 20, 17, 3);

        // Fixed K3 PHY analog setup used by the Linux Inno DP driver. It is
        // required for AUX transactions as well as for the main link.
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL3, 27, 24, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL3, 26, 24, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL3, 25, 24, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL3, 24, 24, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_RTCAL_CTRL, 14, 8, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_RTCAL_CTRL, 15, 15, 1);
        let _ = uart.write_field(DP1_BASE + DP_ANA_RTCAL_CTRL, 23, 16, 240);
        let _ = uart.write_field(DP1_BASE + DP_ANA_RTCAL_CTRL, 26, 25, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL8, 29, 24, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL8, 21, 16, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL8, 13, 8, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL8, 5, 0, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_RTCAL_CTRL, 15, 15, 0);
        uart.delay(100_000);

        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 3, 0, 0xF);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 7, 4, 0xF);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 11, 8, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 15, 12, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 19, 16, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 23, 20, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 4, 4, 1);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 5, 5, 1);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 6, 6, 1);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 7, 7, 1);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 0, 0, 1);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 1, 1, 1);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 2, 2, 1);
        let _ = uart.write_field(DP1_BASE + DP_ANA_LANE_EMPH, 3, 3, 1);
        // Linux programs all four lanes in current-mode; assigning the
        // packed field to 1 would enable only lane 0.
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL3, 27, 24, 0xF);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL3, 14, 12, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL3, 10, 8, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL3, 6, 4, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL3, 2, 0, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_ENABLE, 31, 28, 5);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_ENABLE, 27, 24, 5);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL1, 7, 4, 5);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL1, 3, 0, 5);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL1, 28, 24, 0x19);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL1, 20, 16, 0x19);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL2, 4, 0, 0x19);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL2, 12, 8, 0x19);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL2, 31, 28, 2);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL2, 27, 24, 2);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL2, 23, 20, 2);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL2, 19, 16, 2);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_AUX_CTRL, 13, 12, 0);
        let _ = uart.write_field(DP1_BASE + DP_ANA_BG_CTRL, 2, 2, 1);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_ENABLE, 11, 8, 0xF);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_ENABLE, 3, 0, 0xF);
        let _ = uart.write_field(DP1_BASE + DP_ANA_TX_CTRL3, 31, 28, 0xF);

        uart.write_mmio("DP1+0x180_RB=", DP1_BASE + DP_MPLL_CTRL0);
        uart.write_mmio("DP1+0x190_RB=", DP1_BASE + DP_PREPLL_CTRL0);
        uart.write_mmio("DP1+0x100_RB=", DP1_BASE + DP_PHYIF_CTRL);
        let core_lock = unsafe { read_volatile((DP1_BASE + DP_MPLL_CTRL0) as *const u32) };
        let pixel_lock = unsafe { read_volatile((DP1_BASE + DP_PREPLL_CTRL0) as *const u32) };
        if dp_ok && (core_lock & DP_CORE_PLL_LOCK != 0) && (pixel_lock & DP_PIXEL_PLL_LOCK != 0) {
            uart.write_str("DP_PLL_STAGE_OK\n");
        } else {
            uart.write_str("DP_PLL_STAGE_READBACK_FAIL\n");
        }

        uart.write_str("AUX_AFTER_PLL_BEGIN\n");
        let mut after_pll_dpcd = [0u8; 4];
        let after_pll_aux_ok = uart.dp_aux_read(0x0000, &mut after_pll_dpcd);
        uart.write_mmio("AUX_AFTER_PLL_INT=", DP1_BASE + DP_GENERAL_INTERRUPT);
        uart.write_mmio("AUX_AFTER_PLL_STATUS=", DP1_BASE + DP_AUX_STATUS);
        if after_pll_aux_ok {
            uart.write_str("AUX_AFTER_PLL_OK\n");
        } else {
            uart.write_str("AUX_AFTER_PLL_FAIL\n");
        }

        uart.write_str("AUX_STAGE_BEGIN\n");
        // Linux uses zero in this mask to enable the AUX reply event.
        let _ = uart.write_field(DP1_BASE + DP_GENERAL_INTERRUPT_MASK, 16, 16, 0);
        // Match soc_dp_aux_hw_reset(): reset, settle, release, settle, then
        // clear the stale reply event before issuing the first request.
        let _ = uart.write_field(DP1_BASE + DP_SOFT_RESET, 27, 27, 1);
        uart.delay(2_000_000);
        let _ = uart.write_field(DP1_BASE + DP_SOFT_RESET, 27, 27, 0);
        uart.delay(2_000_000);
        unsafe { write_volatile((DP1_BASE + DP_GENERAL_INTERRUPT) as *mut u32, 1 << 16) };
        let mut dpcd = [0u8; 16];
        let aux_read_ok = uart.dp_aux_read(0x0000, &mut dpcd);
        uart.write_str("DPCD_REV=");
        uart.write_hex_u32(u32::from(dpcd[0]));
        uart.write_str("\nDPCD_RATE=");
        uart.write_hex_u32(u32::from(dpcd[1]));
        uart.write_str("\nDPCD_LANES=");
        uart.write_hex_u32(u32::from(dpcd[2]));
        uart.write_str("\n");
        uart.write_mmio("AUX_INT_RB=", DP1_BASE + DP_GENERAL_INTERRUPT);
        uart.write_mmio("AUX_STATUS_RB=", DP1_BASE + DP_AUX_STATUS);
        uart.write_mmio("AUX_CONFIG_RB=", DP1_BASE + DP_AUX_CONFIG);
        uart.write_mmio("AUX_START_RB=", DP1_BASE + DP_AUX_START);
        uart.write_mmio("AUX_DATA1_RB=", DP1_BASE + DP_AUX_DATA1);

        // 0x06 selects RBR (1.62 Gb/s); 0x82 selects two lanes with enhanced
        // framing. This matches the working Bianbu/Linux configuration.
        let aux_config_ok = aux_read_ok && dpcd[0] != 0 && uart.dp_aux_write(0x0100, &[0x06, 0x82]);
        if !aux_config_ok {
            uart.write_str("AUX_STAGE_READBACK_FAIL\n");
        } else {
            uart.write_str("AUX_STAGE_OK\n");
            uart.write_str("LINK_TRAIN_BEGIN\n");

            // DP 1.2 clock recovery and channel equalization. The sink's
            // adjust request bytes are fed back into the next lane setting,
            // as in drm_dp_link_train_*().
            let mut lane_set = [0u8; 2];
            let mut clock_recovery_ok = false;
            for _ in 0..8 {
                let _ = uart.dp_aux_write(0x0103, &lane_set);
                let _ = uart.write_field(DP1_BASE + DP_PHYIF_CTRL, 31, 31, 1);
                let _ = uart.write_field(DP1_BASE + DP_PHYIF_CTRL, 28, 25, 1);
                if !uart.dp_aux_write(0x0102, &[0x21]) {
                    break;
                }
                uart.delay(4_000_000);
                let mut status = [0u8; 6];
                if !uart.dp_aux_read(0x0202, &mut status) {
                    break;
                }
                uart.write_str("LINK_CR_STATUS=");
                uart.write_hex_u32(u32::from(status[0]));
                uart.write_str("\n");
                let lane0_cr = status[0] & 0x01 != 0;
                let lane1_cr = status[0] & 0x10 != 0;
                if lane0_cr && lane1_cr {
                    clock_recovery_ok = true;
                    break;
                }
                let lane0_v = status[4] & 0x03;
                let lane0_p = (status[4] >> 2) & 0x03;
                let lane1_v = (status[4] >> 4) & 0x03;
                let lane1_p = (status[4] >> 6) & 0x03;
                lane_set[0] = lane0_v
                    | (lane0_p << 3)
                    | if lane0_v == 3 { 1 << 2 } else { 0 }
                    | if lane0_p == 3 { 1 << 5 } else { 0 };
                lane_set[1] = lane1_v
                    | (lane1_p << 3)
                    | if lane1_v == 3 { 1 << 2 } else { 0 }
                    | if lane1_p == 3 { 1 << 5 } else { 0 };
            }

            let mut channel_equalization_ok = false;
            if clock_recovery_ok {
                let pattern: u8 = if dpcd[2] & (1 << 6) != 0 { 0x23 } else { 0x22 };
                for _ in 0..8 {
                    let _ = uart.dp_aux_write(0x0103, &lane_set);
                    let _ = uart.write_field(
                        DP1_BASE + DP_PHYIF_CTRL,
                        28,
                        25,
                        u32::from(pattern & 0x03),
                    );
                    if !uart.dp_aux_write(0x0102, &[pattern]) {
                        break;
                    }
                    uart.delay(4_000_000);
                    let mut status = [0u8; 6];
                    if !uart.dp_aux_read(0x0202, &mut status) {
                        break;
                    }
                    uart.write_str("LINK_EQ_STATUS=");
                    uart.write_hex_u32(u32::from(status[0]));
                    uart.write_str("\n");
                    let lane0_eq = status[0] & 0x07 == 0x07;
                    let lane1_eq = status[0] & 0x70 == 0x70;
                    if lane0_eq && lane1_eq && status[2] & 0x01 != 0 {
                        channel_equalization_ok = true;
                        break;
                    }
                    let lane0_v = status[4] & 0x03;
                    let lane0_p = (status[4] >> 2) & 0x03;
                    let lane1_v = (status[4] >> 4) & 0x03;
                    let lane1_p = (status[4] >> 6) & 0x03;
                    lane_set[0] = lane0_v
                        | (lane0_p << 3)
                        | if lane0_v == 3 { 1 << 2 } else { 0 }
                        | if lane0_p == 3 { 1 << 5 } else { 0 };
                    lane_set[1] = lane1_v
                        | (lane1_p << 3)
                        | if lane1_v == 3 { 1 << 2 } else { 0 }
                        | if lane1_p == 3 { 1 << 5 } else { 0 };
                }
            }

            let link_train_ok = clock_recovery_ok && channel_equalization_ok;
            let _ = uart.dp_aux_write(0x0102, &[0x00]);
            let _ = uart.write_field(DP1_BASE + DP_PHYIF_CTRL, 28, 25, 0);
            // Linux leaves PHY_BUSY_BYP asserted after training. Clearing it
            // here can make AUX/link-training look successful while stopping
            // the main video transmitter before the MSA is sent.
            let _ = uart.write_field(DP1_BASE + DP_PHYIF_CTRL, 31, 31, 1);
            uart.write_mmio("DP1+0x100_POST_TRAIN_RB=", DP1_BASE + DP_PHYIF_CTRL);
            if !link_train_ok {
                uart.write_str("LINK_TRAIN_FAIL\n");
            } else {
                uart.write_str("LINK_TRAIN_OK\n");
                uart.write_str("DP_MSA_BEGIN\n");

                let htotal = DPU1_MODE_WIDTH + DPU1_MODE_HFP + DPU1_MODE_HSYNC + DPU1_MODE_HBP;
                let vtotal = DPU1_MODE_HEIGHT + DPU1_MODE_VFP + DPU1_MODE_VSYNC + DPU1_MODE_VBP;
                let hblank = htotal - DPU1_MODE_WIDTH;
                let vblank = vtotal - DPU1_MODE_HEIGHT;
                let hblank_interval = hblank * DP_LINK_RATE_KHZ / (4 * DPU1_PIXEL_CLOCK_KHZ);
                let tu_scaled = DPU1_PIXEL_CLOCK_KHZ * 24 * 640 / (8 * 2 * DP_LINK_RATE_KHZ);
                let tu_int = tu_scaled / 10;
                let tu_frac = tu_scaled % 10;
                let mut msa_ok = true;
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_VSAMPLE, 26, 22, 1);
                msa_ok &= uart.write_field(DP1_BASE + DP_VINPUT_POLARITY, 29, 29, 1);
                msa_ok &= uart.write_field(DP1_BASE + DP_VINPUT_POLARITY, 28, 28, 1);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG2, 31, 16, DPU1_MODE_HEIGHT);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG2, 15, 0, vblank);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG1, 29, 16, hblank);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG1, 15, 0, DPU1_MODE_WIDTH);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG3, 31, 16, DPU1_MODE_HFP);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG3, 15, 0, DPU1_MODE_HSYNC);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG4, 31, 16, DPU1_MODE_VFP);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG4, 15, 0, DPU1_MODE_VSYNC);
                msa_ok &= uart.write_field(
                    DP1_BASE + DP_VIDEO_MSA1,
                    31,
                    16,
                    DPU1_MODE_HSYNC + DPU1_MODE_HBP,
                );
                msa_ok &= uart.write_field(
                    DP1_BASE + DP_VIDEO_MSA1,
                    15,
                    0,
                    DPU1_MODE_VSYNC + DPU1_MODE_VBP,
                );
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_MSA2, 7, 0, 0x20);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_MSA3, 31, 8, 0);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_MSA3, 7, 0, 0);
                msa_ok &=
                    uart.write_field(DP1_BASE + DP_VIDEO_HBLANK_INTERVAL, 31, 16, hblank_interval);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG5, 31, 25, tu_int);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG5, 24, 21, tu_frac);
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_CONFIG5, 18, 10, 16);
                msa_ok &= uart.write_field(DP1_BASE + DP_PHYIF_CTRL, 8, 8, 1);
                // Use the normal stream. The framebuffer is changed below to
                // cycle blue, red and green through the real Saturn/RDMA path.
                uart.write_str("DP_COLORBAR_TEST_BEGIN\n");
                msa_ok &= uart.write_field(
                    DP1_BASE + DP_VIDEO_COLORBAR_CFG,
                    0,
                    0,
                    if DP_COLORBAR_TEST { 1 } else { 0 },
                );
                // Keep the external DP path in the normal DP stream mode.
                // STREAM_ENC_EN is only used by the eDP branch in Linux.
                msa_ok &= uart.write_field(
                    DP1_BASE + DP_VIDEO_VSAMPLE,
                    DP_STREAM_ENC_EN,
                    DP_STREAM_ENC_EN,
                    0,
                );
                msa_ok &= uart.write_field(DP1_BASE + DP_VIDEO_VSAMPLE, 28, 28, 1);
                uart.write_mmio("DP1+0x200_VIDEO_RB=", DP1_BASE + DP_VIDEO_VSAMPLE);
                uart.write_mmio("DP1+0x238_COLORBAR_RB=", DP1_BASE + DP_VIDEO_COLORBAR_CFG);
                uart.write_mmio("DP1+0x28_CLKSEL_RB=", DP1_BASE + DP_CLK_INV_MUX);
                let timing_enable_ok = uart.write_mmio_value(
                    "TMG0+0x00_ACTIVE_RB=",
                    DPU1_BASE + DPU1_TMG0 + TMG_CTRL,
                    TMG_CTRL_ACTIVE,
                );
                // The command-list POSTPIPE rows are not visible in the
                // active window on this board. For this diagnostic, apply the
                // same two values directly before CFG_READY/SW_START, which
                // is the ordering used by the constructor's scene commit.
                let postpipe_direct_ok = if POSTPIPE_DIRECT_TEST {
                    uart.write_mmio_value("POSTPIPE+0x00_DIRECT_RB=", DPU1_BASE + POSTPIPE_CTRL, 0)
                        && uart.write_mmio_value(
                            "POSTPIPE+0x34_DIRECT_RB=",
                            DPU1_BASE + POSTPIPE_SIZE,
                            DPU1_MODE_WIDTH | (DPU1_MODE_HEIGHT << 16),
                        )
                } else {
                    true
                };

                // Match saturn_hee_cfg_ready() and saturn_hee_sw_start().
                // The command-list CFG_RDY row arms the channel, while this
                // scene-control handshake releases the normal DPU frame.
                let scene_cfg_ready_ok = uart.write_mmio_value(
                    "DPU1+0x348_CFG_READY_RB=",
                    DPU1_BASE + DPU_SCENE_CTL_CONFIG_READY,
                    1,
                );
                let scene_sw_start_ok = uart.write_mmio_command(
                    "DPU1+0x350_SW_START_RB=",
                    DPU1_BASE + DPU_SCENE_CTL_SW_START,
                    1,
                );
                let mut post_status = [0u8; 6];
                let _post_status_ok = uart.dp_aux_read(0x0202, &mut post_status);
                uart.write_str("LINK_POST_STATUS=");
                uart.write_hex_u32(u32::from(post_status[0]));
                uart.write_str("\n");

                // Read the effective state after the command-list commit and
                // the scene start. Readbacks from the staging phase alone do
                // not prove that the composer accepted the values on the
                // active frame. Keep this probe read-only so it can be used
                // with the solid-color test as well as with RDMA.
                uart.write_str("DPU_POST_COMMIT_BEGIN\n");
                uart.write_mmio("DPU1+0x340_POST_RB=", DPU1_BASE + DPU1_SCENE_CTL);
                uart.write_mmio(
                    "DPU1+0x348_POST_RB=",
                    DPU1_BASE + DPU_SCENE_CTL_CONFIG_READY,
                );
                uart.write_mmio("DPU1+0x34C_POST_RB=", DPU1_BASE + DPU_SCENE_CTL_VIDEO_MODE);
                uart.write_mmio("DPU1+0x350_POST_RB=", DPU1_BASE + DPU_SCENE_CTL_SW_START);
                uart.write_mmio("DPU1+0x3E8_POST_RB=", DPU1_BASE + DPU1_CMDLIST_CMPS);
                uart.write_mmio("DPU1+0x3F4_POST_RB=", CMDLIST_CFG_READY_ADDR);
                uart.write_mmio(
                    "DPU1+0x400_REUSE_ACTIVE_RB=",
                    DPU1_BASE + DPU1_CMDLIST_REUSE_ACTIVE,
                );
                uart.write_mmio(
                    "DPU1+0x434_SW_EVENT_RB=",
                    DPU1_BASE + DPU1_CMDLIST_SW_EVENT_BASE + CMDLIST_COMPOSER_CHANNEL * 4,
                );
                uart.write_mmio(
                    "DPU1+0x46C_START_CMPS_Y_RB=",
                    DPU1_BASE + DPU1_CMDLIST_START_CMPS_Y_BASE + CMDLIST_COMPOSER_CHANNEL * 4,
                );
                uart.write_mmio("CMDLIST7+0x1F4_DBG_RB=", DPU1_BASE + CMDLIST_DBG0);
                uart.write_mmio("CMDLIST+0x6F8_STATUS_RB=", DPU1_BASE + CMDLIST_DBG_STATUS);
                uart.write_mmio("CMP1+0x00_POST_RB=", DPU1_BASE + DPU1_COMPOSER1);
                uart.write_mmio("CMP1+0x08_POST_RB=", DPU1_BASE + CMP_BG_R);
                uart.write_mmio("CMP1+0x0C_POST_RB=", DPU1_BASE + CMP_BG_G);
                uart.write_mmio("CMP1+0x10_POST_RB=", DPU1_BASE + CMP_BG_B);
                uart.write_mmio("CMP1+0x14_POST_RB=", DPU1_BASE + CMP_BG_ALPHA);
                uart.write_mmio("CMP1+0x20_POST_RB=", DPU1_BASE + CMP_LAYER0_AREA_X);
                uart.write_mmio("CMP1+0x24_POST_RB=", DPU1_BASE + CMP_LAYER0_AREA_Y);
                uart.write_mmio("CMP1+0x28_POST_RB=", DPU1_BASE + CMP_LAYER0_SOLID_R);
                uart.write_mmio("CMP1+0x2C_POST_RB=", DPU1_BASE + CMP_LAYER0_SOLID_G);
                uart.write_mmio("CMP1+0x30_POST_RB=", DPU1_BASE + CMP_LAYER0_SOLID_B);
                uart.write_mmio("CMP1+0x34_POST_RB=", DPU1_BASE + CMP_LAYER0_SOLID_A);
                uart.write_mmio("CMP1+0x38_POST_RB=", DPU1_BASE + CMP_LAYER0_CTRL);
                uart.write_mmio("POSTPIPE+0x00_POST_RB=", DPU1_BASE + POSTPIPE_CTRL);
                uart.write_mmio("POSTPIPE+0x34_POST_RB=", DPU1_BASE + POSTPIPE_SIZE);
                uart.write_mmio("TMG0+0x00_POST_RB=", DPU1_BASE + DPU1_TMG0 + TMG_CTRL);
                uart.write_mmio("DPU1+0x730_POST_RB=", DPU1_BASE + DPU_ONLINE_IRQ_STATUS);
                uart.write_mmio("RDMA0+0xB4_POST_RB=", DPU1_BASE + RDMA_DBG_IRQ_STATUS);
                uart.write_mmio("RDMA0+0x00_POST_RB=", DPU1_BASE + DPU1_RDMA0 + RDMA_CTRL);
                uart.write_mmio(
                    "RDMA0+0x24_POST_RB=",
                    DPU1_BASE + DPU1_RDMA0 + RDMA_BASE_ADDR0_LOW,
                );
                uart.write_mmio(
                    "RDMA0+0x28_POST_RB=",
                    DPU1_BASE + DPU1_RDMA0 + RDMA_BASE_ADDR0_HIGH,
                );
                uart.write_mmio("RDMA0+0x3C_POST_RB=", DPU1_BASE + DPU1_RDMA0 + RDMA_STRIDE0);
                uart.write_mmio(
                    "RDMA0+0x40_POST_RB=",
                    DPU1_BASE + DPU1_RDMA0 + RDMA_IMAGE_SIZE,
                );
                uart.write_mmio(
                    "RDMA0+0x48_POST_RB=",
                    DPU1_BASE + DPU1_RDMA0 + RDMA_BBOX_END,
                );
                uart.write_mmio("RDMA0+0x74_POST_RB=", DPU1_BASE + DPU1_RDMA0 + RDMA_FORMAT);
                uart.write_mmio("TBU0+0x00_POST_RB=", DPU1_BASE + DMMU_TBU0 + DMMU_TBU_CTRL);
                uart.write_mmio(
                    "TBU0+0x04_POST_RB=",
                    DPU1_BASE + DMMU_TBU0 + DMMU_TBU_BASE0_LOW,
                );
                uart.write_mmio(
                    "TBU0+0x08_POST_RB=",
                    DPU1_BASE + DMMU_TBU0 + DMMU_TBU_BASE0_HIGH,
                );
                uart.write_mmio("TBU0+0x1C_POST_RB=", DPU1_BASE + DMMU_TBU0 + DMMU_TBU_VA0);
                uart.write_mmio("TBU0+0x28_POST_RB=", DPU1_BASE + DMMU_TBU0 + DMMU_TBU_SIZE0);
                uart.write_str("DPU_POST_COMMIT_END\n");
                if msa_ok
                    && timing_enable_ok
                    && scene_cfg_ready_ok
                    && scene_sw_start_ok
                    && postpipe_direct_ok
                    && cmp_layer_active_ok
                {
                    uart.write_str("DP_MSA_STAGE_OK\n");
                    uart.write_str("SCANOUT_STAGE_OK\n");
                    uart.write_str("SCANOUT_DIAG_BEGIN\n");
                    uart.write_mmio("DP1+0x018_CORE_ACTIVE_RB=", DP1_BASE + DP_CORE_CONTROL);
                    uart.write_mmio("DP1+0x088_HPD_RB=", DP1_BASE + DP_HPD_INTERRUPT_STATUS);
                    uart.write_mmio("DP1+0x080_GENERAL_INT_RB=", DP1_BASE + DP_GENERAL_INTERRUPT);
                    uart.write_mmio(
                        "DP1+0x084_GENERAL_MASK_RB=",
                        DP1_BASE + DP_GENERAL_INTERRUPT_MASK,
                    );
                    uart.write_mmio(
                        "DP1+0x08C_HPD_ENABLE_RB=",
                        DP1_BASE + DP_HPD_INTERRUPT_ENABLE,
                    );
                    uart.write_mmio("DPU1+0x70C_IRQ_MASK=", DPU1_BASE + DPU_ONLINE_IRQ_MASK);
                    uart.write_mmio("DPU1+0x730_IRQ_STATUS=", DPU1_BASE + DPU_ONLINE_IRQ_STATUS);
                    uart.write_mmio(
                        "DPU1+0x734_IRQ_ENTER_PENDING=",
                        DPU1_BASE + DPU_ONLINE_IRQ_ENTER_PENDING,
                    );
                    uart.write_mmio("DPU1+0x758_IRQ_RAW=", DPU1_BASE + DPU_ONLINE_IRQ_RAW);
                    uart.write_mmio(
                        "DPU1+0x75C_IRQ_RAW_ENTER_PENDING=",
                        DPU1_BASE + DPU_ONLINE_IRQ_RAW_ENTER_PENDING,
                    );
                    uart.write_mmio(
                        "DPU1+0x760_IRQ_RAW_PADDR=",
                        DPU1_BASE + DPU_ONLINE_IRQ_RAW_PADDR,
                    );
                    uart.write_mmio("DPU1+0x3F4_CFG_READY=", DPU1_BASE + DPU1_CMDLIST_CFG_READY);
                    uart.write_mmio(
                        "DPU1+0x400_REUSE_ACTIVE=",
                        DPU1_BASE + DPU1_CMDLIST_REUSE_ACTIVE,
                    );
                    uart.write_mmio(
                        "DPU1+0x434_SW_EVENT=",
                        DPU1_BASE + DPU1_CMDLIST_SW_EVENT_BASE + CMDLIST_COMPOSER_CHANNEL * 4,
                    );
                    uart.write_mmio(
                        "DPU1+0x46C_START_CMPS_Y=",
                        DPU1_BASE + DPU1_CMDLIST_START_CMPS_Y_BASE + CMDLIST_COMPOSER_CHANNEL * 4,
                    );
                    uart.write_mmio("CMDLIST7+0x1F4_DBG=", DPU1_BASE + CMDLIST_DBG0);
                    uart.write_mmio("CMDLIST+0x6F8_STATUS=", DPU1_BASE + CMDLIST_DBG_STATUS);
                    uart.write_mmio("RDMA0+0xAC_DBG_RAW=", DPU1_BASE + RDMA_DBG_IRQ_RAW);
                    uart.write_mmio("RDMA0+0xB0_DBG_MASK=", DPU1_BASE + RDMA_DBG_IRQ_MASK);
                    uart.write_mmio("RDMA0+0xB4_DBG_STATUS=", DPU1_BASE + RDMA_DBG_IRQ_STATUS);
                    uart.write_mmio("RDMA0+0xBC_DBG_INFO0=", DPU1_BASE + RDMA_DBG_INFO0);
                    uart.write_mmio("RDMA0+0xC0_DBG_INFO1=", DPU1_BASE + RDMA_DBG_INFO1);
                    uart.write_mmio("RDMA0+0xC4_DBG_INFO2=", DPU1_BASE + RDMA_DBG_INFO2);
                    uart.write_mmio("RDMA0+0xC8_DBG_INFO3=", DPU1_BASE + RDMA_DBG_INFO3);
                    uart.write_mmio("RDMA0+0xCC_DBG_INFO4=", DPU1_BASE + RDMA_DBG_INFO4);
                    uart.write_mmio("RDMA0+0x00_DIAG=", DPU1_BASE + DPU1_RDMA0 + RDMA_CTRL);
                    uart.write_mmio(
                        "RDMA0+0x24_DIAG=",
                        DPU1_BASE + DPU1_RDMA0 + RDMA_BASE_ADDR0_LOW,
                    );
                    uart.write_mmio(
                        "RDMA0+0x28_DIAG=",
                        DPU1_BASE + DPU1_RDMA0 + RDMA_BASE_ADDR0_HIGH,
                    );
                    uart.write_mmio("TBU0+0x00_DIAG=", DPU1_BASE + DMMU_TBU0 + DMMU_TBU_CTRL);
                    uart.write_str("SCANOUT_DIAG_OK\n");

                    uart.write_str("RGB_CYCLE_BEGIN\n");
                    if DP_COLORBAR_TEST {
                        // Keep the known-good transmitter blue. This is the
                        // repeatability test for the physical display path.
                        uart.write_str("DP_COLORBAR_BLUE_HOLD\n");
                        loop {
                            unsafe { core::arch::asm!("wfi") };
                        }
                    } else if TMG_BACKGROUND_TEST {
                        uart.write_str("TMG_BACKGROUND_RED_HOLD\n");
                        loop {
                            unsafe { core::arch::asm!("wfi") };
                        }
                    } else if CMP_BACKGROUND_TEST {
                        uart.write_str("CMP_BACKGROUND_RED_HOLD\n");
                        loop {
                            unsafe { core::arch::asm!("wfi") };
                        }
                    } else if SATURN_SOLID_TEST {
                        if SATURN_SOLID_CYCLE_TEST {
                            // Change only CMP1's solid color. No DDR write or
                            // RDMA transaction is involved in this diagnostic.
                            for (label, red, green, blue) in [
                                ("BLUE", 0, 0, SOLID_BLUE),
                                ("RED", SOLID_RED, 0, 0),
                                ("GREEN", 0, SOLID_GREEN, 0),
                            ] {
                                let color_ok = uart.write_mmio_value(
                                    "CMP1+0x28_SOLID_R_RB=",
                                    DPU1_BASE + CMP_LAYER0_SOLID_R,
                                    red,
                                ) && uart.write_mmio_value(
                                    "CMP1+0x2C_SOLID_G_RB=",
                                    DPU1_BASE + CMP_LAYER0_SOLID_G,
                                    green,
                                ) && uart.write_mmio_value(
                                    "CMP1+0x30_SOLID_B_RB=",
                                    DPU1_BASE + CMP_LAYER0_SOLID_B,
                                    blue,
                                );
                                let cfg_ok = uart.write_mmio_value(
                                    "DPU1+0x348_COLOR_CFG_RB=",
                                    DPU1_BASE + DPU_SCENE_CTL_CONFIG_READY,
                                    1,
                                );
                                let start_ok = uart.write_mmio_command(
                                    "DPU1+0x350_COLOR_START_RB=",
                                    DPU1_BASE + DPU_SCENE_CTL_SW_START,
                                    1,
                                );
                                uart.write_str("RGB_CYCLE_SOLID_");
                                uart.write_str(label);
                                uart.write_str(if color_ok && cfg_ok && start_ok {
                                    "_OK\n"
                                } else {
                                    "_READBACK_FAIL\n"
                                });
                                uart.delay(RGB_HOLD_CYCLES);
                            }
                        } else {
                            uart.write_str("SATURN_SOLID_BLUE_HOLD\n");
                            loop {
                                unsafe { core::arch::asm!("wfi") };
                            }
                        }
                    } else {
                        if RDMA_PHYS_DIRECT_TEST {
                            uart.write_str("RDMA_MODE=PHYSICAL_DIRECT\n");
                        } else {
                            uart.write_str("RDMA_MODE=DMMU_VIRTUAL\n");
                        }
                        for (label, color) in [
                            ("BLUE", 0xFF00_00FF),
                            ("RED", 0xFFFF_0000),
                            ("GREEN", 0xFF00_FF00),
                        ] {
                            fill_framebuffer(color);
                            uart.write_str("RGB_CYCLE_");
                            uart.write_str(label);
                            uart.write_str("\n");
                            uart.delay(RGB_HOLD_CYCLES);
                        }
                    }
                    uart.write_str("RGB_CYCLE_DONE\n");
                } else {
                    uart.write_str("DP_MSA_STAGE_READBACK_FAIL\n");
                }
            }
        }
    }
    uart.write_str("MMIO_PROBE_OK\n");

    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
