// Copyright 2024 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::Path;

use acpi_tables::fadt::{FADT_F_HW_REDUCED_ACPI, FADT_F_PWR_BUTTON, FADT_F_SLP_BUTTON};
use acpi_tables::{Dsdt, Fadt, Madt, Mcfg, Rsdp, Sdt, SdtHeader, Xsdt, aml};
use vm_allocator::AllocPolicy;
use vm_memory::Bytes;
use zerocopy::IntoBytes;
use zerocopy::little_endian::U32;

use crate::Vcpu;
use crate::acpi::x86_64::{
    apic_addr, rsdp_addr, setup_arch_dsdt, setup_arch_fadt, setup_interrupt_controllers,
};
use crate::arch::x86_64::layout;
use crate::device_manager::DeviceManager;
use crate::logger::{debug, error};
use crate::pci::PciDevice;
use crate::vstate::memory::{GuestAddress, GuestMemoryMmap};
use crate::vstate::resources::ResourceAllocator;

mod x86_64;

// Our (Original Equipment Manufacturer" (OEM) name. OEM is how ACPI names the manufacturer of the
// hardware that is exposed to the OS, through ACPI tables. The OEM name is passed in every ACPI
// table, to let the OS know that we are the owner of the table.
const OEM_ID: [u8; 6] = *b"FIRECK";

// In reality the OEM revision is per table and it defines the revision of the OEM's implementation
// of the particular ACPI table. For our purpose, we can set it to a fixed value for all the tables
const OEM_REVISION: u32 = 0;

// This is needed for an entry in the FADT table. Populating this entry in FADT is a way to let the
// guest know that it runs within a Firecracker microVM.
const HYPERVISOR_VENDOR_ID: [u8; 8] = *b"FIRECKVM";

