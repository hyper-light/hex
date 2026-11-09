//! The `hex.exe` Hex ships carries its own C runtime: a Windows `oci` box maps it in as its launcher, and
//! nanoserver has `ucrtbase.dll` but no Visual C++ runtime (jupiter, 2026-11-06: an `hex.exe` importing
//! `VCRUNTIME140.dll` couldn't load in the box, so the launcher ended without replying). The repository's
//! `.cargo/config.toml` builds Windows targets with `+crt-static`; this reads the built binary's PE import
//! table and refuses a dynamic Visual C++ runtime (native plan step 4, AC-T7.1.1).
#![cfg(target_os = "windows")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

fn u16_at(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

/// The DLL names in PE image `b`'s import directory (PE/COFF: the optional header's import data
/// directory, its descriptors ending at an all-zero one, each `Name` an RVA mapped through the sections).
fn imports(b: &[u8]) -> Vec<String> {
    let pe = u32_at(b, 0x3c);
    assert_eq!(&b[pe..pe + 4], b"PE\0\0", "not a PE image");
    let coff = pe + 4;
    let sections = u16_at(b, coff + 2);
    let optional = coff + 20;
    let optional_size = u16_at(b, coff + 16);
    assert_eq!(u16_at(b, optional), 0x20b, "not a PE32+ image");
    // PE32+: the data directories start at offset 112; the import directory is the second.
    let import_rva = u32_at(b, optional + 112 + 8);
    let table = optional + optional_size;
    let offset = |rva: usize| -> usize {
        (0..sections)
            .map(|i| table + i * 40)
            .find_map(|s| {
                let (va, size, raw) = (u32_at(b, s + 12), u32_at(b, s + 8), u32_at(b, s + 20));
                (rva >= va && rva < va + size).then(|| rva - va + raw)
            })
            .expect("an RVA outside every section")
    };
    let mut names = Vec::new();
    let mut d = offset(import_rva);
    while u32_at(b, d + 12) != 0 {
        let at = offset(u32_at(b, d + 12));
        let end = b[at..].iter().position(|&c| c == 0).unwrap();
        names.push(String::from_utf8_lossy(&b[at..at + end]).into_owned());
        d += 20;
    }
    names
}

/// Do: read the built `hex.exe`'s imports. Expect: kernel32 among them (the parser reads the table), and no
/// `VCRUNTIME*.dll` or `MSVCP*.dll`.
#[test]
fn hex_exe_imports_no_visual_cpp_runtime() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_hex")).unwrap();
    let names = imports(&exe);
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("kernel32.dll")),
        "{names:?}"
    );
    let dynamic: Vec<&String> = names
        .iter()
        .filter(|n| {
            let n = n.to_ascii_lowercase();
            n.starts_with("vcruntime") || n.starts_with("msvcp")
        })
        .collect();
    assert!(
        dynamic.is_empty(),
        "hex.exe imports {dynamic:?}, which nanoserver doesn't have"
    );
}
