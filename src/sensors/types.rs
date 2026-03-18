use esp_hal::{i2c::master::I2c,
              Blocking,
};


#[derive(Copy, Clone, Debug, defmt::Format)]
pub enum SensorReading {
    Light { lux: f32 },
    Climate { temperature: f32, pressure: f32, humidity: f32 },
    Gas { co2: f32 },
    WindSpeed { speed: f32 },
}

#[derive(Debug, defmt::Format)]
pub enum SensorError {
    I2cError,
    InvalidData,
}

pub trait I2cSensor {
    async fn read_sensor(
        &self,
        i2c_bus: &mut I2c<'_, Blocking>
    ) -> Result<SensorReading, SensorError>;
}