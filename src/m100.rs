use std::time::Duration;

use anyhow::{anyhow, Result};
use serialport::{DataBits, SerialPort, StopBits};

use crate::protocol;

pub static DEFAULT_PASSWORD: [u8; 8] = [0x30; 8];

pub struct M100Device {
    port: Box<dyn SerialPort>,
    read_buf: [u8; 1024],
}

impl M100Device {
    pub fn new(mut port: Box<dyn SerialPort>) -> Result<M100Device> {
        port.set_stop_bits(StopBits::One)?;
        port.set_data_bits(DataBits::Eight)?;
        port.set_timeout(Duration::from_secs(1))?;

        Ok(M100Device {
            port,
            read_buf: [0; 1024],
        })
    }

    pub fn set_baud_rate(&mut self, baud_rate: u32) -> Result<()> {
        self.port.set_baud_rate(baud_rate)?;
        Ok(())
    }

    pub fn upload_firmware(&mut self, firmware: &[u8]) -> Result<()> {
        self.port.set_baud_rate(9600)?;

        // stage 1 -- confirm the m100 is alive
        self.port.write(&[0xFE])?;
        self.port.flush()?;

        self.port.read_exact(&mut self.read_buf[0..1])?;
        if self.read_buf[0] != 0xFF {
            return Err(anyhow!(
                "Could not establish connection to the device: {:#04x}",
                self.read_buf[0]
            ));
        }

        // stage 2 -- set baud rate to 115200
        self.port.write(&[0xB5])?;
        self.port.flush()?;
        std::thread::sleep(Duration::from_millis(50));
        self.port.set_baud_rate(115200)?;

        // stage 3 -- prepare to upload firmware
        self.port.write(&[0xFF, 0xDB])?;
        self.port.flush()?;

        self.port.read_exact(&mut self.read_buf[0..1])?;
        if self.read_buf[0] != 0xBF {
            return Err(anyhow!(
                "Could not prepare firmware upload to the device: {:#04x}",
                self.read_buf[0]
            ));
        }
        self.port.write(&[0xFD])?;
        self.port.flush()?;

        // stage 4 -- upload the firmware
        self.port.write(firmware)?;
        self.port.flush()?;

        // stage 5 -- disable sleep mode
        self.disable_sleep()?;

        Ok(())
    }

    pub fn get_version(&mut self) -> Result<&str> {
        // mode 0x00 = hardware
        // mode 0x01 = software
        // mode 0x02 = manufacturer

        let command = protocol::get_version()?;
        self.port.write(&command)?;
        self.port.flush()?;

        let res = self.receive_response()?;

        Ok(std::str::from_utf8(res)?)
    }

    pub fn set_hfss_status(&mut self, status: HfssStatus) -> Result<()> {
        let command = protocol::set_hfss_status(status)?;
        self.port.write(&command)?;
        self.port.flush()?;

        self.receive_response()?;

        Ok(())
    }

    pub fn query(&mut self) -> Result<Option<TagInfo>> {
        let command = protocol::query()?;
        self.port.write(&command)?;
        self.port.flush()?;

        let res = self.receive_response()?;
        if res.len() <= 1 {
            return Ok(None);
        }
        let rssi = res[0];
        let epc = hex::encode(&res[3..res.len() - 2]).to_uppercase();
        Ok(Some(TagInfo { epc, rssi }))
    }

    pub fn read_all_data(&mut self, password: &[u8; 8], bank: MemoryBank) -> Result<Vec<u8>> {
        match bank {
            MemoryBank::Reserved => Err(anyhow!("cannot read_all_data the Reserved memory bank")),
            MemoryBank::Epc => Ok(self.read_chunked_data(password, bank, 12, 2)?),
            MemoryBank::Tid => Ok(self.read_data(password, bank, 0, 32)?.to_vec()),
            MemoryBank::User => Ok(self.read_chunked_data(password, bank, 0, 512)?),
        }
    }

