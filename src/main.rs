use std::time::Duration;

use crate::m100::{M100Device, MemoryBank};
use anyhow::Result;
use clap::{Parser, ValueEnum};

pub mod m100;
pub mod m100_sys;

#[derive(clap::Parser)]
struct Cli {
    #[command(subcommand)]
    action: Action,
    #[arg(short, long, default_value = "/dev/ttyACM0")]
    port: String,
}

#[derive(clap::Subcommand)]
enum Action {
    #[command(about = "Read the memory from a given memory bank")]
    Read { bank: CliMemoryBank },
    #[command(about = "Write data to a given memory bank")]
    Write {
        bank: CliMemoryBank,
        #[arg(help = "The data to write to the memory bank as a hex string")]
        value: String,
    },
}

#[derive(ValueEnum, Clone, PartialEq)]
enum CliMemoryBank {
    Epc,
    Tid,
    User,
}

fn main() -> Result<()> {
    let args = Cli::parse();

    let port = serialport::new(args.port, 115200).open().unwrap();
    let mut m100 = M100Device::new(port)?;

    if let Err(e) = m100.get_version() {
        println!("Could not receive device version: {}", e);
        println!("Uploading firmware...");
        m100.set_baud_rate(9600)?;
        std::thread::sleep(Duration::from_millis(100));

        m100.upload_firmware(include_bytes!("../native/firmware.bin"))?;
        m100.set_hfss_status(m100::HfssStatus::Auto)
            .expect("set hfss status failed");
        println!("Uploaded firmware.");
        std::thread::sleep(Duration::from_millis(100));
    }

    let version = m100.get_version()?;
    println!("Connected to '{}'.", version);

    println!("Waiting for a tag...");
    match args.action {
        Action::Read { bank } => loop {
            if let Ok(Some(qr)) = m100.query() {
                println!("Tag found! EPC: {}", qr.epc);
                if bank == CliMemoryBank::Epc {
                    break;
                }
                let data = match m100.read_all_data(
                    &m100::DEFAULT_PASSWORD,
                    match bank {
                        CliMemoryBank::Epc => MemoryBank::Epc,
                        CliMemoryBank::Tid => MemoryBank::Tid,
                        CliMemoryBank::User => MemoryBank::User,
                    },
                ) {
                    Ok(data) => data,
                    Err(e) => {
                        eprintln!("Error occurred: {e}.\nTrying again...");
                        continue;
                    }
                };

                println!(
                    "\nData received from tag: {}",
                    hex::encode(data).to_uppercase()
                );

                break;
            }
        },
        Action::Write { bank, value } => loop {
            let bank = match bank {
                CliMemoryBank::Epc => MemoryBank::Epc,
                CliMemoryBank::Tid => MemoryBank::Tid,
                CliMemoryBank::User => MemoryBank::User,
            };
            if let Ok(Some(qr)) = m100.query() {
                println!("Tag found! EPC: {}", qr.epc);

                let mut write_data = hex::decode(&value)?;
                match m100.write_data(&m100::DEFAULT_PASSWORD, bank, 0, &mut write_data) {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("Error occurred during writing: {e}. Retrying...");
                        continue;
                    }
                }

                println!("Verifying data, please keep the tag on the reader...");
                let verify_data = if bank == MemoryBank::Epc {
                    loop {
                        if let Ok(Some(qr)) = m100.query() {
                            break hex::decode(qr.epc)?;
                        }
                    }
                } else {
                    match m100.read_all_data(&m100::DEFAULT_PASSWORD, bank) {
                        Ok(data) => data,
                        Err(e) => {
                            eprintln!("Error occurred during verification: {e}.\nTrying again...");
                            continue;
                        }
                    }
                };

                if write_data != verify_data {
                    eprintln!("Verification failed. Trying again...");
                    continue;
                }

                println!("\nSuccessfully wrote data!");
                break;
            }
        },
    }

    Ok(())
}
