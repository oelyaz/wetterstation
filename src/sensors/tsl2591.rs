use super::types::{I2cSensor, SensorReading, SensorError};

use embassy_time::{Duration, Timer};
use esp_hal::{i2c::master::I2c, Blocking};


// Every access is prefixed with the command bit (0x80) + "normal" transaction
// (0x20), which also auto-increments the register pointer across a burst read.
const TSL2591_COMMAND: u8 = 0xA0;
const TSL2591_REG_ENABLE: u8 = 0x00;
const TSL2591_REG_CONTROL: u8 = 0x01;
const TSL2591_REG_CHAN0_LOW: u8 = 0x14;

const TSL2591_ENABLE_POWERON_ALS: u8 = 0x03; // PON | AEN


const TSL2591_CONTROL_MED_100MS: u8 = 0x10;
const TSL2591_AGAIN: f32 = 25.0;
const TSL2591_ATIME_MS: f32 = 100.0;
const TSL2591_LUX_DF: f32 = 408.0; // device lux coefficient (datasheet/Adafruit)


pub struct TSL2591 {
    pub address: u8
}


impl TSL2591 {
    pub(crate) async fn init_sensor(&self, i2c_bus: &mut I2c<'_, Blocking>) -> Result<(), SensorError> {
        // power on + enable the ambient light sensor
        i2c_bus.write(self.address, &[TSL2591_COMMAND | TSL2591_REG_ENABLE, TSL2591_ENABLE_POWERON_ALS])
            .map_err(|_| SensorError::I2cError)?;
        // set gain + integration time
        i2c_bus.write(self.address, &[TSL2591_COMMAND | TSL2591_REG_CONTROL, TSL2591_CONTROL_MED_100MS])
            .map_err(|_| SensorError::I2cError)?;
        // wait one integration cycle so the first read returns valid data
        Timer::after(Duration::from_millis(120)).await;
        Ok(())
    }
}

impl I2cSensor for TSL2591 {
    async fn read_sensor(&self, i2c_bus: &mut I2c<'_, Blocking>) -> Result<SensorReading, SensorError> {
        // burst read CH0 (full spectrum) + CH1 (IR), 2 bytes each, auto-incremented
        let mut buffer = [0u8; 4];
        i2c_bus.write_read(self.address, &[TSL2591_COMMAND | TSL2591_REG_CHAN0_LOW], &mut buffer)
            .map_err(|_| SensorError::I2cError)?;

        let ch0 = u16::from_le_bytes([buffer[0], buffer[1]]); // full spectrum
        let ch1 = u16::from_le_bytes([buffer[2], buffer[3]]); // infrared

        Ok(SensorReading::Light { lux: lux_from_channels(ch0, ch1) })
    }
}

/// Adafruit's TSL2591 lux approximation for the configured gain/integration.
fn lux_from_channels(ch0: u16, ch1: u16) -> f32 {
    if ch0 == 0 {
        return 0.0; // dark or saturated -> no division
    }
    let cpl = (TSL2591_ATIME_MS * TSL2591_AGAIN) / TSL2591_LUX_DF;
    let ch0 = ch0 as f32;
    let ch1 = ch1 as f32;
    (((ch0 - ch1) * (1.0 - ch1 / ch0)) / cpl).max(0.0)
}
