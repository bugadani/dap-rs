use crate::{
    dap::{self, TransferConfig},
    swd::{APnDP, DPRegister, RnW},
};

/// Describes a JTAG sequence request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SequenceInfo {
    /// The number of bits to shift in/out.
    pub n_bits: u8,

    /// If the TDO data should be captured.
    pub capture: bool,

    /// The TMS level to output.
    pub tms: bool,
}

impl From<u8> for SequenceInfo {
    fn from(byte: u8) -> Self {
        const JTAG_SEQUENCE_TCK: u8 = 0x3F;
        const JTAG_SEQUENCE_TMS: u8 = 0x40;
        const JTAG_SEQUENCE_TDO: u8 = 0x80;

        let n_bits = match byte & JTAG_SEQUENCE_TCK {
            // CMSIS-DAP says 0 means 64 bits
            0 => 64,
            // Other integers are normal.
            n => n,
        };

        Self {
            n_bits: n_bits as u8,
            capture: (byte & JTAG_SEQUENCE_TDO) != 0,
            tms: (byte & JTAG_SEQUENCE_TMS) != 0,
        }
    }
}

/// Describes a single transfer in a JTAG transfer request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct TransferInfo {
    /// Whether the transfer is to an AP or DP.
    pub ap_ndp: APnDP,
    pub r_nw: RnW,
    pub a2a3: DPRegister,
    pub match_value: bool,
    pub match_mask: bool,
    pub timestamp: bool,
}

impl TransferInfo {
    pub(crate) const RDBUFF: Self = Self {
        r_nw: RnW::R,
        a2a3: DPRegister::RDBUFF,
        ap_ndp: APnDP::DP,
        match_value: false,
        match_mask: false,
        timestamp: false,
    };
}

impl From<u8> for TransferInfo {
    fn from(byte: u8) -> Self {
        const DAP_TRANSFER_APNDP: u8 = 1 << 0;
        const DAP_TRANSFER_RNW: u8 = 1 << 1;
        const DAP_TRANSFER_A2: u8 = 1 << 2;
        const DAP_TRANSFER_A3: u8 = 1 << 3;
        const DAP_TRANSFER_MATCH_VALUE: u8 = 1 << 4;
        const DAP_TRANSFER_MATCH_MASK: u8 = 1 << 5;
        const DAP_TRANSFER_TIMESTAMP: u8 = 1 << 7;

        Self {
            ap_ndp: if byte & DAP_TRANSFER_APNDP != 0 {
                APnDP::AP
            } else {
                APnDP::DP
            },
            r_nw: if byte & DAP_TRANSFER_RNW != 0 {
                RnW::R
            } else {
                RnW::W
            },
            a2a3: DPRegister::try_from((byte & (DAP_TRANSFER_A2 | DAP_TRANSFER_A3)) >> 2).unwrap(),
            match_value: byte & DAP_TRANSFER_MATCH_VALUE != 0,
            match_mask: byte & DAP_TRANSFER_MATCH_MASK != 0,
            timestamp: byte & DAP_TRANSFER_TIMESTAMP != 0,
        }
    }
}

/// Describes the position of a TAP in the JTAG chain.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct TapConfig {
    /// The number of bits in the IR register.
    pub ir_length: u8,
    /// The number of bypass bits before the IR register.
    pub ir_before: u16,
    /// The number of bypass bits after the IR register.
    pub ir_after: u16,
}

impl TapConfig {
    /// Empty value for array initialization
    pub const INIT: Self = Self {
        ir_length: 0,
        ir_before: 0,
        ir_after: 0,
    };
}

/// JTAG interface configuraiton.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    /// The number of devices on the JTAG chain.
    pub device_count: u8,
    /// The position of the selected device.
    pub index: u8,
    /// TAPs on the scan chain.
    pub scan_chain: &'static mut [TapConfig],
}

impl Config {
    /// Creates a new, empty JTAG interface configuration.
    ///
    /// The probe will be able to connect to JTAG chains with up to
    /// `chain_buffer.len()` TAPs.
    pub fn new(chain_buffer: &'static mut [TapConfig]) -> Self {
        Self {
            device_count: 0,
            index: 0,
            scan_chain: chain_buffer,
        }
    }

    /// Returns information about the currently selected TAP.
    pub fn current_tap(&self) -> TapConfig {
        self.scan_chain[self.index as usize]
    }

    pub(crate) fn update_device_count(&mut self, count: u8) -> bool {
        if count as usize >= self.scan_chain.len() {
            return false;
        }
        self.device_count = count;
        true
    }
}

impl Config {
    /// Selects the device at the given index.
    ///
    /// If the index is out of bounds, it returns `false` and does not change the
    /// selected device.
    pub fn select_index(&mut self, index: u8) -> bool {
        if index >= self.device_count {
            warn!("Invalid JTAG TAP index: {}", index);
            return false;
        }
        if index != self.index {
            info!("Selecting JTAG TAP #{}", index);
            self.index = index;
        }
        true
    }
}

