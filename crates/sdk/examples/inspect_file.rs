use shikifsrs::GameFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut paths = std::env::args_os().skip(1).peekable();
    if paths.peek().is_none() {
        return Err("expected one or more executable file paths".into());
    }
    for path in paths {
        let bytes = std::fs::read(path)?;
        let file = GameFile::verify(&bytes)?;
        let catalog = file.task_definitions()?;
        println!(
            "{:?}: {}, {} file definitions",
            file.game(),
            file.file().identity().name,
            catalog.records().len()
        );
        for name in ["FrameBegin", "SoundStep", "ChrIns_PostPhysics", "FrameEnd"] {
            let row = catalog.find(name).ok_or("missing phase definition")?;
            println!("{name}: {}", row.registration_index);
        }
    }
    Ok(())
}
