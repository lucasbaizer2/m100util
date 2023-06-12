use anyhow::Result;
use byteorder::*;
use std::io::Write;

use crate::m100::{HfssStatus, MemoryBank};

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Command {
    GetVersion = 0x03,
    Idle = 0x04,
    Query = 0x22,
    ReadData = 0x39,
    SetHfss = 0xAD,
    WriteData = 0x49,
}

pub fn get_version() -> Result<Vec<u8>> {
    make_frame(Command::GetVersion, &[0x00])
}

pub fn set_hfss_status(status: HfssStatus) -> Result<Vec<u8>> {
    make_frame(Command::SetHfss, &[status as u8])
}

pub fn query() -> Result<Vec<u8>> {
    make_frame(Command::Query, &[0x00, 0x00])
}

pub fn idle() -> Result<Vec<u8>> {
    make_frame(Command::Idle, &[0x00, 0x01, 0x00])
}

pub fn read_data(password: &[u8], bank: MemoryBank, address: u16, data_length: u16) -> Result<Vec<u8>> {
    let mut payload = Vec::new();
    payload.write(password)?;
    payload.write_u8(bank as u8)?;
    payload.write_u16::<BE>(address)?;
    payload.write_u16::<BE>(data_length)?;

    make_frame(Command::ReadData, &payload)
}

pub fn write_data(password: &[u8], bank: MemoryBank, address: u16, data: &[u8]) -> Result<Vec<u8>> {
    if bank == MemoryBank::Epc {
        panic!("use write_epc instead");
    }
    
    let mut payload = Vec::new();
    payload.write(password)?;
    payload.write_u8(bank as u8)?;
    payload.write_u16::<BE>(address)?;
    payload.write_u16::<BE>(data.len() as u16)?;
    payload.write(data)?;

    make_frame(Command::WriteData, &payload)
}

pub fn write_epc(password: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let mut payload = Vec::new();
    payload.write(password)?;
    payload.write_u8(MemoryBank::Epc as u8)?;
    payload.write_u8(0x00)?; // magic byte?
    payload.write_u8(0x01)?; // magic byte?
    payload.write_u16::<BE>(data.len() as u16)?;

    let pc = ((data.len() as u16) << 10) & 0xF800; // more magic??
    payload.write_u16::<BE>(pc)?;

    payload.write(data)?;

    make_frame(Command::WriteData, &payload)
}

pub fn make_frame(cmd: Command, payload: &[u8]) -> Result<Vec<u8>> {
    let mut packet = vec![
        0xBB,      // MAGICRF_HEAD
        0x00,      // TYPE_COMMAND
        cmd as u8, // command
    ];

    packet.write_u16::<BE>(payload.len() as u16)?; // length
    packet.write(payload)?; // payload

    let checksum: u32 = packet.iter().skip(1).map(|b| *b as u32).sum();
    packet.write_u8((checksum & 0xFF) as u8)?; // checksum
    packet.write_u8(0x7E)?; // MAGICRF_TAIL

    println!("cmd {:?} made frame {:2x?}", cmd, packet);

    Ok(packet)
}
