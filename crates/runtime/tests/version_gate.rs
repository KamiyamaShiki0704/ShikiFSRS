use sha2::{Digest, Sha256};
use shikifsrs_runtime::code_patches::{CodePatch, OwnedCodePatches, PreparedCodePatches};
use shikifsrs_runtime::{
    Error,
    memory::{AddressSpace, SliceMemory},
    profile::{CodeRange, ExecutableIdentity, OwnedVerifiedFile, VerifiedFile},
};
use std::cell::Cell;
use std::sync::Arc;

#[test]
fn registry_selects_an_exact_identity_and_retains_pe_validation() {
    let bytes = fixture();
    let exact = identity(&bytes);
    let wrong_size = ExecutableIdentity {
        file_size: bytes.len() + 1,
        ..exact
    };
    let mut wrong_hash = exact;
    wrong_hash.sha256[0] ^= 1;
    for registry in [
        [wrong_size, exact, wrong_hash],
        [wrong_hash, wrong_size, exact],
    ] {
        let selected = VerifiedFile::identify(&bytes, &registry).unwrap();
        assert_eq!(selected.identity().sha256, exact.sha256);
        assert_eq!(selected.identity().file_size, exact.file_size);
    }
    assert!(matches!(
        VerifiedFile::identify(&bytes, &[wrong_hash, wrong_size]),
        Err(Error::UnsupportedImage { .. })
    ));
    let mut damaged_pe = bytes.clone();
    damaged_pe[..2].fill(0);
    assert!(matches!(
        VerifiedFile::identify(&damaged_pe, &[identity(&damaged_pe)]),
        Err(Error::InvalidPe(_))
    ));
}

#[test]
fn empty_and_ambiguous_identity_registries_are_rejected() {
    let bytes = fixture();
    let exact = identity(&bytes);
    assert!(matches!(
        VerifiedFile::identify(&bytes, &[]),
        Err(Error::InvalidProfile(_))
    ));
    let alias = ExecutableIdentity {
        name: "another game",
        ..exact
    };
    assert!(matches!(
        VerifiedFile::identify(&bytes, &[exact, alias]),
        Err(Error::InvalidProfile(_))
    ));
}

fn patch(rva: u32, replacement: &[u8]) -> CodePatch {
    CodePatch {
        evidence: CodeRange {
            rva: 0x1000,
            length: 0x200,
        },
        rva,
        replacement: replacement.to_vec(),
    }
}

fn owned(file: &VerifiedFile<'_>, original: &[u8], patches: &[CodePatch]) -> OwnedCodePatches {
    let memory = SliceMemory::new(0x100000, original).unwrap();
    let prepared = PreparedCodePatches::prepare(file, &memory, 0x100000, patches).unwrap();
    // SAFETY: Synthetic owned storage only; no bytes are installed or ever called as code.
    unsafe { prepared.into_owned() }
}

#[test]
fn delayed_hook_declaration_keeps_original_gate_and_exact_future_replacement() {
    use shikifsrs_runtime::code_patches::{CodePatchSite, PreparedCodeSites};
    let bytes = fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let original = loaded(&bytes);
    let site = CodePatchSite {
        evidence: CODE,
        rva: 0x1000,
        length: 5,
    };
    let memory = SliceMemory::new(0x100000, &original).unwrap();
    let prepared = PreparedCodeSites::prepare(&file, &memory, 0x100000, &[site]).unwrap();
    let prepared = prepared
        .declare_replacements(&[(site.rva, vec![0xe9, 1, 2, 3, 4])])
        .unwrap();
    // SAFETY: Original owned synthetic buffers; no executable hook or native invocation.
    let manifest = unsafe { prepared.into_owned() };
    let mut installed = original.clone();
    installed[0x1000..0x1005].copy_from_slice(&[0xe9, 1, 2, 3, 4]);
    let memory = SliceMemory::new(0x100000, &installed).unwrap();
    assert!(
        file.bind_with_patches(&memory, 0x100000, &[CODE], Some(&manifest))
            .is_ok()
    );
    installed[0x1004] = 0xcc;
    let memory = SliceMemory::new(0x100000, &installed).unwrap();
    assert!(
        file.bind_with_patches(&memory, 0x100000, &[CODE], Some(&manifest))
            .is_err()
    );
    assert!(PreparedCodeSites::prepare(&file, &memory, 0x100000, &[site]).is_err());
}

