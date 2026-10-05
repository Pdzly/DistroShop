use std::io;
use std::io::{Read, Write};
use nix::unistd::Uid;
use std::env;
use std::process::Command;
use std::fs::File;
use dioxus::prelude::*;


pub fn flasher(blockdev: &str, isoimg: &str, flashmode: &str) -> io::Result<()> {

    let args = vec![blockdev, isoimg, flashmode]; //the original arguments get shadowed later 

    if Uid::current().is_root(){
    let mut blockdev = File::options().write(true).open(&blockdev)?;
    let mut isoimg = File::open(&isoimg)?;
    match flashmode {
        "safe" => {
            info!("using safe mode");
            io::copy(&mut isoimg, &mut blockdev)?;
            blockdev.sync_all()?;
        }
        "fast" => {
            info!("using fast mode");
            let mut buffer = Vec::new();
            isoimg.read_to_end(&mut buffer)?;
            blockdev.write_all(&buffer)?;
            blockdev.sync_all()?;
        }
        _ => {
            error!("invalid flash mode");
            std::process::exit(1);
        }
    }
 } else {
    let exe_path = env::current_exe()?;

    let mut exit_status = None;
    std::thread::scope(|s| {
        s.spawn(||{
        let result = Command::new("pkexec") 
            .arg(&exe_path)
            .arg("-f")
            .args(&args)
            .status();
        
        exit_status = Some(result);
        });
    }); //lol i spent like 20 minutes figuring threads out and it didnt fix the ui freezing

    let status = exit_status.expect("Thread guaranteed to run and set status")?;

    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("pkexec exited with {status}")
        ));
    }

 }
    Ok(())
}