use shikifsrs_nightreign::NightreignFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("expected the path to nightreign.exe")?;
    let bytes = std::fs::read(path)?;
    let file = NightreignFile::verify(&bytes)?;
    println!(
        "{}: {} checked file definitions",
        file.file().identity().name,
        file.task_definitions().records().len()
    );
    for name in [
        "FrameBegin",
        "SoundStep",
        "ChrIns_PostPhysics",
        "FrameEnd",
        "Dummy",
    ] {
        let row = file
            .task_definitions()
            .find(name)
            .ok_or("missing definition")?;
        println!(
            "{}: index={}, raw seeds={:?}",
            row.name, row.registration_index, row.native_seed_words
        );
    }
    Ok(())
}
