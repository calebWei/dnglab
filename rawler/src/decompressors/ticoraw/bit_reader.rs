// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-production), src/decoders/nikon_he/
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE bitstream reader (MSB-first, big-endian).
//!
//! Pending bits are kept left-justified (MSB-aligned) in a 64-bit register: the
//! next bit to consume is bit 63. Refills pull 32-bit big-endian words and slide
//! them in below the existing pending bits. Reads past the end return
//! zero-padded bits.

pub struct BitReader<'a> {
  data: &'a [u8],
  /// Byte offset of the next word to refill from.
  cursor: usize,
  /// Pending bits, MSB-justified (valid bits occupy the top `bits_available`).
  register: u64,
  bits_available: i32,
}

impl<'a> BitReader<'a> {
  pub fn new(data: &'a [u8]) -> Self {
    Self { data, cursor: 0, register: 0, bits_available: 0 }
  }

  #[inline]
  fn remaining(&self) -> usize {
    self.data.len() - self.cursor
  }

  fn refill(&mut self) {
    let remaining = self.remaining();
    let word: u32;
    let added_bits: i32;
    if remaining >= 4 {
      word = (self.data[self.cursor] as u32) << 24
        | (self.data[self.cursor + 1] as u32) << 16
        | (self.data[self.cursor + 2] as u32) << 8
        | (self.data[self.cursor + 3] as u32);
      self.cursor += 4;
      added_bits = 32;
    } else if remaining > 0 {
      let mut w = 0u32;
      for i in 0..remaining {
        w = (w << 8) | self.data[self.cursor + i] as u32;
      }
      w <<= (4 - remaining) * 8;
      word = w;
      self.cursor = self.data.len();
      added_bits = (remaining * 8) as i32;
    } else {
      return; // exhausted
    }
    let shift = 32 - self.bits_available;
    self.register |= (word as u64) << shift;
    self.bits_available += added_bits;
  }

  /// Read `count` bits MSB-first. `count` must be in `[1, 32]`.
  pub fn read_bits(&mut self, count: i32) -> u32 {
    while self.bits_available < count {
      if self.cursor >= self.data.len() {
        let result = (self.register >> (64 - count)) as u32;
        self.register = 0;
        self.bits_available = 0;
        return result;
      }
      self.refill();
    }
    let result = (self.register >> (64 - count)) as u32;
    self.register <<= count;
    self.bits_available -= count;
    result
  }

  /// Read a unary code: count leading 1-bits, then consume the terminating 0.
  pub fn read_unary(&mut self) -> u32 {
    let mut count = 0u32;
    loop {
      if self.bits_available <= 0 {
        if self.cursor >= self.data.len() {
          return count; // unterminated run at EOF
        }
        self.refill();
      }
      let is_one = (self.register >> 63) & 1 != 0;
      self.register <<= 1;
      self.bits_available -= 1;
      if !is_one {
        return count;
      }
      count += 1;
    }
  }

  /// Discard a partial byte so the next read is byte-aligned.
  pub fn align_to_byte(&mut self) {
    let fractional = self.bits_available & 7;
    if fractional == 0 {
      return;
    }
    self.register <<= fractional;
    self.bits_available -= fractional;
  }

  /// Total whole bytes consumed (excluding fractional bits still in register).
  pub fn bytes_read(&self) -> usize {
    self.cursor - (self.bits_available as usize >> 3)
  }

  pub fn exhausted(&self) -> bool {
    self.cursor >= self.data.len()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_msb_first() {
    // 0b1010_0110, 0b1100_0000
    let data = [0xA6u8, 0xC0];
    let mut r = BitReader::new(&data);
    assert_eq!(r.read_bits(1), 1);
    assert_eq!(r.read_bits(2), 0b01);
    assert_eq!(r.read_bits(5), 0b00110);
    assert_eq!(r.read_bits(2), 0b11);
  }

  #[test]
  fn unary_then_bits() {
    // 1110 1... -> unary = 3 (consumes the 0), then next bit is 1
    let data = [0b1110_1000u8];
    let mut r = BitReader::new(&data);
    assert_eq!(r.read_unary(), 3);
    assert_eq!(r.read_bits(1), 1);
  }

  #[test]
  fn past_end_zero_pads() {
    let data = [0xFFu8];
    let mut r = BitReader::new(&data);
    assert_eq!(r.read_bits(8), 0xFF);
    assert_eq!(r.read_bits(8), 0x00); // zero-padded past EOF
  }

  #[test]
  fn align_to_byte_skips_partial() {
    let data = [0b1010_0000u8, 0x5A];
    let mut r = BitReader::new(&data);
    assert_eq!(r.read_bits(3), 0b101);
    r.align_to_byte();
    assert_eq!(r.read_bits(8), 0x5A);
  }
}
