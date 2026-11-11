// Copyright 2026 Hex contributors.
// SPDX-License-Identifier: Apache-2.0

//! PCI MSI capabilities routed through the existing KVM MSI vector implementation.
//! PCI Local Bus 3.0 section 6.8 and Linux pci_regs.h define the offsets/flags below.
//! Per-vector masking is optional: do not advertise it when emulating MSI through irqfds.

use crate::pci::msix::MsixTableEntry;

#[derive(Debug, thiserror::Error, displaydoc::Display)]
pub enum MsiError {
    /// Malformed PCI MSI capability or access.
    Invalid,
    /// MSI route storage allocation failed.
    Allocation,
}

#[derive(Debug)]
pub struct MsiConfig {
    offset: usize,
    // Largest PCI MSI capability: 64-bit address, data, mask and pending registers (24 bytes).
    bytes: [u8; 24],
    len: usize,
    vectors: u16,
}

impl MsiConfig {
    /// Parse the bounded legacy capability chain and its actual VFIO MSI vector ceiling.
    pub fn parse(space: &[u32], available: u32) -> Result<Option<Self>, MsiError> {
        if available == 0 {
            return Ok(None);
        }
        let mut pointer =
            usize::try_from(space.get(0x34 / 4).copied().ok_or(MsiError::Invalid)? & 0xff)
                .map_err(|_| MsiError::Invalid)?;
        // PCI's legacy capability region has at most 256 / 4 aligned entries.
        for _ in 0..(256 / 4) {
            if pointer == 0 {
                return Ok(None);
            }
            if pointer < 0x40 || pointer > 0xfc || pointer & 3 != 0 {
                return Err(MsiError::Invalid);
            }
            let header = *space.get(pointer / 4).ok_or(MsiError::Invalid)?;
            if header & 0xff == 0x05 {
                let control = u16::try_from(header >> 16).map_err(|_| MsiError::Invalid)?;
                let shift = u32::from((control >> 1) & 7);
                if shift > 5 {
                    return Err(MsiError::Invalid);
                }
                let hardware = 1u32.checked_shl(shift).ok_or(MsiError::Invalid)?;
                let room = hardware.min(available);
                let power = room.checked_ilog2().ok_or(MsiError::Invalid)?;
                let vectors = 1u16.checked_shl(power).ok_or(MsiError::Invalid)?;
                let wide = control & 0x80 != 0;
                let len = if wide { 14 } else { 10 };
                let mut bytes = [0u8; 24];
                for i in 0usize..len {
                    let at = pointer.checked_add(i).ok_or(MsiError::Invalid)?;
                    let word = space.get(at / 4).ok_or(MsiError::Invalid)?;
                    let shift = u32::try_from((at % 4).checked_mul(8).ok_or(MsiError::Invalid)?)
                        .map_err(|_| MsiError::Invalid)?;
                    *bytes.get_mut(i).ok_or(MsiError::Invalid)? =
                        u8::try_from((word >> shift) & 0xff).map_err(|_| MsiError::Invalid)?;
                }
                let mut config = Self {
                    offset: pointer,
                    bytes,
                    len,
                    vectors,
                };
                // Expose only IRQ vectors actually allocated and hide optional per-vector mask.
                config.set_control(
                    (control & 0x80) | (u16::try_from(power).map_err(|_| MsiError::Invalid)? << 1),
                )?;
                return Ok(Some(config));
            }
            pointer = usize::try_from((header >> 8) & 0xff).map_err(|_| MsiError::Invalid)?;
        }
        Err(MsiError::Invalid)
    }

    /// Maximum power-of-two vector count advertised to this guest.
    pub fn vectors(&self) -> u16 {
        self.vectors
    }

    /// Current guest-enabled vector count (zero when disabled), used for the VFIO hardware source.
    pub fn enabled_vectors(&self) -> Result<u16, MsiError> {
        let control = self.control()?;
        if control & 1 == 0 {
            return Ok(0);
        }
        let enabled = 1u16
            .checked_shl(u32::from((control >> 4) & 7))
            .ok_or(MsiError::Invalid)?;
        if enabled > self.vectors {
            return Err(MsiError::Invalid);
        }
        Ok(enabled)
    }

