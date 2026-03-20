#!/usr/bin/env python3
import asyncio
import json
import os

import websockets


async def handler(websocket):
    async for raw in websocket:
        try:
            message = json.loads(raw)
        except json.JSONDecodeError:
            continue

        if (
            isinstance(message, list)
            and len(message) >= 2
            and message[0] == "REQ"
            and isinstance(message[1], str)
        ):
            await websocket.send(json.dumps(["EOSE", message[1]]))


async def main():
    host = os.environ.get("RELAY_HOST", "0.0.0.0")
    port = int(os.environ.get("RELAY_PORT", "8765"))
    async with websockets.serve(handler, host, port):
        print(f"mock relay listening on ws://{host}:{port}", flush=True)
        await asyncio.Future()


if __name__ == "__main__":
    asyncio.run(main())