#[test]
fn delayed_hook_declaration_rejects_missing_duplicate_unprepared_or_wrong_length_sites() {
    use shikifsrs_runtime::code_patches::{CodePatchSite, PreparedCodeSites};
    let bytes = fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let original = loaded(&bytes);
    let memory = SliceMemory::new(0x100000, &original).unwrap();
    let sites = [
        CodePatchSite {
            evidence: CODE,
            rva: 0x1000,
            length: 5,
        },
        CodePatchSite {
            evidence: CodeRange {
                rva: 0x1000,
                length: 0x200,
            },
            rva: 0x1010,
            length: 2,
        },
    ];
    for declarations in [
        vec![(0x1000, vec![0xe9; 5])],
        vec![(0x1000, vec![0xe9; 5]), (0x1000, vec![0xcc; 5])],
        vec![(0x1000, vec![0xe9; 4]), (0x1010, vec![0xcc; 2])],
        vec![(0x1000, vec![0xe9; 5]), (0x1020, vec![0xcc; 2])],
        vec![
            (0x1000, original[0x1000..0x1005].to_vec()),
            (0x1010, vec![0xcc; 2]),
        ],
    ] {
        let prepared = PreparedCodeSites::prepare(&file, &memory, 0x100000, &sites).unwrap();
        assert!(prepared.declare_replacements(&declarations).is_err());
    }
}

#[test]
fn owned_patches_accept_only_original_or_complete_replacement_and_default_stays_strict() {
    let bytes = fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let original = loaded(&bytes);
    let patches = owned(&file, &original, &[patch(0x1000, &[0xe9, 1, 2, 3, 4])]);
    for replacement in [false, true] {
        let mut image = original.clone();
        if replacement {
            image[0x1000..0x1005].copy_from_slice(&[0xe9, 1, 2, 3, 4]);
        }
        let memory = SliceMemory::new(0x100000, &image).unwrap();
        assert_eq!(file.bind(&memory, 0x100000, &[CODE]).is_ok(), !replacement);
        let module = file
            .bind_with_patches(&memory, 0x100000, &[CODE], Some(&patches))
            .unwrap();
        assert_eq!(module.code_address(CODE), Ok(0x101000));
        // The underlying memory seam still exposes actual installed bytes.
        assert_eq!(
            module.memory().read_u8(0x101000).unwrap(),
            if replacement { 0xe9 } else { 0x48 }
        );
    }
}

#[test]
fn patch_lookup_checks_the_full_replacement_outside_the_requested_intersection() {
    let bytes = fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let original = loaded(&bytes);
    let patches = owned(&file, &original, &[patch(0x1000, &[0xe9, 1, 2, 3, 4])]);
    for (at, value) in [(0x1004, 0x90), (0x1000, 0xe9), (0x1010, 0xcc)] {
        let mut image = original.clone();
        image[at] = value;
        let memory = SliceMemory::new(0x100000, &image).unwrap();
        assert!(
            file.bind_with_patches(
                &memory,
                0x100000,
                &[
                    CodeRange {
                        rva: 0x1000,
                        length: 1
                    },
                    CodeRange {
                        rva: 0x1010,
                        length: 1
                    }
                ],
                Some(&patches)
            )
            .is_err()
        );
    }
}

#[test]
fn manifests_reject_wrong_base_file_headers_and_original_preparation_after_patching() {
    let bytes = fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let original = loaded(&bytes);
    let request = patch(0x1000, &[0xe9, 1, 2, 3, 4]);
    let patches = owned(&file, &original, std::slice::from_ref(&request));
    let memory = SliceMemory::new(0x200000, &original).unwrap();
    assert!(
        file.bind_with_patches(&memory, 0x200000, &[CODE], Some(&patches))
            .is_err()
    );
    let mut other = bytes.clone();
    other[0x500] = 1;
    let other_file = VerifiedFile::verify(&other, identity(&other)).unwrap();
    let image = loaded(&other);
    let memory = SliceMemory::new(0x100000, &image).unwrap();
    assert!(
        other_file
            .bind_with_patches(&memory, 0x100000, &[CODE], Some(&patches))
            .is_err()
    );
    for at in [0x90, 0x1000, 0x11ff] {
        let mut image = original.clone();
        image[at] ^= 1;
        let memory = SliceMemory::new(0x100000, &image).unwrap();
        assert!(
            PreparedCodePatches::prepare(&file, &memory, 0x100000, std::slice::from_ref(&request))
                .is_err()
        );
        if at == 0x90 {
            assert!(
                file.bind_with_patches(&memory, 0x100000, &[CODE], Some(&patches))
                    .is_err()
            );
        }
    }
}

