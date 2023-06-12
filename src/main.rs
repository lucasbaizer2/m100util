use std::time::Duration;

use crate::m100::{M100Device, MemoryBank};
use clap::{Parser, ValueEnum};
use serialport::ClearBuffer;

pub mod m100;
pub mod protocol;

#[derive(clap::Parser)]
struct Cli {
    #[command(subcommand)]
    action: Action,
    #[arg(short, long, default_value = "/dev/ttyACM0")]
    port: String,
}

#[derive(clap::Subcommand, PartialEq)]
enum Action {
    #[command(about = "Read the memory from a given memory bank")]
    Read { bank: CliMemoryBank },
    #[command(about = "Write data to a given memory bank")]
    Write {
        bank: CliMemoryBank,
        #[arg(help = "The data to write to the memory bank as a hex string")]
        value: String,
    },
    #[command(about = "Read information about the EPC Gen2 tag")]
    Identify,
}

#[derive(ValueEnum, Clone, PartialEq)]
enum CliMemoryBank {
    Epc,
    Tid,
    User,
}

fn main() {
    let args = Cli::parse();

    let mut port = match serialport::new(&args.port, 115200).open() {
        Ok(port) => port,
        Err(_) => {
            println!("Failed to open serial port `{}`.", args.port);
            std::process::exit(1);
        }
    };
    port.set_timeout(Duration::from_secs(1)).unwrap();
    port.clear(ClearBuffer::All).unwrap();

    let mut m100 = match M100Device::new(port) {
        Ok(m100) => m100,
        Err(_) => {
            println!("Failed to create M100Device.");
            std::process::exit(1);
        }
    };

    let version = match m100.get_version() {
        Ok(version) => version,
        Err(e) => {
            println!("Failed to identify device. Are you sure it's working? {}", e);
            std::process::exit(1);
        }
    };
    if args.action == Action::Identify {
        println!("Identity: {}", version);
        return;
    }
    println!("Connected to '{}'.", version);

    println!("Waiting for a tag...");
    match args.action {
        Action::Identify => unreachable!(),
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

                let mut write_data = hex::decode(&value).unwrap();
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
                            break hex::decode(qr.epc).unwrap();
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
}
