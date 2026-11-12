"""Deterministic capability fixture using the existing Controls MCP endpoint."""
import json
import os
import urllib.request
import uuid
import time


def call(connection, method, params):
    request_id = str(uuid.uuid4())
    request = urllib.request.Request(connection["url"], data=json.dumps({
        "jsonrpc": "2.0", "id": request_id, "method": method, "params": params,
    }).encode(), headers={
        "Content-Type": "application/json", "Accept": "application/json, text/event-stream",
        "Authorization": "Bearer " + connection["token"],
        "MCP-Protocol-Version": "2025-03-26",
    })
    with urllib.request.urlopen(request, timeout=40) as response:
        body = response.read().decode()
    try:
        value = json.loads(body)
    except json.JSONDecodeError:
        value = next(json.loads(line[5:].strip()) for line in body.splitlines()
                     if line.startswith("data:") and json.loads(line[5:].strip()).get("id") == request_id)
    if "error" in value:
        raise RuntimeError(value["error"])
    result = value["result"]
    if result.get("isError"):
        raise RuntimeError(result)
    return result


def main():
    with open(os.environ["FABRIC_CONTEXT_PATH"]) as source:
        context = json.load(source)
    with open(os.environ["FACTORY_INPUTS_PATH"]) as source:
        output = json.load(source)
    time.sleep(output.get("_delay", 0))
    connection = context["connection"]
    call(connection, "initialize", {"protocolVersion": "2025-03-26", "capabilities": {},
         "clientInfo": {"name": "fabric-echo", "version": "1"}})
    inbox = call(connection, "tools/call", {"name": "read_messages", "arguments": {"thread_id": context["thread_id"]}})
    batch = inbox.get("structuredContent") or json.loads(inbox["content"][0]["text"])
    for message in batch["messages"]:
        if message["payload"]["kind"] == "question":
            call(connection, "tools/call", {"name": "send_message", "arguments": {
                "operation_id": str(uuid.uuid4()), "thread_id": context["thread_id"],
                "recipients": [message["sender"]], "in_reply_to": message["id"],
                "payload": {"kind": "answer", "version": 1, "parts": [{"kind": "text", "text": "Specialist inspected the supplied context."}]},
            }})
    call(connection, "tools/call", {"name": "send_message", "arguments": {
        "operation_id": str(uuid.uuid4()), "thread_id": context["thread_id"],
        "recipients": [context["result_recipient"]],
        "payload": {"kind": "fabric.result", "version": 1, "parts": [{
            "kind": "data", "data": {"invocation_id": context["invocation_id"], "output": output},
        }]},
    }})


if __name__ == "__main__":
    main()
