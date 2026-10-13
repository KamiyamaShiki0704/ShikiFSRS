use shikifsrs_elden_ring::Ww171File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: verify_ww171 <eldenring.exe>")?;
    let bytes = std::fs::read(path)?;
    let file = Ww171File::verify(&bytes)?;
    println!(
        "PASS: exact file identity and independent task RTTI: {}",
        file.file().identity().name
    );
    let phases = file.phase_definitions()?;
    for selector in 0..=4 {
        println!(
            "torrent native selector={selector} appearance={}",
            file.torrent_appearance(selector)?
        );
    }
    println!("PASS: {} native phase definitions", phases.len());
    println!(
        "PASS: {} independent character/parameter object types",
        file.object_types()?.len()
    );
    println!(
        "PASS: {} independently identified parameter names",
        file.parameter_definitions()?.len()
    );
    for name in ["EquipParamWeapon", "NpcParam", "SpEffectParam"] {
        println!(
            "parameter={name} slot={}",
            file.parameter_slot(name)?
                .ok_or("missing parameter name")?
                .index()
        );
    }
    for rva in [0xeb3e50, 0x2657880, 0x1ec7c50] {
        let range = file.file().image().runtime_function(rva)?;
        println!(
            "PASS: exact runtime entry rva={:#x} length={:#x}",
            range.rva, range.length
        );
    }
    for name in ["FrameBegin", "SoundStep", "ChrIns_PostPhysics", "FrameEnd"] {
        println!(
            "phase={name} registration_index={}",
            file.phase(name)?.registration_index()
        );
    }
    if std::env::args_os()
        .nth(2)
        .is_some_and(|arg| arg == "--profile-timing")
    {
        let start = std::time::Instant::now();
        for _ in 0..2 {
            std::hint::black_box(file.object_types()?);
        }
        println!(
            "offline immutable object metadata, two repeated checks: {} us",
            start.elapsed().as_micros()
        );
    }
    println!("No game process opened, native calls executed, or consumer dependencies changed.");
    Ok(())
}