    fn control(&self) -> Result<u16, MsiError> {
        let bytes: [u8; 2] = self
            .bytes
            .get(2..4)
            .ok_or(MsiError::Invalid)?
            .try_into()
            .map_err(|_| MsiError::Invalid)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn set_control(&mut self, control: u16) -> Result<(), MsiError> {
        self.bytes
            .get_mut(2..4)
            .ok_or(MsiError::Invalid)?
            .copy_from_slice(&control.to_le_bytes());
        Ok(())
    }

    /// Emulate a capability dword read, leaving unrelated registers to the caller.
    pub fn read(&self, register: u16) -> Option<u32> {
        let at = usize::from(register)
            .checked_mul(4)?
            .checked_sub(self.offset)?;
        if at >= self.len {
            return None;
        }
        let end = at.checked_add(4)?;
        let bytes: [u8; 4] = self.bytes.get(at..end)?.try_into().ok()?;
        Some(u32::from_le_bytes(bytes))
    }

    /// Update guest-writable address/data/enable bits without writing host PCI configuration.
    pub fn write(&mut self, register: u16, offset: u8, data: &[u8]) -> Result<bool, MsiError> {
        let absolute = usize::from(register)
            .checked_mul(4)
            .ok_or(MsiError::Invalid)?;
        let Some(base) = absolute
            .checked_sub(self.offset)
            .filter(|at| *at < self.len)
        else {
            return Ok(false);
        };
        if usize::from(offset)
            .checked_add(data.len())
            .is_none_or(|end| end > 4)
        {
            return Err(MsiError::Invalid);
        }
        let at = base
            .checked_add(usize::from(offset))
            .ok_or(MsiError::Invalid)?;
        let old = self.control()?;
        for (index, byte) in data.iter().enumerate() {
            let target = at.checked_add(index).ok_or(MsiError::Invalid)?;
            if target >= 2 && target < self.len {
                *self.bytes.get_mut(target).ok_or(MsiError::Invalid)? = *byte;
            }
        }
        // Enable and multiple-message-enable are the only guest-writable control bits.
        let control = (old & !0x71) | (self.control()? & 0x71);
        let enabled = 1u16
            .checked_shl(u32::from((control >> 4) & 7))
            .ok_or(MsiError::Invalid)?;
        if enabled > self.vectors {
            self.set_control(old & !1)?;
            return Err(MsiError::Invalid);
        }
        self.set_control(control)?;
        Ok(true)
    }

    /// Build exactly the allocated KVM route set, masking vectors beyond the enabled count.
    pub fn routes(&self) -> Result<Vec<MsixTableEntry>, MsiError> {
        let control = self.control()?;
        let enabled = 1u16
            .checked_shl(u32::from((control >> 4) & 7))
            .ok_or(MsiError::Invalid)?;
        if enabled > self.vectors {
            return Err(MsiError::Invalid);
        }
        let word = |at: usize| -> Result<u32, MsiError> {
            let end = at.checked_add(4).ok_or(MsiError::Invalid)?;
            let bytes: [u8; 4] = self
                .bytes
                .get(at..end)
                .ok_or(MsiError::Invalid)?
                .try_into()
                .map_err(|_| MsiError::Invalid)?;
            Ok(u32::from_le_bytes(bytes))
        };
        let wide = control & 0x80 != 0;
        let data_offset: usize = if wide { 12 } else { 8 };
        let data: [u8; 2] = self
            .bytes
            .get(data_offset..data_offset.checked_add(2).ok_or(MsiError::Invalid)?)
            .ok_or(MsiError::Invalid)?
            .try_into()
            .map_err(|_| MsiError::Invalid)?;
        let data = u32::from(u16::from_le_bytes(data));
        let mask = u32::from(enabled.checked_sub(1).ok_or(MsiError::Invalid)?);
        let mut routes = Vec::new();
        routes
            .try_reserve_exact(usize::from(self.vectors))
            .map_err(|_| MsiError::Allocation)?;
        for vector in 0..self.vectors {
            routes.push(MsixTableEntry {
                msg_addr_lo: word(4)?,
                msg_addr_hi: if wide { word(8)? } else { 0 },
                msg_data: (data & !mask) | (u32::from(vector) & mask),
                vector_ctl: u32::from(control & 1 == 0 || vector >= enabled),
            });
        }
        Ok(routes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(wide: bool) -> [u32; 64] {
        let mut words = [0u32; 64];
        words[0x34 / 4] = 0x40;
        words[0x40 / 4] = 5 | (u32::from(if wide { 0x0184u16 } else { 0x0104u16 }) << 16);
        words
    }

    #[test]
    fn writes_program_exact_guest_msi_routes_and_enable_only_allocated_vectors() {
        for wide in [false, true] {
            let mut msi = MsiConfig::parse(&space(wide), 4).unwrap().unwrap();
            assert_eq!(msi.vectors(), 4);
            assert_eq!(msi.enabled_vectors().unwrap(), 0);
            assert!(msi.routes().unwrap().iter().all(|route| route.masked()));
            msi.write(0x44 / 4, 0, &0xfee0_0000u32.to_le_bytes())
                .unwrap();
            if wide {
                msi.write(0x48 / 4, 0, &1u32.to_le_bytes()).unwrap();
            }
            msi.write(
                if wide { 0x4c / 4 } else { 0x48 / 4 },
                0,
                &0x44u16.to_le_bytes(),
            )
            .unwrap();
            let control = if wide { 0xa5u16 } else { 0x25u16 };
            msi.write(0x40 / 4, 2, &control.to_le_bytes()).unwrap();
            assert_eq!(msi.enabled_vectors().unwrap(), 4);
            let routes = msi.routes().unwrap();
            assert_eq!(routes.len(), 4);
            for (index, route) in routes.iter().enumerate() {
                assert_eq!(route.msg_addr_lo, 0xfee0_0000);
                assert_eq!(route.msg_addr_hi, u32::from(wide));
                assert_eq!(route.msg_data, 0x44 | u32::try_from(index).unwrap());
                assert!(!route.masked());
            }
            assert_eq!(msi.read(0x40 / 4).unwrap() >> 16 & 0x100, 0);
        }
    }

    #[test]
    fn malformed_chain_or_excessive_message_enable_refuses() {
        let mut words = space(false);
        words[0x34 / 4] = 0xff;
        assert!(MsiConfig::parse(&words, 4).is_err());
        let mut msi = MsiConfig::parse(&space(false), 2).unwrap().unwrap();
        assert!(msi.write(0x40 / 4, 2, &0x31u16.to_le_bytes()).is_err());
        assert!(msi.routes().unwrap().iter().all(|route| route.masked()));
        assert!(MsiConfig::parse(&space(false), 0).unwrap().is_none());
    }
}
