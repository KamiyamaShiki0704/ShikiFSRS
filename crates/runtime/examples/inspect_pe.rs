use shikifsrs_runtime::pe::FileImage;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: inspect_pe <AMD64 PE file>")?;
    let bytes = std::fs::read(path)?;
    let image = FileImage::parse(&bytes)?;
    println!(
        "AMD64 image: base={:#x} size={:#x} sections={}",
        image.preferred_base,
        image.image_size,
        image.sections.len()
    );
    for section in &image.sections {
        println!(
            "{} rva={:#x} raw={:#x} executable={} writable={}",
            String::from_utf8_lossy(&section.name),
            section.rva,
            section.raw_size,
            section.is_executable(),
            section.is_writable()
        );
    }
    if let Some(name) = std::env::args().nth(2) {
        for table in image.vtables(&name)? {
            println!(
                "RTTI {name}: descriptor={:#x} locator={:#x} vtable={:#x} object_offset={:#x} entries={:?}",
                table.type_descriptor_rva,
                table.locator_rva,
                table.table_rva,
                table.object_offset,
                table.entries
            );
        }
    }
    Ok(())
}
