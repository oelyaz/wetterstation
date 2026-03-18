use super::types::{I2cSensor, SensorReading, SensorError};

use embassy_time::{Duration, Timer};
use esp_hal::{i2c::master::I2c, Blocking};


const BME280_HUM_OVERSAMPLE_ONE: [u8; 2] = [0xF2, 0x01];
const BME280_P_FILTER_FOUR: [u8; 2] =[0xF5, 0x08];
const BME280_SLEEP_OVERSAMPLE_ONE: [u8; 2] = [0xF4, 0x24];


pub struct BME280Builder {
    pub address: u8
}

pub struct BME280 {
    pub address: u8,
    calibration: BME280Calibration,
}

#[derive(Default, Debug)]
pub struct BME280Calibration {
    pub dig_t1: u16, pub dig_t2: i16, pub dig_t3: i16,
    pub dig_p1: u16, pub dig_p2: i16, pub dig_p3: i16,
    pub dig_p4: i16, pub dig_p5: i16, pub dig_p6: i16,
    pub dig_p7: i16, pub dig_p8: i16, pub dig_p9: i16,
    pub dig_h1: u8,  pub dig_h2: i16, pub dig_h3: u8,
    pub dig_h4: i16, pub dig_h5: i16, pub dig_h6: i8,
}


impl BME280Builder {
    pub(crate) async fn init_sensor(self, i2c_bus: &mut I2c<'_, Blocking>) -> Result<BME280, SensorError> {
        // start-up sequence
        let init_commands = [
            BME280_HUM_OVERSAMPLE_ONE,
            BME280_P_FILTER_FOUR,
            BME280_SLEEP_OVERSAMPLE_ONE];
        for command in init_commands.iter() {
            if i2c_bus.write(self.address, command).is_err() {
                return Err(SensorError::I2cError);
            } else {
                // wait for command processing
                Timer::after(Duration::from_millis(3)).await;
            }
        }
        let bme280 = BME280 {
            address: self.address,
            calibration: self.read_calibration(i2c_bus)?
        };
        Ok(bme280)
    }

    pub fn read_calibration(&self, i2c_bus: &mut I2c<'_, Blocking>) -> Result<BME280Calibration, SensorError> {
        let mut cal = BME280Calibration::default();

        // 1. Read Bank 1 (0x88 to 0xA1) - 26 bytes
        let mut bank1 = [0u8; 26];
        i2c_bus.write_read(self.address, &[0x88], &mut bank1).map_err(|_| SensorError::I2cError)?;

        cal.dig_t1 = u16::from_le_bytes([bank1[0], bank1[1]]);
        cal.dig_t2 = i16::from_le_bytes([bank1[2], bank1[3]]);
        cal.dig_t3 = i16::from_le_bytes([bank1[4], bank1[5]]);

        cal.dig_p1 = u16::from_le_bytes([bank1[6], bank1[7]]);
        cal.dig_p2 = i16::from_le_bytes([bank1[8], bank1[9]]);
        cal.dig_p3 = i16::from_le_bytes([bank1[10], bank1[11]]);
        cal.dig_p4 = i16::from_le_bytes([bank1[12], bank1[13]]);
        cal.dig_p5 = i16::from_le_bytes([bank1[14], bank1[15]]);
        cal.dig_p6 = i16::from_le_bytes([bank1[16], bank1[17]]);
        cal.dig_p7 = i16::from_le_bytes([bank1[18], bank1[19]]);
        cal.dig_p8 = i16::from_le_bytes([bank1[20], bank1[21]]);
        cal.dig_p9 = i16::from_le_bytes([bank1[22], bank1[23]]);

        cal.dig_h1 = bank1[25];

        // 2. Read Bank 2 (0xE1 to 0xE7) - 7 bytes
        let mut bank2 = [0u8; 7];
        i2c_bus.write_read(self.address, &[0xE1], &mut bank2).map_err(|_| SensorError::I2cError)?;

        cal.dig_h2 = i16::from_le_bytes([bank2[0], bank2[1]]);
        cal.dig_h3 = bank2[2];

        // The humidity 4 and 5 registers are weirdly interleaved in the datasheet!
        cal.dig_h4 = ((bank2[3] as i16) << 4) | ((bank2[4] as i16) & 0x0F);
        cal.dig_h5 = ((bank2[5] as i16) << 4) | ((bank2[4] as i16) >> 4);
        cal.dig_h6 = bank2[6] as i8;

        Ok(cal)
    }
}