#[derive(Debug, thiserror::Error, displaydoc::Display)]
/// Error type for ACPI related operations
pub enum AcpiError {
    /// Could not allocate resources: {0}
    VmAllocator(#[from] vm_allocator::Error),
    /// ACPI tables error: {0}
    AcpiTables(#[from] acpi_tables::AcpiError),
    /// Error creating AML bytecode: {0}
    AmlError(#[from] aml::AmlError),
    /// Could not read the video BIOS image of passthrough device {0}: {1}
    VbiosImage(String, std::io::Error),
    /// The video BIOS image of passthrough device {0} exceeds the ACPI tables' room of {1} bytes
    VbiosSize(String, u64),
}

/// One passthrough function's video BIOS image, keyed as the guest finds that function: its guest
/// PCI address and the IDs its configuration space shows (Hex).
#[derive(Debug, Clone, PartialEq, Eq)]
struct VfctImage {
    bus: u32,
    device: u32,
    function: u32,
    vendor_id: u16,
    device_id: u16,
    subsystem_vendor_id: u16,
    subsystem_id: u16,
    revision: u32,
    image: Vec<u8>,
}

/// The VFCT table: how platform firmware publishes the video BIOS image of a GPU that has no ROM
/// of its own, an integrated one's (AMD's UEFI_ACPI_VFCT), which a guest's driver reads before any
/// ROM BAR (Linux's amdgpu_acpi_vfct_bios in drivers/gpu/drm/amd/amdgpu/amdgpu_bios.c, and
/// radeon's). Its fixed part follows the header: a 16-byte table UUID, the first image's offset
/// from the table's start, a GOP library's offset (none) and 16 reserved bytes; each image is then
/// a VFCT_IMAGE_HEADER (bus, device, function as u32; vendor, device, subsystem vendor and
/// subsystem IDs as u16; revision and image length as u32) and the image's bytes (Hex).
struct Vfct(Vec<u8>);

impl Vfct {
    /// The fixed part's end, where the first image starts: the 36-byte header, 16, 4, 4 and 16.
    const FIRST_IMAGE: u32 = 76;

    fn new(images: &[VfctImage], room: u64) -> Result<Self, AcpiError> {
        let header = SdtHeader {
            signature: *b"VFCT",
            length: U32::new(0),
            revision: 1,
            checksum: 0,
            oem_id: OEM_ID,
            oem_table_id: *b"FCVMVFCT",
            oem_revision: U32::new(OEM_REVISION),
            // The creator acpi_tables names in the tables it builds.
            creator_id: *b"FCAT",
            creator_revision: U32::new(0x2024_0119),
        };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&Self::FIRST_IMAGE.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        for image in images {
            let size = |bytes: usize| {
                u32::try_from(bytes)
                    .ok()
                    .filter(|_| u64::try_from(bytes).is_ok_and(|bytes| bytes <= room))
                    .ok_or_else(|| AcpiError::VbiosSize(image_name(image), room))
            };
            let length = size(image.image.len())?;
            for word in [image.bus, image.device, image.function] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            for half in [
                image.vendor_id,
                image.device_id,
                image.subsystem_vendor_id,
                image.subsystem_id,
            ] {
                bytes.extend_from_slice(&half.to_le_bytes());
            }
            bytes.extend_from_slice(&image.revision.to_le_bytes());
            bytes.extend_from_slice(&length.to_le_bytes());
            bytes.extend_from_slice(&image.image);
            size(bytes.len())?;
        }
        let length = u32::try_from(bytes.len())
            .map_err(|_| AcpiError::VbiosSize(String::from("VFCT"), room))?;
        bytes[4..8].copy_from_slice(&length.to_le_bytes());
        let sum = bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        bytes[9] = 0u8.wrapping_sub(sum);
        Ok(Self(bytes))
    }
}

/// An image's guest PCI address, as errors name it.
fn image_name(image: &VfctImage) -> String {
    format!(
        "{:02x}:{:02x}.{:x}",
        image.bus, image.device, image.function
    )
}

impl Sdt for Vfct {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn write_to_guest<M: vm_memory::GuestMemory>(
        &mut self,
        mem: &M,
        address: GuestAddress,
    ) -> acpi_tables::Result<()> {
        mem.write_slice(&self.0, address)?;
        Ok(())
    }
}

/// A passthrough device's video BIOS image, read whole, no larger than `room`.
fn read_vbios(id: &str, path: &Path, room: u64) -> Result<Vec<u8>, AcpiError> {
    let fail = |error| AcpiError::VbiosImage(id.to_string(), error);
    let file = std::fs::File::open(path).map_err(fail)?;
    let mut image = Vec::new();
    // One byte past the room tells an image that is too large from one that fills it.
    file.take(room.saturating_add(1))
        .read_to_end(&mut image)
        .map_err(fail)?;
    if !u64::try_from(image.len()).is_ok_and(|size| size <= room) {
        return Err(AcpiError::VbiosSize(id.to_string(), room));
    }
    Ok(image)
}

/// The VFCT images of the passthrough devices configured with one, each keyed to the device's
/// guest PCI address and the IDs the guest reads from its configuration space.
fn vfct_images(device_manager: &DeviceManager, room: u64) -> Result<Vec<VfctImage>, AcpiError> {
    let Some(pci_devices) = device_manager.pci_devices() else {
        return Ok(Vec::new());
    };
    let mut images = Vec::new();
    for device in pci_devices.vfio_devices.iter() {
        let mut device = device.lock().expect("Poisoned lock");
        let Some(path) = device.config.vbios_path.clone() else {
            continue;
        };
        let image = read_vbios(&device.config.id, &path, room)?;
        // Registers 0, 2 and 11: vendor and device IDs; revision (with the class); subsystem
        // vendor and subsystem IDs (PCI Local Bus Specification, the type 0 header).
        let ids = device.read_config_register(0);
        let class = device.read_config_register(2);
        let subsystem = device.read_config_register(11);
        let [vendor_low, vendor_high, device_low, device_high] = ids.to_le_bytes();
        let [svid_low, svid_high, ssid_low, ssid_high] = subsystem.to_le_bytes();
        images.push(VfctImage {
            bus: u32::from(device.sbdf.bus()),
            device: u32::from(device.sbdf.device()),
            function: u32::from(device.sbdf.function()),
            vendor_id: u16::from_le_bytes([vendor_low, vendor_high]),
            device_id: u16::from_le_bytes([device_low, device_high]),
            subsystem_vendor_id: u16::from_le_bytes([svid_low, svid_high]),
            subsystem_id: u16::from_le_bytes([ssid_low, ssid_high]),
            revision: class & 0xff,
            image,
        });
    }
    Ok(images)
}

/// Helper type that holds the guest memory in which we write the tables in and a resource
/// allocator for allocating space for the tables
struct AcpiTableWriter<'a> {
    mem: &'a GuestMemoryMmap,
}

impl AcpiTableWriter<'_> {
    /// Write a table in guest memory
    ///
    /// This will allocate enough space inside guest memory and write the table in the allocated
    /// buffer. It returns the address in which it wrote the table.
    fn write_acpi_table<S>(
        &mut self,
        resource_allocator: &mut ResourceAllocator,
        table: &mut S,
    ) -> Result<u64, AcpiError>
    where
        S: Sdt,
    {
        let addr = resource_allocator
            .system_memory
            .allocate(table.len().try_into().unwrap(), 1, AllocPolicy::FirstMatch)?
            .start();

        table
            .write_to_guest(self.mem, GuestAddress(addr))
            .inspect_err(|err| error!("acpi: Could not write table in guest memory: {err}"))?;

        debug!(
            "acpi: Wrote table ({} bytes) at address: {:#010x}",
            table.len(),
            addr
        );

        Ok(addr)
    }

    /// Build the DSDT table for the guest
    fn build_dsdt(
        &mut self,
        device_manager: &mut DeviceManager,
        resource_allocator: &mut ResourceAllocator,
    ) -> Result<u64, AcpiError> {
        let mut dsdt_data = Vec::new();

        // Device DSDT data.
        device_manager.append_aml_bytes(&mut dsdt_data)?;

        // Architecture specific DSDT data
        setup_arch_dsdt(&mut dsdt_data)?;

        let mut dsdt = Dsdt::new(OEM_ID, *b"FCVMDSDT", OEM_REVISION, dsdt_data);
        self.write_acpi_table(resource_allocator, &mut dsdt)
    }

    /// Build the FADT table for the guest
    ///
    /// This includes a pointer with the location of the DSDT in guest memory
    fn build_fadt(
        &mut self,
        resource_allocator: &mut ResourceAllocator,
        dsdt_addr: u64,
    ) -> Result<u64, AcpiError> {
        let mut fadt = Fadt::new(OEM_ID, *b"FCVMFADT", OEM_REVISION);
        fadt.set_hypervisor_vendor_id(HYPERVISOR_VENDOR_ID);
        fadt.set_x_dsdt(dsdt_addr);
        fadt.set_flags(
            (1 << FADT_F_HW_REDUCED_ACPI) | (1 << FADT_F_PWR_BUTTON) | (1 << FADT_F_SLP_BUTTON),
        );
        setup_arch_fadt(&mut fadt);
        self.write_acpi_table(resource_allocator, &mut fadt)
    }

    /// Build the MADT table for the guest
    ///
    /// This includes information about the interrupt controllers supported in the platform
    fn build_madt(
        &mut self,
        resource_allocator: &mut ResourceAllocator,
        nr_vcpus: u8,
    ) -> Result<u64, AcpiError> {
        let mut madt = Madt::new(
            OEM_ID,
            *b"FCVMMADT",
            OEM_REVISION,
            apic_addr(),
            setup_interrupt_controllers(nr_vcpus),
        );
        self.write_acpi_table(resource_allocator, &mut madt)
    }

    /// Build the XSDT table for the guest
    ///
    /// It points the guest to the FADT, MADT and MCFG tables, and to a VFCT table where one was
    /// built.
    fn build_xsdt(
        &mut self,
        resource_allocator: &mut ResourceAllocator,
        tables: Vec<u64>,
    ) -> Result<u64, AcpiError> {
        let mut xsdt = Xsdt::new(OEM_ID, *b"FCMVXSDT", OEM_REVISION, tables);
        self.write_acpi_table(resource_allocator, &mut xsdt)
    }

    /// Build the VFCT table for the guest's passthrough devices with a video BIOS image.
    fn build_vfct(
        &mut self,
        resource_allocator: &mut ResourceAllocator,
        images: &[VfctImage],
    ) -> Result<u64, AcpiError> {
        let mut vfct = Vfct::new(images, layout::SYSTEM_MEM_SIZE)?;
        self.write_acpi_table(resource_allocator, &mut vfct)
    }

    /// Build the MCFG table for the guest.
    fn build_mcfg(
        &mut self,
        resource_allocator: &mut ResourceAllocator,
        pci_mmio_config_addr: u64,
        end_bus: u8,
    ) -> Result<u64, AcpiError> {
        let mut mcfg = Mcfg::new(
            OEM_ID,
            *b"FCMVMCFG",
            OEM_REVISION,
            pci_mmio_config_addr,
            end_bus,
        );
        self.write_acpi_table(resource_allocator, &mut mcfg)
    }

    /// Build the RSDP pointer for the guest.
    ///
    /// This will build the RSDP pointer which points to the XSDT table and write it in guest
    /// memory. The address in which we write RSDP is pre-determined for every architecture.
    /// We will not allocate arbitrary memory for it
    fn build_rsdp(&mut self, xsdt_addr: u64) -> Result<(), AcpiError> {
        let mut rsdp = Rsdp::new(OEM_ID, xsdt_addr);
        rsdp.write_to_guest(self.mem, rsdp_addr())
            .inspect_err(|err| error!("acpi: Could not write RSDP in guest memory: {err}"))?;

        debug!(
            "acpi: Wrote RSDP ({} bytes) at address: {:#010x}",
            rsdp.len(),
            rsdp_addr().0
        );
        Ok(())
    }
}

/// Create ACPI tables for the guest
///
/// This will create the ACPI tables needed to describe to the guest OS the available hardware,
/// such as interrupt controllers, vCPUs and VirtIO devices.
pub(crate) fn create_acpi_tables(
    mem: &GuestMemoryMmap,
    device_manager: &mut DeviceManager,
    resource_allocator: &mut ResourceAllocator,
    vcpus: &[Vcpu],
) -> Result<(), AcpiError> {
    let last_bus = device_manager
        .pci_devices()
        .map_or(0, |pci_devices| pci_devices.pci_segment().last_bus_number());

    let mut writer = AcpiTableWriter { mem };
    let dsdt_addr = writer.build_dsdt(device_manager, resource_allocator)?;

    let fadt_addr = writer.build_fadt(resource_allocator, dsdt_addr)?;
    let madt_addr = writer.build_madt(resource_allocator, vcpus.len().try_into().unwrap())?;
    let mcfg_addr = writer.build_mcfg(resource_allocator, layout::PCI_MMCONFIG_START, last_bus)?;
    let mut tables = vec![fadt_addr, madt_addr, mcfg_addr];
    let images = vfct_images(device_manager, layout::SYSTEM_MEM_SIZE)?;
    if !images.is_empty() {
        tables.push(writer.build_vfct(resource_allocator, &images)?);
    }
    let xsdt_addr = writer.build_xsdt(resource_allocator, tables)?;
    writer.build_rsdp(xsdt_addr)
}

#[cfg(test)]
mod tests {
    use acpi_tables::Sdt;
    use vm_memory::Bytes;

