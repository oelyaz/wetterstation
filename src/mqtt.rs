use core::str::FromStr;
use heapless::String;
use core::fmt::Write;
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
use crate::sensors;

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


    let connect_options = ConnectOptions {
        keep_alive: KeepAlive::Seconds(30),
        session_expiry_interval: Default::default(),
        user_name: None,
        password: None,
        clean_start: true,
        will: None,
    };

    stack.wait_config_up().await;

    // connect to broker
    loop {
        let mut buffer = AllocBuffer;
        let mut client: Client<'_, TcpSocket<'_>, _, 1024, 1024, 1> = loop {
            let mut socket = TcpSocket::new(stack, &mut *rx_buffer, &mut *tx_buffer);
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

        // publishing loop, breaks into outer loop if fails to reconnect
        loop {
            let incoming_reading = sensors::SENSOR_CHANNEL.receive().await;
            defmt::info!("Incoming sensor data: {:?} to be published", incoming_reading);
            let topic_str;
            let mut payload_buf: String<64> = String::new();

            match incoming_reading {
                sensors::SensorReading::Light { lux } => {
                    topic_str = "balkon/licht";
                    write!(&mut payload_buf, "Light: {:.1} lx", lux).unwrap();
                },
                sensors::SensorReading::Climate { temperature, pressure, humidity } => {
                    topic_str = "balkon/klima";
                    write!(&mut payload_buf, "Temp: {:.1}C, Hum: {:.1}%, Press: {:.1}hPa", temperature, humidity, pressure).unwrap();
                },
                sensors::SensorReading::Gas { co2 } => {
                    topic_str = "balkon/gas";
                    write!(&mut payload_buf, "{:.1}C", co2).unwrap();
                },
                sensors::SensorReading::WindSpeed { speed } => {
                    topic_str = "balkon/wind";
                    write!(&mut payload_buf, "{:.1}m/s", speed).unwrap();
                }
            };

            // publishing
            let topic = unsafe {
                TopicName::new_unchecked(
                    MqttString::from_slice(topic_str).unwrap()
                )
            };
            let payload = Bytes::from(payload_buf.as_bytes());
            let publication_options = PublicationOptions {
                retain: false,
                topic,
                qos: QoS::AtMostOnce,
            };

            if let Err(e) = client.publish(&publication_options, payload).await {
                defmt::error!("MQTT Publish Failed! Connection lost. Error: {:?}", defmt::Debug2Format(&e));
                break;
            }

            defmt::info!("Published to MQTT: {} -> {}", topic_str, payload_buf.as_str());
        }
        Timer::after(Duration::from_secs(5)).await;
    }
}