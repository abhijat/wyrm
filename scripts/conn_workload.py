import uuid

import redis

KEY = f"w:poc:{uuid.uuid4()}"
PAYLOAD_BYTES = 8 * 1024 * 1024

client = redis.Redis(
    host="127.0.0.1",
    port=6379,
    single_connection_client=True,
    socket_timeout=10,
)

try:
    client.ping()
    input("Connected. After baseline press Enter")

    client.set(KEY, b"x" * PAYLOAD_BYTES, ex=300)
    client.delete(KEY)

    input("Connection is open, key is deleted. Take snapshots and press ENTER")
finally:
    try:
        client.delete(KEY)
    finally:
        client.close()

print("done, take a snapshot")