#[test]
fn patch_preparation_bounds_count_span_evidence_and_rejects_overlap_data_and_noops() {
    let bytes = fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let image = loaded(&bytes);
    let memory = SliceMemory::new(0x100000, &image).unwrap();
    for requests in [
        vec![],
        vec![patch(0x1000, &[])],
        vec![patch(0x1000, &[0x48])],
        vec![patch(0x1000, &[0xcc; 4097])],
        vec![patch(0xfff, &[0xcc])],
        vec![patch(0x11ff, &[0xcc, 0xcc])],
        vec![patch(0x2000, &[0xcc])],
        vec![patch(0x1000, &[0xcc; 5]), patch(0x1004, &[0x90])],
        vec![patch(0x1000, &[0xcc]); 65],
        vec![CodePatch {
            evidence: CodeRange {
                rva: 0x1000,
                length: 1024 * 1024 + 1,
            },
            ..patch(0x1000, &[0xcc])
        }],
        vec![CodePatch {
            evidence: CodeRange {
                rva: 0x2000,
                length: 1,
            },
            ..patch(0x2000, &[0xcc])
        }],
    ] {
        assert!(PreparedCodePatches::prepare(&file, &memory, 0x100000, &requests).is_err());
    }
    // Adjacent declarations are allowed and sorted independent of caller order.
    assert!(
        PreparedCodePatches::prepare(
            &file,
            &memory,
            0x100000,
            &[patch(0x1002, &[0xcc; 2]), patch(0x1000, &[0xcc; 2])]
        )
        .is_ok()
    );
}

#[test]
fn every_declared_patch_is_checked_at_binding_even_outside_requested_code() {
    let bytes = fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let original = loaded(&bytes);
    let patches = owned(
        &file,
        &original,
        &[patch(0x1000, &[0xcc; 5]), patch(0x1010, &[0x90; 5])],
    );
    let mut image = original.clone();
    image[0x1014] = 0x90;
    let memory = SliceMemory::new(0x100000, &image).unwrap();
    assert!(
        file.bind_with_patches(&memory, 0x100000, &[CODE], Some(&patches))
            .is_err()
    );
}

#[test]
fn owned_hooks_do_not_authorize_virtual_slot_changes() {
    let bytes = virtual_slot_fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let mut original = loaded(&bytes);
    original[0x2050..0x2058].copy_from_slice(&0x101000u64.to_le_bytes());
    let patches = owned(&file, &original, &[patch(0x1000, &[0xcc; 5])]);
    let mut image = original.clone();
    image[0x1000..0x1005].fill(0xcc);
    let memory = SliceMemory::new(0x100000, &image).unwrap();
    let module = file
        .bind_with_patches(&memory, 0x100000, &[CODE], Some(&patches))
        .unwrap();
    assert!(module.checked_virtual_function(0x102000, 0x50).is_ok());
    image[0x2050..0x2058].copy_from_slice(&0x101010u64.to_le_bytes());
    let memory = SliceMemory::new(0x100000, &image).unwrap();
    let module = file
        .bind_with_patches(&memory, 0x100000, &[CODE], Some(&patches))
        .unwrap();
    assert!(module.checked_virtual_function(0x102000, 0x50).is_err());
}