    fn read_chunked_data(
        &mut self,
        password: &[u8; 8],
        bank: MemoryBank,
        start_address: u16,
        chunk_size: u16,
    ) -> Result<Vec<u8>> {
        let mut data = Vec::with_capacity(start_address as usize);

        // read all the data up to the start address
        if start_address != 0 {
            let start_data = self.read_data(password, bank, 0, start_address)?;
            data.extend_from_slice(start_data);
        }

        let mut address = start_address;
        loop {
            match self.read_data(password, bank, address, chunk_size) {
                Ok(chunk) => {
                    data.extend_from_slice(chunk);
                    address += chunk_size;
                }
                Err(e) => {
                    println!("Error {} at {}.", e, address);
                    return Ok(data);
                }
            }
        }
    }

    pub fn write_data(
        &mut self,
        password: &[u8; 8],
        bank: MemoryBank,
        address: u16,
        data: &mut [u8],
    ) -> Result<()> {
        let command = match bank {
            MemoryBank::Epc => protocol::write_epc(password, data),
            other => protocol::write_data(password, other, address, data),
        }?;
        self.port.write(&command)?;
        self.port.flush()?;

        let res = self.receive_response()?;
        if res.len() == 1 {
            if res[0] == 0xB0 {
                return Err(anyhow!("Unexpected write response: HEXIN_ERROR_WRITE"));
            } else if res[0] == 0x10 {
                return Err(anyhow!("Unexpected write response: HEXIN_FAIL_WRITE"));
            }
        }

        Ok(())
    }

    pub fn read_data(
        &mut self,
        password: &[u8; 8],
        bank: MemoryBank,
        address: u16,
        data_length: u16,
    ) -> Result<&[u8]> {
        if data_length % 2 != 0 || data_length == 0 {
            return Err(anyhow!(
                "Data length must be a positive even number: {}",
                data_length
            ));
        }
        let command = protocol::read_data(password, bank, address, data_length)?;
        self.port.write(&command)?;
        self.port.flush()?;

        let res = self.receive_response()?;
        println!("res: {:2x}", res[0]);
        if res.len() == 1 {
            if res[0] == 0x09 {
                return Err(anyhow!("Read failure HEXIN_FAIL_READ"));
            } else if res[0] == 0xA3 {
                return Err(anyhow!("Read failure HEXIN_ERROR_READ_MEMORY_OVERRUN"));
            }
        }

        Ok(res)
    }

    fn disable_sleep(&mut self) -> Result<()> {
        let command = protocol::idle()?;
        self.port.write(&command)?;
        self.port.flush()?;

        self.receive_response()?;

        Ok(())
    }

    fn receive_response(&mut self) -> Result<&[u8]> {
        self.port.read_exact(&mut self.read_buf[0..5])?;
        let length = i16::from_be_bytes([self.read_buf[3], self.read_buf[4]]); // header
                                                                               // println!("Incoming data length from response: {}", length);
        self.port
            .read_exact(&mut self.read_buf[5..5 + length as usize])?; // body
        self.port
            .read_exact(&mut self.read_buf[5 + length as usize..7 + length as usize])?; // end

        let tail = self.read_buf[length as usize + 6];
        if tail != 0x7E {
            return Err(anyhow!("Invalid packet (received invalid tail: {})", tail));
        }

        let unpacked = &self.read_buf[5..length as usize + 5];
        // println!("{:02X?}", unpacked);

        Ok(unpacked)
    }
}

#[derive(Debug, PartialEq)]
#[repr(u8)]
pub enum HfssStatus {
    Auto = 0xFF,
    Stop = 0x00,
}

#[derive(Debug)]
pub struct TagInfo {
    pub epc: String,
    pub rssi: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum MemoryBank {
    Reserved = 0x00,
    Epc = 0x01,
    Tid = 0x02,
    User = 0x03,
}