const IDLE_TO_SHIFT_DR: &[bool] = &[true, false, false];
const IDLE_TO_SHIFT_IR: &[bool] = &[true, true, false, false];
const SHIFT_TO_IDLE: &[bool] = &[true, true, false];
const EXIT1_TO_IDLE: &[bool] = &[true, false];

pub(crate) const JTAG_IR_ABORT: u32 = 0x08;
pub(crate) const JTAG_IR_DPACC: u32 = 0x0A;
pub(crate) const JTAG_IR_APACC: u32 = 0x0B;
pub(crate) const JTAG_IR_IDCODE: u32 = 0x0E;

// TODO: currently this only supports JTAG DP V0.
const DAP_TRANSFER_OK_FAULT: u32 = 0x02;

/// JTAG interface.
pub trait Jtag<DEPS>: From<DEPS> {
    /// If JTAG is available or not.
    const AVAILABLE: bool;

    /// Returns a mutable reference to the JTAG interface configuration.
    fn config(&mut self) -> &mut Config;

    /// Handle a JTAG sequence request.
    ///
    /// The implementor is responsible for parsing the request. The first byte contains
    /// the number of sequences to process. Each sequence is described by a byte, which
    /// contains the number of bits to shift in/out, whether to capture TDO, and the TMS
    /// level to output. This info byte may be parsed by [`SequenceInfo`]'s `From<u8>`
    /// implementation.
    fn sequences(&mut self, mut req: dap::Request, resp: &mut dap::ResponseWriter) {
        // Run requested JTAG sequences. Cannot fail.
        let sequence_count = req.next_u8();

        for _ in 0..sequence_count {
            let sequence_info = SequenceInfo::from(req.next_u8());
            let n_bytes = sequence_info.n_bits.div_ceil(8) as usize;
            self.sequence(sequence_info, &req.data[..n_bytes], resp.remaining());

            req.consume(n_bytes);
            if sequence_info.capture {
                resp.skip(n_bytes);
            }
        }
    }

    /// Handle a single JTAG sequence.
    ///
    /// This function handles the individual JTAG sequences that are broken up by
    /// the `sequences` function. It is called for each sequence in the request.
    ///
    /// For better performance, you may choose to provide an empty implementation
    /// of this function and handle the sequences by overriding `sequences` instead.
    fn sequence(&mut self, info: SequenceInfo, tdi: &[u8], rxbuf: &mut [u8]);

    /// Send out a sequence of TMS bits, while keeping TDI unchanged.
    fn tms_sequence(&mut self, tms: &[bool]);

    /// Set the maximum clock frequency, return `true` if it is valid.
    fn set_clock(&mut self, max_frequency: u32) -> bool;

    /// Shift out the instruction register (IR).
    ///
    /// This function starts from Test/Idle and ends in Test/Idle, after shifting out idle bits.
    fn shift_ir(&mut self, ir: u32) {
        self.tms_sequence(IDLE_TO_SHIFT_IR);

        let tap = self.config().current_tap();
        let ir_length = tap.ir_length;
        let bypass_bits_before = tap.ir_before;
        let bypass_bits_after = tap.ir_after;

        shift_repeated_tdi(self, 0xFF, bypass_bits_before, false);

        shift_register_data(self, ir, ir_length, bypass_bits_after == 0);
        bypass_after_data(self, bypass_bits_after);

        self.tms_sequence(EXIT1_TO_IDLE);
    }

    /// Shift out the data register (DR) and return the captured bits.
    ///
    /// This function starts from Test/Idle and ends in Test/Idle, after shifting out idle bits.
    fn shift_dr(&mut self, dr: u32) -> u32 {
        self.tms_sequence(IDLE_TO_SHIFT_DR);

        let device_index = self.config().index as usize;
        let device_count = self.config().device_count as usize;
        let bypass_bits_before = device_index as u16;
        let bypass_bits_after = device_count as u16 - bypass_bits_before - 1;

        shift_repeated_tdi(self, 0xFF, bypass_bits_before, false);

        let dr = shift_register_data(self, dr, 32, bypass_bits_after == 0);
        bypass_after_data(self, bypass_bits_after);

        self.tms_sequence(EXIT1_TO_IDLE);

        dr
    }

    /// Shift out the data register (DR) for an ABORT command.
    fn write_abort(&mut self, data: u32) {
        transfer(self, RnW::W, 0, 8, data, false);
    }

    /// Execute a JTAG DAP transfer.
    ///
    /// This function executes the data part of a DPACC or APACC scan, starting from Test/Idle
    /// and ending in Test/Idle, after shifting out idle bits.
    fn transfer(
        &mut self,
        r_nw: RnW,
        a2a3: u8,
        transfer_config: &TransferConfig,
        data: u32,
    ) -> TransferResult {
        transfer(self, r_nw, a2a3, transfer_config.idle_cycles, data, true)
    }
}

