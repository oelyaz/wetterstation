pub mod types;
pub mod bh1750;
pub mod bme280;

pub use types::{SensorReading, SensorError, I2cSensor};

use bh1750::BH1750;
use bme280::BME280Builder;

use embassy_time::{Duration, Timer};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use esp_hal::{i2c::master::{Config, I2c, }, peripherals::{GPIO8, GPIO9, I2C0}};


pub static SENSOR_CHANNEL: Channel<CriticalSectionRawMutex, SensorReading, 2> = Channel::new();
const CCS811_ADDRESS: u8 = 0x5A;
const BME280_ADDRESS: u8 = 0x76;
const BH1750_ADDRESS: u8 = 0x23;


#[embassy_executor::task]
pub async fn sensor_task(
    i2c0: I2C0<'static>,
    sda_pin: GPIO8<'static>,
    scl_pin: GPIO9<'static>,
) {
    let mut i2c = I2c::new(i2c0, Config::default())
        .unwrap()
        .with_sda(sda_pin)
        .with_scl(scl_pin);

    let light_sensor = BH1750 { address: BH1750_ADDRESS };
    let climate_sensor_builder = BME280Builder { address: BME280_ADDRESS };

    match light_sensor.init_sensor(&mut i2c).await {
        Ok(()) => defmt::info!("light-sensor initialized"),
        Err(_) => defmt::error!("could not initialize light-sensor"),
    }

    let climate_sensor = match climate_sensor_builder.init_sensor(&mut i2c).await {
        Ok(bme280) => {
            defmt::info!("climate-sensor initialized");
            bme280
        },
        Err(_) => {
            defmt::error!("could not initialize climate-sensor");
            panic!();
        }
    };

    loop {
        if let Ok(light_reading) = light_sensor.read_sensor(&mut i2c).await {
            SENSOR_CHANNEL.send(light_reading).await;
            defmt::info!("Sent light reading: {}", light_reading);
        }
        if let Ok(climate_reading) = climate_sensor.read_sensor(&mut i2c).await {
            SENSOR_CHANNEL.send(climate_reading).await;
            defmt::info!("Sent climate reading: {}", climate_reading);
        }

        Timer::after(Duration::from_secs(10)).await;
    }
}