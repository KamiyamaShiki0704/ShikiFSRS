//! One copied observation; no polling, process mutation, injection or native game calls.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use shikifsrs_elden_ring::Ww171File;
    use shikifsrs_runtime::memory::RemoteProcess;
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err(
            "usage: observe_task_group <eldenring.exe> <PID> <module-base-hex> <group-index-or-phase-name>"
                .into(),
        );
    }
    let pid = args[2].parse::<u32>()?;
    let base = u64::from_str_radix(args[3].trim_start_matches("0x"), 16)?;
    let bytes = std::fs::read(&args[1])?;
    // Validate before opening even a read-only process handle.
    let file = Ww171File::verify(&bytes)?;
    let index = match args[4].parse::<usize>() {
        Ok(index) => index,
        Err(_) => file.phase(&args[4])?.registration_index(),
    };
    let process = RemoteProcess::open_readonly(pid)?;
    let catalog = file.bind(&process, base)?;
    match catalog.observe(index)? {
        Some(item) => {
            println!(
                "group={} pointer={:#x} native_words={:08x?}",
                item.registration_index, item.native_pointer, item.native_words
            );
            if let Some(id) = item.native_group_id() {
                println!("native_group_id={id:#x}");
                match catalog.task_registrations(id, 4096, 4096)? {
                    Some(tasks) => {
                        println!("registered_task_count={}", tasks.len());
                        for task in tasks {
                            println!(
                                "task={:#x} callbacker={:#x?} group_id={:#x}",
                                task.task_address, task.callbacker_address, task.group_id
                            );
                        }
                    }
                    None => println!("registered_group=absent"),
                }
            } else {
                println!("native_group_id=unassigned");
            }
        }
        None => println!("group={index} uninitialized"),
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("observe_task_group requires Windows");
    std::process::exit(1);
}