fn transfer<DEPS>(
    jtag: &mut impl Jtag<DEPS>,
    r_nw: RnW,
    a2a3: u8,
    idle_cycles: u8,
    data: u32,
    check_ack: bool,
) -> TransferResult {
    jtag.tms_sequence(IDLE_TO_SHIFT_DR);

    let device_index = jtag.config().index as usize;
    let device_count = jtag.config().device_count as usize;
    let bypass_bits_before = device_index as u16;
    let bypass_bits_after = device_count as u16 - bypass_bits_before - 1;

    shift_repeated_tdi(jtag, 0xFF, bypass_bits_before, false);

    // Shift out the register address and read/write bits, read back ack.
    let ack = shift_register_data(jtag, (a2a3 << 1) as u32 | (r_nw as u32), 3, false);

    // Based on ack, shift out data or stop the transfer.
    let result = if ack == DAP_TRANSFER_OK_FAULT || !check_ack {
        let captured_dr = shift_register_data(jtag, data, 32, bypass_bits_after == 0);
        bypass_after_data(jtag, bypass_bits_after);

        jtag.tms_sequence(EXIT1_TO_IDLE);

        TransferResult::Ok(captured_dr)
    } else {
        jtag.tms_sequence(SHIFT_TO_IDLE);
        TransferResult::Wait
    };

    shift_repeated_tdi(jtag, 0xFF, idle_cycles as u16, false);

    result
}

/// Shift out data, while assuming to be in either Shift-DR or Shift-IR state.
///
/// If `exit_shift` is true, it will exit the shift state (into Exit1-DR or Exit1-IR).
///
/// The function will return the captured TDO data.
fn shift_register_data<DEPS>(
    jtag: &mut impl Jtag<DEPS>,
    mut data: u32,
    clock_cycles: u8,
    exit_shift: bool,
) -> u32 {
    // All bits except last with TMS = 0
    let mut captured_dr = 0;
    let mut clocks = clock_cycles;
    while clocks > 1 {
        let bits = (clocks - 1).min(8);

        let captured_byte = shift_tdi(jtag, data as u8, bits, false);
        captured_dr >>= bits;
        captured_dr |= (captured_byte as u32) << (clock_cycles - bits);

        data >>= bits;
        clocks -= bits;
    }

    // Last bit (with TMS = 1 if exit_shift)
    let captured_byte = shift_tdi(jtag, data as u8, 1, exit_shift);
    captured_dr >>= 1;
    captured_dr |= (captured_byte as u32) << (clock_cycles - 1);

    captured_dr
}

/// Shift out `clocks` bits of TDI data, with TMS set to the given value.
fn shift_repeated_tdi<DEPS>(jtag: &mut impl Jtag<DEPS>, tdi: u8, mut clocks: u16, tms: bool) {
    while clocks > 0 {
        let n = clocks.min(8);
        clocks -= n;

        shift_tdi(jtag, tdi, n as u8, tms);
    }
}

/// Shift out `clocks` (at most 8) bits of TDI data, with TMS set to the given value.
///
/// Returns the captured TDO data.
fn shift_tdi<DEPS>(jtag: &mut impl Jtag<DEPS>, tdi: u8, clocks: u8, tms: bool) -> u8 {
    let mut tdo = 0;
    jtag.sequence(
        SequenceInfo {
            n_bits: clocks as u8,
            capture: true,
            tms,
        },
        &[tdi],
        core::slice::from_mut(&mut tdo),
    );
    tdo
}

/// Send the bypass bits after data bits.
///
/// Bypass bits are used to skip over TAPs in the JTAG chain. The TDI value is 1, while TMS is
/// driven to stay in Shift-DR or Shift-IR until the last bit, where TMS is driven to 1 to exit
/// the shift state.
fn bypass_after_data<DEPS>(jtag: &mut impl Jtag<DEPS>, bypass_after: u16) {
    if bypass_after > 0 {
        if bypass_after > 1 {
            // Send the bypass bits after the DR.
            shift_repeated_tdi(jtag, 0xFF, bypass_after.saturating_sub(1), false);
        }

        // Send the last bypass bit with TMS = 1
        shift_repeated_tdi(jtag, 0xFF, 1, true);
    }
}

/// The result of a single DAP JTAG transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TransferResult {
    /// The transfer was successful and this variant contains the captured data.
    Ok(u32),
    /// The device returned a wait response.
    Wait,
    /// The device returned a fault response.
    Fault,
    /// The device returned data that does not match what the transfer expects.
    Mismatch, // TODO shouldn't be part of the public API.
}

impl TransferResult {
    pub(crate) fn status(&self) -> u8 {
        match self {
            TransferResult::Ok(_) => 0x1,
            TransferResult::Wait => 0x2,
            TransferResult::Fault => 0x8,
            TransferResult::Mismatch => 0x10,
        }
    }
}