    use crate::acpi::{AcpiError, AcpiTableWriter, Vfct, VfctImage, read_vbios};
    use crate::arch::x86_64::layout::{SYSTEM_MEM_SIZE, SYSTEM_MEM_START};
    use crate::builder::tests::default_vmm;
    use crate::utils::u64_to_usize;
    use crate::vstate::resources::ResourceAllocator;
    use crate::vstate::vm::tests::setup_vm_with_memory;

    struct MockSdt(Vec<u8>);

    impl Sdt for MockSdt {
        fn len(&self) -> usize {
            self.0.len()
        }

        fn write_to_guest<M: vm_memory::GuestMemory>(
            &mut self,
            mem: &M,
            address: vm_memory::GuestAddress,
        ) -> acpi_tables::Result<()> {
            mem.write_slice(&self.0, address)?;
            Ok(())
        }
    }

    // Currently we are allocating up to SYSTEM_MEM_SIZE memory for ACPI tables. We are allocating
    // using the FirstMatch policy, with an 1 byte alignment. This test checks that we are able to
    // allocate up to this size, and get back the expected addresses.
    #[test]
    fn test_write_acpi_table_memory_allocation() {
        // A mocke Vmm object with 128MBs of memory
        let vmm = default_vmm();
        let mut writer = AcpiTableWriter {
            mem: vmm.vm.as_kvm().unwrap().guest_memory(),
        };
        let mut resource_allocator = vmm.vm.as_kvm().unwrap().resource_allocator();

        // This should succeed
        let mut sdt = MockSdt(vec![0; 4096]);
        let addr = writer
            .write_acpi_table(&mut resource_allocator, &mut sdt)
            .unwrap();
        assert_eq!(addr, SYSTEM_MEM_START);

        // Let's try to write two 4K pages plus one byte
        let mut sdt = MockSdt(vec![0; usize::try_from(SYSTEM_MEM_SIZE + 1).unwrap()]);
        let err = writer
            .write_acpi_table(&mut resource_allocator, &mut sdt)
            .unwrap_err();
        assert!(
            matches!(
                err,
                AcpiError::VmAllocator(vm_allocator::Error::ResourceNotAvailable)
            ),
            "{:?}",
            err
        );

        // We are allocating memory for tables with alignment of 1 byte. All of these should
        // succeed.
        let mut sdt = MockSdt(vec![0; 5]);
        let addr = writer
            .write_acpi_table(&mut resource_allocator, &mut sdt)
            .unwrap();
        assert_eq!(addr, SYSTEM_MEM_START + 4096);
        let mut sdt = MockSdt(vec![0; 2]);
        let addr = writer
            .write_acpi_table(&mut resource_allocator, &mut sdt)
            .unwrap();
        assert_eq!(addr, SYSTEM_MEM_START + 4101);
        let mut sdt = MockSdt(vec![0; 4]);
        let addr = writer
            .write_acpi_table(&mut resource_allocator, &mut sdt)
            .unwrap();
        assert_eq!(addr, SYSTEM_MEM_START + 4103);
        let mut sdt = MockSdt(vec![0; 8]);
        let addr = writer
            .write_acpi_table(&mut resource_allocator, &mut sdt)
            .unwrap();
        assert_eq!(addr, SYSTEM_MEM_START + 4107);
        let mut sdt = MockSdt(vec![0; 16]);
        let addr = writer
            .write_acpi_table(&mut resource_allocator, &mut sdt)
            .unwrap();
        assert_eq!(addr, SYSTEM_MEM_START + 4115);
    }