impl I2cSensor for BME280 {
    async fn read_sensor(&self, i2c_bus: &mut I2c<'_, Blocking>) -> Result<SensorReading, SensorError> {
        i2c_bus.write(self.address, &[0xF4, 0x25]).map_err(|_| SensorError::I2cError)?;

        Timer::after(Duration::from_millis(20)).await;

        // burst read all 8 bytes starting from 0xF7
        let mut buffer = [0u8; 8];
        if i2c_bus.write_read(self.address, &[0xF7], &mut buffer).is_err() {
            return Err(SensorError::I2cError);
        }

        // 20bit pressure
        let raw_press = ((buffer[0] as u32) << 12)
            | ((buffer[1] as u32) << 4)
            | ((buffer[2] as u32) >> 4);

        // 20bit temp
        let raw_temp = ((buffer[3] as u32) << 12)
            | ((buffer[4] as u32) << 4)
            | ((buffer[5] as u32) >> 4);

        // 16bit humidity
        let raw_hum = ((buffer[6] as u32) << 8)
            | (buffer[7] as u32);

        // calculate physical data
        let (temp, tfine) = BME280::compensate_temperature(raw_temp as i32, &self.calibration);
        let press = BME280::compensate_pressure(raw_press as i32, tfine, &self.calibration);
        let hum = BME280::compensate_pressure(raw_hum as i32, tfine, &self.calibration);

        Ok(SensorReading::Climate {
            temperature: temp,
            pressure: press,
            humidity: hum
        })
    }
}

impl BME280 {
    pub fn compensate_temperature(adc_t: i32, cal: &BME280Calibration) -> (f32, i32) {
        let var1 = (((adc_t >> 3) - ((cal.dig_t1 as i32) << 1)) * (cal.dig_t2 as i32)) >> 11;
        let var2 = (((((adc_t >> 4) - (cal.dig_t1 as i32)) * ((adc_t >> 4) - (cal.dig_t1 as i32))) >> 12) * (cal.dig_t3 as i32)) >> 14;
        let t_fine = var1 + var2;

        // Output format is Q5.2 (e.g., 5123 equals 51.23°C)
        let t = (t_fine * 5 + 128) >> 8;

        (t as f32 / 100.0, t_fine)
    }

    /// Returns Pressure in hPa (requires 64-bit integer support)
    pub fn compensate_pressure(adc_p: i32, t_fine: i32, cal: &BME280Calibration) -> f32 {
        let mut var1 = (t_fine as i64) - 128000;
        let mut var2 = var1 * var1 * (cal.dig_p6 as i64);
        var2 += (var1 * (cal.dig_p5 as i64)) << 17;
        var2 += (cal.dig_p4 as i64) << 35;
        var1 = ((var1 * var1 * (cal.dig_p3 as i64)) >> 8) + ((var1 * (cal.dig_p2 as i64)) << 12);
        var1 = (((1i64 << 47) + var1) * (cal.dig_p1 as i64)) >> 33;

        if var1 == 0 {
            return 0.0; // avoid division by zero
        }

        let mut p = 1048576 - (adc_p as i64);
        p = (((p << 31) - var2) * 3125) / var1;
        var1 = ((cal.dig_p9 as i64) * (p >> 13) * (p >> 13)) >> 25;
        var2 = ((cal.dig_p8 as i64) * p) >> 19;
        p = ((p + var1 + var2) >> 8) + ((cal.dig_p7 as i64) << 4);

        // Output format is Q24.8. Divide by 256 for Pascals, then by 100 for hPa
        (p as f32 / 256.0) / 100.0
    }

    /// Returns Humidity as %RH
    pub fn compensate_humidity(adc_h: i32, t_fine: i32, cal: &BME280Calibration) -> f32 {
        let mut v_x1_u32r = t_fine - 76800;

        v_x1_u32r = ((((adc_h << 14) - ((cal.dig_h4 as i32) << 20) - ((cal.dig_h5 as i32) * v_x1_u32r)) + 16384) >> 15) *
            (((((((v_x1_u32r * (cal.dig_h6 as i32)) >> 10) * (((v_x1_u32r * (cal.dig_h3 as i32)) >> 11) + 32768)) >> 10) + 2097152) * (cal.dig_h2 as i32) + 8192) >> 14);
        v_x1_u32r = v_x1_u32r - (((((v_x1_u32r >> 15) * (v_x1_u32r >> 15)) >> 7) * (cal.dig_h1 as i32)) >> 4);

        v_x1_u32r = if v_x1_u32r < 0 { 0 } else { v_x1_u32r };
        v_x1_u32r = if v_x1_u32r > 419430400 { 419430400 } else { v_x1_u32r };

        // Output format is Q22.10. Divide by 1024 for %RH
        (v_x1_u32r >> 12) as f32 / 1024.0
    }
}