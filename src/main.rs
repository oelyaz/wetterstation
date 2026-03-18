#![no_std]
#![no_main]
esp_bootloader_esp_idf::esp_app_desc!();
extern crate alloc;

pub mod config;
pub mod network;
pub mod mqtt;
pub mod sensors;

use core::panic::PanicInfo;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::{
    interrupt::software::SoftwareInterruptControl,
    timer::timg::TimerGroup,
    Config,
};
use crate::sensors::{SensorReading, SENSOR_CHANNEL};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    defmt::error!("Panic! {}", defmt::Display2Format(_info));
    loop{}
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> !{
    // create a 120KB memory pool and register it globally
    esp_alloc::heap_allocator!(size: 120 * 1024);

    // get peripherals
    let peripherals = esp_hal::init(Config::default());
    // start esp_rtos scheduler
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let software_interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupts.software_interrupt0);

    let (wifi_controller, stack, runner) = network::init_wifi_and_net(peripherals.WIFI);

    // spawn network task
    spawner.spawn(network::net_task(runner)).unwrap();
    // connect to wifi
    spawner.spawn(network::wifi_connection_task(wifi_controller)).unwrap();

    // wait for wifi stack
    while !stack.is_config_up() {
        Timer::after(Duration::from_secs(3)).await;
    }

    spawner.spawn(mqtt::mqtt_task(stack)).unwrap();
    spawner.spawn(sensors::sensor_task(peripherals.I2C0,
                                       peripherals.GPIO8,
                                       peripherals.GPIO9
    )).unwrap();

    let mut cycles = 0;
    loop {
        defmt::info!( "running since {} min ago", cycles );
        cycles += 1;
        Timer::after(Duration::from_secs(60)).await;
    }
}