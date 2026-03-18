import os
import json
import time
from datetime import datetime
import psycopg2
import paho.mqtt.client as mqtt
from paho.mqtt.enums import CallbackAPIVersion

# --- Configuration from Environment Variables ---
MQTT_BROKER = os.getenv("MQTT_BROKER", "mosquitto")
MQTT_PORT = int(os.getenv("MQTT_PORT", 1883))
MQTT_TOPIC = os.getenv("MQTT_TOPIC", "wetterstation/balkon/#")

DB_HOST = os.getenv("DB_HOST", "postgres")
DB_NAME = os.getenv("POSTGRES_DB", "weather_db")
DB_USER = os.getenv("POSTGRES_USER", "weather_user")
DB_PASS = os.getenv("POSTGRES_PASSWORD", "weather_pass")

def get_db_connection():
    """Connects to the PostgreSQL database, retrying until successful."""
    while True:
        try:
            conn = psycopg2.connect(
                host=DB_HOST,
                database=DB_NAME,
                user=DB_USER,
                password=DB_PASS
            )
            print("Connected to PostgreSQL database!")
            return conn
        except psycopg2.OperationalError:
            print("Database not ready yet. Retrying in 5 seconds...")
            time.sleep(5)

def init_db(conn):
    """Creates the sensor_data table if it doesn't exist."""
    with conn.cursor() as cursor:
        cursor.execute("""
CREATE TABLE IF NOT EXISTS climate (
id SERIAL PRIMARY KEY,
timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
celsius FLOAT,
humidity_rh FLOAT,
hpa FLOAT
);
CREATE TABLE IF NOT EXISTS brightness (
id SERIAL PRIMARY KEY,
timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
lux FLOAT
);
        """)
        conn.commit()
    print("Database tables are ready.")

def on_connect(client, userdata, flags, reason_code, properties):
    """Callback for when the client connects to the MQTT broker."""
    print(f"Connected to MQTT Broker with result code: {reason_code}")
    client.subscribe(MQTT_TOPIC)
    print(f"Subscribed to topic: {MQTT_TOPIC}")

def on_message(client, userdata, msg):
    """Callback for when a PUBLISH message is received from the broker."""
    try:
        # Decode the payload
        payload = msg.payload.decode('utf-8')
        data = json.loads(payload)

        print(f"Received from {msg.topic}: {data}")

        # Extract values (defaults to None/NULL if key is missing)
        lux = data.get("lux")
        celsius = data.get("celsius")
        humidity = data.get("%rh")
        hpa = data.get("hpa")

        if lux:
            with conn.cursor() as cursor:
                cursor.execute("""
INSERT INTO brightness (timestamp, lux)
VALUES (%s, %s)
                    """, (datetime.now(), lux))
                conn.commit()
        else:
            with conn.cursor() as cursor:
                cursor.execute("""
    INSERT INTO climate (timestamp, celsius, humidity_rh, hpa)
    VALUES (%s, %s, %s, %s)
                    """, (datetime.now(), celsius, humidity, hpa))
                conn.commit()

    except json.JSONDecodeError:
        print(f"Failed to decode JSON from payload: {msg.payload}")
    except Exception as e:
        print(f"Error processing message: {e}")

if __name__ == "__main__":
    # 1. Initialize Database Connection
    conn = get_db_connection()
    init_db(conn)

    # 2. Setup MQTT Client
    # We use VERSION2 as it is the standard for the modern paho-mqtt library
    client = mqtt.Client(CallbackAPIVersion.VERSION2, "python_db_logger")
    client.on_connect = on_connect
    client.on_message = on_message

    # 3. Connect to MQTT Broker
    while True:
        try:
            client.connect(MQTT_BROKER, MQTT_PORT, 60)
            break
        except ConnectionRefusedError:
            print("MQTT Broker not ready yet. Retrying in 5 seconds...")
            time.sleep(5)

    # 4. Start the blocking network loop
    client.loop_forever()