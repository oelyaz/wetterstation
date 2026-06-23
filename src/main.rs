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
    rtc_cntl::{Rtc, RwdtStage},
    timer::timg::TimerGroup,
    Config,
};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    defmt::error!("Panic! {}", defmt::Display2Format(info));
    // Spin briefly so the defmt-rtt log has a chance to drain before we reset.
    for _ in 0..10_000_000 {
        core::hint::spin_loop();
    }
    esp_hal::system::software_reset()
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

    // Hardware watchdog: any task hanging for >60s triggers a system reset.
    // TIMG0 is owned by esp_rtos, so we use the RTC watchdog instead.
    let mut rtc = Rtc::new(peripherals.LPWR);
    rtc.rwdt.set_timeout(RwdtStage::Stage0, esp_hal::time::Duration::from_secs(60));
    rtc.rwdt.enable();
    defmt::info!("RWDT enabled with 60s timeout.");

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
                                       peripherals.GPIO22,
                                       peripherals.GPIO23,
                                       peripherals.ADC1,
                                       peripherals.GPIO0
    )).unwrap();

    let mut ticks: u32 = 0;
    loop {
        rtc.rwdt.feed();
        if ticks % 4 == 0 {
            defmt::info!("alive, uptime ~{} min", ticks / 4);
        }
        ticks = ticks.wrapping_add(1);
        Timer::after(Duration::from_secs(15)).await;
    }
}