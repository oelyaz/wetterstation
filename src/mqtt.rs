use core::str::FromStr;
use embassy_net::{tcp::TcpSocket, Ipv4Address};
use embassy_time::{Duration, Timer};
use rust_mqtt::{
    buffer::AllocBuffer,
    client::{
        options::{ConnectOptions, PublicationOptions},
        Client,
    },
    config::KeepAlive,
    types::{MqttString, QoS, TopicName},
    Bytes,
};
use smoltcp::wire::IpAddress;
use static_cell::StaticCell;

static RX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();
static TX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();

#[embassy_executor::task]
pub async fn mqtt_task(stack: embassy_net::Stack<'static>) {
    // mqtt setup
    let rx_buffer = RX_BUFFER.init([0; 4096]);
    let tx_buffer = TX_BUFFER.init([0; 4096]);

    let broker_ip = Ipv4Address::from_str(crate::config::MQTT_BROKER_IP).unwrap();
    let broker_address = embassy_net::IpEndpoint::new(
        IpAddress::Ipv4(broker_ip),
        1883
    );

    let mut buffer = AllocBuffer;

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