    // If, for whatever weird reason, we end up with microVM that has less memory than the maximum
    // address we allocate for ACPI tables, we would be able to allocate the tables but we would
    // not be able to write them. This is practically impossible in our case. If we get such a
    // guest memory, we won't be able to load the guest kernel, but the function does
    // return an error on this case, so let's just check that in case any of these assumptions
    // change in the future.
    #[test]
    fn test_write_acpi_table_small_memory() {
        let vm = setup_vm_with_memory(u64_to_usize(SYSTEM_MEM_START + SYSTEM_MEM_SIZE - 4096));
        let mut writer = AcpiTableWriter {
            mem: vm.guest_memory(),
        };
        let mut resource_allocator = ResourceAllocator::new();

        let mut sdt = MockSdt(vec![0; usize::try_from(SYSTEM_MEM_SIZE).unwrap()]);
        let err = writer
            .write_acpi_table(&mut resource_allocator, &mut sdt)
            .unwrap_err();
        assert!(
            matches!(
                err,
                AcpiError::AcpiTables(acpi_tables::AcpiError::GuestMemory(
                    vm_memory::GuestMemoryError::PartialBuffer {
                        expected: 263168,  // SYSTEM_MEM_SIZE
                        completed: 259072  // SYSTEM_MEM_SIZE - 4096
                    },
                ))
            ),
            "{:?}",
            err
        );
    }