#[test]
fn full_patch_changes_between_validation_reads_are_rejected() {
    struct Changing {
        bytes: Vec<u8>,
        reads: Cell<usize>,
    }
    impl AddressSpace for Changing {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> shikifsrs_runtime::Result<()> {
            SliceMemory::new(0x100000, &self.bytes)?.read_exact(address, output)?;
            if address == 0x101000 && output.len() == 5 {
                let count = self.reads.get();
                self.reads.set(count + 1);
                if count >= 3 {
                    output.copy_from_slice(&[0xcc; 5]);
                }
            }
            Ok(())
        }
    }
    let bytes = fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let original = loaded(&bytes);
    let patches = owned(&file, &original, &[patch(0x1000, &[0xcc; 5])]);
    let memory = Changing {
        bytes: original,
        reads: Cell::new(0),
    };
    assert!(
        file.bind_with_patches(&memory, 0x100000, &[CODE], Some(&patches))
            .is_err()
    );
}

#[test]
fn code_patch_comparison_covers_replacements_across_read_chunk_boundaries() {
    let mut bytes = fixture();
    bytes.resize(0x2800, 0);
    for (at, value) in [
        (0xd0, 0x5000u32),
        (0x190, 0x2200),
        (0x198, 0x2200),
        (0x1bc, 0x4000),
        (0x1c4, 0x2400),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let mut original = vec![0; 0x5000];
    original[..0x200].copy_from_slice(&bytes[..0x200]);
    original[0x1000..0x3200].copy_from_slice(&bytes[0x200..0x2400]);
    original[0x4000..0x4400].copy_from_slice(&bytes[0x2400..0x2800]);
    let full = CodeRange {
        rva: 0x1000,
        length: 0x2200,
    };
    let requests = [CodePatch {
        evidence: full,
        rva: 0x1ffe,
        replacement: vec![0xcc; 8],
    }];
    let patches = owned(&file, &original, &requests);
    let mut image = original.clone();
    image[0x1ffe..0x2006].fill(0xcc);
    let memory = SliceMemory::new(0x100000, &image).unwrap();
    assert!(
        file.bind_with_patches(&memory, 0x100000, &[full], Some(&patches))
            .is_ok()
    );
    image[0x2005] = 0x90;
    let memory = SliceMemory::new(0x100000, &image).unwrap();
    assert!(
        file.bind_with_patches(&memory, 0x100000, &[full], Some(&patches))
            .is_err()
    );
}

fn exception_fixture() -> Vec<u8> {
    let mut bytes = fixture();
    bytes[0x104..0x108].copy_from_slice(&16u32.to_le_bytes());
    bytes[0x120..0x124].copy_from_slice(&0x2100u32.to_le_bytes());
    bytes[0x124..0x128].copy_from_slice(&24u32.to_le_bytes());
    for (at, begin, end) in [(0x500, 0x1000u32, 0x1005u32), (0x50c, 0x1010, 0x1018)] {
        bytes[at..at + 4].copy_from_slice(&begin.to_le_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&end.to_le_bytes());
    }
    bytes
}

#[test]
fn owned_verified_input_lends_views_and_rejects_wrong_identity() {
    let bytes = fixture();
    let expected = identity(&bytes);
    let owned = OwnedVerifiedFile::verify(Arc::from(bytes), expected).unwrap();
    for _ in 0..2 {
        let view = owned.borrow().unwrap();
        assert_eq!(view.identity().sha256, expected.sha256);
        assert_eq!(
            view.image().read_rva(0x1000, 5).unwrap(),
            &[0x48, 0x8b, 0x41, 0x08, 0xc3]
        );
    }
    assert!(OwnedVerifiedFile::verify(Arc::from(b"unknown file".as_slice()), expected).is_err());
}

#[test]
fn runtime_function_requires_an_exact_entry_and_reports_only_its_fragment() {
    let bytes = exception_fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let range = file.image().runtime_function(0x1000).unwrap();
    assert_eq!((range.rva, range.length), (0x1000, 5));
    assert!(file.image().runtime_function(0x1001).is_err());
    assert!(file.image().runtime_function(0x1020).is_err());
}

#[test]
fn runtime_function_rejects_malformed_directory_ordering_and_code_boundaries() {
    for (at, value) in [
        (0x124, 23u32),
        (0x120, 0x3000),
        (0x504, 0x1000),
        (0x504, 0x1201),
        (0x50c, 0x1000),
        (0x50c, 0x2000),
    ] {
        let mut bytes = exception_fixture();
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
        assert!(file.image().runtime_function(0x1000).is_err(), "at {at:#x}");
    }
    let bytes = fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    assert!(file.image().runtime_function(0x1000).is_err());
}

fn virtual_slot_fixture() -> Vec<u8> {
    let mut bytes = exception_fixture();
    bytes[0x1d4..0x1d8].copy_from_slice(&0x40000040u32.to_le_bytes());
    bytes[0x450..0x458].copy_from_slice(&0x140001000u64.to_le_bytes());
    bytes
}

#[test]
fn virtual_slot_checks_aslr_and_the_full_exact_runtime_entry() {
    let bytes = virtual_slot_fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let base = 0x7ff000000000;
    let mut image = loaded(&bytes);
    image[0x2050..0x2058].copy_from_slice(&(base + 0x1000u64).to_le_bytes());
    let memory = SliceMemory::new(base, &image).unwrap();
    let module = file.bind(&memory, base, &[CODE]).unwrap();
    let range = module
        .checked_virtual_function(base + 0x2000, 0x50)
        .unwrap();
    assert_eq!((range.rva, range.length), (0x1000, 5));
    for (table, offset) in [
        (base + 0x2000, 0x51),
        (base + 0x1000, 0),
        (base - 8, 0),
        (base + 0x23f8, 8),
    ] {
        assert!(module.checked_virtual_function(table, offset).is_err());
    }
}

#[test]
fn virtual_slot_rejects_changed_targets_code_and_writable_tables() {
    for (change, value) in [(0x2050, 0x99), (0x1004, 0x90)] {
        let bytes = virtual_slot_fixture();
        let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
        let base = 0x100000;
        let mut image = loaded(&bytes);
        image[0x2050..0x2058].copy_from_slice(&(base + 0x1000u64).to_le_bytes());
        image[change] = value;
        let memory = SliceMemory::new(base, &image).unwrap();
        let module = file
            .bind(
                &memory,
                base,
                &[CodeRange {
                    rva: 0x1010,
                    length: 8,
                }],
            )
            .unwrap();
        assert!(
            module
                .checked_virtual_function(base + 0x2000, 0x50)
                .is_err()
        );
    }
    let bytes = exception_fixture();
    let file = VerifiedFile::verify(&bytes, identity(&bytes)).unwrap();
    let image = loaded(&bytes);
    let memory = SliceMemory::new(0x100000, &image).unwrap();
    let module = file.bind(&memory, 0x100000, &[CODE]).unwrap();
    assert!(module.checked_virtual_function(0x102000, 0x50).is_err());
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x800];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    bytes[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
    bytes[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
    bytes[0xb0..0xb8].copy_from_slice(&0x140000000u64.to_le_bytes());
    bytes[0xd0..0xd4].copy_from_slice(&0x3000u32.to_le_bytes());
    bytes[0xd4..0xd8].copy_from_slice(&0x200u32.to_le_bytes());
    for (table, rva, raw, size, flags) in [
        (0x188, 0x1000u32, 0x200u32, 0x200u32, 0x60000020u32),
        (0x1b0, 0x2000, 0x400, 0x400, 0xc0000040),
    ] {
        for (offset, value) in [(8, size), (12, rva), (16, size), (20, raw), (36, flags)] {
            bytes[table + offset..table + offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[0x200..0x205].copy_from_slice(&[0x48, 0x8b, 0x41, 0x08, 0xc3]);
    bytes
}

fn identity(bytes: &[u8]) -> ExecutableIdentity {
    ExecutableIdentity {
        name: "synthetic test image",
        file_size: bytes.len(),
        sha256: Sha256::digest(bytes).into(),
    }
}

fn loaded(file: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 0x3000];
    bytes[..0x200].copy_from_slice(&file[..0x200]);
    bytes[0x1000..0x1200].copy_from_slice(&file[0x200..0x400]);
    bytes[0x2000..0x2400].copy_from_slice(&file[0x400..]);
    bytes
}

const CODE: CodeRange = CodeRange {
    rva: 0x1000,
    length: 5,
};

#[test]
fn exact_file_identity_precedes_any_live_memory_access() {
    let bytes = fixture();
    let mut corrupted = bytes.clone();
    corrupted[0x500] ^= 1;
    assert!(matches!(
        VerifiedFile::verify(&corrupted, identity(&bytes)),
        Err(Error::UnsupportedImage { .. })
    ));
    assert!(matches!(
        VerifiedFile::verify(&bytes[..bytes.len() - 1], identity(&bytes)),
        Err(Error::UnsupportedImage { .. })
    ));
    let bad_pe = b"not a PE";
    assert!(matches!(
        VerifiedFile::verify(bad_pe, identity(bad_pe)),
        Err(Error::InvalidPe(_))
    ));
}

#[test]
fn file_offsets_are_mapped_with_aslr_before_resolving_code_or_globals() {
    let file = fixture();
    let verified = VerifiedFile::verify(&file, identity(&file)).unwrap();
    let mut image = loaded(&file);
    image[0x2000..0x2008].copy_from_slice(&0xabcdef00u64.to_le_bytes());
    let memory = SliceMemory::new(0x7ff000000000, &image).unwrap();
    let module = verified.bind(&memory, 0x7ff000000000, &[CODE]).unwrap();
    assert_eq!(module.code_address(CODE).unwrap(), 0x7ff000001000);
    assert_eq!(module.global_pointer(0x2000).unwrap(), Some(0xabcdef00));
    assert_eq!(module.global_pointer(0x2008).unwrap(), None);
    assert!(module.global_pointer(0x1000).is_err());
    assert!(module.global_pointer(0x2001).is_err());
    assert!(module.global_pointer(0x23fc).is_err());
}

#[test]
fn loaded_headers_code_and_invalid_ranges_refuse_binding() {
    let file = fixture();
    let verified = VerifiedFile::verify(&file, identity(&file)).unwrap();
    for (offset, expected_rva) in [(0x90, 0), (0x1004, 0x1000)] {
        let mut bytes = loaded(&file);
        bytes[offset] ^= 1;
        let memory = SliceMemory::new(0x100000, &bytes).unwrap();
        assert!(
            matches!(verified.bind(&memory, 0x100000, &[CODE]), Err(Error::ImageMismatch { rva }) if rva == expected_rva)
        );
    }
    let image = loaded(&file);
    let memory = SliceMemory::new(0x100000, &image).unwrap();
    for range in [
        CodeRange {
            rva: 0x1000,
            length: 0,
        },
        CodeRange {
            rva: 0x11ff,
            length: 2,
        },
        CodeRange {
            rva: 0x2000,
            length: 1,
        },
    ] {
        assert!(matches!(
            verified.bind(&memory, 0x100000, &[range]),
            Err(Error::InvalidProfile(_))
        ));
    }
    assert!(matches!(
        verified.bind(&memory, u64::MAX, &[CODE]),
        Err(Error::AddressOverflow)
    ));
    assert!(matches!(
        verified.bind(&memory, 0, &[CODE]),
        Err(Error::InvalidProfile(_))
    ));
    assert!(matches!(
        verified.bind(&memory, 0x100000, &[]),
        Err(Error::InvalidProfile(_))
    ));
}

#[test]
fn code_lookup_revalidates_after_a_later_patch() {
    struct Patchable {
        bytes: Vec<u8>,
        patched: Cell<bool>,
    }
    impl AddressSpace for Patchable {
        fn read_exact(&self, address: u64, output: &mut [u8]) -> shikifsrs_runtime::Result<()> {
            SliceMemory::new(0x100000, &self.bytes)?.read_exact(address, output)?;
            if self.patched.get() && address == 0x101000 && !output.is_empty() {
                output[0] = 0xcc;
            }
            Ok(())
        }
    }
    let file = fixture();
    let verified = VerifiedFile::verify(&file, identity(&file)).unwrap();
    let memory = Patchable {
        bytes: loaded(&file),
        patched: Cell::new(false),
    };
    let module = verified.bind(&memory, 0x100000, &[CODE]).unwrap();
    memory.patched.set(true);
    assert_eq!(
        module.code_address(CODE),
        Err(Error::ImageMismatch { rva: 0x1000 })
    );
}
