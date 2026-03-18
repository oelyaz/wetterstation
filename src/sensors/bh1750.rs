use super::types::{I2cSensor, SensorReading, SensorError};

use embassy_time::{Duration, Timer};
use esp_hal::{i2c::master::I2c, Blocking};



const BH1759_POWER_UP: u8 = 0x01;
const BH1759_HIGH_RES_ONE_TIME: u8 = 0x20;


pub struct BH1750 {
    pub address: u8
}


impl BH1750 {
    pub(crate) async fn init_sensor(&self, i2c_bus: &mut I2c<'_, Blocking>) -> Result<(), SensorError> {
        // start-up sequence
        let init_commands = [BH1759_POWER_UP, BH1759_HIGH_RES_ONE_TIME];
        for command in init_commands.iter() {
            if i2c_bus.write(self.address, &[*command]).is_err() {
                return Err(SensorError::I2cError);
            } else {
                // wait for command processing
                Timer::after(Duration::from_millis(3)).await;
            }
        }
        Ok(())
    }
}

impl I2cSensor for BH1750 {
    async fn read_sensor(&self, i2c_bus: &mut I2c<'_, Blocking>) -> Result<SensorReading, SensorError> {
        if i2c_bus.write(self.address, &[BH1759_HIGH_RES_ONE_TIME]).is_err() {
            return Err(SensorError::I2cError);
        } else {
            // wait for command processing
            Timer::after(Duration::from_millis(3)).await;
        }

        let mut buffer = [0u8; 2];
        if i2c_bus.read(self.address, &mut buffer).is_err() {
            return Err(SensorError::I2cError);
        }

        // create 16bit value from two 8bit transmits
        let raw_value = ((buffer[0] as u16) << 8) | (buffer[1] as u16);
        // sensor needs linear scaling
        let lux_value = (raw_value as f32) / 1.2;

        Ok(SensorReading::Light { lux: lux_value })
    }
}