    fn image(device: u32, vendor_id: u16, device_id: u16, image: &[u8]) -> VfctImage {
        VfctImage {
            bus: 0,
            device,
            function: 0,
            vendor_id,
            device_id,
            subsystem_vendor_id: 0x1043,
            subsystem_id: 0x8877,
            revision: 0xe5,
            image: image.to_vec(),
        }
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    fn u16_at(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
    }

    // Hex: a passthrough GPU's video BIOS image is where the guest's amdgpu looks for it. Build a
    // VFCT of two functions' images; read it as amdgpu_acpi_vfct_bios does (Linux v6.18,
    // drivers/gpu/drm/amd/amdgpu/amdgpu_bios.c): from the first image's offset, each image header
    // then its bytes, to the table's length, matching bus, device, function, vendor and device IDs.
    // Expect each function's own image, the header's length and a checksum that sums to zero.
    #[test]
    fn test_vfct_publishes_each_image_where_the_guest_driver_reads_it() {
        let images = [
            image(2, 0x1002, 0x13c0, &[0x55, 0xaa, 1, 2, 3]),
            image(3, 0x10de, 0x2684, &[0x55, 0xaa, 9]),
        ];
        let vfct = Vfct::new(&images, 1 << 20).unwrap();
        let bytes = vfct.0.clone();
        assert_eq!(&bytes[0..4], b"VFCT");
        assert_eq!(u32_at(&bytes, 4) as usize, bytes.len());
        assert_eq!(vfct.len(), bytes.len());
        assert_eq!(bytes.iter().fold(0u8, |sum, b| sum.wrapping_add(*b)), 0);
        assert_eq!(u32_at(&bytes, 52), 76);
        assert_eq!(u32_at(&bytes, 56), 0);
        for want in &images {
            let mut offset = u32_at(&bytes, 52) as usize;
            let mut found = None;
            while offset < bytes.len() {
                let header = offset;
                offset += 28;
                assert!(offset <= bytes.len(), "image header truncated");
                let length = u32_at(&bytes, header + 24) as usize;
                offset += length;
                assert!(offset <= bytes.len(), "image truncated");
                if length != 0
                    && u32_at(&bytes, header) == want.bus
                    && u32_at(&bytes, header + 4) == want.device
                    && u32_at(&bytes, header + 8) == want.function
                    && u16_at(&bytes, header + 12) == want.vendor_id
                    && u16_at(&bytes, header + 14) == want.device_id
                {
                    assert_eq!(u16_at(&bytes, header + 16), want.subsystem_vendor_id);
                    assert_eq!(u16_at(&bytes, header + 18), want.subsystem_id);
                    assert_eq!(u32_at(&bytes, header + 20), want.revision);
                    found = Some(bytes[header + 28..offset].to_vec());
                    break;
                }
            }
            assert_eq!(found.as_deref(), Some(want.image.as_slice()));
        }
    }

    // Hex: an image larger than the ACPI tables' room is refused by size, and read whole up to it.
    #[test]
    fn test_vbios_images_are_bounded_by_the_acpi_tables_room() {
        assert!(matches!(
            Vfct::new(&[image(2, 0x1002, 0x13c0, &[0; 9])], 8),
            Err(AcpiError::VbiosSize(name, 8)) if name == "00:02.0"
        ));
        let file = vmm_sys_util::tempfile::TempFile::new().unwrap();
        std::fs::write(file.as_path(), [0x55, 0xaa, 7, 7]).unwrap();
        assert_eq!(
            read_vbios("gpu0", file.as_path(), 4).unwrap(),
            vec![0x55, 0xaa, 7, 7]
        );
        assert!(matches!(
            read_vbios("gpu0", file.as_path(), 3),
            Err(AcpiError::VbiosSize(id, 3)) if id == "gpu0"
        ));
        assert!(matches!(
            read_vbios("gpu0", std::path::Path::new("/nonexistent/vbios.rom"), 4),
            Err(AcpiError::VbiosImage(id, _)) if id == "gpu0"
        ));
    }
}
