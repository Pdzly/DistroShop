use dioxus::prelude::*;
use serde::Deserialize;
use runas::Command;
use wmi::{COMLibrary, WMIConnection};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct Partition {
    disk_index: u32,
    index: u32,
}

fn physical_drive_for(letter: char) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let com = COMLibrary::new()?;
    let wmi = WMIConnection::new(com)?;
    let query = format!(
        "ASSOCIATORS OF {{Win32_LogicalDisk.DeviceID='{}:'}} \
         WHERE AssocClass = Win32_LogicalDiskToPartition",
        letter.to_ascii_uppercase()
    );
    let partitions: Vec<Partition> = wmi.raw_query(&query)?;

    Ok(partitions
    .iter()
    .map(|p| format!(r"\\.\PhysicalDrive{}", p.disk_index))
    .collect())
}

pub fn flasher(driveletter: &str, isoimg: &str, flashmode: &str) { 
    todo!();
}