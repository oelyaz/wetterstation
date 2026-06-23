pub mod types;
pub mod tsl2591;
pub mod bme280;
pub mod battery;

pub use types::{SensorReading, SensorError, I2cSensor};

use tsl2591::TSL2591;
use bme280::BME280Builder;
use battery::Battery;

use embassy_time::{Duration, Timer};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use esp_hal::{i2c::master::{Config, I2c, }, peripherals::{ADC1, GPIO0, GPIO22, GPIO23, I2C0}};


pub static SENSOR_CHANNEL: Channel<CriticalSectionRawMutex, SensorReading, 2> = Channel::new();
const CCS811_ADDRESS: u8 = 0x5A;
const BME280_ADDRESS: u8 = 0x76;
const TSL2591_ADDRESS: u8 = 0x29;


#[embassy_executor::task]
pub async fn sensor_task(
    i2c0: I2C0<'static>,
    sda_pin: GPIO22<'static>,
    scl_pin: GPIO23<'static>,
    adc1: ADC1<'static>,
    battery_pin: GPIO0<'static>,
) {
    let mut i2c = I2c::new(i2c0, Config::default())
        .unwrap()
        .with_sda(sda_pin)
        .with_scl(scl_pin);

    let mut battery = Battery::new(adc1, battery_pin);
    let light_sensor = TSL2591 { address: TSL2591_ADDRESS };
    let climate_sensor_builder = BME280Builder { address: BME280_ADDRESS };

    match light_sensor.init_sensor(&mut i2c).await {
        Ok(()) => defmt::info!("light-sensor initialized"),
        Err(_) => {defmt::error!("could not initialize light-sensor"); panic!()},
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
            queue_reading(light_reading, "light");
        }
        if let Ok(climate_reading) = climate_sensor.read_sensor(&mut i2c).await {
            queue_reading(climate_reading, "climate");
        }
        queue_reading(battery.read().await, "battery");

        Timer::after(Duration::from_secs(10)).await;
    }
}

fn queue_reading(reading: SensorReading, label: &str) {
    match SENSOR_CHANNEL.try_send(reading) {
        Ok(()) => defmt::info!("Sent {} reading: {}", label, reading),
        Err(_) => defmt::warn!("Sensor channel full, dropping {} reading", label),
    }
}