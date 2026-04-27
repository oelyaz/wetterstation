use alloc::string::ToString;
use core::str::FromStr;
use embassy_net::{
    Config as NetConfig, Ipv4Address, Ipv4Cidr, Runner, StackResources, StaticConfigV4,
};
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::rng::Rng;
use esp_radio::wifi::{self, ClientConfig as WifiClientConfig, ModeConfig, WifiDevice, WifiEvent};
use static_cell::StaticCell;


static NETWORK_STACK: StaticCell<StackResources<3>> = StaticCell::new();
static RADIO_CONTROLLER: StaticCell<esp_radio::Controller> = StaticCell::new();

/// Runs the network stack
#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

pub fn init_wifi_and_net(
    peripherals_wifi: esp_hal::peripherals::WIFI<'static>,
) -> (wifi::WifiController<'static>, embassy_net::Stack<'static>, Runner<'static, WifiDevice<'static>>) {
    // wifi init
    let radio_init = esp_radio::init().unwrap();
    let radio_controller = RADIO_CONTROLLER.init(radio_init);
    let wifi_config = wifi::Config::default();

    let (mut wifi_controller, interfaces) = wifi::new(
        radio_controller,
        peripherals_wifi,
        wifi_config,
    ).unwrap();
    defmt::info!("Wifi initialized.");

    // init static stack
    let resources = NETWORK_STACK.init(StackResources::new());

    // get random network seed
    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | (rng.random() as u64);

    // net config
    let static_ip = Ipv4Address::from_str(crate::config::STATIC_IP).unwrap();
    let gateway_ip = Ipv4Address::from_str(crate::config::GATEWAY_IP).unwrap();
    let dns_server_ip = Ipv4Address::from_str(crate::config::DNS_SERVER_IP).unwrap();
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

    // configure wifi_client
    let client_config = ModeConfig::Client(
        WifiClientConfig::default()
            .with_ssid(crate::config::SSID.to_string())
            .with_password(crate::config::PASSWORD.to_string())
    );
    wifi_controller.set_config(&client_config).unwrap();

    (wifi_controller, stack, runner)
}

#[embassy_executor::task]
pub async fn wifi_connection_task(mut controller: wifi::WifiController<'static>) {
    defmt::info!("Starting WiFi connection task...");

    // Retry start_async instead of unwrapping; a transient radio init error
    // shouldn't panic the whole device.
    loop {
        match controller.start_async().await {
            Ok(()) => break,
            Err(e) => {
                defmt::error!("WiFi start failed: {:?}, retrying in 3s", defmt::Debug2Format(&e));
                Timer::after(Duration::from_secs(3)).await;
            }
        }
    }

    loop {
        match controller.connect_async().await {
            Ok(_) => {
                defmt::info!("WiFi connected successfully!");
                supervise_connection(&mut controller).await;
                defmt::warn!("WiFi link lost, attempting to reconnect...");
            }
            Err(e) => {
                defmt::error!("Failed to connect to WiFi: {:?}", defmt::Debug2Format(&e));
            }
        }

        Timer::after(Duration::from_secs(5)).await;
    }
}

/// Watch the WiFi link until it dies. Returns once we've decided we're
/// disconnected — either because StaDisconnected fired, or because periodic
/// polling caught a silent drop the event subsystem missed.
async fn supervise_connection(controller: &mut wifi::WifiController<'static>) {
    const POLL: Duration = Duration::from_secs(30);

    loop {
        match with_timeout(POLL, controller.wait_for_event(WifiEvent::StaDisconnected)).await {
            Ok(()) => {
                defmt::warn!("StaDisconnected event received.");
                return;
            }
            Err(_) => match controller.is_connected() {
                Ok(true) => continue,
                Ok(false) => {
                    defmt::warn!("Link silently dead (is_connected=false), forcing reconnect.");
                    let _ = controller.disconnect_async().await;
                    return;
                }
                Err(e) => {
                    defmt::warn!(
                        "is_connected() error: {:?}, forcing reconnect.",
                        defmt::Debug2Format(&e)
                    );
                    let _ = controller.disconnect_async().await;
                    return;
                }
            },
        }
    }
}
