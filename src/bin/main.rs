#![no_std]
#![no_main]
esp_bootloader_esp_idf::esp_app_desc!();
extern crate alloc;

use core::panic::PanicInfo;
use alloc::string::ToString;
use core::str::FromStr;
use static_cell::StaticCell;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_net::{Config as NetConfig, Runner, StackResources, Ipv4Address, StaticConfigV4, Ipv4Cidr};
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Timer};
use esp_hal::{Config,
              timer::timg::TimerGroup,
              interrupt::software::SoftwareInterruptControl,
              rng::Rng,
};
use esp_radio::wifi::{self, WifiDevice, ModeConfig, ClientConfig as WifiClientConfig};
use smoltcp::wire::IpAddress;
use rust_mqtt::{client::Client,
                client::options::ConnectOptions,
                config::KeepAlive,
                client::options::PublicationOptions,
                types::TopicName,
                types::MqttString,
                Bytes,
};
use rust_mqtt::types::QoS;


static SSID: &str = env!("SSID");
static PASSWORD: &str = env!("PASSWORD");
static STATIC_IP: &str = env!("STATIC_IP");
static GATEWAY_IP: &str = env!("GATEWAY_IP");
static DNS_SERVER_IP: &str = env!("DNS_SERVER_IP");
static MQTT_BROKER_IP: &str = env!("MQTT_BROKER_IP");

static NETWORK_STACK: StaticCell<StackResources<3>> = StaticCell::new();
static RADIO_CONTROLLER: StaticCell<esp_radio::Controller> = StaticCell::new();
static RX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();
static TX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();

// unsafe extern "C" {
//     fn esp_wifi_set_max_tx_power(power: i8) -> i32;
// }

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    defmt::error!("Panic! {}", defmt::Display2Format(_info));
    loop{}
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static,WifiDevice<'static>>) {
    runner.run().await
}

#[embassy_executor::task]
async fn mqtt_task(stack: embassy_net::Stack<'static>) {
    // mqtt setup
    let rx_buffer = RX_BUFFER.init([0; 4096]);
    let tx_buffer = TX_BUFFER.init([0; 4096]);

    let broker_ip = Ipv4Address::from_str(MQTT_BROKER_IP).unwrap();
    let broker_address = embassy_net::IpEndpoint::new(
        IpAddress::Ipv4(broker_ip),
        1883
    );

    let mut buffer = rust_mqtt::buffer::AllocBuffer;

    let connect_options = ConnectOptions {
        keep_alive: KeepAlive::Seconds(30),
        session_expiry_interval: Default::default(),
        user_name: None,
        password: None,
        clean_start: true,
        will: None,
    };

    // connect to broker
    let mut client: Client<'_, TcpSocket<'_>, _, 1024, 1024, 1> = loop {
        let mut socket = TcpSocket::new(stack, rx_buffer, tx_buffer);
        match socket.connect(broker_address).await {
            Ok(_) => {
                defmt::info!("TCP successfully connected to broker host!");
                let device_str = MqttString::from_slice("wetterstation").unwrap();
                let mut temp_client = Client::new(&mut buffer);
                match temp_client.connect(
                    socket,
                    &connect_options,
                    Some(device_str),
                ).await {
                    Ok(_) => {
                        defmt::info!("Successfully connected to MQTT broker!");
                        break temp_client;
                    }
                    Err(_e) => {
                        defmt::warn!("MQTT handshake failed: {}", defmt::Debug2Format(&_e));
                        Timer::after(Duration::from_secs(5)).await;
                    }
                }
            }
            Err(_e) => {
                defmt::warn!("TCP socket connection failed. Retrying in 1 second...");
                Timer::after(Duration::from_secs(3)).await;
            }
        }

    };
    defmt::info!("Connected to broker.");

    loop {
        // publishing
        let topic_str = MqttString::from_slice("wetterstation").unwrap();
        let topic = unsafe { TopicName::new_unchecked(topic_str) };
        let payload = Bytes::from("Hello from esp32c3!");
        let publication_options = PublicationOptions {
            retain: false,
            topic,
            qos: QoS::AtMostOnce
        };
        client.publish(
            &publication_options,
            payload,
        ).await.unwrap();
        defmt::info!("Published!");

        Timer::after(Duration::from_secs(2)).await;
    }
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> !{
    // create a 72KB memory pool and registers it globally.
    esp_alloc::heap_allocator!(size: 120 * 1024);

    // get peripherals
    let peripherals = esp_hal::init(Config::default());
    // start esp_rtos scheduler
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let software_interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupts.software_interrupt0);

    // wifi init
    let radio_init = esp_radio::init().unwrap();
    let radio_controller = RADIO_CONTROLLER.init(radio_init);
    let wifi_config = wifi::Config::default();

    let (mut wifi_controller, interfaces) = wifi::new(
        radio_controller,
        peripherals.WIFI,
        wifi_config,
    ).unwrap();
    defmt::info!("Wifi initialized.");

    // init static stack
    let resources = NETWORK_STACK.init(StackResources::new());

    // get random network seed
    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | (rng.random() as u64); // two 32bit numbers

    // net config
    let static_ip = Ipv4Address::from_str(STATIC_IP).unwrap();
    let gateway_ip = Ipv4Address::from_str(GATEWAY_IP).unwrap();
    let dns_server_ip = Ipv4Address::from_str(DNS_SERVER_IP).unwrap();
    let mut dns_servers: heapless::Vec<Ipv4Address, 3> = heapless::Vec::new();
    dns_servers.push(dns_server_ip).unwrap();

    let static_config = StaticConfigV4 {
        address: Ipv4Cidr::new(static_ip, 8),
        gateway: Some(gateway_ip),
        dns_servers,
    };
    let net_config = NetConfig::ipv4_static(static_config);

    // init network stack
    let (stack, runner) = embassy_net::new(
        interfaces.sta,
        net_config,
        resources,
        seed,
    );

    // spawn network task
    spawner.spawn(net_task(runner)).unwrap();

    // configure wifi_client
    let client_config = ModeConfig::Client(
        WifiClientConfig::default()
            .with_ssid(SSID.to_string())
            .with_password(PASSWORD.to_string())
    );
    wifi_controller.set_config(&client_config).unwrap();

    // connect to wifi
    wifi_controller.start_async().await.unwrap();
    // unsafe {
    //     let result = esp_wifi_set_max_tx_power(16);
    //     if result == 0 {
    //         defmt::info!("Successfully lowered TX power to 8.5dBm.");
    //     } else {
    //         defmt::warn!("Failed to lower TX power, error code: {}", result);
    //     }
    // }
    defmt::info!("Attempting to connect to WiFi network: {}", SSID);
    loop {
        match wifi_controller.connect_async().await {
            Ok(_) => {
                defmt::info!("Wifi successfully connected!");
                break;
            }
            Err(_e) => {
                defmt::warn!("Connection failed: {} \n Retrying in 1 second...", _e);
                Timer::after(Duration::from_secs(3)).await;
            }
        }
    }

    // wait for wifi stack
    while !stack.is_config_up() {
        Timer::after(Duration::from_secs(3)).await;
    }

    spawner.spawn(mqtt_task(stack)).unwrap();
    loop {
        Timer::after(Duration::from_secs(10)).await;
    }
}
