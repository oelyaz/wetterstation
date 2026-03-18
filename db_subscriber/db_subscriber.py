import paho.mqtt.client as mqtt
import psycopg2
import json
import os

# Connect to PostgreSQL using environment variables from docker-compose
conn = psycopg2.connect(
    host=os.environ.get("PG_HOST", "postgres"),
    database=os.environ.get("PG_DB", "weatherdb"),
    user=os.environ.get("PG_USER", "weatheruser"),
    password=os.environ.get("PG_PASSWORD", "weatherpassword")
)

def on_connect(client, userdata, flags, rc):
    print("Connected to MQTT Broker!")
    client.subscribe("wetterstation/balkon/#") # Subscribe to all balkon topics

def on_message(client, userdata, msg):
    try:
        # 1. Parse the JSON payload natively
        payload = json.loads(msg.payload.decode('utf-8'))
        cursor = conn.cursor()

        # 2. Route the data based on the topic
        if msg.topic == "wetterstation/balkon/klima":
            # .get() is safe; if a key is missing, it returns None (NULL in Postgres)
            cursor.execute(
                """
INSERT INTO weather_metrics (temperature, humidity, pressure)
VALUES (%s, %s, %s)
                """,
                (payload.get('temperature'), payload.get('humidity'), payload.get('pressure'))
            )

        elif msg.topic == "wetterstation/balkon/licht":
            cursor.execute(
                "INSERT INTO weather_metrics (light) VALUES (%s)",
                (payload.get('light'),)
            )

        # 3. Commit the transaction
        conn.commit()
        cursor.close()
        print(f"Saved data from {msg.topic}")

    except json.JSONDecodeError:
        print("Received malformed JSON.")
    except Exception as e:
        print(f"Database error: {e}")
        conn.rollback() # Prevent broken transactions

# Setup MQTT Client
client = mqtt.Client()
client.on_connect = on_connect
client.on_message = on_message

# Connect to the broker specified in docker-compose
broker_ip = os.environ.get("MQTT_BROKER", "10.0.1.107")
client.connect(broker_ip, 1883, 60)

# Keep running forever
client.loop_forever()